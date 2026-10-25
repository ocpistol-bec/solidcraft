//! Expression evaluation, assignment targets and comprehensions.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Env, Flow, HeapObj, Interp, Res};
use crate::grammar::ast::{Arg, CmpOp, CompKind, Expr, Generator, UnaryOp};
use crate::grammar::value::{HeapId, Value};

impl Interp {
    pub fn eval(&mut self, e: &'static Expr, env: HeapId) -> Res<Value> {
        match e {
            Expr::Const(v) => Ok(v.clone()),
            Expr::Name(n) => self.lookup(n, env),
            Expr::Attr(obj, name) => {
                let o = self.eval(obj, env)?;
                self.getattr(&o, name)
            }
            Expr::Subscript(obj, index) => {
                let o = self.eval(obj, env)?;
                let i = self.eval_index(index, env)?;
                self.getitem(&o, &i)
            }
            Expr::Slice(..) => self.eval_index(e, env),
            Expr::Call(f, args, kwargs) => self.eval_call(f, args, kwargs, env),
            Expr::Bin(l, op, r) => {
                let a = self.eval(l, env)?;
                let b = self.eval(r, env)?;
                self.binop(*op, &a, &b)
            }
            Expr::Unary(op, x) => self.eval_unary(*op, x, env),
            Expr::Bool(and, items) => self.eval_bool(*and, items, env),
            Expr::Compare(left, rest) => self.eval_compare(left, rest, env),
            Expr::IfExp(test, body, orelse) => {
                let t = self.eval(test, env)?;
                if self.truthy(&t)? {
                    self.eval(body, env)
                } else {
                    self.eval(orelse, env)
                }
            }
            Expr::Tuple(items) => Ok(Value::tuple(self.eval_items(items, env)?)),
            Expr::List(items) => {
                let v = self.eval_items(items, env)?;
                Ok(self.new_list(v))
            }
            Expr::Set(items) => {
                let v = self.eval_items(items, env)?;
                self.new_set(v)
            }
            Expr::Dict(pairs) => self.eval_dict(pairs, env),
            Expr::Comp(kind, elt, value, gens) => {
                self.eval_comp(*kind, elt, value.as_deref(), gens, env)
            }
            Expr::Lambda(def) => self.make_closure(def, env),
            Expr::Starred(_) => Err(Flow::Fatal(
                "starred expression outside a call or list".into(),
            )),
            Expr::Joined(parts) => self.eval_joined(parts, env),
            Expr::Formatted(v, conv, _spec) => {
                let v = self.eval(v, env)?;
                let s = if *conv == 'r' as i64 {
                    self.repr(&v)?
                } else {
                    self.py_str(&v)?
                };
                Ok(Value::str(&s))
            }
            Expr::Unsupported(kind) => {
                Err(Flow::Fatal(format!("expression {kind} is not supported")))
            }
        }
    }

