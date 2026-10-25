//! Attribute and item access, iteration, `len` and `in`.

use std::sync::Arc;

use super::{Flow, HeapObj, Interp, Res};
use crate::grammar::snapshot::Obj;
use crate::grammar::value::Value;

impl Interp {
    pub fn getattr(&mut self, obj: &Value, name: &Arc<str>) -> Res<Value> {
        match obj {
            Value::Obj(id) => self.snap_attr(*id, name),
            Value::Heap(id) => self.heap_attr(*id, name),
            Value::Native(n) => Ok(super::super::native::native_attr(self, n, name)),
            _ => Ok(Value::Method(Box::new((obj.clone(), name.clone())))),
        }
    }

    fn snap_attr(&mut self, id: u32, name: &Arc<str>) -> Res<Value> {
        let prog = self.prog;
        match prog.obj(id) {
            Some(Obj::Namespace(_)) => match self.global(id, name) {
                Some(v) => Ok(v),
                None => self.raise(
                    "builtins.AttributeError",
                    format!("namespace has no attribute {name}"),
                ),
            },
            Some(Obj::Function(f)) => Ok(match name.as_ref() {
                "__name__" => Value::Str(f.name.clone()),
                "__qualname__" => Value::Str(f.qualname.clone()),
                "__module__" => Value::Str(f.module.clone()),
                "__doc__" => Value::None,
                _ => match f.attrs.get(name) {
                    Some(v) => prog.value(*v),
                    None => {
                        return self.raise(
                            "builtins.AttributeError",
                            format!("function has no attribute {name}"),
                        );
                    }
                },
            }),
            _ => Ok(Value::Method(Box::new((Value::Obj(id), name.clone())))),
        }
    }

    fn heap_attr(&mut self, id: u32, name: &Arc<str>) -> Res<Value> {
        match self.obj(id) {
            HeapObj::Token(t) => match t.field(name) {
                Some(v) => Ok(v),
                None => Ok(Value::Method(Box::new((Value::Heap(id), name.clone())))),
            },
            HeapObj::Parser(p) => match p.field(name) {
                Some(v) => Ok(v),
                None => Ok(Value::Method(Box::new((Value::Heap(id), name.clone())))),
            },
            HeapObj::Attrs(map) => match map.get(name) {
                Some(v) => Ok(v.clone()),
                None => self.ctx_attr(id, name),
            },
            HeapObj::Closure(c) => match name.as_ref() {
                "__name__" | "__qualname__" => Ok(Value::Str(c.def.name.clone())),
                "__module__" => Ok(Value::str("")),
                _ => match c.attrs.get(name) {
                    Some(v) => Ok(v.clone()),
                    None => self.raise(
                        "builtins.AttributeError",
                        format!("function has no attribute {name}"),
                    ),
                },
            },
            HeapObj::Exception(e) => Ok(match name.as_ref() {
                "args" => Value::tuple(vec![Value::str(&e.message)]),
                _ => Value::Method(Box::new((Value::Heap(id), name.clone()))),
            }),
            _ => Ok(Value::Method(Box::new((Value::Heap(id), name.clone())))),
        }
    }

    /// Computed attributes of the parse context (`ctx.block`).
    fn ctx_attr(&mut self, id: u32, name: &Arc<str>) -> Res<Value> {
        if name.as_ref() == "block" {
            let file = match self.obj(id) {
                HeapObj::Attrs(m) => m.get("file").and_then(Value::as_int).unwrap_or(2027),
                _ => 2027,
            };
            return Ok(Value::Int(if file > 2010 { 0 } else { 4 }));
        }
        if matches!(name.as_ref(), "start" | "stop" | "step") {
            return Ok(Value::None);
        }
        self.raise(
            "builtins.AttributeError",
            format!("object has no attribute {name}"),
        )
    }

    pub fn setattr(&mut self, obj: &Value, name: &Arc<str>, v: Value) -> Res<()> {
        let Value::Heap(id) = obj else {
            return Err(Flow::Fatal(format!(
                "cannot set attribute {name} on a snapshot value"
            )));
        };
        match self.obj_mut(*id) {
            HeapObj::Token(t) => t.set_field(name, v),
            HeapObj::Parser(p) => p.set_field(name, v),
            HeapObj::Attrs(map) => {
                map.insert(name.clone(), v);
                Ok(())
            }
            HeapObj::Closure(c) => {
                c.attrs.insert(name.clone(), v);
                Ok(())
            }
            _ => Err(Flow::Fatal(format!("cannot set attribute {name}"))),
        }
    }

