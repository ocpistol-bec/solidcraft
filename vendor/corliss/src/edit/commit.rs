//! Rebuilding an edited segment's B stream, M stream and directory entry (spec clause 20
//! [W-09]).

use std::collections::HashMap;

use super::idset::{self, T5};
use super::segment::EditSegment;
use crate::database::SegmentEntry;
use crate::error::{CorlissError, Result};
use crate::guid::Guid;
use crate::segment::{Block, BlockSlot, MBody};

const STRUCTURE: &str = "segment commit";

/// The rebuilt streams of one segment.
pub(crate) struct Rebuilt {
    pub m: Vec<u8>,
    pub b: Vec<u8>,
}

/// Rebuilds `seg`; updates its RSeSegInfo `entry`.
pub(crate) fn commit(
    seg: &mut EditSegment,
    entry: Option<&mut SegmentEntry>,
    trailers: bool,
) -> Result<Rebuilt> {
    let index = type_index(&seg.segment.meta.body)?;
    let blocks = rebuild_blocks(seg, &index, trailers)?;
    let sizes: Vec<usize> = blocks.iter().map(|b| b.payload.len()).collect();
    let added = seg.nodes.len() as i64 - seg.original_classes.len() as i64;
    update_tables(seg, &sizes, added);
    allocate_new_ids(seg)?;
    seg.segment.data.data.blocks = blocks;
    rederive(seg);
    if let Some(entry) = entry {
        update_entry(entry, &seg.segment.meta.body, added);
    }
    Ok(Rebuilt {
        m: seg.segment.meta.encode()?,
        b: seg.segment.data.encode()?,
    })
}

/// Block table, class counters and slot count after `added` nodes.
fn update_tables(seg: &mut EditSegment, sizes: &[usize], added: i64) {
    let body = &mut seg.segment.meta.body;
    body.blocks = merge_block_table(&body.blocks, sizes);
    bump_counters(
        body,
        &seg.original_classes,
        seg.nodes.iter().map(|n| n.class),
    );
    body.slot_count = (i64::from(body.slot_count) + added).max(0) as u32;
}

fn type_index(body: &MBody) -> Result<HashMap<Guid, usize>> {
    Ok(body
        .types
        .iter()
        .enumerate()
        .map(|(k, t)| (t.class, k))
        .collect())
}

/// Blocks in node order: header (type index in the low byte), payload, trailer.
fn rebuild_blocks(
    seg: &EditSegment,
    index: &HashMap<Guid, usize>,
    trailers: bool,
) -> Result<Vec<Block>> {
    seg.nodes
        .iter()
        .enumerate()
        .map(|(k, n)| {
            let Some(idx) = index.get(&n.class).map(|i| *i as u32) else {
                let found = format!("class {} of node #{}", n.class, k + 1);
                return Err(CorlissError::invalid(
                    STRUCTURE,
                    "types",
                    k,
                    found,
                    "a class of the segment type table",
                ));
            };
            let header = n.header.map_or(0x100 | idx, |h| (h & 0xFFFF_FF00) | idx);
            let trailer = if !trailers || !n.trailer.is_empty() {
                n.trailer.clone()
            } else {
                vec![0]
            };
            Ok(Block {
                slot: k,
                header,
                payload: n.payload(),
                trailer,
            })
        })
        .collect()
}

/// Present entries take the new lengths in order; absent entries stay; extra lengths are
/// appended as present entries.
fn merge_block_table(table: &[BlockSlot], sizes: &[usize]) -> Vec<BlockSlot> {
    let present = |n: usize| BlockSlot(0x8000_0000 | (n as u32 & 0x7FFF_FFFF));
    let mut out = Vec::with_capacity(sizes.len());
    let mut next = sizes.iter();
    for slot in table {
        if !slot.is_present() {
            out.push(*slot);
        } else if let Some(n) = next.next() {
            out.push(present(*n));
        }
    }
    out.extend(next.map(|n| present(*n)));
    out
}

