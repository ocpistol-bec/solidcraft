//! Typed node contents: what a node grammar decodes a payload into.

use crate::guid::{Guid, TypeId};
use crate::node::NodeRef;

/// How completely a node was decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeStatus {
    /// The grammar read the whole payload.
    Full,
    /// The grammar stopped early or left bytes; the rest is one `partial_tail` raw field.
    Partial,
    /// No grammar is known for the node type; the payload is one raw field.
    NoGrammar,
}

/// Whether a NodeRef field names a child (owned) or a cross-referenced node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// Cross reference.
    Cross,
    /// Child reference.
    Child,
    /// Parent reference.
    Parent,
    /// Any other code a grammar used.
    Other(i64),
}

impl RefKind {
    pub(crate) fn from_code(code: i64) -> Self {
        match code {
            1 => RefKind::Cross,
            2 => RefKind::Child,
            3 => RefKind::Parent,
            other => RefKind::Other(other),
        }
    }

    pub(crate) fn code(self) -> i64 {
        match self {
            RefKind::Cross => 1,
            RefKind::Child => 2,
            RefKind::Parent => 3,
            RefKind::Other(c) => c,
        }
    }
}

/// A framed list field (spec 13.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ListField {
    /// List code: 2 (list), 3 or 8 (array), 4, 6 or 7 (map).
    pub code: i64,
    /// Element type code (the reference grammar engine's type codes); `None` for lists a
    /// grammar framed by hand.
    pub element_type: Option<i64>,
    /// The four framing bytes (u16 code, u16 0x3000).
    pub framing: Vec<u8>,
    /// The meta words after the count, absent in empty lists.
    pub meta: Option<Vec<u8>>,
    /// Elements; each is the fields one element walker read (a map entry is key, value).
    pub elements: Vec<Vec<Field>>,
}

/// A value of a token type the field model has no variant for, kept as Python held it.
#[derive(Debug, Clone, PartialEq)]
pub enum OtherValue {
    /// None.
    None,
    /// A bool.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Str(String),
    /// Bytes.
    Bytes(Vec<u8>),
    /// A tuple.
    Tuple(Vec<OtherValue>),
}

/// A field value.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// u8.
    U8(u8),
    /// i8.
    I8(i8),
    /// u16.
    U16(u16),
    /// i16.
    I16(i16),
    /// u32.
    U32(u32),
    /// i32.
    I32(i32),
    /// f32.
    F32(f32),
    /// f64.
    F64(f64),
    /// u8 array.
    U8s(Vec<u8>),
    /// i8 array.
    I8s(Vec<i8>),
    /// u16 array.
    U16s(Vec<u16>),
    /// i16 array.
    I16s(Vec<i16>),
    /// u32 array.
    U32s(Vec<u32>),
    /// i32 array.
    I32s(Vec<i32>),
    /// f32 array.
    F32s(Vec<f32>),
    /// f64 array.
    F64s(Vec<f64>),
    /// Bool.
    Bool(bool),
    /// GUID.
    Guid(Guid),
    /// Text16 (UTF-16), without the trailing NUL / newline the decoder strips.
    Text(String),
    /// Text8 that is valid UTF-8, stripped the same way.
    Text8(String),
    /// Text8 that is not UTF-8.
    Text8Bytes(Vec<u8>),
    /// A NodeRef.
    Ref(NodeRef, RefKind),
    /// RGBA colour.
    Color([f32; 4]),
    /// Bytes no field meaning is attached to (their content is the field's `raw`).
    Raw,
    /// A framed list.
    List(ListField),
    /// A token kind the model does not type, with its kind name.
    Other(String, OtherValue),
}

/// One decoded field.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Field name as the grammar (and the specification) names it; `None` for unnamed bytes.
    pub name: Option<String>,
    /// Typed value.
    pub value: FieldValue,
    /// The source bytes (empty for lists, whose bytes are in their elements).
    pub raw: Vec<u8>,
    /// Earlier names of the field, before the specification renamed it (newest first).
    pub aliases: Vec<String>,
}

impl Field {
    /// True when the field is called `name` now or was before a rename.
    pub fn is(&self, name: &str) -> bool {
        self.name.as_deref() == Some(name) || self.aliases.iter().any(|a| a == name)
    }
}

/// One decoded node.
///
/// ```no_run
/// let doc = corliss::Document::open("part.ipt")?;
/// let loc = doc.segment_location("PmDCSegment")?;
/// for node in doc.decode_nodes(&loc)?.nodes {
///     if let Some(name) = node.text("name") {
///         println!("{} {name}", node.type_id);
///     }
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedNode {
    /// TypeID of the node.
    pub type_id: TypeId,
    /// Decode completeness.
    pub status: DecodeStatus,
    /// Fields in payload order.
    pub fields: Vec<Field>,
    /// Why decoding stopped early, for partial nodes.
    pub error: Option<String>,
}