    /// Materialises any iterable.
    pub fn iterate(&mut self, v: &Value) -> Res<Vec<Value>> {
        let prog = self.prog;
        Ok(match v {
            Value::Tuple(t) => t.to_vec(),
            Value::Str(s) => s.chars().map(|c| Value::str(&c.to_string())).collect(),
            Value::Bytes(b) => b.iter().map(|x| Value::Int(i64::from(*x))).collect(),
            Value::Obj(id) => match prog.obj(*id) {
                Some(Obj::List(items) | Obj::Set(items)) => {
                    items.iter().map(|i| prog.value(*i)).collect()
                }
                Some(Obj::Dict(items, _)) => items.iter().map(|(k, _)| prog.value(*k)).collect(),
                _ => return self.raise("builtins.TypeError", "object is not iterable"),
            },
            Value::Heap(id) => match self.obj(*id) {
                HeapObj::List(items) => items.clone(),
                HeapObj::Dict(d) | HeapObj::Set(d) => {
                    d.items.iter().map(|(k, _)| k.clone()).collect()
                }
                HeapObj::Iter(items, at) => items[*at..].to_vec(),
                _ => return self.raise("builtins.TypeError", "object is not iterable"),
            },
            _ => return self.raise("builtins.TypeError", "object is not iterable"),
        })
    }

    /// (key, value) pairs of a dict.
    pub fn dict_items(&mut self, v: &Value) -> Res<Vec<(Value, Value)>> {
        let prog = self.prog;
        Ok(match v {
            Value::Obj(id) => match prog.obj(*id) {
                Some(Obj::Dict(items, _)) => items
                    .iter()
                    .map(|(k, x)| (prog.value(*k), prog.value(*x)))
                    .collect(),
                _ => return self.raise("builtins.TypeError", "not a dict"),
            },
            Value::Heap(id) => match self.obj(*id) {
                HeapObj::Dict(d) => d.items.clone(),
                _ => return self.raise("builtins.TypeError", "not a dict"),
            },
            _ => return self.raise("builtins.TypeError", "not a dict"),
        })
    }

    pub fn len_of(&self, v: &Value) -> Option<usize> {
        Some(match v {
            Value::Str(s) => s.chars().count(),
            Value::Bytes(b) => b.len(),
            Value::Tuple(t) => t.len(),
            Value::Obj(id) => match self.prog.obj(*id)? {
                Obj::List(items) | Obj::Set(items) => items.len(),
                Obj::Dict(items, _) => items.len(),
                _ => return None,
            },
            Value::Heap(id) => match self.obj(*id) {
                HeapObj::List(items) => items.len(),
                HeapObj::Dict(d) | HeapObj::Set(d) => d.items.len(),
                HeapObj::Iter(items, at) => items.len() - at,
                _ => return None,
            },
            _ => return None,
        })
    }

    /// `item in container`.
    pub fn contains(&mut self, container: &Value, item: &Value) -> Res<bool> {
        match container {
            Value::Str(s) => Ok(item.as_str().is_some_and(|x| s.contains(x))),
            Value::Bytes(b) => Ok(match item {
                Value::Bytes(x) => x.is_empty() || b.windows(x.len()).any(|w| w == x.as_ref()),
                _ => item.as_int().is_some_and(|i| b.contains(&(i as u8))),
            }),
            Value::Obj(id) if matches!(self.prog.obj(*id), Some(Obj::Dict(..))) => {
                Ok(self.prog.dict_get(*id, item).is_some())
            }
            Value::Heap(id) => match self.obj(*id) {
                HeapObj::Dict(d) | HeapObj::Set(d) => Ok(d.get(item).is_some()),
                _ => self.contains_seq(container, item),
            },
            _ => self.contains_seq(container, item),
        }
    }

    fn contains_seq(&mut self, container: &Value, item: &Value) -> Res<bool> {
        let items = self.iterate(container)?;
        Ok(items.iter().any(|x| self.py_eq(x, item)))
    }

    pub fn getitem(&mut self, obj: &Value, index: &Value) -> Res<Value> {
        if let Some(range) = self.slice_bounds(index, obj)? {
            return self.get_slice(obj, range);
        }
        let prog = self.prog;
        match obj {
            Value::Obj(id) if matches!(prog.obj(*id), Some(Obj::Dict(..))) => {
                match prog.dict_get(*id, index) {
                    Some(v) => Ok(v),
                    None => self.raise("builtins.KeyError", self_repr(index)),
                }
            }
            Value::Heap(id) if matches!(self.obj(*id), HeapObj::Dict(_)) => {
                let HeapObj::Dict(d) = self.obj(*id) else {
                    return Ok(Value::None);
                };
                match d.get(index) {
                    Some(v) => Ok(v.clone()),
                    None => self.raise("builtins.KeyError", self_repr(index)),
                }
            }
            _ => self.seq_item(obj, index),
        }
    }

