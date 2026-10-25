//! A Python-subset interpreter that runs the snapshot grammars.
//!
//! One [`Interp`] is a decoding session. It owns a heap of runtime objects, which are dropped
//! with the session; nothing it allocates escapes into the decoded output.

mod attrs;
mod eval;
mod exec;
pub(crate) mod ops;

use std::collections::HashMap;
use std::sync::Arc;

use super::ast::FuncDef;
use super::native::parser::ParserState;
use super::native::token::Token;
use super::snapshot::{Obj, Program, SnapFn};
use super::value::{HeapId, Key, ObjId, Value};

/// Why evaluation stopped early.
#[derive(Debug, Clone)]
pub(crate) enum Flow {
    /// A Python exception (a heap `Exception` object); grammars may catch it.
    Exception(Value),
    /// A construct the interpreter does not implement; never caught by grammar code.
    Fatal(String),
}

pub(crate) type Res<T> = Result<T, Flow>;

/// A Python exception object.
#[derive(Debug, Clone)]
pub(crate) struct Exception {
    pub class: Arc<str>,
    pub message: String,
}

/// A function defined while running (nested `def` or `lambda`).
#[derive(Debug)]
pub(crate) struct Closure {
    pub def: &'static FuncDef,
    pub defaults: Vec<Value>,
    pub kw_defaults: Vec<(Arc<str>, Value)>,
    pub env: HeapId,
    pub globals: ObjId,
    pub attrs: HashMap<Arc<str>, Value>,
}

/// A call frame's variables.
#[derive(Debug)]
pub(crate) struct Env {
    pub vars: HashMap<Arc<str>, Value>,
    pub parent: Option<HeapId>,
    pub cells: Option<ObjId>,
    pub globals: ObjId,
    pub global_names: Vec<Arc<str>>,
}

/// An insertion-ordered dict (also the storage of sets).
#[derive(Debug, Default, Clone)]
pub(crate) struct Dict {
    pub items: Vec<(Value, Value)>,
    pub index: HashMap<Key, usize>,
}

impl Dict {
    pub fn get(&self, key: &Value) -> Option<&Value> {
        let at = self.index.get(&Key::of(key)?)?;
        Some(&self.items[*at].1)
    }

    pub fn insert(&mut self, key: Value, value: Value) -> Res<()> {
        let k = Key::of(&key).ok_or_else(|| Flow::Fatal("unhashable dict key".into()))?;
        match self.index.get(&k) {
            Some(at) => self.items[*at].1 = value,
            None => {
                self.index.insert(k, self.items.len());
                self.items.push((key, value));
            }
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &Value) -> Option<Value> {
        let k = Key::of(key)?;
        let at = self.index.remove(&k)?;
        let (_, v) = self.items.remove(at);
        for slot in self.index.values_mut() {
            if *slot > at {
                *slot -= 1;
            }
        }
        Some(v)
    }
}

/// A runtime object.
#[derive(Debug)]
pub(crate) enum HeapObj {
    List(Vec<Value>),
    Dict(Dict),
    Set(Dict),
    Token(Box<Token>),
    Parser(Box<ParserState>),
    Closure(Box<Closure>),
    Env(Box<Env>),
    Exception(Exception),
    /// Attribute bag: the parse context, runtime namespaces.
    Attrs(HashMap<Arc<str>, Value>),
    /// An iterator over materialised items.
    Iter(Vec<Value>, usize),
}

/// SolidCraft: grammar call depth the decoder may nest. The grammar programs are
/// recursive descent, so their call chains track the nesting of the file being
/// decoded; on a 2 MiB thread stack a deep enough part overflows the stack and
/// aborts the process. Capping the depth makes the depth a clean error instead.
const MAX_CALL_DEPTH: usize = 500;

/// A decoding session.
pub(crate) struct Interp {
    pub prog: &'static Program,
    pub heap: Vec<HeapObj>,
    /// Module globals assigned at run time (`global` statements), by (namespace, name).
    pub globals: HashMap<(ObjId, Arc<str>), Value>,
    /// SolidCraft: current grammar call depth (see `MAX_CALL_DEPTH`).
    depth: usize,
}

impl Interp {
    pub fn new(prog: &'static Program) -> Self {
        Interp {
            prog,
            heap: Vec::new(),
            globals: HashMap::new(),
            depth: 0, // SolidCraft
        }
    }

