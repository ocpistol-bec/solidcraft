//! Methods of strings, bytes, tuples, lists, dicts, sets and tokens.

use super::args::Args;
use crate::grammar::interp::{Flow, HeapObj, Interp, Res};
use crate::grammar::snapshot::Obj;
use crate::grammar::value::Value;

const STR_METHODS: [&str; 22] = [
    "startswith",
    "endswith",
    "split",
    "rsplit",
    "partition",
    "rpartition",
    "find",
    "rfind",
    "index",
    "lower",
    "upper",
    "strip",
    "rstrip",
    "lstrip",
    "replace",
    "join",
    "encode",
    "isdigit",
    "isalpha",
    "isascii",
    "count",
    "format",
];
const LIST_METHODS: [&str; 11] = [
    "append", "extend", "insert", "pop", "index", "count", "remove", "reverse", "sort", "copy",
    "clear",
];
const DICT_METHODS: [&str; 9] = [
    "get",
    "items",
    "keys",
    "values",
    "setdefault",
    "update",
    "pop",
    "copy",
    "clear",
];

/// True when `recv` has a method `name`.
pub(crate) fn method_exists(it: &Interp, recv: &Value, name: &str) -> bool {
    match recv {
        Value::Str(_) => STR_METHODS.contains(&name),
        Value::Bytes(_) => matches!(
            name,
            "hex" | "find" | "decode" | "startswith" | "count" | "index"
        ),
        Value::Tuple(_) => matches!(name, "index" | "count"),
        Value::Obj(id) => match it.prog.obj(*id) {
            Some(Obj::List(_)) => matches!(name, "index" | "count" | "copy"),
            Some(Obj::Dict(..)) => matches!(name, "get" | "items" | "keys" | "values" | "copy"),
            _ => false,
        },
        Value::Heap(id) => match it.obj(*id) {
            HeapObj::List(_) => LIST_METHODS.contains(&name),
            HeapObj::Dict(_) => DICT_METHODS.contains(&name),
            HeapObj::Set(_) => matches!(name, "add" | "discard" | "update"),
            HeapObj::Token(_) => matches!(name, "serialize" | "had" | "is_"),
            HeapObj::Parser(_) => true,
            _ => false,
        },
        _ => false,
    }
}

impl Interp {
    pub fn method(&mut self, recv: Value, name: &str, a: Args) -> Res<Value> {
        match &recv {
            Value::Str(s) => self.str_method(s.clone(), name, &a),
            Value::Bytes(b) => self.bytes_method(b.clone(), name, &a),
            Value::Tuple(_) => self.seq_method(&recv, name, &a),
            Value::Obj(id) => match self.prog.obj(*id) {
                Some(Obj::Dict(..)) => self.dict_method(&recv, name, &a),
                _ => self.seq_method(&recv, name, &a),
            },
            Value::Heap(id) => self.heap_method(*id, recv.clone(), name, a),
            _ => Err(Flow::Fatal(format!(
                "method {name} on this value is not implemented"
            ))),
        }
    }

    fn heap_method(&mut self, id: u32, recv: Value, name: &str, a: Args) -> Res<Value> {
        match self.obj(id) {
            HeapObj::Parser(_) => self.parser_method(id, name, a),
            HeapObj::Token(_) => self.token_method(id, name, &a),
            HeapObj::List(_) => self.list_method(id, &recv, name, &a),
            HeapObj::Dict(_) => self.dict_method(&recv, name, &a),
            HeapObj::Set(_) => self.set_method(id, name, &a),
            _ => Err(Flow::Fatal(format!(
                "method {name} on this object is not implemented"
            ))),
        }
    }

    fn token_method(&mut self, id: u32, name: &str, a: &Args) -> Res<Value> {
        match name {
            "serialize" => Ok(Value::bytes(&self.serialize_token(id)?)),
            "had" | "is_" => {
                let n = a.or(0, "name", Value::None);
                let Some(t) = self.token(id) else {
                    return Ok(Value::Bool(false));
                };
                let named = name == "is_" && t.name.as_str().is_some_and(|x| Some(x) == n.as_str());
                Ok(Value::Bool(named || n.as_str().is_some_and(|x| t.had(x))))
            }
            other => Err(Flow::Fatal(format!("Tok.{other} is not implemented"))),
        }
    }

    fn seq_method(&mut self, recv: &Value, name: &str, a: &Args) -> Res<Value> {
        let items = self.iterate(recv)?;
        let x = a.or(0, "x", Value::None);
        match name {
            "index" => match items.iter().position(|v| self.py_eq(v, &x)) {
                Some(p) => Ok(Value::Int(p as i64)),
                None => self.raise("builtins.ValueError", "value is not in list"),
            },
            "count" => Ok(Value::Int(
                items.iter().filter(|v| self.py_eq(v, &x)).count() as i64,
            )),
            "copy" => Ok(self.new_list(items)),
            other => Err(Flow::Fatal(format!(
                "method {other} on a sequence is not implemented"
            ))),
        }
    }