    fn seq_item(&mut self, obj: &Value, index: &Value) -> Res<Value> {
        let Some(i) = index.as_int() else {
            return self.raise("builtins.TypeError", "indices must be integers");
        };
        let n = self.len_of(obj).unwrap_or(0) as i64;
        let at = if i < 0 { i + n } else { i };
        if at < 0 || at >= n {
            return self.raise("builtins.IndexError", "index out of range");
        }
        let at = at as usize;
        let prog = self.prog;
        Ok(match obj {
            Value::Bytes(b) => Value::Int(i64::from(b[at])),
            Value::Tuple(t) => t[at].clone(),
            Value::Str(s) => Value::str(&s.chars().nth(at).map(String::from).unwrap_or_default()),
            Value::Obj(id) => match prog.obj(*id) {
                Some(Obj::List(items)) => prog.value(items[at]),
                _ => Value::None,
            },
            Value::Heap(id) => match self.obj(*id) {
                HeapObj::List(items) => items[at].clone(),
                _ => Value::None,
            },
            _ => Value::None,
        })
    }

    /// The (start, stop) of a step-1 slice over `obj`, or `None` when `index` is not a slice.
    fn slice_bounds(&mut self, index: &Value, obj: &Value) -> Res<Option<(usize, usize)>> {
        let Value::Heap(id) = index else {
            return Ok(None);
        };
        let HeapObj::Attrs(m) = self.obj(*id) else {
            return Ok(None);
        };
        if m.get("__kind__").and_then(Value::as_str) != Some("slice") {
            return Ok(None);
        }
        let get = |k: &str| m.get(k).and_then(Value::as_int);
        let (start, stop, step) = (get("start"), get("stop"), get("step"));
        if step.is_some_and(|s| s != 1) {
            return Err(Flow::Fatal("slices with a step are not supported".into()));
        }
        let n = self.len_of(obj).unwrap_or(0) as i64;
        let clamp = |v: Option<i64>, default: i64| -> usize {
            let v = v.unwrap_or(default);
            let v = if v < 0 { v + n } else { v };
            v.clamp(0, n) as usize
        };
        let (a, b) = (clamp(start, 0), clamp(stop, n));
        Ok(Some((a, b.max(a))))
    }

    fn get_slice(&mut self, obj: &Value, (a, b): (usize, usize)) -> Res<Value> {
        Ok(match obj {
            Value::Bytes(x) => Value::bytes(&x[a..b]),
            Value::Str(s) => Value::str(&s.chars().skip(a).take(b - a).collect::<String>()),
            Value::Tuple(t) => Value::tuple(t[a..b].to_vec()),
            _ => {
                let items = self.iterate(obj)?;
                self.new_list(items[a..b].to_vec())
            }
        })
    }

    pub fn setitem(&mut self, obj: &Value, index: &Value, v: Value) -> Res<()> {
        let Value::Heap(id) = obj else {
            return Err(Flow::Fatal("item assignment on a snapshot value".into()));
        };
        if let Some((a, b)) = self.slice_bounds(index, obj)? {
            let items = self.iterate(&v)?;
            if let HeapObj::List(list) = self.obj_mut(*id) {
                list.splice(a..b, items);
            }
            return Ok(());
        }
        let n = self.len_of(obj).unwrap_or(0) as i64;
        match self.obj_mut(*id) {
            HeapObj::Dict(d) => d.insert(index.clone(), v),
            HeapObj::List(list) => {
                let i = index.as_int().unwrap_or(i64::MAX);
                let at = if i < 0 { i + n } else { i };
                match list.get_mut(at as usize).filter(|_| at >= 0) {
                    Some(slot) => {
                        *slot = v;
                        Ok(())
                    }
                    None => self.raise("builtins.IndexError", "list assignment index out of range"),
                }
            }
            _ => Err(Flow::Fatal("item assignment on this object".into())),
        }
    }

    pub fn delitem(&mut self, obj: &Value, index: &Value) -> Res<()> {
        let Value::Heap(id) = obj else {
            return Err(Flow::Fatal("del on a snapshot value".into()));
        };
        let range = match self.slice_bounds(index, obj)? {
            Some(r) => r,
            None => {
                let n = self.len_of(obj).unwrap_or(0) as i64;
                let i = index.as_int().unwrap_or(0);
                let at = (if i < 0 { i + n } else { i }).max(0) as usize;
                (at, at + 1)
            }
        };
        match self.obj_mut(*id) {
            HeapObj::List(list) => {
                let end = range.1.min(list.len());
                list.drain(range.0.min(end)..end);
                Ok(())
            }
            HeapObj::Dict(d) => {
                d.remove(index);
                Ok(())
            }
            _ => Err(Flow::Fatal("del on this object".into())),
        }
    }
}

fn self_repr(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("'{s}'"),
        Value::Int(i) => i.to_string(),
        _ => "key".into(),
    }
}
