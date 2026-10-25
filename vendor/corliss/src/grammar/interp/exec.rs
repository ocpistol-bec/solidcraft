//! Statement execution.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Closure, Flow, HeapObj, Interp, Res};
use crate::grammar::ast::{Expr, FuncDef, Stmt, TryStmt};
use crate::grammar::value::{HeapId, Value};

/// How a block ended.
pub(crate) enum Exit {
    Next,
    Return(Value),
    Break,
    Continue,
}

impl Interp {
    pub fn exec_block(&mut self, body: &'static [Stmt], env: HeapId) -> Res<Exit> {
        for stmt in body {
            match self.exec(stmt, env)? {
                Exit::Next => {}
                other => return Ok(other),
            }
        }
        Ok(Exit::Next)
    }

    fn exec(&mut self, stmt: &'static Stmt, env: HeapId) -> Res<Exit> {
        match stmt {
            Stmt::Expr(e) => {
                self.eval(e, env)?;
            }
            Stmt::Assign(targets, value) => {
                let v = self.eval(value, env)?;
                for t in targets {
                    self.assign(t, v.clone(), env)?;
                }
            }
            Stmt::AugAssign(target, op, value) => {
                let cur = self.eval(target, env)?;
                let rhs = self.eval(value, env)?;
                let v = self.binop(*op, &cur, &rhs)?;
                self.assign(target, v, env)?;
            }
            Stmt::Return(v) => {
                let v = match v {
                    Some(e) => self.eval(e, env)?,
                    None => Value::None,
                };
                return Ok(Exit::Return(v));
            }
            Stmt::If(test, body, orelse) => {
                let t = self.eval(test, env)?;
                let block = if self.truthy(&t)? { body } else { orelse };
                return self.exec_block(block, env);
            }
            Stmt::For(target, iter, body, orelse) => {
                return self.exec_for(target, iter, body, orelse, env);
            }
            Stmt::While(test, body, orelse) => return self.exec_while(test, body, orelse, env),
            Stmt::Break => return Ok(Exit::Break),
            Stmt::Continue => return Ok(Exit::Continue),
            Stmt::Pass => {}
            Stmt::Raise(e) => return self.exec_raise(e.as_ref(), env),
            Stmt::Try(t) => return self.exec_try(t, env),
            Stmt::Def(def) => {
                let f = self.make_closure(def, env)?;
                self.set_var(env, def.name.clone(), f);
            }
            Stmt::Import(names) => self.exec_import(names, env)?,
            Stmt::ImportFrom(module, names) => self.exec_import_from(module, names, env)?,
            Stmt::Global(names) => {
                if let HeapObj::Env(e) = self.obj_mut(env) {
                    e.global_names.extend(names.iter().cloned());
                }
            }
            Stmt::Delete(targets) => {
                for t in targets {
                    self.delete(t, env)?;
                }
            }
            Stmt::Assert(test) => {
                let t = self.eval(test, env)?;
                if !self.truthy(&t)? {
                    return self.raise("builtins.AssertionError", "assertion failed");
                }
            }
            Stmt::Unsupported(kind) => {
                return Err(Flow::Fatal(format!("statement {kind} is not supported")));
            }
        }
        Ok(Exit::Next)
    }

    fn exec_for(
        &mut self,
        target: &'static Expr,
        iter: &'static Expr,
        body: &'static [Stmt],
        orelse: &'static [Stmt],
        env: HeapId,
    ) -> Res<Exit> {
        let seq = self.eval(iter, env)?;
        let items = self.iterate(&seq)?;
        for item in items {
            self.assign(target, item, env)?;
            match self.exec_block(body, env)? {
                Exit::Break => return Ok(Exit::Next),
                Exit::Return(v) => return Ok(Exit::Return(v)),
                _ => {}
            }
        }
        self.exec_block(orelse, env)
    }

    fn exec_while(
        &mut self,
        test: &'static Expr,
        body: &'static [Stmt],
        orelse: &'static [Stmt],
        env: HeapId,
    ) -> Res<Exit> {
        loop {
            let t = self.eval(test, env)?;
            if !self.truthy(&t)? {
                return self.exec_block(orelse, env);
            }
            match self.exec_block(body, env)? {
                Exit::Break => return Ok(Exit::Next),
                Exit::Return(v) => return Ok(Exit::Return(v)),
                _ => {}
            }
        }
    }

    fn exec_raise(&mut self, e: Option<&'static Expr>, env: HeapId) -> Res<Exit> {
        let Some(e) = e else {
            return self.raise("builtins.RuntimeError", "re-raise outside except");
        };
        let v = self.eval(e, env)?;
        match &v {
            Value::Heap(id) if matches!(self.obj(*id), HeapObj::Exception(_)) => {
                Err(Flow::Exception(v))
            }
            Value::Native(name) => {
                let class = name.clone();
                self.raise(&class, "")
            }
            _ => self.raise(
                "builtins.TypeError",
                "exceptions must derive from BaseException",
            ),
        }
    }

