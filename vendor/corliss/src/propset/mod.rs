//! Property sets (iProperties): [MS-OLEPS] streams at the container root (spec clause 8).

mod value;

pub use value::PropertyValue;

use crate::bytes::{ByteReader, ByteWriter};
use crate::error::{CorlissError, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "property set stream";
/// Property id of the dictionary.
const DICTIONARY_ID: u32 = 0;
/// Property id of the code page.
const CODEPAGE_ID: u32 = 1;

/// Format ids of Inventor's property sets (spec Table 8-1).
pub mod fmtid {
    use crate::guid::Guid;

    const fn g(bytes: [u8; 16]) -> Guid {
        Guid::from_bytes(bytes)
    }

    /// Inventor Summary Information `3d38de39-0588-4c14-bb37-18f4d5dd31c7`.
    pub const SUMMARY: Guid = g([
        0x39, 0xde, 0x38, 0x3d, 0x88, 0x05, 0x14, 0x4c, 0xbb, 0x37, 0x18, 0xf4, 0xd5, 0xdd, 0x31,
        0xc7,
    ]);
    /// Inventor Document Summary Information `8cf58000-da66-4ae6-8ff0-7b58406fb049`.
    pub const DOCUMENT_SUMMARY: Guid = g([
        0x00, 0x80, 0xf5, 0x8c, 0x66, 0xda, 0xe6, 0x4a, 0x8f, 0xf0, 0x7b, 0x58, 0x40, 0x6f, 0xb0,
        0x49,
    ]);
    /// Design Tracking Properties `32853f0f-3444-11d1-9e93-0060b03c1ca6`.
    pub const DESIGN_TRACKING: Guid = g([
        0x0f, 0x3f, 0x85, 0x32, 0x44, 0x34, 0xd1, 0x11, 0x9e, 0x93, 0x00, 0x60, 0xb0, 0x3c, 0x1c,
        0xa6,
    ]);
    /// Design Tracking Control `d861fb30-3136-11d1-9e92-0060b03c1ca6`.
    pub const DESIGN_TRACKING_CONTROL: Guid = g([
        0x30, 0xfb, 0x61, 0xd8, 0x36, 0x31, 0xd1, 0x11, 0x9e, 0x92, 0x00, 0x60, 0xb0, 0x3c, 0x1c,
        0xa6,
    ]);
    /// Inventor User Defined Properties `9929adb8-6407-413e-b3dc-cb9ad2f564b7`.
    pub const USER_DEFINED: Guid = g([
        0xb8, 0xad, 0x29, 0x99, 0x07, 0x64, 0x3e, 0x41, 0xb3, 0xdc, 0xcb, 0x9a, 0xd2, 0xf5, 0x64,
        0xb7,
    ]);
    /// _Private Model Information `bb586990-af3e-11d3-95a9-00a0c9b6e37a`.
    pub const PRIVATE_MODEL: Guid = g([
        0x90, 0x69, 0x58, 0xbb, 0x3e, 0xaf, 0xd3, 0x11, 0x95, 0xa9, 0x00, 0xa0, 0xc9, 0xb6, 0xe3,
        0x7a,
    ]);
}

/// One property: its id and value.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// Property id.
    pub id: u32,
    /// Value.
    pub value: PropertyValue,
}

/// One property set (a section of the stream).
///
/// ```
/// use corliss::propset::{PropertySet, PropertyValue};
/// let mut set = PropertySet::new(corliss::propset::fmtid::DESIGN_TRACKING);
/// set.set(5, PropertyValue::Text("PN-001".into()));
/// assert_eq!(set.get(5).and_then(PropertyValue::as_text), Some("PN-001"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PropertySet {
    /// Format id naming the set.
    pub fmtid: Guid,
    /// Properties in id-table order.
    pub properties: Vec<Property>,
}

impl PropertySet {
    /// An empty set with code page 1200 (UTF-16), as Inventor writes its sets [W-08].
    pub fn new(fmtid: Guid) -> Self {
        let codepage = Property {
            id: CODEPAGE_ID,
            value: PropertyValue::I2(1200),
        };
        PropertySet {
            fmtid,
            properties: vec![codepage],
        }
    }

    /// The value of property `id`.
    pub fn get(&self, id: u32) -> Option<&PropertyValue> {
        self.properties
            .iter()
            .find(|p| p.id == id)
            .map(|p| &p.value)
    }

    /// Sets property `id`, replacing its value or appending it.
    pub fn set(&mut self, id: u32, value: PropertyValue) {
        match self.properties.iter_mut().find(|p| p.id == id) {
            Some(p) => p.value = value,
            None => self.properties.push(Property { id, value }),
        }
    }

    /// The code page of property 1 (1200 = UTF-16; 1252 when absent).
    pub fn codepage(&self) -> u16 {
        match self.get(CODEPAGE_ID) {
            Some(PropertyValue::I2(cp)) => *cp as u16,
            _ => 1252,
        }
    }

    /// The name of property `id` from the dictionary, for sets that name their properties.
    pub fn name_of(&self, id: u32) -> Option<&str> {
        match self.get(DICTIONARY_ID)? {
            PropertyValue::Dictionary(entries) => entries
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, n)| n.as_str()),
            _ => None,
        }
    }
}

/// The stream header fields kept for re-encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamHeader {
    version: u16,
    system_id: u32,
    clsid: Guid,
}

/// A property-set stream: header and sets, with the original bytes for an exact round trip.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertySetStream {
    header: StreamHeader,
    /// The sets of the stream (one in Inventor's own streams).
    pub sets: Vec<PropertySet>,
    original: Option<(Vec<u8>, Vec<PropertySet>)>,
}

