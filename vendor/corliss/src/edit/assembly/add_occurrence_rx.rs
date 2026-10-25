//! Adding an occurrence: the AmRx and browser records.

use super::add_occurrence::append_clone;
use super::fields::{append_ref, find_of_type, first_of_type, int, int_list, missing, node};
use super::fields::{node_mut, object_id, ref_list, required_ref, same_width, set, set_int};
use super::fields::{set_int_list, set_ref};
use super::lookup::is_browser_indexed;
use super::lookup::{
    self, BROWSER, T_BROWSER_ARRANGEMENT, T_BROWSER_DOCUMENT, T_BROWSER_OCCURRENCE,
};
use super::order;
use super::remove_occurrence::{T_COMPONENT_DEF, T_RX_PATH, T_RX_ROOT};
use super::tables;
use super::transform::{self, Placement};
use crate::edit::{EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, Field, FieldValue};

/// Browser status word bits: 0x20 pinned (grounded), 0x2000_0000 free.
const BROWSER_PINNED: i64 = 0x20;
const BROWSER_FREE: i64 = 0x2000_0000;

/// The AmRx records: FDCD32FE, E5DDE747 and the placement (object ids in that order, as
/// Inventor allocates them; as_place2: 4/5/6, 7/8/9), and the A20D9DC5 path record of each
/// component definition. Returns the object ids of the placement and its FDCD32FE record.
pub(super) fn add_rx(
    rx: &mut EditSegment,
    sid: i64,
    oid: i64,
    dc_index: i64,
    m: &Placement,
) -> Result<(u16, u16)> {
    let place = lookup::rx_placement(rx, sid)?;
    let fd = required_ref(node(rx, place)?, "ref_0")?;
    let e5 = required_ref(node(rx, fd)?, "ref_0")?;
    let (nf, ne) = (append_clone(rx, fd)?, append_clone(rx, e5)?);
    let np = append_clone(rx, place)?;
    link_placement(node_mut(rx, np)?, nf, oid, dc_index, m)?;
    let f = node_mut(rx, nf)?;
    set_int(f, "u32_0", oid)?;
    set_ref(f, &["ref_0"], ne)?;
    let root = first_of_type(rx, T_RX_ROOT)?;
    append_ref(node_mut(rx, root)?, "lst0", np, None, None)?;
    let new: Vec<usize> = [np, nf, ne]
        .into_iter()
        .chain(add_path_records(rx, sid, oid)?)
        .collect();
    // Inventor keeps each record kind together: the new records go right after the last
    // existing record of the same type (as_place3)
    let placed = order::group_with_type(rx, &new)?;
    Ok((object_id(rx, placed[0]), object_id(rx, placed[1])))
}

/// The placement's occurrence id, AmDc object index, FDCD32FE record and transform.
fn link_placement(
    p: &mut DecodedNode,
    nf: usize,
    oid: i64,
    dc_index: i64,
    m: &Placement,
) -> Result<()> {
    set_int(p, "u32_1", oid)?;
    set_tail_u32(p, dc_index)?;
    set_ref(p, &["ref_0"], nf)?;
    transform::replace(p, m)
}

/// The placement's trailing u32 is the AmDc occurrence's object index (as_place2: 10, 13) -
/// a cross-segment reference; a stale value fails the load ("Invalid reference to segment
/// object used").
fn set_tail_u32(p: &mut DecodedNode, v: i64) -> Result<()> {
    let tid = p.type_id;
    let last = p
        .fields
        .last_mut()
        .ok_or_else(|| missing("AmRxSegment", format!("trailing u32 of {tid}")))?;
    if matches!(last.value, FieldValue::Raw) && last.raw.len() >= 4 {
        let at = last.raw.len() - 4;
        last.raw[at..].copy_from_slice(&(v as u32).to_le_bytes());
        return Ok(());
    }
    let named = matches!(last.name.as_deref(), Some("s32_skip" | "index"));
    if !named || !matches!(last.value, FieldValue::U32(_) | FieldValue::I32(_)) {
        return Err(bad_tail(last));
    }
    let value = same_width(&last.value, i64::from(v as u32 as i32))?;
    last.set(value)
}

fn bad_tail(last: &Field) -> CorlissError {
    let found = format!("{:?} {:?}", last.name, last.value);
    CorlissError::invalid(
        "AmRx placement",
        "tail",
        0,
        found,
        "a trailing raw u32 or index field",
    )
}

