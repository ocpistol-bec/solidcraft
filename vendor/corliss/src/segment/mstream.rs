//! The M stream: segment metadata (spec 11.2).

use super::mtail::MTail;
use crate::bytes::{ByteReader, ByteWriter};
use crate::compression::{Compression, Packed};
use crate::database::VersionWords;
use crate::error::{CorlissError, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "M stream";
/// Bytes per sec2 entry in M versions 7 and 8 (spec 11.2.4).
const SEC2_WIDTH: usize = 10;

/// The identity fields that only segment M streams carry (absent in template M streams).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentIdentity {
    /// Segment name; equals the RSeSegInfo `name` [F-04].
    pub name: String,
    /// Segment id; equals the RSeSegInfo `seg_id` [F-04].
    pub seg_id: Guid,
    /// Highest object id handed out [W-09].
    pub id_space: u64,
    /// Segment creation flags (& 0x50).
    pub seg_flags: u32,
}

/// The uncompressed outer header (spec 11.2.1, 11.2.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MHeader {
    /// "RSe Meta Stream Version 8" (or 7).
    pub magic: Vec<u8>,
    /// M stream version, 7 or 8.
    pub version: u16,
    /// Release that created the segment.
    pub created_version: VersionWords,
    /// Release of the last save.
    pub saved_version: VersionWords,
    /// Present in segment M streams, absent in template M streams.
    pub identity: Option<SegmentIdentity>,
    /// Creation date text.
    pub created: Vec<u8>,
    /// Last-save date text.
    pub saved: Vec<u8>,
    /// Compression of the rest of the stream (and of the B stream) [F-06].
    pub compression: Compression,
}

/// A type table entry (spec 11.2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeEntry {
    /// Class GUID; its first u32 is the node TypeID.
    pub class: Guid,
    /// First u16 after the GUID (per-class schema word).
    pub schema_a: u16,
    /// Following u32 (per-class schema word).
    pub schema_b: u32,
    /// 0xFFFF exactly when the block table has absent entries, else 0 (spec 12.4).
    pub sparse_marker: u16,
    /// Cumulative instance counter of the class (spec 11.5.4).
    pub instance_counter: u32,
}

/// A block table entry: payload length and the present bit (spec 11.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSlot(pub u32);

impl BlockSlot {
    /// True when the block is stored in the B stream (bit 31).
    pub fn is_present(self) -> bool {
        self.0 & 0x8000_0000 != 0
    }

    /// Payload length (bits 0–30).
    pub fn length(self) -> usize {
        (self.0 & 0x7FFF_FFFF) as usize
    }
}

/// The decompressed body of an M stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MBody {
    /// Live objects at save time; ≥ the block count [W-09].
    pub slot_count: u32,
    /// = end table count (spec 12.3).
    pub end_count: u16,
    /// len(t9) + 1.
    pub n_roots: u32,
    /// Outer header length + 10.
    pub header_size: u32,
    /// The block table.
    pub blocks: Vec<BlockSlot>,
    /// Section 2 entries (u32, u32 N, u16); the last N is the id space.
    pub sec2: Vec<(u32, u32, u16)>,
    /// Section 3 entries; empty in every sample.
    pub sec3: Vec<(Guid, [u16; 6])>,
    /// The type table.
    pub types: Vec<TypeEntry>,
    /// t4 … t10.
    pub tail: MTail,
}

/// A whole M stream, with the original compressed bytes kept for an exact round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MStream {
    /// Outer header.
    pub header: MHeader,
    /// Decompressed body.
    pub body: MBody,
    packed: Packed,
}

impl MStream {
    /// Decodes a segment M stream (`template = false`) or a template M stream.
    pub fn decode(data: &[u8], template: bool) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        let header = read_header(&mut r, template)?;
        let packed = Packed::unpack(header.compression, r.rest(), STRUCTURE)?;
        let body = MBody::decode(packed.inflated(), template)?;
        Ok(MStream {
            header,
            body,
            packed,
        })
    }

    /// Encodes the stream; an unchanged body keeps its original compressed bytes [W-01].
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut w = ByteWriter::default();
        write_header(&mut w, &self.header);
        w.bytes(
            &self
                .packed
                .repack(self.header.compression, &self.body.encode(), STRUCTURE)?,
        );
        Ok(w.into_bytes())
    }

    /// True for the M stream of a template segment (no name, id or t5).
    pub fn is_template(&self) -> bool {
        self.header.identity.is_none()
    }
}

impl MHeader {
    /// Decodes only the uncompressed outer header, e.g. to learn a segment's name cheaply.
    pub fn decode(data: &[u8], template: bool) -> Result<Self> {
        read_header(&mut ByteReader::new(data, STRUCTURE), template)
    }
}