impl PropertySetStream {
    /// Decodes a stream.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        let byte_order = r.u16("byte_order")?;
        if byte_order != 0xFFFE {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "byte_order",
                0,
                format!("{byte_order:#06x}"),
                "0xfffe",
            ));
        }
        let header = StreamHeader {
            version: r.u16("version")?,
            system_id: r.u32("system_id")?,
            clsid: r.guid("clsid")?,
        };
        let n = r.count("num_property_sets", 20)?;
        let mut sets = Vec::with_capacity(n);
        for _ in 0..n {
            let fmtid = r.guid("fmtid")?;
            let offset = r.u32("offset")? as usize;
            sets.push(decode_set(data, fmtid, offset)?);
        }
        let original = Some((data.to_vec(), sets.clone()));
        Ok(PropertySetStream {
            header,
            sets,
            original,
        })
    }

    /// Encodes the stream; unchanged sets give back the original bytes [W-01].
    pub fn encode(&self) -> Vec<u8> {
        if let Some((bytes, sets)) = &self.original
            && *sets == self.sets
        {
            return bytes.clone();
        }
        let sections: Vec<Vec<u8>> = self.sets.iter().map(encode_set).collect();
        let mut w = ByteWriter::default();
        w.u16(0xFFFE)
            .u16(self.header.version)
            .u32(self.header.system_id)
            .guid(&self.header.clsid);
        w.len_u32(self.sets.len());
        let mut offset = 28 + 20 * self.sets.len();
        for (set, section) in self.sets.iter().zip(&sections) {
            w.guid(&set.fmtid).len_u32(offset);
            offset += section.len();
        }
        sections.iter().for_each(|s| {
            w.bytes(s);
        });
        w.into_bytes()
    }

    /// The set with `fmtid`.
    pub fn set(&self, fmtid: &Guid) -> Option<&PropertySet> {
        self.sets.iter().find(|s| s.fmtid == *fmtid)
    }

    /// The set with `fmtid`, mutable.
    pub fn set_mut(&mut self, fmtid: &Guid) -> Option<&mut PropertySet> {
        self.sets.iter_mut().find(|s| s.fmtid == *fmtid)
    }
}

fn decode_set(data: &[u8], fmtid: Guid, offset: usize) -> Result<PropertySet> {
    let section = data.get(offset..).ok_or_else(|| {
        CorlissError::invalid(
            STRUCTURE,
            "offset",
            0,
            offset,
            "an offset inside the stream",
        )
    })?;
    let mut r = ByteReader::new(section, "property set");
    let size = r.u32("size")? as usize;
    let section = section.get(..size).ok_or_else(|| {
        CorlissError::invalid("property set", "size", 0, size, "a size inside the stream")
    })?;
    let n = r.count("num_properties", 8)?;
    let table: Vec<(u32, usize)> = (0..n)
        .map(|_| Ok((r.u32("id")?, r.u32("offset")? as usize)))
        .collect::<Result<_>>()?;
    let codepage = find_codepage(section, &table)?;
    let properties = table
        .iter()
        .map(|(id, at)| decode_property(section, &table, *id, *at, codepage))
        .collect::<Result<Vec<_>>>()?;
    Ok(PropertySet { fmtid, properties })
}

fn find_codepage(section: &[u8], table: &[(u32, usize)]) -> Result<u16> {
    let Some((_, at)) = table.iter().find(|(id, _)| *id == CODEPAGE_ID) else {
        return Ok(1252);
    };
    let mut r = ByteReader::new(section.get(*at..).unwrap_or_default(), "property set");
    r.u32("codepage.type")?;
    r.u16("codepage")
}

/// Value bytes run to the next value's offset (or the section end), whatever the table order.
fn value_len(section: &[u8], table: &[(u32, usize)], at: usize) -> usize {
    let next = table
        .iter()
        .map(|(_, o)| *o)
        .filter(|o| *o > at)
        .min()
        .unwrap_or(section.len());
    next.saturating_sub(at + 4)
}

fn decode_property(
    section: &[u8],
    table: &[(u32, usize)],
    id: u32,
    at: usize,
    codepage: u16,
) -> Result<Property> {
    let bytes = section.get(at..).ok_or_else(|| {
        CorlissError::invalid("property set", "offset", 0, at, "an offset inside the set")
    })?;
    let mut r = ByteReader::new(bytes, "property value");
    if id == DICTIONARY_ID {
        return Ok(Property {
            id,
            value: value::decode_dictionary(&mut r, codepage)?,
        });
    }
    let vt = r.u16("type")?;
    r.u16("padding")?;
    let value = PropertyValue::decode(&mut r, vt, value_len(section, table, at))?;
    Ok(Property { id, value })
}

/// Encodes a set canonically: id table in order, values contiguous and 4-byte aligned.
fn encode_set(set: &PropertySet) -> Vec<u8> {
    let codepage = set.codepage();
    let mut values = ByteWriter::default();
    let mut offsets = Vec::with_capacity(set.properties.len());
    let table_len = 8 + 8 * set.properties.len();
    for p in &set.properties {
        offsets.push(table_len + values.len());
        p.value.encode(&mut values, codepage);
    }
    let mut w = ByteWriter::default();
    w.len_u32(table_len + values.len())
        .len_u32(set.properties.len());
    for (p, offset) in set.properties.iter().zip(offsets) {
        w.u32(p.id).len_u32(offset);
    }
    w.bytes(&values.into_bytes());
    w.into_bytes()
}
