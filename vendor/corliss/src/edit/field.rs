//! Changing decoded fields: a new value re-encodes the field's bytes the way the reference
//! engine serializes a changed token.

use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, Field, FieldValue, ListField, RefKind};
use crate::node::NodeRef;

const STRUCTURE: &str = "node field";

impl Field {
    /// A new field whose bytes encode `value` (a ref, scalar, text …).
    ///
    /// ```
    /// use corliss::grammar::{Field, FieldValue};
    /// let f = Field::encoded("count", FieldValue::U32(7)).unwrap();
    /// assert_eq!(f.raw, 7u32.to_le_bytes());
    /// ```
    pub fn encoded(name: &str, value: FieldValue) -> Result<Field> {
        let mut f = Field {
            name: Some(name.to_string()),
            value: FieldValue::Raw,
            raw: Vec::new(),
            aliases: Vec::new(),
        };
        f.set(value)?;
        Ok(f)
    }

    /// Replaces the value and re-encodes the bytes; lists and raw fields cannot be set this way.
    pub fn set(&mut self, value: FieldValue) -> Result<()> {
        self.raw = encode(&value).ok_or_else(|| {
            CorlissError::invalid(
                STRUCTURE,
                "value",
                0,
                format!("{value:?}"),
                "a scalar, array, text, GUID, colour or NodeRef",
            )
        })?;
        self.value = value;
        Ok(())
    }

    /// Rewrites every NodeRef in the field (lists included) through `map` (old → new index).
    pub fn remap_refs(&mut self, map: &dyn Fn(usize) -> Option<usize>) {
        match &mut self.value {
            FieldValue::Ref(r, kind) => {
                let Some(new) = r.index().and_then(map) else {
                    return;
                };
                let kind = *kind;
                let value = (new as u32 & 0x7FFF_FFFF) | (r.0 & 0x8000_0000);
                // Re-encoding a ref cannot fail.
                let _ = self.set(FieldValue::Ref(NodeRef(value), kind));
            }
            FieldValue::List(l) => l
                .elements
                .iter_mut()
                .flatten()
                .for_each(|f| f.remap_refs(map)),
            _ => {}
        }
    }
}

/// The bytes of a value, as the reference grammar engine re-encodes a changed token; `None`
/// for lists and raw fields.
fn encode(v: &FieldValue) -> Option<Vec<u8>> {
    Some(match v {
        FieldValue::U8(x) => vec![*x],
        FieldValue::I8(x) => x.to_le_bytes().to_vec(),
        FieldValue::U16(x) => x.to_le_bytes().to_vec(),
        FieldValue::I16(x) => x.to_le_bytes().to_vec(),
        FieldValue::U32(x) => x.to_le_bytes().to_vec(),
        FieldValue::I32(x) => x.to_le_bytes().to_vec(),
        FieldValue::F32(x) => x.to_le_bytes().to_vec(),
        FieldValue::F64(x) => x.to_le_bytes().to_vec(),
        FieldValue::U8s(x) => x.clone(),
        FieldValue::I8s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::U16s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::I16s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::U32s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::I32s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::F32s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::Color(c) => c.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::F64s(x) => x.iter().flat_map(|v| v.to_le_bytes()).collect(),
        FieldValue::Bool(b) => vec![u8::from(*b)],
        FieldValue::Guid(g) => g.as_bytes().to_vec(),
        FieldValue::Text(s) => text16(s),
        FieldValue::Text8(s) => [
            (s.len() as u32).to_le_bytes().to_vec(),
            s.as_bytes().to_vec(),
        ]
        .concat(),
        FieldValue::Text8Bytes(b) => [(b.len() as u32).to_le_bytes().to_vec(), b.clone()].concat(),
        FieldValue::Ref(r, _) => r.0.to_le_bytes().to_vec(),
        FieldValue::Raw | FieldValue::List(_) | FieldValue::Other(..) => return None,
    })
}

/// Text16 as the reference grammar engine writes it: the character count, then UTF-16LE, no
/// terminator.
fn text16(s: &str) -> Vec<u8> {
    let mut out = (s.chars().count() as u32).to_le_bytes().to_vec();
    s.encode_utf16().for_each(|u| out.extend(u.to_le_bytes()));
    out
}

impl ListField {
    /// Appends an element; the meta capacity word grows when the count passes it.
    pub fn push(&mut self, element: Vec<Field>) {
        self.elements.push(element);
        self.fit_meta();
    }

    /// Inserts an element at `at`.
    pub fn insert(&mut self, at: usize, element: Vec<Field>) {
        self.elements.insert(at.min(self.elements.len()), element);
        self.fit_meta();
    }

    /// Raises the capacity (first meta u32) to the count when needed (spec 13.4). An empty list
    /// has no meta word; callers that fill one must set `meta` first.
    fn fit_meta(&mut self) {
        let count = self.elements.len() as u32;
        let Some(meta) = self.meta.as_mut() else {
            return;
        };
        if meta.len() >= 4 && u32::from_le_bytes([meta[0], meta[1], meta[2], meta[3]]) < count {
            meta[..4].copy_from_slice(&count.to_le_bytes());
        }
    }
}

impl DecodedNode {
    /// The first top-level field named `name` (or that had the name), mutable.
    pub fn field_mut(&mut self, name: &str) -> Option<&mut Field> {
        let at = self
            .fields
            .iter()
            .position(|f| f.name.as_deref() == Some(name));
        let at = at.or_else(|| {
            self.fields
                .iter()
                .position(|f| f.aliases.iter().any(|a| a == name))
        })?;
        self.fields.get_mut(at)
    }

    /// Sets the first field named `name`; fails when there is none.
    pub fn set(&mut self, name: &str, value: FieldValue) -> Result<()> {
        let field = self
            .field_mut(name)
            .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("field {name}")))?;
        field.set(value)
    }

    /// The list field `name`, mutable.
    pub fn list_mut(&mut self, name: &str) -> Option<&mut ListField> {
        match &mut self.field_mut(name)?.value {
            FieldValue::List(l) => Some(l),
            _ => None,
        }
    }

    /// Rewrites every NodeRef of the node through `map`.
    pub fn remap_refs(&mut self, map: &dyn Fn(usize) -> Option<usize>) {
        self.fields.iter_mut().for_each(|f| f.remap_refs(map));
    }
}

/// A NodeRef field value, flagged (bit 31) or not.
pub fn node_ref(index: usize, flagged: bool, kind: RefKind) -> FieldValue {
    let flag = if flagged { 0x8000_0000 } else { 0 };
    FieldValue::Ref(NodeRef(index as u32 & 0x7FFF_FFFF | flag), kind)
}