/// A copy of each component definition's A20D9DC5 path record of occurrence `sid`, pointed
/// at `oid`; returns the last copy.
fn add_path_records(rx: &mut EditSegment, sid: i64, oid: i64) -> Result<Option<usize>> {
    let is_mine = |n: &DecodedNode| n.type_id.0 == T_RX_PATH && int_list(n, "lst0") == [sid];
    let mut last = None;
    for comp_def in rx.of_type(T_COMPONENT_DEF) {
        let kids = ref_list(node(rx, comp_def)?, "lst0");
        let Some(mine) = kids
            .into_iter()
            .find(|k| rx.node(*k).is_some_and(|n| is_mine(&n.node)))
        else {
            continue;
        };
        let na = append_clone(rx, mine)?;
        set_int_list(node_mut(rx, na)?, "lst0", &[oid])?;
        append_ref(node_mut(rx, comp_def)?, "lst0", na, None, None)?;
        last = Some(na);
    }
    Ok(last)
}

/// The browser entry: a copy of the source's, in the shared index counter right after the
/// last occurrence entry (later entries shift up, as_place3), stored after the last
/// occurrence entry, with its t9 entry.
pub(super) fn add_browser(
    ed: &mut Editor,
    source: &str,
    name: &str,
    oid: i64,
    dc_oid: u16,
) -> Result<()> {
    let br = ed.segment(BROWSER)?;
    let entries = br.of_type(T_BROWSER_OCCURRENCE);
    let be = find_of_type(br, T_BROWSER_OCCURRENCE, |n| n.text("name") == Some(source));
    let be = be.ok_or_else(|| missing("AmBrowserSegment", format!("entry of {source}")))?;
    let nb = append_clone(br, be)?;
    set(
        node_mut(br, nb)?,
        "name",
        FieldValue::Text(name.to_string()),
    )?;
    let newi = max_of(br, &br.of_type(T_BROWSER_OCCURRENCE), "index")? + 1;
    raise_indexes(br, nb, newi)?;
    set_int(node_mut(br, nb)?, "index", newi)?;
    set_int(node_mut(br, nb)?, "u32_2", oid)?;
    set_group_index(br, nb, &br.of_type(T_BROWSER_OCCURRENCE))?;
    free_entry(node_mut(br, nb)?)?;
    let doc = first_of_type(br, T_BROWSER_DOCUMENT)?;
    append_ref(node_mut(br, doc)?, "lst0", nb, None, None)?;
    order::move_after(br, nb, entries.iter().copied().max())?;
    tables::t9_add_browser(ed, BROWSER, dc_oid)
}

/// A new occurrence starts free: status word without the pin bit, with the free bit.
fn free_entry(b: &mut DecodedNode) -> Result<()> {
    let status = int(b, "u32_1")?;
    set_int(b, "u32_1", (status | BROWSER_FREE) & !BROWSER_PINNED)
}

/// The largest value of integer field `name` over `nodes`.
pub(super) fn max_of(seg: &EditSegment, nodes: &[usize], name: &str) -> Result<i64> {
    let values = nodes
        .iter()
        .map(|i| int(node(seg, *i)?, name))
        .collect::<Result<Vec<_>>>()?;
    Ok(values.into_iter().max().unwrap_or(0))
}

/// Entries at or above `newi` in the shared index counter move up one.
pub(super) fn raise_indexes(br: &mut EditSegment, new: usize, newi: i64) -> Result<()> {
    for i in (1..=br.nodes.len()).filter(|i| *i != new) {
        let n = node_mut(br, i)?;
        let iv = n.int("index").unwrap_or(0);
        if is_browser_indexed(n.type_id.0) && iv != 0 && iv >= newi {
            set_int(n, "index", iv + 1)?;
        }
    }
    Ok(())
}

/// The new entry and the arrangement take the next graphics index (+2 past the largest).
pub(super) fn set_group_index(br: &mut EditSegment, new: usize, peers: &[usize]) -> Result<()> {
    let arr = first_of_type(br, T_BROWSER_ARRANGEMENT)?;
    let gi = int(node(br, arr)?, "grIdx")?.max(max_of(br, peers, "grIdx")?) + 2;
    set_int(node_mut(br, new)?, "grIdx", gi)?;
    set_int(node_mut(br, arr)?, "grIdx", gi)
}
