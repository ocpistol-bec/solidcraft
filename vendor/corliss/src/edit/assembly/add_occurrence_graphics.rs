//! Adding an occurrence: the AmGraphics display group, the design-view item and the UFRxDoc
//! record.

use std::collections::{HashMap, VecDeque};

use super::add_occurrence::append_clone;
use super::add_occurrence_rx::max_of;
use super::fields::{append_ref, find_of_type, missing, node, node_mut, ref_list, required_ref};
use super::fields::{set_int, set_ref};
use super::lookup::GRAPHICS;
use super::order;
use super::remove_occurrence_parts::{
    T_DISPLAY_GROUP, T_TREE_STATE, change_file_count, set_tree_state, tree_state,
};
use super::tables;
use super::transform::{self, Placement};
use super::ufrx_records::{self, occ_ids, set_occ_ids, set_uint};
use crate::edit::{EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::RefKind;
use crate::ufrx::Record;

/// Clones the source occurrence's AmGraphics display group: CA7163A2 (u32_1 / u32_3 =
/// occurrence id, u32_2 = group number, obj -> the group) and its 61530B1E subtree (indexDC =
/// group number), both listed in the root's `parts`; t9 gains (next key, 0, 0, AmDc
/// occurrence object id) (as_place3).
pub(super) fn add_graphics(
    ed: &mut Editor,
    sid: i64,
    oid: i64,
    m: &Placement,
    dc_oid: u16,
) -> Result<()> {
    let g = ed.segment(GRAPHICS)?;
    let ca = display_group(g, sid)?;
    let grp = required_ref(node(g, ca)?, "obj")?;
    let clones = clone_subtree(g, grp)?;
    let nca = append_clone(g, ca)?;
    let group = clones.first().map_or(grp, |(_, new)| *new);
    link_display_group(g, nca, group, oid, m)?;
    let root = node_mut(g, 1)?;
    append_ref(root, "parts", group, None, None)?;
    append_ref(root, "parts", nca, None, None)?;
    let others = clones
        .iter()
        .map(|(_, n)| *n)
        .filter(|n| *n != group)
        .collect();
    place_graphics(g, group, nca, others)?;
    tables::t9_append(ed, GRAPHICS, dc_oid)?;
    tables::t7_add(ed, GRAPHICS, 0, 1)
}

/// The display group record (CA7163A2) of occurrence id `sid`.
fn display_group(g: &EditSegment, sid: i64) -> Result<usize> {
    let ca = find_of_type(g, T_DISPLAY_GROUP, |n| n.int("u32_1") == Some(sid));
    ca.ok_or_else(|| {
        missing(
            "AmGraphicsSegment",
            format!("display group of occurrence id {sid}"),
        )
    })
}

/// The new display group record: occurrence id, next group number, its group and placement.
fn link_display_group(
    g: &mut EditSegment,
    nca: usize,
    group: usize,
    oid: i64,
    m: &Placement,
) -> Result<()> {
    let num = max_of(g, &g.of_type(T_DISPLAY_GROUP), "u32_2")? + 1;
    let c = node_mut(g, nca)?;
    set_int(c, "u32_1", oid)?;
    set_int(c, "u32_3", oid)?;
    set_int(c, "u32_2", num)?;
    set_ref(c, &["obj"], group)?;
    transform::replace(c, m)?;
    set_int(node_mut(g, group)?, "indexDC", num)
}

/// Inventor's order (as_place3): the two new root children right after the existing ones,
/// every other new node after the last node of its type.
fn place_graphics(
    g: &mut EditSegment,
    group: usize,
    nca: usize,
    mut others: Vec<usize>,
) -> Result<()> {
    let kids = ref_list(node(g, 1)?, "parts");
    let mut last_kid = kids.into_iter().filter(|i| *i != group && *i != nca).max();
    let mut tracked = [group, nca];
    for k in 0..2 {
        let map = order::move_after(g, tracked[k], last_kid)?;
        let follow = |i: &usize| map.get(i).copied().unwrap_or(*i);
        tracked = tracked.map(|i| follow(&i));
        others = others.iter().map(follow).collect();
        last_kid = Some(tracked[k]);
    }
    order::group_with_type(g, &others).map(|_| ())
}

/// Copies `root` and every node reachable from it over child refs (breadth first); refs
/// between the copies point at the copies. Returns (old, new) pairs in copy order.
fn clone_subtree(seg: &mut EditSegment, root: usize) -> Result<Vec<(usize, usize)>> {
    let order = child_order(seg, root);
    let pairs = order
        .iter()
        .map(|i| append_clone(seg, *i).map(|n| (*i, n)))
        .collect::<Result<Vec<_>>>()?;
    let map: HashMap<usize, usize> = pairs.iter().copied().collect();
    for (_, new) in &pairs {
        node_mut(seg, *new)?.remap_refs(&|i| map.get(&i).copied());
    }
    Ok(pairs)
}

/// `root` and the nodes reachable over child refs, breadth first.
fn child_order(seg: &EditSegment, root: usize) -> Vec<usize> {
    let (mut order, mut queue) = (Vec::new(), VecDeque::from([root]));
    while let Some(i) = queue.pop_front() {
        if order.contains(&i) {
            continue;
        }
        order.push(i);
        let refs = seg.node(i).map(|n| n.node.refs()).unwrap_or_default();
        queue.extend(
            refs.into_iter()
                .filter(|(_, k)| *k == RefKind::Child)
                .filter_map(|(r, _)| r.index()),
        );
    }
    order
}

/// DesignViewSegment tree state: a0[0] = 0x8000 | (3 + top-level browser items), then
/// per-item words and a 1 marker (as_place2 (0x8005, 0x8003, 0, 0, 0, 0, 1, 0, 0, 0) ->
/// as_place3 (0x8006, 0x8003, 0, 0, 0, 0, 0, 1, 0, 0, 0)): a new top-level item bumps a0[0]
/// and inserts a 0 before the marker.
pub(super) fn add_tree_state_item(dv: &mut EditSegment) -> Result<()> {
    for n in dv.of_type(T_TREE_STATE) {
        let node = node_mut(dv, n)?;
        let mut a0 = tree_state(node);
        let Some(k) = (2..a0.len()).find(|i| a0[*i] == 1) else {
            let found = format!("{a0:?}");
            return Err(CorlissError::invalid(
                "design view",
                "a0",
                n,
                found,
                "tree-state words with a 1 marker",
            ));
        };
        a0[0] += 1;
        a0.insert(k, 0);
        set_tree_state(node, &a0)?;
    }
    Ok(())
}

/// The UFRxDoc occurrence record: a copy of the source's with the new id, order and
/// instance number; the file's count and the document's highest occurrence id follow.
pub(super) fn add_ufrx_record(ed: &mut Editor, sid: i64, oid: i64) -> Result<()> {
    ufrx_records::edit(ed, |root| {
        let (mut rec, order) = source_record(root, sid)?;
        let ref_id = occ_ids(&rec).map_or(0, |v| v[1]);
        let n = change_file_count(root, ref_id, 1)?;
        set_occ_ids(&mut rec, [0, ref_id, oid as u32, order, n]);
        let occ = ufrx_records::record_mut(root, "occurrences")?;
        ufrx_records::list_mut(occ, "entries")?.push(rec);
        ufrx_records::sync_counts(occ)?;
        set_uint(
            ufrx_records::record_mut(root, "header2")?,
            "max_occurrence_id",
            oid as u64,
        )
    })
}

/// A copy of the occurrence record of `sid` and the order number a new record takes.
fn source_record(root: &mut Record, sid: i64) -> Result<(Record, u32)> {
    let occ = ufrx_records::record_mut(root, "occurrences")?;
    let entries = ufrx_records::list_mut(occ, "entries")?;
    let src = entries
        .iter()
        .find(|o| occ_ids(o).is_some_and(|v| i64::from(v[2]) == sid));
    let rec = src
        .cloned()
        .ok_or_else(|| missing("UFRxDoc", format!("occurrence record of id {sid}")))?;
    Ok((rec, entries.len() as u32 + 1))
}
