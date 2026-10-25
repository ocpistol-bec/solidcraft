//! The B stream: compressed blocks and the segment end table (spec 11.3, clause 12).

use super::mstream::{BlockSlot, MBody};
use crate::bytes::{ByteReader, ByteWriter};
use crate::compression::{Compression, Packed};
use crate::error::{CorlissError, Result};
use crate::guid::{Guid, TypeId};

const STRUCTURE: &str = "B stream";
/// The constant GUID at the start of every B stream.
pub const B_STREAM_GUID: Guid = Guid::from_bytes([
    0x9e, 0xc2, 0x2b, 0xa4, 0xd4, 0x11, 0xe8, 0x01, 0x60, 0x00, 0x2d, 0xb3, 0xee, 0x29, 0xfb, 0xb0,
]);
/// Block headers are 0x100 + the type-table index (spec 12.1).
const TYPE_INDEX_BASE: u32 = 0x100;

/// One block of a decompressed B stream: a framed node payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Index of the block's entry in the M block table (absent entries counted).
    pub slot: usize,
    /// Block header: 0x100 + type-table index.
    pub header: u32,
    /// The node payload (clause 13).
    pub payload: Vec<u8>,
    /// The trailer after the length echo, kept as stored (empty before 2015).
    pub trailer: Vec<u8>,
}

impl Block {
    /// Index of the block's class in the M type table.
    pub fn type_index(&self) -> usize {
        self.header.wrapping_sub(TYPE_INDEX_BASE) as usize
    }

    /// TypeID of the block's node, looked up in the segment's type table.
    pub fn type_id(&self, body: &MBody) -> Option<TypeId> {
        body.types.get(self.type_index()).map(|t| t.class.type_id())
    }
}

/// The segment end table (spec 12.3); its meaning is open (OI-022), so it is kept field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndTable {
    /// Count of `indexes` (= M `end_count`).
    pub a: u16,
    /// Meaning open.
    pub b: u16,
    /// Always 0.
    pub zero: u16,
    /// Index list; its last value is the record count.
    pub indexes: Vec<u32>,
    /// Six-byte records.
    pub records: Vec<[u8; 6]>,
}

/// The decompressed content of a B stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentData {
    /// Blocks in block-table order, skipping absent entries [F-03].
    pub blocks: Vec<Block>,
    /// The end table.
    pub end_table: EndTable,
}

/// A B stream: its method word and decoded data, with the original bytes for an exact round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BStream {
    /// Compression method; its code is the method word's high byte [F-06].
    pub compression: Compression,
    /// Low byte of the method word (4 in every sample).
    pub method_low: u8,
    /// Decoded blocks and end table.
    pub data: SegmentData,
    packed: Packed,
}

impl BStream {
    /// Decodes a B stream against its M body; `trailers` follows RSeDb (saved-with major > 18).
    pub fn decode(bytes: &[u8], body: &MBody, trailers: bool) -> Result<Self> {
        let mut r = ByteReader::new(bytes, STRUCTURE);
        let guid = r.guid("guid")?;
        if guid != B_STREAM_GUID {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "guid",
                0,
                guid,
                B_STREAM_GUID,
            ));
        }
        let method = r.u16("method")?;
        let compression = Compression::from_code((method >> 8) as u8).ok_or_else(|| {
            CorlissError::invalid(
                STRUCTURE,
                "method",
                16,
                format!("{method:#06x}"),
                "0x0104 or 0x0204",
            )
        })?;
        let packed = Packed::unpack(compression, r.rest(), STRUCTURE)?;
        let data = SegmentData::decode(packed.inflated(), &body.blocks, trailers)?;
        Ok(BStream {
            compression,
            method_low: method as u8,
            data,
            packed,
        })
    }

    /// Encodes the stream; unchanged data keeps its original compressed bytes [W-01].
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut w = ByteWriter::default();
        w.guid(&B_STREAM_GUID)
            .u16(u16::from(self.compression.code()) << 8 | u16::from(self.method_low));
        w.bytes(
            &self
                .packed
                .repack(self.compression, &self.data.encode(), STRUCTURE)?,
        );
        Ok(w.into_bytes())
    }
}