impl DecodedNode {
    /// The last top-level field named `name`, else the last one that had that name before a
    /// rename (the grammar engine's lookup rule).
    pub fn field(&self, name: &str) -> Option<&Field> {
        let named = self
            .fields
            .iter()
            .rev()
            .find(|f| f.name.as_deref() == Some(name));
        named.or_else(|| {
            self.fields
                .iter()
                .rev()
                .find(|f| f.aliases.iter().any(|a| a == name))
        })
    }

    /// The value of field `name`.
    pub fn get(&self, name: &str) -> Option<&FieldValue> {
        self.field(name).map(|f| &f.value)
    }

    /// An integer field as i64 (any integer width, or a bool).
    pub fn int(&self, name: &str) -> Option<i64> {
        self.get(name).and_then(FieldValue::as_int)
    }

    /// A float field as f64.
    pub fn float(&self, name: &str) -> Option<f64> {
        self.get(name).and_then(FieldValue::as_f64)
    }

    /// A text field (Text16 or UTF-8 Text8).
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            FieldValue::Text(s) | FieldValue::Text8(s) => Some(s),
            _ => None,
        }
    }

    /// A NodeRef field.
    pub fn node_ref(&self, name: &str) -> Option<NodeRef> {
        match self.get(name)? {
            FieldValue::Ref(r, _) => Some(*r),
            _ => None,
        }
    }

    /// A list field's elements.
    pub fn list(&self, name: &str) -> Option<&[Vec<Field>]> {
        match self.get(name)? {
            FieldValue::List(l) => Some(&l.elements),
            _ => None,
        }
    }

    /// Every NodeRef in the node, nested lists included, in payload order.
    pub fn refs(&self) -> Vec<(NodeRef, RefKind)> {
        let mut out = Vec::new();
        collect_refs(&self.fields, &mut out);
        out
    }

    /// The node's payload rebuilt from its fields; equals the stored payload for every decoded node.
    pub fn payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.fields.iter().for_each(|f| f.write(&mut out));
        out
    }
}

fn collect_refs(fields: &[Field], out: &mut Vec<(NodeRef, RefKind)>) {
    for f in fields {
        match &f.value {
            FieldValue::Ref(r, k) => out.push((*r, *k)),
            FieldValue::List(l) => l.elements.iter().for_each(|el| collect_refs(el, out)),
            _ => {}
        }
    }
}

impl Field {
    /// Appends the field's bytes (lists rebuilt from framing, count, meta and elements).
    pub fn write(&self, out: &mut Vec<u8>) {
        let FieldValue::List(l) = &self.value else {
            out.extend_from_slice(&self.raw);
            return;
        };
        out.extend_from_slice(&l.framing);
        let mut body = Vec::new();
        l.elements.iter().flatten().for_each(|f| f.write(&mut body));
        let count = if l.element_type == Some(0x10) {
            body.len()
        } else {
            l.elements.len()
        };
        out.extend_from_slice(&(count as u32).to_le_bytes());
        if count == 0 {
            return;
        }
        if let Some(meta) = &l.meta {
            out.extend_from_slice(meta);
        }
        out.extend_from_slice(&body);
    }
}

impl FieldValue {
    /// An integer value (scalars and bools).
    pub fn as_int(&self) -> Option<i64> {
        Some(match self {
            FieldValue::U8(v) => i64::from(*v),
            FieldValue::I8(v) => i64::from(*v),
            FieldValue::U16(v) => i64::from(*v),
            FieldValue::I16(v) => i64::from(*v),
            FieldValue::U32(v) => i64::from(*v),
            FieldValue::I32(v) => i64::from(*v),
            FieldValue::Bool(b) => i64::from(*b),
            FieldValue::Ref(r, _) => i64::from(r.0),
            _ => return None,
        })
    }

    /// A float value.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            FieldValue::F32(v) => Some(f64::from(*v)),
            FieldValue::F64(v) => Some(*v),
            _ => self.as_int().map(|i| i as f64),
        }
    }

    /// A float array as f64 values.
    pub fn as_f64s(&self) -> Option<Vec<f64>> {
        match self {
            FieldValue::F64s(v) => Some(v.clone()),
            FieldValue::F32s(v) => Some(v.iter().map(|x| f64::from(*x)).collect()),
            _ => None,
        }
    }

    /// An integer array as i64 values.
    pub fn as_ints(&self) -> Option<Vec<i64>> {
        Some(match self {
            FieldValue::U8s(v) => v.iter().map(|x| i64::from(*x)).collect(),
            FieldValue::I8s(v) => v.iter().map(|x| i64::from(*x)).collect(),
            FieldValue::U16s(v) => v.iter().map(|x| i64::from(*x)).collect(),
            FieldValue::I16s(v) => v.iter().map(|x| i64::from(*x)).collect(),
            FieldValue::U32s(v) => v.iter().map(|x| i64::from(*x)).collect(),
            FieldValue::I32s(v) => v.iter().map(|x| i64::from(*x)).collect(),
            _ => return None,
        })
    }
}
