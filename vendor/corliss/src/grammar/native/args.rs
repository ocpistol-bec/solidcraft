//! Positional and keyword arguments of a native call.

use std::sync::Arc;

use crate::grammar::value::Value;

/// The arguments of a native call.
pub(crate) struct Args {
    pub pos: Vec<Value>,
    pub kw: Vec<(Arc<str>, Value)>,
}

impl Args {
    pub fn new(pos: Vec<Value>, kw: Vec<(Arc<str>, Value)>) -> Self {
        Args { pos, kw }
    }

    /// Parameter `index`, named `name`, as Python binds it.
    pub fn get(&self, index: usize, name: &str) -> Option<Value> {
        if let Some(v) = self.pos.get(index) {
            return Some(v.clone());
        }
        self.kw
            .iter()
            .find(|(k, _)| k.as_ref() == name)
            .map(|(_, v)| v.clone())
    }

    /// Like [`Args::get`] but with a default.
    pub fn or(&self, index: usize, name: &str, default: Value) -> Value {
        self.get(index, name).unwrap_or(default)
    }

    pub fn int(&self, index: usize, name: &str) -> Option<i64> {
        self.get(index, name).and_then(|v| v.as_int())
    }
}