    pub fn alloc(&mut self, obj: HeapObj) -> Value {
        Value::Heap(self.alloc_id(obj))
    }

    pub fn alloc_id(&mut self, obj: HeapObj) -> HeapId {
        self.heap.push(obj);
        (self.heap.len() - 1) as HeapId
    }

    pub fn obj(&self, id: HeapId) -> &HeapObj {
        &self.heap[id as usize]
    }

    pub fn obj_mut(&mut self, id: HeapId) -> &mut HeapObj {
        &mut self.heap[id as usize]
    }

    /// A Python exception of `class` with `message`.
    pub fn raise<T>(&mut self, class: &str, message: impl Into<String>) -> Res<T> {
        let exc = Exception {
            class: Arc::from(class),
            message: message.into(),
        };
        Err(Flow::Exception(self.alloc(HeapObj::Exception(exc))))
    }

    /// The reference grammar engine's grammar error, with `message`.
    pub fn grammar_error<T>(&mut self, message: impl Into<String>) -> Res<T> {
        self.raise("dcgram.GrammarError", message)
    }

    pub fn new_list(&mut self, items: Vec<Value>) -> Value {
        self.alloc(HeapObj::List(items))
    }

    pub fn new_dict(&mut self, items: Vec<(Value, Value)>) -> Res<Value> {
        let mut d = Dict::default();
        for (k, v) in items {
            d.insert(k, v)?;
        }
        Ok(self.alloc(HeapObj::Dict(d)))
    }

    /// The snapshot function behind `id`, if it is one.
    pub fn snap_fn(&self, id: ObjId) -> Option<&'static SnapFn> {
        match self.prog.obj(id)? {
            Obj::Function(f) => Some(f),
            _ => None,
        }
    }

    /// Calls any callable value.
    pub fn call(
        &mut self,
        f: &Value,
        args: Vec<Value>,
        kwargs: Vec<(Arc<str>, Value)>,
    ) -> Res<Value> {
        match f {
            Value::Obj(id) => match self.snap_fn(*id) {
                Some(sf) => self.call_snap(*id, sf, args, kwargs),
                None => Err(Flow::Fatal(format!("snapshot object {id} is not callable"))),
            },
            Value::Heap(id) => self.call_heap(*id, args, kwargs),
            Value::Native(name) => super::native::call_native(self, name.clone(), args, kwargs),
            Value::Method(m) => {
                let (recv, name) = (m.0.clone(), m.1.clone());
                super::native::call_method(self, recv, name, args, kwargs)
            }
            other => self.raise(
                "builtins.TypeError",
                format!("{} is not callable", ops::type_name(self, other)),
            ),
        }
    }

    fn call_heap(
        &mut self,
        id: HeapId,
        args: Vec<Value>,
        kwargs: Vec<(Arc<str>, Value)>,
    ) -> Res<Value> {
        let HeapObj::Closure(c) = self.obj(id) else {
            return Err(Flow::Fatal("heap object is not callable".into()));
        };
        let (def, env, globals) = (c.def, c.env, c.globals);
        let defaults = c.defaults.clone();
        let kw_defaults = c.kw_defaults.clone();
        let frame = Env {
            vars: HashMap::new(),
            parent: Some(env),
            cells: None,
            globals,
            global_names: Vec::new(),
        };
        self.run(def, frame, &defaults, &kw_defaults, args, kwargs)
    }

    fn call_snap(
        &mut self,
        id: ObjId,
        f: &'static SnapFn,
        args: Vec<Value>,
        kwargs: Vec<(Arc<str>, Value)>,
    ) -> Res<Value> {
        let def = &self.prog.codes[f.code].def;
        let defaults: Vec<Value> = f.defaults.iter().map(|d| self.prog.value(*d)).collect();
        let kw_defaults: Vec<(Arc<str>, Value)> = f
            .kw_defaults
            .iter()
            .map(|(k, d)| (k.clone(), self.prog.value(*d)))
            .collect();
        let frame = Env {
            vars: HashMap::new(),
            parent: None,
            cells: Some(id),
            globals: f.globals,
            global_names: Vec::new(),
        };
        self.run(def, frame, &defaults, &kw_defaults, args, kwargs)
    }

