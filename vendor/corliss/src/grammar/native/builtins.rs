//! Python builtins used by the grammars.

use std::sync::Arc;

use super::args::Args;
use crate::grammar::interp::ops::{as_f64, type_name};
use crate::grammar::interp::{Flow, HeapObj, Interp, Res};
use crate::grammar::snapshot::Obj;
use crate::grammar::value::Value;

/// Exception classes grammars raise or catch.
const EXCEPTIONS: [&str; 14] = [
    "Exception",
    "BaseException",
    "ValueError",
    "KeyError",
    "IndexError",
    "TypeError",
    "RuntimeError",
    "AssertionError",
    "AttributeError",
    "NotImplementedError",
    "StopIteration",
    "LookupError",
    "OverflowError",
    "ZeroDivisionError",
];

impl Interp {
    pub fn builtin(&mut self, name: &str, a: Args) -> Res<Value> {
        if EXCEPTIONS.contains(&name) {
            let msg = match a.get(0, "msg") {
                Some(v) => self.py_str(&v)?,
                None => String::new(),
            };
            return self.raise_value(&format!("builtins.{name}"), msg);
        }
        match name {
            "len" => self.b_len(&a),
            "int" => self.b_int(&a),
            "bool" => {
                let v = a.or(0, "x", Value::Bool(false));
                Ok(Value::Bool(self.truthy(&v)?))
            }
            "range" => self.b_range(&a),
            "enumerate" => self.b_enumerate(&a),
            "list" | "tuple" | "set" | "sorted" | "reversed" => self.b_sequence(name, &a),
            "dict" => self.b_dict(&a),
            "sum" | "any" | "all" | "max" | "min" => self.b_fold(name, &a),
            "zip" => self.b_zip(&a),
            "type" => Ok(type_of(self, &a.or(0, "o", Value::None))),
            "isinstance" => self.b_isinstance(&a),
            "callable" => Ok(Value::Bool(is_callable(self, &a.or(0, "o", Value::None)))),
            "getattr" | "hasattr" => self.b_getattr(name, &a),
            "setattr" => {
                let (o, n, v) = (
                    a.or(0, "o", Value::None),
                    a.or(1, "name", Value::None),
                    a.or(2, "v", Value::None),
                );
                let n: Arc<str> = Arc::from(n.as_str().unwrap_or(""));
                self.setattr(&o, &n, v)?;
                Ok(Value::None)
            }
            "next" => self.b_next(&a),
            "iter" => {
                let items = self.iterate(&a.or(0, "o", Value::None))?;
                Ok(self.alloc(HeapObj::Iter(items, 0)))
            }
            "str" => Ok(Value::str(&self.py_str(&a.or(0, "o", Value::str("")))?)),
            "repr" => Ok(Value::str(&self.repr(&a.or(0, "o", Value::None))?)),
            "hex" => Ok(Value::str(&hex_text(a.int(0, "x").unwrap_or(0)))),
            "abs" => Ok(match a.or(0, "x", Value::Int(0)) {
                Value::Float(f) => Value::Float(f.abs()),
                v => Value::Int(v.as_int().unwrap_or(0).abs()),
            }),
            "round" => Ok(Value::Int(
                as_f64(&a.or(0, "x", Value::Int(0))).unwrap_or(0.0).round() as i64,
            )),
            "float" => Ok(Value::Float(
                as_f64(&a.or(0, "x", Value::Int(0))).unwrap_or(0.0),
            )),
            "print" => Ok(Value::None),
            other => Err(Flow::Fatal(format!("builtin {other} is not implemented"))),
        }
    }

    /// Builds an exception object without raising it (calling an exception class).
    pub fn raise_value(&mut self, class: &str, message: String) -> Res<Value> {
        let exc = crate::grammar::interp::Exception {
            class: Arc::from(class),
            message,
        };
        Ok(self.alloc(HeapObj::Exception(exc)))
    }

    fn b_len(&mut self, a: &Args) -> Res<Value> {
        let v = a.or(0, "o", Value::None);
        match self.len_of(&v) {
            Some(n) => Ok(Value::Int(n as i64)),
            None => self.raise(
                "builtins.TypeError",
                format!("object of type {} has no len()", type_name(self, &v)),
            ),
        }
    }

    fn b_int(&mut self, a: &Args) -> Res<Value> {
        let v = a.or(0, "x", Value::Int(0));
        let base = a.int(1, "base").unwrap_or(10) as u32;
        match &v {
            Value::Float(f) => Ok(Value::Int(f.trunc() as i64)),
            Value::Str(s) => {
                let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
                match i64::from_str_radix(t, if s.trim().starts_with("0x") { 16 } else { base }) {
                    Ok(i) => Ok(Value::Int(i)),
                    Err(_) => self.raise(
                        "builtins.ValueError",
                        format!("invalid literal for int(): '{s}'"),
                    ),
                }
            }
            _ => match v.as_int() {
                Some(i) => Ok(Value::Int(i)),
                None => self.raise("builtins.TypeError", "int() argument must be a number"),
            },
        }
    }