    fn exec_try(&mut self, t: &'static TryStmt, env: HeapId) -> Res<Exit> {
        let outcome = match self.exec_block(&t.body, env) {
            Ok(exit) => match exit {
                Exit::Next => self.exec_block(&t.orelse, env),
                other => Ok(other),
            },
            Err(Flow::Exception(exc)) => self.handle(t, exc, env),
            Err(fatal) => Err(fatal),
        };
        if t.finalbody.is_empty() {
            return outcome;
        }
        match self.exec_block(&t.finalbody, env)? {
            Exit::Next => outcome,
            other => Ok(other),
        }
    }

    fn handle(&mut self, t: &'static TryStmt, exc: Value, env: HeapId) -> Res<Exit> {
        for h in &t.handlers {
            if let Some(class) = &h.class {
                let wanted = self.eval(class, env)?;
                if !self.exception_matches(&exc, &wanted) {
                    continue;
                }
            }
            if let Some(name) = &h.name {
                self.set_var(env, name.clone(), exc.clone());
            }
            return self.exec_block(&h.body, env);
        }
        Err(Flow::Exception(exc))
    }

    /// Python's `except Class` test for the exception classes grammars use.
    pub fn exception_matches(&self, exc: &Value, wanted: &Value) -> bool {
        let Value::Heap(id) = exc else { return false };
        let HeapObj::Exception(e) = self.obj(*id) else {
            return false;
        };
        match wanted {
            Value::Tuple(items) => items.iter().any(|w| self.exception_matches(exc, w)),
            Value::Native(name) => class_matches(&e.class, name),
            _ => false,
        }
    }

    pub fn make_closure(&mut self, def: &'static FuncDef, env: HeapId) -> Res<Value> {
        let mut defaults = Vec::with_capacity(def.params.defaults.len());
        for d in &def.params.defaults {
            defaults.push(self.eval(d, env)?);
        }
        let mut kw_defaults = Vec::new();
        for (name, d) in def.params.kwonly.iter().zip(&def.params.kw_defaults) {
            if let Some(d) = d {
                let v = self.eval(d, env)?;
                kw_defaults.push((name.clone(), v));
            }
        }
        let globals = self.env_globals(env);
        let closure = Closure {
            def,
            defaults,
            kw_defaults,
            env,
            globals,
            attrs: HashMap::new(),
        };
        Ok(self.alloc(HeapObj::Closure(Box::new(closure))))
    }

    pub fn env_globals(&self, env: HeapId) -> u32 {
        match self.obj(env) {
            HeapObj::Env(e) => e.globals,
            _ => 0,
        }
    }

    /// Binds a name in the frame, or in the module when the frame declared it `global`.
    pub fn set_var(&mut self, env: HeapId, name: Arc<str>, v: Value) {
        let HeapObj::Env(e) = self.obj_mut(env) else {
            return;
        };
        if e.global_names.contains(&name) {
            let ns = e.globals;
            self.globals.insert((ns, name), v);
            return;
        }
        e.vars.insert(name, v);
    }

    fn exec_import(
        &mut self,
        names: &'static [(Arc<str>, Option<Arc<str>>)],
        env: HeapId,
    ) -> Res<()> {
        for (module, alias) in names {
            let m = self.import(module)?;
            let bound = alias
                .clone()
                .unwrap_or_else(|| Arc::from(module.split('.').next().unwrap_or(module)));
            self.set_var(env, bound, m);
        }
        Ok(())
    }

    fn exec_import_from(
        &mut self,
        module: &Arc<str>,
        names: &'static [(Arc<str>, Option<Arc<str>>)],
        env: HeapId,
    ) -> Res<()> {
        for (name, alias) in names {
            let full = format!("{module}.{name}");
            let v = if let Some(m) = self
                .module(&full)
                .filter(|_| self.prog.modules.contains_key(&full))
            {
                m
            } else if let Some(m) = self.module(module) {
                self.getattr(&m, name)?
            } else {
                Value::Native(Arc::from(format!("module:{name}")))
            };
            self.set_var(env, alias.clone().unwrap_or_else(|| name.clone()), v);
        }
        Ok(())
    }

    /// A module by name: an exported grammar module, or a native one.
    pub fn import(&mut self, name: &str) -> Res<Value> {
        if let Some(m) = self.module(name) {
            return Ok(m);
        }
        let short = name.rsplit('.').next().unwrap_or(name);
        Ok(Value::Native(Arc::from(format!("module:{short}"))))
    }
}

/// True when an exception of class `raised` is caught by `except wanted`.
fn class_matches(raised: &str, wanted: &str) -> bool {
    if raised == wanted {
        return true;
    }
    let base = |n: &str| n.rsplit('.').next().unwrap_or(n).to_string();
    match base(wanted).as_str() {
        "Exception" | "BaseException" => true,
        "LookupError" => matches!(base(raised).as_str(), "KeyError" | "IndexError"),
        "ValueError" => base(raised) == "UnicodeDecodeError",
        "ArithmeticError" => matches!(base(raised).as_str(), "ZeroDivisionError" | "OverflowError"),
        _ => false,
    }
}