    fn eval_index(&mut self, e: &'static Expr, env: HeapId) -> Res<Value> {
        let Expr::Slice(lo, hi, step) = e else {
            return self.eval(e, env);
        };
        let part = |x: &'static Option<Box<Expr>>, me: &mut Self| -> Res<Value> {
            match x {
                Some(x) => me.eval(x, env),
                None => Ok(Value::None),
            }
        };
        let (a, b, c) = (part(lo, self)?, part(hi, self)?, part(step, self)?);
        let slice = HashMap::from([
            (Arc::from("start"), a),
            (Arc::from("stop"), b),
            (Arc::from("step"), c),
        ]);
        Ok(self.alloc(HeapObj::Attrs(with_kind(slice, "slice"))))
    }

    /// Evaluates items, expanding `*x` entries.
    pub fn eval_items(&mut self, items: &'static [Expr], env: HeapId) -> Res<Vec<Value>> {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            match item {
                Expr::Starred(x) => {
                    let v = self.eval(x, env)?;
                    out.extend(self.iterate(&v)?);
                }
                _ => out.push(self.eval(item, env)?),
            }
        }
        Ok(out)
    }

    fn eval_call(
        &mut self,
        f: &'static Expr,
        args: &'static [Arg],
        kwargs: &'static [(Option<Arc<str>>, Expr)],
        env: HeapId,
    ) -> Res<Value> {
        let func = self.eval(f, env)?;
        let mut values = Vec::with_capacity(args.len());
        for a in args {
            match a {
                Arg::Plain(e) => values.push(self.eval(e, env)?),
                Arg::Star(e) => {
                    let v = self.eval(e, env)?;
                    values.extend(self.iterate(&v)?);
                }
            }
        }
        let mut named = Vec::with_capacity(kwargs.len());
        for (name, e) in kwargs {
            let v = self.eval(e, env)?;
            match name {
                Some(n) => named.push((n.clone(), v)),
                None => named.extend(self.kwargs_of(&v)?),
            }
        }
        self.call(&func, values, named)
    }

    fn kwargs_of(&mut self, v: &Value) -> Res<Vec<(Arc<str>, Value)>> {
        let pairs = self.dict_items(v)?;
        Ok(pairs
            .into_iter()
            .filter_map(|(k, v)| k.as_str().map(|s| (Arc::from(s), v)))
            .collect())
    }

    fn eval_unary(&mut self, op: UnaryOp, x: &'static Expr, env: HeapId) -> Res<Value> {
        let v = self.eval(x, env)?;
        match (op, &v) {
            (UnaryOp::Not, _) => Ok(Value::Bool(!self.truthy(&v)?)),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Pos, Value::Float(_)) => Ok(v.clone()),
            (_, _) => {
                let Some(i) = v.as_int() else {
                    return self.raise("builtins.TypeError", "bad operand type for unary operator");
                };
                Ok(Value::Int(match op {
                    UnaryOp::Neg => -i,
                    UnaryOp::Invert => !i,
                    _ => i,
                }))
            }
        }
    }

    fn eval_bool(&mut self, and: bool, items: &'static [Expr], env: HeapId) -> Res<Value> {
        let mut last = Value::Bool(and);
        for item in items {
            last = self.eval(item, env)?;
            if self.truthy(&last)? != and {
                return Ok(last);
            }
        }
        Ok(last)
    }

    fn eval_compare(
        &mut self,
        left: &'static Expr,
        rest: &'static [(CmpOp, Expr)],
        env: HeapId,
    ) -> Res<Value> {
        let mut a = self.eval(left, env)?;
        for (op, e) in rest {
            let b = self.eval(e, env)?;
            if !self.compare(*op, &a, &b)? {
                return Ok(Value::Bool(false));
            }
            a = b;
        }
        Ok(Value::Bool(true))
    }

    fn eval_dict(&mut self, pairs: &'static [(Option<Expr>, Expr)], env: HeapId) -> Res<Value> {
        let mut items = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            let value = self.eval(v, env)?;
            match k {
                Some(k) => items.push((self.eval(k, env)?, value)),
                None => items.extend(self.dict_items(&value)?),
            }
        }
        self.new_dict(items)
    }

    fn eval_joined(&mut self, parts: &'static [Expr], env: HeapId) -> Res<Value> {
        let mut out = String::new();
        for p in parts {
            let v = self.eval(p, env)?;
            out.push_str(&self.py_str(&v)?);
        }
        Ok(Value::str(&out))
    }

    fn eval_comp(
        &mut self,
        kind: CompKind,
        elt: &'static Expr,
        value: Option<&'static Expr>,
        gens: &'static [Generator],
        env: HeapId,
    ) -> Res<Value> {
        let globals = self.env_globals(env);
        let frame = Env {
            vars: HashMap::new(),
            parent: Some(env),
            cells: None,
            globals,
            global_names: Vec::new(),
        };
        let scope = self.alloc_id(HeapObj::Env(Box::new(frame)));
        let mut out = Vec::new();
        self.comp_level(elt, value, gens, scope, &mut out)?;
        match kind {
            CompKind::List => Ok(self.new_list(out)),
            CompKind::Gen => Ok(self.alloc(HeapObj::Iter(out, 0))),
            CompKind::Set => self.new_set(out),
            CompKind::Dict => {
                let pairs = out
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|kv| (kv[0].clone(), kv[1].clone()))
                    .collect();
                self.new_dict(pairs)
            }
        }
    }

    fn comp_level(
        &mut self,
        elt: &'static Expr,
        value: Option<&'static Expr>,
        gens: &'static [Generator],
        scope: HeapId,
        out: &mut Vec<Value>,
    ) -> Res<()> {
        let Some((first, rest)) = gens.split_first() else {
            out.push(self.eval(elt, scope)?);
            if let Some(v) = value {
                out.push(self.eval(v, scope)?);
            }
            return Ok(());
        };
        let seq = self.eval(&first.iter, scope)?;
        'items: for item in self.iterate(&seq)? {
            self.assign(&first.target, item, scope)?;
            for cond in &first.ifs {
                let c = self.eval(cond, scope)?;
                if !self.truthy(&c)? {
                    continue 'items;
                }
            }
            self.comp_level(elt, value, rest, scope, out)?;
        }
        Ok(())
    }

    /// Binds `v` to an assignment target.
    pub fn assign(&mut self, target: &'static Expr, v: Value, env: HeapId) -> Res<()> {
        match target {
            Expr::Name(n) => {
                self.set_var(env, n.clone(), v);
                Ok(())
            }
            Expr::Tuple(items) | Expr::List(items) => self.assign_seq(items, v, env),
            Expr::Attr(obj, name) => {
                let o = self.eval(obj, env)?;
                self.setattr(&o, name, v)
            }
            Expr::Subscript(obj, index) => {
                let o = self.eval(obj, env)?;
                let i = self.eval_index(index, env)?;
                self.setitem(&o, &i, v)
            }
            _ => Err(Flow::Fatal("unsupported assignment target".into())),
        }
    }

    fn assign_seq(&mut self, items: &'static [Expr], v: Value, env: HeapId) -> Res<()> {
        let values = self.iterate(&v)?;
        let star = items.iter().position(|e| matches!(e, Expr::Starred(_)));
        let Some(star) = star else {
            if values.len() != items.len() {
                let msg = format!("cannot unpack {} values into {}", values.len(), items.len());
                return self.raise("builtins.ValueError", msg);
            }
            for (t, x) in items.iter().zip(values) {
                self.assign(t, x, env)?;
            }
            return Ok(());
        };
        let tail = items.len() - star - 1;
        if values.len() < star + tail {
            return self.raise("builtins.ValueError", "not enough values to unpack");
        }
        for (t, x) in items[..star].iter().zip(values.iter()) {
            self.assign(t, x.clone(), env)?;
        }
        let middle = values[star..values.len() - tail].to_vec();
        if let Expr::Starred(inner) = &items[star] {
            let list = self.new_list(middle);
            self.assign(inner, list, env)?;
        }
        for (t, x) in items[star + 1..]
            .iter()
            .zip(values[values.len() - tail..].iter())
        {
            self.assign(t, x.clone(), env)?;
        }
        Ok(())
    }

    pub fn delete(&mut self, target: &'static Expr, env: HeapId) -> Res<()> {
        match target {
            Expr::Subscript(obj, index) => {
                let o = self.eval(obj, env)?;
                let i = self.eval_index(index, env)?;
                self.delitem(&o, &i)
            }
            Expr::Name(n) => {
                if let HeapObj::Env(e) = self.obj_mut(env) {
                    e.vars.remove(n);
                }
                Ok(())
            }
            _ => Err(Flow::Fatal("unsupported del target".into())),
        }
    }

    pub fn new_set(&mut self, items: Vec<Value>) -> Res<Value> {
        let mut d = super::Dict::default();
        for v in items {
            d.insert(v, Value::None)?;
        }
        Ok(self.alloc(HeapObj::Set(d)))
    }
}

fn with_kind(mut m: HashMap<Arc<str>, Value>, kind: &str) -> HashMap<Arc<str>, Value> {
    m.insert(Arc::from("__kind__"), Value::str(kind));
    m
}
