//! Runtime values of the grammar interpreter.
//!
//! Immutable values (numbers, strings, bytes, tuples) are held directly; snapshot containers
//! and functions are referenced by object id; mutable runtime objects (lists, dicts, tokens,
//! parsers, frames) live in the session heap and are referenced by heap id. No value owns a
//! reference-counted cell, so a session is `Send`.

use std::sync::Arc;

/// Index of an object in the snapshot.
pub(crate) type ObjId = u32;
/// Index of an object in a session heap.
pub(crate) type HeapId = u32;

/// A Python value.
#[derive(Debug, Clone)]
pub(crate) enum Value {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Arc<str>),
    Bytes(Arc<[u8]>),
    Tuple(Arc<[Value]>),
    /// A snapshot list, dict, set, function, namespace or native.
    Obj(ObjId),
    /// A session heap object.
    Heap(HeapId),
    /// A native callable or class, by qualified name (e.g. `builtins.len`).
    Native(Arc<str>),
    /// A method bound to its receiver.
    Method(Box<(Value, Arc<str>)>),
}

impl Value {
    /// A string value.
    pub fn str(s: &str) -> Value {
        Value::Str(Arc::from(s))
    }

    /// A tuple value.
    pub fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(Arc::from(items))
    }

    /// A bytes value.
    pub fn bytes(b: &[u8]) -> Value {
        Value::Bytes(Arc::from(b))
    }

    /// The integer of an `Int` or `Bool`.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }

    /// The text of a `Str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// True for `None`.
    pub fn is_none(&self) -> bool {
        matches!(self, Value::None)
    }
}

/// A hashable dict key (Python hashing: `True == 1`, `1 == 1.0`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    None,
    Int(i64),
    Float(u64),
    Str(Arc<str>),
    Bytes(Arc<[u8]>),
    Tuple(Vec<Key>),
    Obj(ObjId),
    Heap(HeapId),
    Native(Arc<str>),
}

impl Key {
    /// The key of a value, or `None` for an unhashable one.
    pub fn of(v: &Value) -> Option<Key> {
        Some(match v {
            Value::None => Key::None,
            Value::Bool(b) => Key::Int(i64::from(*b)),
            Value::Int(i) => Key::Int(*i),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Key::Int(*f as i64),
            Value::Float(f) => Key::Float(f.to_bits()),
            Value::Str(s) => Key::Str(s.clone()),
            Value::Bytes(b) => Key::Bytes(b.clone()),
            Value::Tuple(t) => Key::Tuple(t.iter().map(Key::of).collect::<Option<Vec<_>>>()?),
            Value::Obj(id) => Key::Obj(*id),
            Value::Heap(id) => Key::Heap(*id),
            Value::Native(n) => Key::Native(n.clone()),
            Value::Method(_) => return None,
        })
    }
}
