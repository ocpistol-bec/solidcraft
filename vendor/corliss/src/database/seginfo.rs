//! `RSeStorage/RSeSegInfo`, the segment directory (spec 9.2).

use super::version::Version;
use crate::bytes::{ByteReader, ByteWriter};
use crate::error::{CorlissError, FailureClass, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "RSeSegInfo";
/// Schemas up to this value store `member_offset` as a u32 (spec 9.2.4).
const NARROW_OFFSET_SCHEMA: u32 = 0x1D;

/// A reference from a segment to another segment (spec 9.2.3, variant 0: by segment id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentReference {
    /// Reference state; 0 in every sample (0xFFFF when unresolved).
    pub ref_state: u32,
    /// The referenced segment's revision when the link was recorded (= M `t6` revision).
    pub ref_revision: Guid,
    /// 0, or 1 / 2 for the drawing document DC ↔ sheet DC links.
    pub link_kind: u32,
    /// 0x100 bit of the reference state; 0 in every sample.
    pub ref_flags: u32,
    /// The referenced segment (= M `t6` seg_ref).
    pub ref_seg_id: Guid,
    /// Objects referenced in that segment (= sum of M `t7` counts) [W-09].
    pub ref_count: u32,
}

/// A root record: the mirror of one M `t9` entry (spec 9.2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootRecord {
    /// Index of the root's (segment, revision) pair in M `t7` (= t9 key).
    pub root_key: u16,
    /// Index in M `t6` of the referenced segment.
    pub root_class: u16,
    /// Index in `uid_list1` of the target's class; −1 when the target was not loaded.
    pub root_a: i16,
    /// Index in `uid_list2` of the holder's class.
    pub root_b: i16,
    /// Offset of the reference member inside the holder object (Inventor's memory layout).
    pub member_offset: i64,
    /// High part of the 48-bit object id of the target.
    pub obj_id_hi: u32,
    /// Low part of the object id.
    pub obj_id_lo: u16,
}

impl RootRecord {
    /// The 48-bit object id `(hi << 16) | lo`.
    pub fn object_id(&self) -> u64 {
        (u64::from(self.obj_id_hi) << 16) | u64::from(self.obj_id_lo)
    }
}

/// One segment directory entry (spec 9.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentEntry {
    /// Segment name (= M `name`) [F-04].
    pub name: String,
    /// Segment id (= M `seg_id`) [F-04]; names the M/B streams.
    pub seg_id: Guid,
    /// Revision of the save that last wrote the segment.
    pub revision: Guid,
    /// Huge and region heaps at save time.
    pub heap_stat0: u32,
    /// Heaps at save time.
    pub heap_stat1: u32,
    /// Reserved memory in 8 KiB pages.
    pub heap_pages: u32,
    /// Blocks stored in the B stream [F-05].
    pub n_blocks: u32,
    /// Live objects with an object id at save time.
    pub heap_objects: u32,
    /// len(M `t9`) + 1 [F-05].
    pub n_roots: u32,
    /// Segment type name, e.g. "PmDcSegmentType".
    pub segment_type: String,
    /// Schema level of the segment type.
    pub type_schema: u32,
    /// 0x400 for the current segment of its type, else 0.
    pub active_flag: u32,
    /// Segment version.
    pub version: Version,
    /// GUID list whose meaning is open (OI-014); empty in every sample.
    pub ids: Vec<Guid>,
    /// References into other segments.
    pub refs: Vec<SegmentReference>,
    /// 0, or `n_roots` when the segment has root records.
    pub n_root_records: u32,
    /// Root records (the dummy root 0 is not stored).
    pub roots: Vec<RootRecord>,
}

/// The segment directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegInfo {
    /// Database schema the stream was read with (from RSeDb).
    pub schema: u32,
    /// One entry per persistent segment.
    pub segments: Vec<SegmentEntry>,
    /// Dirty flags of the last save (spec 9.4).
    pub save_flags: u32,
    /// Root target classes; `root_a` indexes it.
    pub uid_list1: Vec<Guid>,
    /// Root holder classes; `root_b` indexes it.
    pub uid_list2: Vec<Guid>,
}

impl SegInfo {
    /// Decodes the stream with the database `schema` of RSeDb.
    pub fn decode(data: &[u8], schema: u32) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        let count = r.count("count", 4)?;
        let segments = (0..count)
            .map(|_| read_entry(&mut r, schema))
            .collect::<Result<Vec<_>>>()?;
        let save_flags = r.u32("save_flags")?;
        let uid_list1 = read_guids(&mut r, "uid_list1")?;
        let uid_list2 = read_guids(&mut r, "uid_list2")?;
        r.finish()?;
        Ok(SegInfo {
            schema,
            segments,
            save_flags,
            uid_list1,
            uid_list2,
        })
    }

    /// Encodes the stream.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        w.len_u32(self.segments.len());
        for entry in &self.segments {
            write_entry(&mut w, entry, self.schema);
        }
        w.u32(self.save_flags);
        write_guids(&mut w, &self.uid_list1);
        write_guids(&mut w, &self.uid_list2);
        w.into_bytes()
    }

    /// The entry named `name`.
    pub fn entry(&self, name: &str) -> Option<&SegmentEntry> {
        self.segments.iter().find(|e| e.name == name)
    }
}

fn read_guids(r: &mut ByteReader<'_>, field: &'static str) -> Result<Vec<Guid>> {
    let n = r.count(field, 16)?;
    (0..n).map(|_| r.guid(field)).collect()
}