    fn list_method(&mut self, id: u32, recv: &Value, name: &str, a: &Args) -> Res<Value> {
        let x = a.or(0, "x", Value::None);
        match name {
            "append" => self.with_list(id, |l| l.push(x)),
            "extend" => {
                let items = self.iterate(&x)?;
                self.with_list(id, |l| l.extend(items))
            }
            "insert" => {
                let v = a.or(1, "v", Value::None);
                let n = self.len_of(recv).unwrap_or(0) as i64;
                let at = x.as_int().unwrap_or(0);
                let at = (if at < 0 { at + n } else { at }).clamp(0, n) as usize;
                self.with_list(id, |l| l.insert(at, v))
            }
            "pop" => self.list_pop(id, recv, a.int(0, "i").unwrap_or(-1)),
            "remove" => {
                let items = self.iterate(recv)?;
                let Some(p) = items.iter().position(|v| self.py_eq(v, &x)) else {
                    return self.raise("builtins.ValueError", "list.remove(x): x not in list");
                };
                self.with_list(id, |l| {
                    l.remove(p);
                })
            }
            "reverse" => self.with_list(id, |l| l.reverse()),
            "clear" => self.with_list(id, |l| l.clear()),
            "sort" => {
                let mut items = self.iterate(recv)?;
                self.sort_values(&mut items, a.get(0, "key"), a.get(1, "reverse"))?;
                self.with_list(id, |l| *l = items)
            }
            _ => self.seq_method(recv, name, a),
        }
    }

    fn with_list(&mut self, id: u32, f: impl FnOnce(&mut Vec<Value>)) -> Res<Value> {
        if let HeapObj::List(l) = self.obj_mut(id) {
            f(l);
        }
        Ok(Value::None)
    }

    fn list_pop(&mut self, id: u32, recv: &Value, i: i64) -> Res<Value> {
        let n = self.len_of(recv).unwrap_or(0) as i64;
        let at = if i < 0 { i + n } else { i };
        if at < 0 || at >= n {
            return self.raise("builtins.IndexError", "pop index out of range");
        }
        match self.obj_mut(id) {
            HeapObj::List(l) => Ok(l.remove(at as usize)),
            _ => Ok(Value::None),
        }
    }

    fn dict_method(&mut self, recv: &Value, name: &str, a: &Args) -> Res<Value> {
        let key = a.or(0, "k", Value::None);
        match name {
            "get" => {
                let has = self.contains(recv, &key)?;
                if has {
                    self.getitem(recv, &key)
                } else {
                    Ok(a.or(1, "d", Value::None))
                }
            }
            "items" | "keys" | "values" => {
                let pairs = self.dict_items(recv)?;
                let out = pairs
                    .into_iter()
                    .map(|(k, v)| match name {
                        "items" => Value::tuple(vec![k, v]),
                        "keys" => k,
                        _ => v,
                    })
                    .collect();
                Ok(self.new_list(out))
            }
            "copy" => {
                let pairs = self.dict_items(recv)?;
                self.new_dict(pairs)
            }
            _ => self.dict_mutate(recv, name, key, a),
        }
    }

    fn dict_mutate(&mut self, recv: &Value, name: &str, key: Value, a: &Args) -> Res<Value> {
        let Value::Heap(id) = recv else {
            return Err(Flow::Fatal(format!("dict.{name} on a snapshot dict")));
        };
        let id = *id;
        match name {
            "setdefault" => {
                if self.contains(recv, &key)? {
                    return self.getitem(recv, &key);
                }
                let d = a.or(1, "d", Value::None);
                self.setitem(recv, &key, d.clone())?;
                Ok(d)
            }
            "update" => {
                let mut pairs = self.dict_items(&key)?;
                pairs.extend(a.kw.iter().map(|(k, v)| (Value::Str(k.clone()), v.clone())));
                for (k, v) in pairs {
                    self.setitem(recv, &k, v)?;
                }
                Ok(Value::None)
            }
            "pop" => {
                let removed = match self.obj_mut(id) {
                    HeapObj::Dict(d) => d.remove(&key),
                    _ => None,
                };
                match (removed, a.get(1, "d")) {
                    (Some(v), _) | (None, Some(v)) => Ok(v),
                    (None, None) => self.raise("builtins.KeyError", "key"),
                }
            }
            "clear" => {
                if let HeapObj::Dict(d) = self.obj_mut(id) {
                    *d = Default::default();
                }
                Ok(Value::None)
            }
            other => Err(Flow::Fatal(format!("dict.{other} is not implemented"))),
        }
    }

    fn set_method(&mut self, id: u32, name: &str, a: &Args) -> Res<Value> {
        let x = a.or(0, "x", Value::None);
        let items = if name == "update" {
            self.iterate(&x)?
        } else {
            vec![x.clone()]
        };
        let HeapObj::Set(d) = self.obj_mut(id) else {
            return Ok(Value::None);
        };
        match name {
            "add" | "update" => {
                for v in items {
                    d.insert(v, Value::None)?;
                }
            }
            "discard" => {
                d.remove(&x);
            }
            other => return Err(Flow::Fatal(format!("set.{other} is not implemented"))),
        }
        Ok(Value::None)
    }
}
