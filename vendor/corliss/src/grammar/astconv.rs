//! Reads the snapshot's generic AST nodes and converts them to [`super::ast`] nodes.

use std::collections::HashMap;
use std::sync::Arc;

use super::ast::*;
use super::value::Value;
use crate::bytes::ByteReader;
use crate::error::{CorlissError, Result};

const STRUCTURE: &str = "grammar snapshot AST";

/// A field of a generic node.
pub(crate) enum GField {
    None,
    Node(Box<GNode>),
    List(Vec<GField>),
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    /// A constant the interpreter cannot hold (big integers, Ellipsis).
    Unsupported,
}

/// A generic node: kind index and fields in the kind's field order.
pub(crate) struct GNode {
    pub kind: u16,
    pub fields: Vec<GField>,
}

/// The node kinds of the snapshot and a reader/converter over them.
pub(crate) struct Kinds {
    names: Vec<String>,
    fields: Vec<HashMap<String, usize>>,
    counts: Vec<usize>,
}

impl Kinds {
    pub fn read(r: &mut ByteReader<'_>) -> Result<Self> {
        let n = r.count("kinds", 8)?;
        let mut kinds = Kinds {
            names: Vec::new(),
            fields: Vec::new(),
            counts: Vec::new(),
        };
        for _ in 0..n {
            kinds.names.push(read_str(r)?);
            let m = r.count("fields", 4)?;
            let mut map = HashMap::new();
            for k in 0..m {
                map.insert(read_str(r)?, k);
            }
            kinds.fields.push(map);
            kinds.counts.push(m);
        }
        Ok(kinds)
    }

    pub fn read_node(&self, r: &mut ByteReader<'_>) -> Result<GNode> {
        let at = r.position();
        let kind = r.u16("kind")?;
        let count = *self.counts.get(kind as usize).ok_or_else(|| {
            CorlissError::invalid(STRUCTURE, "kind", at, kind, "a known node kind")
        })?;
        r.u32("line")?;
        let fields = (0..count)
            .map(|_| self.read_field(r))
            .collect::<Result<Vec<_>>>()?;
        Ok(GNode { kind, fields })
    }

    pub(crate) fn read_field(&self, r: &mut ByteReader<'_>) -> Result<GField> {
        let at = r.position();
        Ok(match r.u8("field")? {
            0 => GField::None,
            1 => GField::Node(Box::new(self.read_node(r)?)),
            2 => {
                let n = r.count("list", 1)?;
                GField::List((0..n).map(|_| self.read_field(r)).collect::<Result<_>>()?)
            }
            3 => GField::Bool(true),
            4 => GField::Bool(false),
            5 => GField::Int(r.i64("int")?),
            6 => GField::Float(r.f64("float")?),
            7 => GField::Str(read_str(r)?),
            8 => GField::Bytes(r.text8("bytes")?),
            9 => {
                read_str(r)?;
                GField::Unsupported
            }
            10 => GField::Unsupported,
            tag => {
                return Err(CorlissError::invalid(
                    STRUCTURE,
                    "field",
                    at,
                    tag,
                    "a field tag 0-10",
                ));
            }
        })
    }

    pub(crate) fn name(&self, n: &GNode) -> &str {
        &self.names[n.kind as usize]
    }

    pub(crate) fn get<'a>(&self, n: &'a GNode, field: &str) -> &'a GField {
        self.fields[n.kind as usize]
            .get(field)
            .and_then(|k| n.fields.get(*k))
            .unwrap_or(&GField::None)
    }

    pub(crate) fn node<'a>(&self, n: &'a GNode, field: &str) -> Option<&'a GNode> {
        match self.get(n, field) {
            GField::Node(x) => Some(x),
            _ => None,
        }
    }

    pub(crate) fn nodes<'a>(&self, n: &'a GNode, field: &str) -> Vec<&'a GNode> {
        match self.get(n, field) {
            GField::List(items) => items
                .iter()
                .filter_map(|f| match f {
                    GField::Node(x) => Some(&**x),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn text(&self, n: &GNode, field: &str) -> Option<Arc<str>> {
        match self.get(n, field) {
            GField::Str(s) => Some(Arc::from(s.as_str())),
            _ => None,
        }
    }

    pub(crate) fn texts(&self, n: &GNode, field: &str) -> Vec<Arc<str>> {
        match self.get(n, field) {
            GField::List(items) => items
                .iter()
                .filter_map(|f| match f {
                    GField::Str(s) => Some(Arc::from(s.as_str())),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

pub(crate) fn read_str(r: &mut ByteReader<'_>) -> Result<String> {
    Ok(String::from_utf8_lossy(&r.text8("str")?).into_owned())
}

pub(crate) fn unsupported(what: &str) -> CorlissError {
    CorlissError::invalid(
        STRUCTURE,
        "node",
        0,
        what,
        "a node kind the interpreter supports",
    )
}

pub(crate) fn cmp_op(name: &str) -> Result<CmpOp> {
    Ok(match name {
        "Eq" => CmpOp::Eq,
        "NotEq" => CmpOp::NotEq,
        "Lt" => CmpOp::Lt,
        "LtE" => CmpOp::LtE,
        "Gt" => CmpOp::Gt,
        "GtE" => CmpOp::GtE,
        "Is" => CmpOp::Is,
        "IsNot" => CmpOp::IsNot,
        "In" => CmpOp::In,
        "NotIn" => CmpOp::NotIn,
        other => return Err(unsupported(other)),
    })
}

pub(crate) fn constant(f: &GField) -> Result<Value> {
    Ok(match f {
        GField::None => Value::None,
        GField::Bool(b) => Value::Bool(*b),
        GField::Int(i) => Value::Int(*i),
        GField::Float(x) => Value::Float(*x),
        GField::Str(s) => Value::str(s),
        GField::Bytes(b) => Value::bytes(b),
        _ => return Err(unsupported("constant")),
    })
}