fn read_header(r: &mut ByteReader<'_>, template: bool) -> Result<MHeader> {
    let magic = r.text8("magic")?;
    let at = r.position();
    let version = r.u16("version")?;
    if version != 7 && version != 8 {
        return Err(CorlissError::invalid(
            STRUCTURE, "version", at, version, "7 or 8",
        ));
    }
    let created_version = VersionWords::read(r, "created_version")?;
    let saved_version = VersionWords::read(r, "saved_version")?;
    let identity = if template {
        None
    } else {
        Some(read_identity(r)?)
    };
    let created = r.text8("created")?;
    let saved = r.text8("saved")?;
    let at = r.position();
    let code = r.u8("compression")?;
    let compression = Compression::from_code(code).ok_or_else(|| {
        CorlissError::invalid(STRUCTURE, "compression", at, code, "1 (zlib) or 2 (zstd)")
    })?;
    Ok(MHeader {
        magic,
        version,
        created_version,
        saved_version,
        identity,
        created,
        saved,
        compression,
    })
}

fn read_identity(r: &mut ByteReader<'_>) -> Result<SegmentIdentity> {
    Ok(SegmentIdentity {
        name: r.text16("name")?,
        seg_id: r.guid("seg_id")?,
        id_space: r.u64("id_space")?,
        seg_flags: r.u32("seg_flags")?,
    })
}

fn write_header(w: &mut ByteWriter, h: &MHeader) {
    w.text8(&h.magic).u16(h.version);
    h.created_version.write(w);
    h.saved_version.write(w);
    if let Some(id) = &h.identity {
        w.text16(&id.name)
            .guid(&id.seg_id)
            .u64(id.id_space)
            .u32(id.seg_flags);
    }
    w.text8(&h.created).text8(&h.saved).u8(h.compression.code());
}

impl MBody {
    /// Decodes a decompressed M body.
    pub fn decode(data: &[u8], template: bool) -> Result<Self> {
        let mut r = ByteReader::new(data, "M stream body");
        let slot_count = r.u32("slot_count")?;
        let end_count = r.u16("end_count")?;
        let n_roots = r.u32("n_roots")?;
        let header_size = r.u32("header_size")?;
        let blocks = sized_section(&mut r, "block table", 4, |r| Ok(BlockSlot(r.u32("entry")?)))?;
        let sec2 = sized_section(&mut r, "sec2", SEC2_WIDTH, |r| {
            Ok((r.u32("sec2")?, r.u32("sec2")?, r.u16("sec2")?))
        })?;
        let sec3 = sized_section(&mut r, "sec3", 28, read_sec3)?;
        let types = sized_section(&mut r, "types", 28, read_type)?;
        let tail = MTail::decode(r.rest(), template)?;
        Ok(MBody {
            slot_count,
            end_count,
            n_roots,
            header_size,
            blocks,
            sec2,
            sec3,
            types,
            tail,
        })
    }

    /// Encodes the body; section sizes are recomputed from the content [W-09].
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        w.u32(self.slot_count)
            .u16(self.end_count)
            .u32(self.n_roots)
            .u32(self.header_size);
        write_sized(&mut w, &self.blocks, |w, b| {
            w.u32(b.0);
        });
        write_sized(&mut w, &self.sec2, |w, e| {
            w.u32(e.0).u32(e.1).u16(e.2);
        });
        write_sized(&mut w, &self.sec3, |w, e| {
            w.guid(&e.0);
            e.1.iter().for_each(|v| {
                w.u16(*v);
            });
        });
        write_sized(&mut w, &self.types, write_type);
        self.tail.encode(&mut w);
        w.into_bytes()
    }

    /// Number of block-table entries whose block is stored in the B stream.
    pub fn present_blocks(&self) -> usize {
        self.blocks.iter().filter(|b| b.is_present()).count()
    }
}

/// Reads `u32 n`, n elements, then the `u32` byte size that MUST equal 4 + width·n.
fn sized_section<T>(
    r: &mut ByteReader<'_>,
    field: &'static str,
    width: usize,
    read: fn(&mut ByteReader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let n = r.count(field, width)?;
    let items = (0..n).map(|_| read(r)).collect::<Result<Vec<_>>>()?;
    let at = r.position();
    let size = r.u32(field)? as usize;
    if size != 4 + width * n {
        return Err(CorlissError::invalid(
            "M stream body",
            field,
            at,
            size,
            format!("byte size 4 + {width}·{n}"),
        ));
    }
    Ok(items)
}

fn write_sized<T>(w: &mut ByteWriter, items: &[T], write: impl Fn(&mut ByteWriter, &T)) {
    let start = w.len();
    w.len_u32(items.len());
    items.iter().for_each(|item| write(w, item));
    let size = w.len() - start;
    w.len_u32(size);
}

fn read_sec3(r: &mut ByteReader<'_>) -> Result<(Guid, [u16; 6])> {
    let guid = r.guid("sec3")?;
    let mut words = [0u16; 6];
    for word in &mut words {
        *word = r.u16("sec3")?;
    }
    Ok((guid, words))
}

fn read_type(r: &mut ByteReader<'_>) -> Result<TypeEntry> {
    Ok(TypeEntry {
        class: r.guid("class")?,
        schema_a: r.u16("schema_a")?,
        schema_b: r.u32("schema_b")?,
        sparse_marker: r.u16("sparse_marker")?,
        instance_counter: r.u32("instance_counter")?,
    })
}

fn write_type(w: &mut ByteWriter, t: &TypeEntry) {
    w.guid(&t.class)
        .u16(t.schema_a)
        .u32(t.schema_b)
        .u16(t.sparse_marker)
        .u32(t.instance_counter);
}