    fn b_range(&mut self, a: &Args) -> Res<Value> {
        let (start, stop, step) = match (a.int(0, "start"), a.int(1, "stop"), a.int(2, "step")) {
            (Some(stop), None, _) => (0, stop, 1),
            (Some(start), Some(stop), step) => (start, stop, step.unwrap_or(1)),
            _ => return self.raise("builtins.TypeError", "range() needs integers"),
        };
        if step == 0 {
            return self.raise("builtins.ValueError", "range() arg 3 must not be zero");
        }
        let span = if step > 0 {
            (stop - start).max(0) / step
        } else {
            (start - stop).max(0) / -step
        };
        if span > 1 << 26 {
            return self.raise("builtins.MemoryError", format!("range of {span} items"));
        }
        let mut items = Vec::with_capacity(span as usize + 1);
        let mut x = start;
        while (step > 0 && x < stop) || (step < 0 && x > stop) {
            items.push(Value::Int(x));
            x += step;
        }
        Ok(self.new_list(items))
    }

    fn b_enumerate(&mut self, a: &Args) -> Res<Value> {
        let items = self.iterate(&a.or(0, "it", Value::None))?;
        let start = a.int(1, "start").unwrap_or(0);
        let pairs = items
            .into_iter()
            .enumerate()
            .map(|(k, v)| Value::tuple(vec![Value::Int(start + k as i64), v]))
            .collect();
        Ok(self.alloc(HeapObj::Iter(pairs, 0)))
    }

    fn b_sequence(&mut self, name: &str, a: &Args) -> Res<Value> {
        let mut items = match a.get(0, "iterable") {
            Some(v) => self.iterate(&v)?,
            None => Vec::new(),
        };
        match name {
            "tuple" => Ok(Value::tuple(items)),
            "set" => self.new_set(items),
            "reversed" => {
                items.reverse();
                Ok(self.alloc(HeapObj::Iter(items, 0)))
            }
            "sorted" => {
                self.sort_values(&mut items, a.get(1, "key"), a.get(2, "reverse"))?;
                Ok(self.new_list(items))
            }
            _ => Ok(self.new_list(items)),
        }
    }