impl SegmentData {
    /// Walks the blocks of decompressed B data in block-table order, then reads the end table.
    pub fn decode(data: &[u8], slots: &[BlockSlot], trailers: bool) -> Result<Self> {
        let mut r = ByteReader::new(data, "B stream blocks");
        let mut blocks = Vec::with_capacity(slots.len());
        for (slot, entry) in slots.iter().enumerate().filter(|(_, e)| e.is_present()) {
            blocks.push(read_block(&mut r, slot, entry.length(), trailers)?);
        }
        let end_table = EndTable::decode(&mut r)?;
        r.finish()?;
        Ok(SegmentData { blocks, end_table })
    }

    /// Encodes blocks with their length echoes [W-04], then the end table.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        for block in &self.blocks {
            w.u32(block.header)
                .bytes(&block.payload)
                .len_u32(block.payload.len())
                .bytes(&block.trailer);
        }
        self.end_table.encode(&mut w);
        w.into_bytes()
    }

    /// Rewrites the lengths of the present block-table entries from the payloads [W-09].
    ///
    /// Absent entries keep their stored value (spec 12.4).
    pub fn sync_block_table(&self, slots: &mut [BlockSlot]) {
        for block in &self.blocks {
            if let Some(slot) = slots.get_mut(block.slot) {
                *slot = BlockSlot(0x8000_0000 | (block.payload.len() as u32 & 0x7FFF_FFFF));
            }
        }
    }
}

fn read_block(r: &mut ByteReader<'_>, slot: usize, length: usize, trailers: bool) -> Result<Block> {
    let header = r.u32("header")?;
    let payload = r.take(length, "payload")?.to_vec();
    let at = r.position();
    let echo = r.u32("length_echo")? as usize;
    if echo != length {
        return Err(CorlissError::invalid(
            "B stream blocks",
            "length_echo",
            at,
            echo,
            length,
        ));
    }
    let start = r.position();
    if trailers {
        skip_trailer(r)?;
    }
    let trailer = r.since(start).to_vec();
    Ok(Block {
        slot,
        header,
        payload,
        trailer,
    })
}

/// Skips a block trailer (spec 12.2); value sizes follow InventorLoader (status L).
fn skip_trailer(r: &mut ByteReader<'_>) -> Result<()> {
    if r.u8("has_properties")? == 0 {
        return Ok(());
    }
    let n = r.u32("n")?;
    if n & 0x8000_0000 != 0 {
        return Ok(());
    }
    for _ in 0..n {
        skip_trailer_property(r)?;
    }
    r.take(4, "list_marker")?;
    let m = r.count("m", 8)?;
    if m > 0 {
        r.take(8, "reserved")?;
        for _ in 0..m {
            r.text8("name")?;
            r.u32("node_ref")?;
        }
    }
    Ok(())
}

fn skip_trailer_property(r: &mut ByteReader<'_>) -> Result<()> {
    r.text8("key")?;
    let at = r.position();
    let width = match r.u32("value_type")? {
        1 => 3,
        3 | 7 => 4,
        8 | 10 => 6,
        11 => 10,
        14 => {
            r.u16("subtype")?;
            r.count("size", 1)?
        }
        other => {
            return Err(CorlissError::invalid(
                "block trailer",
                "value_type",
                at,
                other,
                "1, 3, 7, 8, 10, 11 or 14",
            ));
        }
    };
    r.take(width, "value")?;
    Ok(())
}

impl EndTable {
    fn decode(r: &mut ByteReader<'_>) -> Result<Self> {
        let at = r.position();
        let marker = r.i32("marker")?;
        if marker != -1 {
            return Err(CorlissError::invalid(
                "segment end table",
                "marker",
                at,
                marker,
                -1,
            ));
        }
        let a = r.u16("a")?;
        let b = r.u16("b")?;
        let zero = r.u16("zero")?;
        let indexes = (0..a)
            .map(|_| r.u32("indexes"))
            .collect::<Result<Vec<_>>>()?;
        let n_records = indexes.last().copied().unwrap_or(0) as usize;
        let records = (0..n_records)
            .map(|_| read_record(r))
            .collect::<Result<Vec<_>>>()?;
        Ok(EndTable {
            a,
            b,
            zero,
            indexes,
            records,
        })
    }

    fn encode(&self, w: &mut ByteWriter) {
        w.i32(-1).u16(self.a).u16(self.b).u16(self.zero);
        self.indexes.iter().for_each(|v| {
            w.u32(*v);
        });
        self.records.iter().for_each(|rec| {
            w.bytes(rec);
        });
    }
}

fn read_record(r: &mut ByteReader<'_>) -> Result<[u8; 6]> {
    let mut rec = [0u8; 6];
    rec.copy_from_slice(r.take(6, "records")?);
    Ok(rec)
}
