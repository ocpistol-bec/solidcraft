//! The chained tail of the M stream: t4 … t10 and the closing GUID (spec 11.2.6–11.2.9).

use crate::bytes::{ByteReader, ByteWriter};
use crate::error::{CorlissError, FailureClass, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "M stream tail";

/// The constant GUID that ends every M stream (spec 11.2.9).
pub const CLOSING_GUID: Guid = Guid::from_bytes([
    0xa4, 0xe6, 0x44, 0x97, 0xd1, 0x11, 0xd8, 0x8d, 0x00, 0x08, 0x29, 0x98, 0xbe, 0xdd, 0xdc, 0x09,
]);

/// A t6 entry: a referenced segment and *that segment's* current revision [F-08].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentLink {
    /// The referenced segment's id.
    pub seg_ref: Guid,
    /// Its revision.
    pub revision: Guid,
}

/// A t7 entry: (revision, referenced segment = t6 index, count of t9 entries it owns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ownership {
    /// Revision of the save that recorded the entries.
    pub revision: Guid,
    /// Index into t6.
    pub segment: u16,
    /// Number of t9 entries this record owns.
    pub count: u16,
}

/// A t8 entry: a template reference (spec 11.2.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRef {
    /// Number of node fields that refer to this entry.
    pub uses: u16,
    /// 1 built-in template GUID, 2 embedded template GUID, 3 key + object id.
    pub kind: u8,
    /// 16 bytes for kinds 1 and 2, 8 bytes for kind 3.
    pub payload: Vec<u8>,
}

/// The sections after the type table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MTail {
    /// t4, the class graph.
    pub t4: Vec<u16>,
    /// t5, the id allocation set, kept as stored (absent in template M streams).
    pub t5: Option<Vec<u8>>,
    /// t6, referenced segments.
    pub t6: Vec<SegmentLink>,
    /// t7, ownership records.
    pub t7: Vec<Ownership>,
    /// t8, template references.
    pub t8: Vec<TemplateRef>,
    /// t9, root records: (key, ., ., object id) words.
    pub t9: Vec<[u16; 4]>,
    /// t10, the 18 write counters (Table 11-2).
    pub write_counts: Vec<u32>,
}

impl MTail {
    /// Decodes the tail `data` that follows the type table; `template` selects the 5-section form.
    pub fn decode(data: &[u8], template: bool) -> Result<Self> {
        let body_end = closing_guid_start(data)?;
        let (sections, t10_start) = chain_sections(data, body_end)?;
        let expected = if template { 5 } else { 6 };
        if sections.len() != expected {
            let found = format!("{} chained sections", sections.len());
            return Err(CorlissError::invalid(
                STRUCTURE, "sections", 0, found, expected,
            ));
        }
        let mut sections = sections.into_iter();
        let mut next = || sections.next().unwrap_or_default();
        let t4 = decode_t4(next())?;
        let t5 = (!template).then(|| next().to_vec());
        Ok(MTail {
            t4,
            t5,
            t6: decode_fixed(next(), "t6", 32, read_link)?,
            t7: decode_fixed(next(), "t7", 20, read_ownership)?,
            t8: decode_t8(next())?,
            t9: decode_fixed(next(), "t9", 8, read_t9)?,
            write_counts: decode_t10(&data[t10_start..body_end])?,
        })
    }

    /// Encodes the tail, each chained section followed by its byte size.
    pub fn encode(&self, w: &mut ByteWriter) {
        let mut t4 = ByteWriter::default();
        self.t4.iter().for_each(|v| {
            t4.u16(*v);
        });
        sized(w, &t4.into_bytes());
        if let Some(t5) = &self.t5 {
            sized(w, t5);
        }
        sized(w, &encode_fixed(&self.t6, write_link));
        sized(w, &encode_fixed(&self.t7, write_ownership));
        sized(w, &encode_fixed(&self.t8, write_template_ref));
        sized(w, &encode_fixed(&self.t9, write_t9));
        w.len_u32(self.write_counts.len());
        self.write_counts.iter().for_each(|v| {
            w.u32(*v);
        });
        w.guid(&CLOSING_GUID);
    }
}

fn write_link(w: &mut ByteWriter, e: &SegmentLink) {
    w.guid(&e.seg_ref).guid(&e.revision);
}

fn write_ownership(w: &mut ByteWriter, e: &Ownership) {
    w.guid(&e.revision).u16(e.segment).u16(e.count);
}

fn write_template_ref(w: &mut ByteWriter, e: &TemplateRef) {
    w.u16(e.uses).u8(e.kind).bytes(&e.payload);
}

fn write_t9(w: &mut ByteWriter, e: &[u16; 4]) {
    e.iter().for_each(|v| {
        w.u16(*v);
    });
}

