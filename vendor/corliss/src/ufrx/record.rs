//! An ordered record of typed fields: the lossless model of the UFRxDoc stream.
//!
//! Every byte of the stream belongs to exactly one field, so encoding the parsed record gives
//! back the stream byte for byte; edits change field values and re-encode.

use crate::bytes::ByteWriter;
use crate::guid::Guid;

/// The value of one field.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// u8.
    U8(u8),
    /// u16.
    U16(u16),
    /// u32.
    U32(u32),
    /// i32.
    I32(i32),
    /// u64.
    U64(u64),
    /// A GUID.
    Guid(Guid),
    /// An 8-byte Version / BuildInfo version.
    Version([u8; 8]),
    /// A Text16 string.
    Text16(String),
    /// A Text8 string, kept as bytes.
    Text8(Vec<u8>),
    /// A fixed-length u16 array.
    U16s(Vec<u16>),
    /// A fixed-length u32 array.
    U32s(Vec<u32>),
    /// A fixed-length f32 array.
    F32s(Vec<f32>),
    /// A nested record.
    Record(Record),
    /// A list of records.
    List(Vec<Record>),
}

/// A named field.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Field name, as in the specification's clause 10 tables.
    pub name: &'static str,
    /// Field value.
    pub value: Value,
}

/// A record: its kind and its fields in stream order.
///
/// ```
/// use corliss::ufrx::{Record, Value};
/// let mut r = Record::new("inv_file");
/// r.push("path", Value::Text16("C:\\a.ipt".into()));
/// assert_eq!(r.text("path"), Some("C:\\a.ipt"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Record kind ("inv_file", "occurrence" …).
    pub kind: &'static str,
    /// Fields in stream order.
    pub fields: Vec<Field>,
}

impl Record {
    /// An empty record of `kind`.
    pub fn new(kind: &'static str) -> Self {
        Record {
            kind,
            fields: Vec::new(),
        }
    }

    /// Appends a field.
    pub fn push(&mut self, name: &'static str, value: Value) {
        self.fields.push(Field { name, value });
    }

    /// The value of the first field named `name`.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| &f.value)
    }

    /// The mutable value of the first field named `name`.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.fields
            .iter_mut()
            .find(|f| f.name == name)
            .map(|f| &mut f.value)
    }

    /// A field as an unsigned integer (u8, u16, u32 or u64).
    pub fn uint(&self, name: &str) -> Option<u64> {
        match self.get(name)? {
            Value::U8(v) => Some(u64::from(*v)),
            Value::U16(v) => Some(u64::from(*v)),
            Value::U32(v) => Some(u64::from(*v)),
            Value::U64(v) => Some(*v),
            _ => None,
        }
    }

    /// A Text16 field.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Text16(s) => Some(s),
            _ => None,
        }
    }

    /// A GUID field.
    pub fn guid(&self, name: &str) -> Option<Guid> {
        match self.get(name)? {
            Value::Guid(g) => Some(*g),
            _ => None,
        }
    }

    /// A nested record field.
    pub fn record(&self, name: &str) -> Option<&Record> {
        match self.get(name)? {
            Value::Record(r) => Some(r),
            _ => None,
        }
    }

    /// A list field; empty when absent.
    pub fn list(&self, name: &str) -> &[Record] {
        match self.get(name) {
            Some(Value::List(items)) => items,
            _ => &[],
        }
    }

    /// Appends the record's bytes to `w`.
    pub fn encode(&self, w: &mut ByteWriter) {
        self.fields.iter().for_each(|f| encode_value(w, &f.value));
    }
}

fn encode_value(w: &mut ByteWriter, value: &Value) {
    match value {
        Value::U8(v) => w.u8(*v),
        Value::U16(v) => w.u16(*v),
        Value::U32(v) => w.u32(*v),
        Value::I32(v) => w.i32(*v),
        Value::U64(v) => w.u64(*v),
        Value::Guid(g) => w.guid(g),
        Value::Version(b) => w.bytes(b),
        Value::Text16(s) => w.text16(s),
        Value::Text8(b) => w.text8(b),
        Value::U16s(v) => v.iter().fold(w, |w, x| w.u16(*x)),
        Value::U32s(v) => v.iter().fold(w, |w, x| w.u32(*x)),
        Value::F32s(v) => v.iter().fold(w, |w, x| w.u32(x.to_bits())),
        Value::Record(r) => {
            r.encode(w);
            w
        }
        Value::List(items) => {
            items.iter().for_each(|r| r.encode(w));
            w
        }
    };
}