fn write_guids(w: &mut ByteWriter, guids: &[Guid]) {
    w.len_u32(guids.len());
    guids.iter().for_each(|g| {
        w.guid(g);
    });
}

fn read_entry(r: &mut ByteReader<'_>, schema: u32) -> Result<SegmentEntry> {
    let name = r.text16("name")?;
    let seg_id = r.guid("seg_id")?;
    let revision = r.guid("revision")?;
    let n_ids = r.count("n_ids", 16)?;
    let n_refs = r.count("n_refs", 49)?;
    let mut entry = SegmentEntry {
        name,
        seg_id,
        revision,
        heap_stat0: r.u32("heap_stat0")?,
        heap_stat1: r.u32("heap_stat1")?,
        heap_pages: r.u32("heap_pages")?,
        n_blocks: r.u32("n_blocks")?,
        heap_objects: r.u32("heap_objects")?,
        n_roots: r.u32("n_roots")?,
        segment_type: r.text16("type")?,
        type_schema: r.u32("type_schema")?,
        active_flag: r.u32("active_flag")?,
        version: Version::read(r, "version")?,
        ids: Vec::new(),
        refs: Vec::new(),
        n_root_records: 0,
        roots: Vec::new(),
    };
    entry.ids = (0..n_ids).map(|_| r.guid("ids")).collect::<Result<_>>()?;
    entry.refs = (0..n_refs)
        .map(|_| read_reference(r))
        .collect::<Result<_>>()?;
    read_roots(r, schema, &mut entry)?;
    Ok(entry)
}

fn read_reference(r: &mut ByteReader<'_>) -> Result<SegmentReference> {
    let ref_state = r.u32("ref_state")?;
    let ref_revision = r.guid("ref_revision")?;
    let link_kind = r.u32("link_kind")?;
    let ref_flags = r.u32("ref_flags")?;
    let at = r.position();
    let variant = r.u8("variant")?;
    if variant != 0 {
        // Variant 1 (RSeRefdSegInfoByRoots) appears in no sample, so its length is not trusted.
        let class = FailureClass::Unsupported {
            reason: format!(
                "segment reference variant {variant}; only variant 0 (by segment id) is specified"
            ),
        };
        return Err(CorlissError::decode(STRUCTURE, "variant", at, class));
    }
    Ok(SegmentReference {
        ref_state,
        ref_revision,
        link_kind,
        ref_flags,
        ref_seg_id: r.guid("ref_seg_id")?,
        ref_count: r.u32("ref_count")?,
    })
}

fn read_roots(r: &mut ByteReader<'_>, schema: u32, entry: &mut SegmentEntry) -> Result<()> {
    entry.n_root_records = r.u32("n_root_records")?;
    let stored = entry.n_root_records.saturating_sub(1) as usize;
    if stored.saturating_mul(18) > r.remaining() {
        let at = r.position() - 4;
        return Err(CorlissError::invalid(
            STRUCTURE,
            "n_root_records",
            at,
            entry.n_root_records,
            "a count that fits the stream",
        ));
    }
    entry.roots = (0..stored)
        .map(|_| read_root(r, schema))
        .collect::<Result<_>>()?;
    Ok(())
}

fn read_root(r: &mut ByteReader<'_>, schema: u32) -> Result<RootRecord> {
    let root_key = r.u16("root_key")?;
    let root_class = r.u16("root_class")?;
    let root_a = r.i16("root_a")?;
    let root_b = r.i16("root_b")?;
    let member_offset = if schema <= NARROW_OFFSET_SCHEMA {
        i64::from(r.u32("member_offset")?)
    } else {
        r.i64("member_offset")?
    };
    Ok(RootRecord {
        root_key,
        root_class,
        root_a,
        root_b,
        member_offset,
        obj_id_hi: r.u32("obj_id_hi")?,
        obj_id_lo: r.u16("obj_id_lo")?,
    })
}

fn write_entry(w: &mut ByteWriter, e: &SegmentEntry, schema: u32) {
    w.text16(&e.name).guid(&e.seg_id).guid(&e.revision);
    w.len_u32(e.ids.len()).len_u32(e.refs.len());
    w.u32(e.heap_stat0).u32(e.heap_stat1).u32(e.heap_pages);
    w.u32(e.n_blocks).u32(e.heap_objects).u32(e.n_roots);
    w.text16(&e.segment_type)
        .u32(e.type_schema)
        .u32(e.active_flag);
    e.version.write(w);
    e.ids.iter().for_each(|g| {
        w.guid(g);
    });
    e.refs
        .iter()
        .for_each(|reference| write_reference(w, reference));
    w.u32(e.n_root_records);
    e.roots.iter().for_each(|root| write_root(w, root, schema));
}

fn write_reference(w: &mut ByteWriter, s: &SegmentReference) {
    w.u32(s.ref_state)
        .guid(&s.ref_revision)
        .u32(s.link_kind)
        .u32(s.ref_flags);
    w.u8(0).guid(&s.ref_seg_id).u32(s.ref_count);
}

fn write_root(w: &mut ByteWriter, root: &RootRecord, schema: u32) {
    w.u16(root.root_key)
        .u16(root.root_class)
        .i16(root.root_a)
        .i16(root.root_b);
    if schema <= NARROW_OFFSET_SCHEMA {
        w.u32(root.member_offset as u32);
    } else {
        w.i64(root.member_offset);
    }
    w.u32(root.obj_id_hi).u16(root.obj_id_lo);
}
