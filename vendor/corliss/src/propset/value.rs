//! Typed property values ([MS-OLEPS] TypedPropertyValue).

use crate::bytes::{ByteReader, ByteWriter};
use crate::error::Result;
use crate::guid::Guid;

/// Variant type codes the decoder knows (spec 8.4).
mod vt {
    pub const I2: u16 = 2;
    pub const I4: u16 = 3;
    pub const R4: u16 = 4;
    pub const R8: u16 = 5;
    pub const BOOL: u16 = 11;
    pub const UI4: u16 = 19;
    pub const I8: u16 = 20;
    pub const UI8: u16 = 21;
    pub const LPSTR: u16 = 30;
    pub const LPWSTR: u16 = 31;
    pub const FILETIME: u16 = 64;
    pub const BLOB: u16 = 65;
    pub const CF: u16 = 71;
    pub const CLSID: u16 = 72;
}

/// A property value.
///
/// ```
/// let v = corliss::propset::PropertyValue::Text("Part1".into());
/// assert_eq!(v.as_text(), Some("Part1"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// VT_I2.
    I2(i16),
    /// VT_I4.
    I4(i32),
    /// VT_UI4.
    UI4(u32),
    /// VT_R4.
    R4(f32),
    /// VT_R8.
    R8(f64),
    /// VT_BOOL.
    Bool(bool),
    /// VT_I8.
    I8(i64),
    /// VT_UI8.
    UI8(u64),
    /// VT_LPSTR, the stored bytes including the terminator (their encoding follows the code page).
    Ansi(Vec<u8>),
    /// VT_LPWSTR without the terminating NUL.
    Text(String),
    /// VT_FILETIME: 100 ns intervals since 1601-01-01 UTC.
    FileTime(u64),
    /// VT_BLOB.
    Blob(Vec<u8>),
    /// VT_CF: the clipboard data after its size field (format word, then the data).
    Clipboard(Vec<u8>),
    /// VT_CLSID.
    Clsid(Guid),
    /// The dictionary of property 0: (property id, name) pairs.
    Dictionary(Vec<(u32, String)>),
    /// A type this decoder does not interpret, with the bytes up to the next value.
    Unknown {
        /// Variant type code.
        vt: u16,
        /// Bytes after the type and padding words.
        bytes: Vec<u8>,
    },
}