    fn run(
        &mut self,
        def: &'static FuncDef,
        mut frame: Env,
        defaults: &[Value],
        kw_defaults: &[(Arc<str>, Value)],
        args: Vec<Value>,
        kwargs: Vec<(Arc<str>, Value)>,
    ) -> Res<Value> {
        // SolidCraft: cap the recursive-descent depth (a clean error, not a stack overflow).
        let cap = std::env::var("SOLIDCRAFT_CORLISS_MAX_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MAX_CALL_DEPTH);
        if self.depth + 1 > cap {
            return Err(Flow::Fatal(format!("grammar call depth {cap} exceeded")));
        }
        self.depth += 1;
        let outcome = {
            self.bind(def, &mut frame, defaults, kw_defaults, args, kwargs)?;
            let env = self.alloc_id(HeapObj::Env(Box::new(frame)));
            match self.exec_block(&def.body, env)? {
                exec::Exit::Return(v) => Ok(v),
                _ => Ok(Value::None),
            }
        };
        self.depth -= 1;
        outcome
    }

    fn bind(
        &mut self,
        def: &FuncDef,
        frame: &mut Env,
        defaults: &[Value],
        kw_defaults: &[(Arc<str>, Value)],
        args: Vec<Value>,
        kwargs: Vec<(Arc<str>, Value)>,
    ) -> Res<()> {
        let p = &def.params;
        let mut rest = Vec::new();
        for (k, a) in args.into_iter().enumerate() {
            match p.positional.get(k) {
                Some(name) => {
                    frame.vars.insert(name.clone(), a);
                }
                None => rest.push(a),
            }
        }
        if let Some(name) = &p.vararg {
            frame.vars.insert(name.clone(), Value::tuple(rest));
        } else if !rest.is_empty() {
            return self.raise(
                "builtins.TypeError",
                format!("{}() got too many positional arguments", def.name),
            );
        }
        let mut extra = Vec::new();
        for (k, v) in kwargs {
            if p.positional.contains(&k) || p.kwonly.contains(&k) {
                frame.vars.insert(k, v);
            } else {
                extra.push((Value::Str(k), v));
            }
        }
        let first_default = p.positional.len().saturating_sub(defaults.len());
        for (k, name) in p.positional.iter().enumerate() {
            if frame.vars.contains_key(name) {
                continue;
            }
            match defaults
                .get(k.wrapping_sub(first_default))
                .filter(|_| k >= first_default)
            {
                Some(d) => {
                    frame.vars.insert(name.clone(), d.clone());
                }
                None => {
                    return self.raise(
                        "builtins.TypeError",
                        format!("{}() missing argument {name}", def.name),
                    );
                }
            }
        }
        for name in &p.kwonly {
            if !frame.vars.contains_key(name) {
                let d = kw_defaults
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::None);
                frame.vars.insert(name.clone(), d);
            }
        }
        if let Some(name) = &p.kwarg {
            let d = self.new_dict(extra)?;
            frame.vars.insert(name.clone(), d);
        }
        Ok(())
    }

    /// Looks a name up: frame chain, closure cells, module globals, builtins.
    pub fn lookup(&mut self, name: &Arc<str>, env: HeapId) -> Res<Value> {
        let mut cur = Some(env);
        let mut globals = 0;
        while let Some(id) = cur {
            let HeapObj::Env(e) = self.obj(id) else { break };
            if let Some(v) = e.vars.get(name) {
                return Ok(v.clone());
            }
            if let Some(cells) = e.cells
                && let Some(f) = self.snap_fn(cells)
                && let Some(cell) = f.cells.get(name)
            {
                return Ok(self.prog.value(*cell));
            }
            globals = e.globals;
            cur = e.parent;
        }
        if let Some(v) = self.global(globals, name) {
            return Ok(v);
        }
        Ok(Value::Native(Arc::from(format!("builtins.{name}"))))
    }

    /// A module global, run-time assignments first.
    pub fn global(&self, ns: ObjId, name: &Arc<str>) -> Option<Value> {
        if let Some(v) = self.globals.get(&(ns, name.clone())) {
            return Some(v.clone());
        }
        match self.prog.obj(ns)? {
            Obj::Namespace(entries) => entries.get(name).map(|id| self.prog.value(*id)),
            _ => None,
        }
    }

    /// The module namespace named `name` (a grammar module of the snapshot).
    pub fn module(&self, name: &str) -> Option<Value> {
        let short = name.rsplit('.').next().unwrap_or(name);
        let id = self
            .prog
            .modules
            .get(name)
            .or_else(|| self.prog.modules.get(short))?;
        Some(Value::Obj(*id))
    }
}