/// Raises each class's instance counter by the number of nodes it gained.
fn bump_counters(body: &mut MBody, before: &[Guid], after: impl Iterator<Item = Guid>) {
    let count = |it: &mut dyn Iterator<Item = Guid>| {
        let mut m: HashMap<Guid, i64> = HashMap::new();
        it.for_each(|g| *m.entry(g).or_default() += 1);
        m
    };
    let (old, new) = (
        count(&mut before.iter().copied()),
        count(&mut after.into_iter()),
    );
    for t in &mut body.types {
        let grow =
            new.get(&t.class).copied().unwrap_or(0) - old.get(&t.class).copied().unwrap_or(0);
        if grow > 0 {
            t.instance_counter += grow as u32;
        }
    }
}

/// Keeps the ids still in use, raises the id space and marks the ids in the allocation set.
fn allocate_new_ids(seg: &mut EditSegment) -> Result<()> {
    let wide = seg.id_space() > 0xFFFF;
    let live: Vec<u64> = seg
        .nodes
        .iter()
        .flat_map(|n| [Some(n.object_id()), if wide { n.wide_id() } else { None }])
        .flatten()
        .collect();
    seg.new_ids.retain(|i| live.contains(i));
    let Some(top) = seg.new_ids.iter().copied().max() else {
        return Ok(());
    };
    let id_space = seg.id_space().max(top);
    let body = &mut seg.segment.meta.body;
    mark_allocated(body, &seg.new_ids, id_space)?;
    if let Some(last) = body.sec2.last_mut() {
        last.1 = id_space as u32;
    }
    if let Some(id) = seg.segment.meta.header.identity.as_mut() {
        id.id_space = id_space;
    }
    Ok(())
}

/// Marks `ids` in the allocation set (t5) and records the new id space there.
fn mark_allocated(body: &mut MBody, ids: &[u64], id_space: u64) -> Result<()> {
    // The t5 split is located by the id space it records, which equals the last sec2 N.
    let recorded = body.sec2.last().map_or(0, |e| e.1);
    let Some(section) = body.tail.t5.as_mut() else {
        return Ok(());
    };
    let Some(mut t5) = T5::split(section, recorded) else {
        return Ok(());
    };
    let (first, bitset) = idset::add(t5.first_type, &t5.bitset, ids, id_space)?;
    t5.first_type = first;
    t5.bitset = bitset;
    t5.id_space = id_space as u32;
    *section = t5.encode();
    Ok(())
}

/// Fields derived from others: end count, root count, id space.
fn rederive(seg: &mut EditSegment) {
    let end_count = seg.segment.data.data.end_table.a;
    let body = &mut seg.segment.meta.body;
    body.end_count = end_count;
    body.n_roots = body.tail.t9.len() as u32 + 1;
    let last = body.sec2.last().map(|e| u64::from(e.1));
    if let (Some(last), Some(id)) = (last, seg.segment.meta.header.identity.as_mut()) {
        id.id_space = last;
    }
}

/// The directory entry's counts and references mirror the M stream (spec 9.2.2–9.2.3).
fn update_entry(entry: &mut SegmentEntry, body: &MBody, added: i64) {
    entry.n_blocks = body.present_blocks() as u32;
    entry.n_roots = body.tail.t9.len() as u32 + 1;
    entry.heap_objects = (i64::from(entry.heap_objects) + added).max(0) as u32;
    let mut sums: HashMap<u16, u32> = HashMap::new();
    for t in &body.tail.t7 {
        *sums.entry(t.segment).or_default() += u32::from(t.count);
    }
    for (k, r) in entry.refs.iter_mut().enumerate() {
        if let Some(link) = body.tail.t6.get(k) {
            r.ref_seg_id = link.seg_ref;
            r.ref_revision = link.revision;
        }
        r.ref_count = sums.get(&(k as u16)).copied().unwrap_or(0);
    }
    if !entry.refs.is_empty() && !entry.roots.is_empty() {
        entry.n_root_records = entry.n_roots;
    }
}