impl PropertyValue {
    /// The text of a VT_LPWSTR value.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            PropertyValue::Text(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn decode(r: &mut ByteReader<'_>, vt: u16, len: usize) -> Result<Self> {
        Ok(match vt {
            vt::I2 => PropertyValue::I2(r.i16("I2")?),
            vt::I4 => PropertyValue::I4(r.i32("I4")?),
            vt::UI4 => PropertyValue::UI4(r.u32("UI4")?),
            vt::R4 => PropertyValue::R4(f32::from_bits(r.u32("R4")?)),
            vt::R8 => PropertyValue::R8(r.f64("R8")?),
            vt::BOOL => PropertyValue::Bool(r.u16("BOOL")? != 0),
            vt::I8 => PropertyValue::I8(r.i64("I8")?),
            vt::UI8 => PropertyValue::UI8(r.u64("UI8")?),
            vt::LPSTR => PropertyValue::Ansi(r.text8("LPSTR")?),
            vt::LPWSTR => PropertyValue::Text(read_wide(r)?),
            vt::FILETIME => PropertyValue::FileTime(r.u64("FILETIME")?),
            vt::BLOB => PropertyValue::Blob(r.text8("BLOB")?),
            vt::CF => PropertyValue::Clipboard(r.text8("CF")?),
            vt::CLSID => PropertyValue::Clsid(r.guid("CLSID")?),
            _ => PropertyValue::Unknown {
                vt,
                bytes: r.take(len, "value")?.to_vec(),
            },
        })
    }

    /// Writes the TypedPropertyValue (type, padding, value) padded to 4 bytes.
    pub(crate) fn encode(&self, w: &mut ByteWriter, codepage: u16) {
        let start = w.len();
        match self {
            PropertyValue::Dictionary(entries) => write_dictionary(w, entries, codepage),
            _ => self.encode_typed(w),
        }
        pad4(w, start);
    }

    fn encode_typed(&self, w: &mut ByteWriter) {
        w.u16(self.vt()).u16(0);
        match self {
            PropertyValue::I2(v) => w.i16(*v),
            PropertyValue::I4(v) => w.i32(*v),
            PropertyValue::UI4(v) => w.u32(*v),
            PropertyValue::R4(v) => w.u32(v.to_bits()),
            PropertyValue::R8(v) => w.u64(v.to_bits()),
            PropertyValue::Bool(v) => w.u16(if *v { 0xFFFF } else { 0 }),
            PropertyValue::I8(v) => w.i64(*v),
            PropertyValue::UI8(v) | PropertyValue::FileTime(v) => w.u64(*v),
            PropertyValue::Ansi(b) | PropertyValue::Blob(b) | PropertyValue::Clipboard(b) => {
                w.text8(b)
            }
            PropertyValue::Text(s) => write_wide(w, s),
            PropertyValue::Clsid(g) => w.guid(g),
            PropertyValue::Unknown { bytes, .. } => w.bytes(bytes),
            PropertyValue::Dictionary(_) => w,
        };
    }

    fn vt(&self) -> u16 {
        match self {
            PropertyValue::I2(_) => vt::I2,
            PropertyValue::I4(_) => vt::I4,
            PropertyValue::UI4(_) => vt::UI4,
            PropertyValue::R4(_) => vt::R4,
            PropertyValue::R8(_) => vt::R8,
            PropertyValue::Bool(_) => vt::BOOL,
            PropertyValue::I8(_) => vt::I8,
            PropertyValue::UI8(_) => vt::UI8,
            PropertyValue::Ansi(_) => vt::LPSTR,
            PropertyValue::Text(_) => vt::LPWSTR,
            PropertyValue::FileTime(_) => vt::FILETIME,
            PropertyValue::Blob(_) => vt::BLOB,
            PropertyValue::Clipboard(_) => vt::CF,
            PropertyValue::Clsid(_) => vt::CLSID,
            PropertyValue::Dictionary(_) => 0,
            PropertyValue::Unknown { vt, .. } => *vt,
        }
    }
}

fn read_wide(r: &mut ByteReader<'_>) -> Result<String> {
    let mut text = r.text16("LPWSTR")?;
    while text.ends_with('\0') {
        text.pop();
    }
    Ok(text)
}

fn write_wide<'w>(w: &'w mut ByteWriter, s: &str) -> &'w mut ByteWriter {
    let mut with_nul = String::from(s);
    with_nul.push('\0');
    w.text16(&with_nul)
}

fn pad4(w: &mut ByteWriter, start: usize) {
    while !(w.len() - start).is_multiple_of(4) {
        w.u8(0);
    }
}

/// Reads the dictionary of property 0; names are UTF-16 padded to 4 bytes under code page 1200.
pub(crate) fn decode_dictionary(r: &mut ByteReader<'_>, codepage: u16) -> Result<PropertyValue> {
    let n = r.count("dictionary", 8)?;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u32("dictionary.id")?;
        let name = if codepage == 1200 {
            let start = r.position();
            let name = r.text16("dictionary.name")?;
            let used = r.position() - start - 4;
            r.take((4 - used % 4) % 4, "dictionary.padding")?;
            name
        } else {
            r.text8("dictionary.name")?
                .iter()
                .map(|b| char::from(*b))
                .collect()
        };
        entries.push((id, name.trim_end_matches('\0').to_string()));
    }
    Ok(PropertyValue::Dictionary(entries))
}

fn write_dictionary(w: &mut ByteWriter, entries: &[(u32, String)], codepage: u16) {
    w.len_u32(entries.len());
    for (id, name) in entries {
        w.u32(*id);
        let terminated = format!("{name}\0");
        if codepage == 1200 {
            let start = w.len() + 4;
            w.text16(&terminated);
            pad4(w, start);
        } else {
            let bytes: Vec<u8> = terminated
                .chars()
                .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
                .collect();
            w.text8(&bytes);
        }
    }
}