fn closing_guid_start(data: &[u8]) -> Result<usize> {
    let start = data
        .len()
        .checked_sub(16)
        .ok_or_else(|| truncated(data.len(), 16))?;
    if data[start..] != CLOSING_GUID.as_bytes()[..] {
        let found = Guid::from_bytes(data[start..].try_into().unwrap_or_default());
        return Err(CorlissError::invalid(
            STRUCTURE,
            "closing_guid",
            start,
            found,
            CLOSING_GUID,
        ));
    }
    Ok(start)
}

fn truncated(available: usize, needed: usize) -> CorlissError {
    CorlissError::decode(
        STRUCTURE,
        "",
        0,
        FailureClass::Truncated { needed, available },
    )
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Finds t10 by its self-describing count, then walks the size fields back to the start.
///
/// The sections have no forward framing; each one's trailing u32 is its own byte size, so
/// they are located from the end (spec 11.2 introduction).
fn chain_sections(data: &[u8], body_end: usize) -> Result<(Vec<&[u8]>, usize)> {
    for count in 0..body_end / 4 {
        let Some(t10_start) = body_end.checked_sub(4 * count + 4) else {
            break;
        };
        if u32_at(data, t10_start) != Some(count as u32) {
            continue;
        }
        if let Some(sections) = chain_back(data, t10_start) {
            return Ok((sections, t10_start));
        }
    }
    Err(CorlissError::invalid(
        STRUCTURE,
        "sections",
        0,
        "no consistent size chain",
        "t4…t9 each followed by its byte size",
    ))
}

fn chain_back(data: &[u8], mut end: usize) -> Option<Vec<&[u8]>> {
    let mut sections = Vec::new();
    while end > 0 {
        let size = u32_at(data, end.checked_sub(4)?)? as usize;
        let start = (end - 4).checked_sub(size)?;
        sections.push(&data[start..end - 4]);
        end = start;
    }
    sections.reverse();
    Some(sections)
}

fn decode_t4(section: &[u8]) -> Result<Vec<u16>> {
    if !section.len().is_multiple_of(2) {
        return Err(CorlissError::invalid(
            STRUCTURE,
            "t4",
            0,
            section.len(),
            "an even byte count",
        ));
    }
    let mut r = ByteReader::new(section, "M stream t4");
    (0..section.len() / 2).map(|_| r.u16("t4")).collect()
}

fn decode_fixed<T>(
    section: &[u8],
    field: &'static str,
    width: usize,
    read: fn(&mut ByteReader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let mut r = ByteReader::new(section, STRUCTURE);
    let n = r.count(field, width)?;
    let out = (0..n).map(|_| read(&mut r)).collect::<Result<Vec<_>>>()?;
    r.finish()?;
    Ok(out)
}

fn read_link(r: &mut ByteReader<'_>) -> Result<SegmentLink> {
    Ok(SegmentLink {
        seg_ref: r.guid("t6.seg_ref")?,
        revision: r.guid("t6.revision")?,
    })
}

fn read_ownership(r: &mut ByteReader<'_>) -> Result<Ownership> {
    Ok(Ownership {
        revision: r.guid("t7.revision")?,
        segment: r.u16("t7.segment")?,
        count: r.u16("t7.count")?,
    })
}

fn read_t9(r: &mut ByteReader<'_>) -> Result<[u16; 4]> {
    Ok([r.u16("t9")?, r.u16("t9")?, r.u16("t9")?, r.u16("t9")?])
}

fn decode_t8(section: &[u8]) -> Result<Vec<TemplateRef>> {
    let mut r = ByteReader::new(section, "M stream t8");
    let n = r.count("t8", 3)?;
    let out = (0..n)
        .map(|_| read_template_ref(&mut r))
        .collect::<Result<Vec<_>>>()?;
    r.finish()?;
    Ok(out)
}

fn read_template_ref(r: &mut ByteReader<'_>) -> Result<TemplateRef> {
    let uses = r.u16("uses")?;
    let at = r.position();
    let kind = r.u8("kind")?;
    let width = match kind {
        1 | 2 => 16,
        3 => 8,
        other => {
            return Err(CorlissError::invalid(
                "M stream t8",
                "kind",
                at,
                other,
                "1, 2 or 3",
            ));
        }
    };
    let payload = r.take(width, "payload")?.to_vec();
    Ok(TemplateRef {
        uses,
        kind,
        payload,
    })
}

fn decode_t10(section: &[u8]) -> Result<Vec<u32>> {
    let mut r = ByteReader::new(section, "M stream t10");
    let n = r.count("t10", 4)?;
    let out = (0..n)
        .map(|_| r.u32("write_counts"))
        .collect::<Result<Vec<_>>>()?;
    r.finish()?;
    Ok(out)
}

fn sized(w: &mut ByteWriter, section: &[u8]) {
    w.bytes(section).len_u32(section.len());
}

fn encode_fixed<T>(items: &[T], write: impl Fn(&mut ByteWriter, &T)) -> Vec<u8> {
    let mut w = ByteWriter::default();
    w.len_u32(items.len());
    items.iter().for_each(|item| write(&mut w, item));
    w.into_bytes()
}