    /// Sorts in place by `key(x)` (or the values), stable as Python's sort.
    pub fn sort_values(
        &mut self,
        items: &mut Vec<Value>,
        key: Option<Value>,
        reverse: Option<Value>,
    ) -> Res<()> {
        let mut keyed = Vec::with_capacity(items.len());
        for v in items.drain(..) {
            let k = match &key {
                Some(f) if !f.is_none() => self.call(f, vec![v.clone()], Vec::new())?,
                _ => v.clone(),
            };
            keyed.push((k, v));
        }
        let mut err = None;
        keyed.sort_by(|x, y| {
            let lt = self.compare(crate::grammar::ast::CmpOp::Lt, &x.0, &y.0);
            let gt = self.compare(crate::grammar::ast::CmpOp::Gt, &x.0, &y.0);
            match (lt, gt) {
                (Ok(true), _) => std::cmp::Ordering::Less,
                (_, Ok(true)) => std::cmp::Ordering::Greater,
                (Err(e), _) | (_, Err(e)) => {
                    err = Some(e);
                    std::cmp::Ordering::Equal
                }
                _ => std::cmp::Ordering::Equal,
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        if reverse
            .as_ref()
            .is_some_and(|r| matches!(r, Value::Bool(true)))
        {
            keyed.reverse();
        }
        items.extend(keyed.into_iter().map(|(_, v)| v));
        Ok(())
    }

    fn b_dict(&mut self, a: &Args) -> Res<Value> {
        let mut items = match a.get(0, "map") {
            Some(v @ (Value::Heap(_) | Value::Obj(_))) if self.dict_items(&v).is_ok() => {
                self.dict_items(&v)?
            }
            Some(v) => {
                let pairs = self.iterate(&v)?;
                let mut out = Vec::new();
                for p in pairs {
                    let kv = self.iterate(&p)?;
                    if kv.len() == 2 {
                        out.push((kv[0].clone(), kv[1].clone()));
                    }
                }
                out
            }
            None => Vec::new(),
        };
        items.extend(a.kw.iter().map(|(k, v)| (Value::Str(k.clone()), v.clone())));
        self.new_dict(items)
    }

    fn b_fold(&mut self, name: &str, a: &Args) -> Res<Value> {
        let items = if a.pos.len() > 1 {
            a.pos.clone()
        } else {
            self.iterate(&a.or(0, "it", Value::None))?
        };
        match name {
            "sum" => {
                let mut acc = a.get(1, "start").unwrap_or(Value::Int(0));
                for v in items {
                    acc = self.binop(crate::grammar::ast::BinOp::Add, &acc, &v)?;
                }
                Ok(acc)
            }
            "any" | "all" => {
                for v in items {
                    if self.truthy(&v)? == (name == "any") {
                        return Ok(Value::Bool(name == "any"));
                    }
                }
                Ok(Value::Bool(name == "all"))
            }
            _ => self.b_extreme(name == "max", items, a),
        }
    }

    fn b_extreme(&mut self, max: bool, items: Vec<Value>, a: &Args) -> Res<Value> {
        let key =
            a.kw.iter()
                .find(|(k, _)| k.as_ref() == "key")
                .map(|(_, v)| v.clone());
        let mut best: Option<(Value, Value)> = None;
        for v in items {
            let k = match &key {
                Some(f) => self.call(f, vec![v.clone()], Vec::new())?,
                None => v.clone(),
            };
            let op = if max {
                crate::grammar::ast::CmpOp::Gt
            } else {
                crate::grammar::ast::CmpOp::Lt
            };
            let better = match &best {
                None => true,
                Some((bk, _)) => self.compare(op, &k, bk)?,
            };
            if better {
                best = Some((k, v));
            }
        }
        match best {
            Some((_, v)) => Ok(v),
            None => match a.kw.iter().find(|(k, _)| k.as_ref() == "default") {
                Some((_, d)) => Ok(d.clone()),
                None => self.raise("builtins.ValueError", "max() arg is an empty sequence"),
            },
        }
    }

    fn b_zip(&mut self, a: &Args) -> Res<Value> {
        let seqs = a
            .pos
            .iter()
            .map(|v| self.iterate(v))
            .collect::<Res<Vec<_>>>()?;
        let n = seqs.iter().map(Vec::len).min().unwrap_or(0);
        let rows = (0..n)
            .map(|k| Value::tuple(seqs.iter().map(|s| s[k].clone()).collect()))
            .collect();
        Ok(self.alloc(HeapObj::Iter(rows, 0)))
    }

    fn b_isinstance(&mut self, a: &Args) -> Res<Value> {
        let v = a.or(0, "o", Value::None);
        let class = a.or(1, "cls", Value::None);
        let classes = match &class {
            Value::Tuple(t) => t.to_vec(),
            other => vec![other.clone()],
        };
        let actual = type_of(self, &v);
        Ok(Value::Bool(classes.iter().any(|c| match (c, &actual) {
            (Value::Native(c), Value::Native(t)) => {
                c == t || (c.as_ref() == "builtins.int" && t.as_ref() == "builtins.bool")
            }
            _ => false,
        })))
    }

    fn b_getattr(&mut self, name: &str, a: &Args) -> Res<Value> {
        let o = a.or(0, "o", Value::None);
        let n: Arc<str> = Arc::from(a.or(1, "name", Value::None).as_str().unwrap_or(""));
        let found = self.getattr(&o, &n);
        let missing = matches!(&found, Err(Flow::Exception(_)))
            || matches!(&found, Ok(Value::Method(_)) if !has_method(self, &o, &n));
        if name == "hasattr" {
            return Ok(Value::Bool(!missing));
        }
        match (missing, a.get(2, "default")) {
            (true, Some(d)) => Ok(d),
            (true, None) => self.raise("builtins.AttributeError", format!("no attribute {n}")),
            (false, _) => found,
        }
    }

    fn b_next(&mut self, a: &Args) -> Res<Value> {
        let it = a.or(0, "it", Value::None);
        if let Value::Heap(id) = it
            && let HeapObj::Iter(items, at) = self.obj_mut(id)
            && let Some(v) = items.get(*at).cloned()
        {
            *at += 1;
            return Ok(v);
        }
        match a.get(1, "default") {
            Some(d) => Ok(d),
            None => self.raise("builtins.StopIteration", ""),
        }
    }
}

/// `type(v)`, as a native class name so that `type(p) is dg.Parser` holds.
pub(crate) fn type_of(it: &Interp, v: &Value) -> Value {
    let name = match type_name(it, v) {
        "Parser" => "dcgram.Parser".to_string(),
        "Tok" => "dcgram.Tok".to_string(),
        other => format!("builtins.{other}"),
    };
    Value::Native(Arc::from(name))
}

fn is_callable(it: &Interp, v: &Value) -> bool {
    match v {
        Value::Native(n) => !n.starts_with("module:") && !n.starts_with("unsupported:"),
        Value::Method(_) => true,
        Value::Obj(id) => matches!(it.prog.obj(*id), Some(Obj::Function(_))),
        Value::Heap(id) => matches!(it.obj(*id), HeapObj::Closure(_)),
        _ => false,
    }
}

/// True when a bound-method value names a method the receiver really has.
fn has_method(it: &Interp, recv: &Value, name: &str) -> bool {
    super::methods::method_exists(it, recv, name)
}

fn hex_text(i: i64) -> String {
    if i < 0 {
        format!("-0x{:x}", -(i as i128))
    } else {
        format!("0x{i:x}")
    }
}
