//! Deleting an occurrence: the browser, AmGraphics, design-view and UFRxDoc records.

use std::collections::BTreeSet;

use super::fields::{find_of_type, first_of_type, int, missing, named_refs, node, node_mut};
use super::fields::{remove_ref, required_ref, set, set_int};
use super::lookup::{
    BROWSER, GRAPHICS, T_BROWSER_DOCUMENT, T_BROWSER_OCCURRENCE, is_browser_indexed,
};
use super::remove_constraint::lower_index;
use super::remove_occurrence::Removed;
use super::tables;
use super::ufrx_records::{self, occ_ids, set_occ_ids, set_uint};
use crate::edit::{EditSegment, Editor};
use crate::error::Result;
use crate::grammar::{DecodedNode, FieldValue, RefKind};
use crate::ufrx::Record;

pub(super) const T_DISPLAY_GROUP: u32 = 0xCA71_63A2;
pub(super) const T_TREE_STATE: u32 = 0xEBA3_1E2E;

/// The browser entry, the shared index counter, the document entry's list and the t9 entry.
pub(super) fn remove_browser(ed: &mut Editor, gone: &Removed) -> Result<()> {
    let br = ed.segment(BROWSER)?;
    let b = find_of_type(br, T_BROWSER_OCCURRENCE, |n| {
        n.int("u32_2") == Some(gone.sid)
    });
    let b = b.ok_or_else(|| missing("AmBrowserSegment", format!("entry of id {}", gone.sid)))?;
    let bi = node(br, b)?.int("index").unwrap_or(0);
    for i in (1..=br.nodes.len()).filter(|i| *i != b) {
        let n = node_mut(br, i)?;
        if is_browser_indexed(n.type_id.0) {
            lower_index(n, bi)?;
        }
    }
    let doc = first_of_type(br, T_BROWSER_DOCUMENT)?;
    remove_ref(node_mut(br, doc)?, "lst0", b, true)?;
    br.delete(&[b])?;
    remove_browser_root(ed, gone)
}

/// The browser t9 entry of the occurrence and the t7 entry that owned it.
fn remove_browser_root(ed: &mut Editor, gone: &Removed) -> Result<()> {
    let t9 = tables::t9(ed, BROWSER)?;
    let end1 = tables::first_run_end(&t9)?;
    let (_, pos) = tables::t9_remove(ed, BROWSER, &[gone.dc_oid], &[(0, end1)])?;
    tables::shorten_browser_run(ed, BROWSER)?;
    tables::t7_release(ed, BROWSER, 0, end1, pos.unwrap_or(0), 1).map(|_| ())
}

/// The AmGraphics display group: its subtree goes, later groups move down one number.
pub(super) fn remove_graphics(ed: &mut Editor, gone: &Removed) -> Result<()> {
    let g = ed.segment(GRAPHICS)?;
    let Some(ca) = find_of_type(g, T_DISPLAY_GROUP, |n| n.int("u32_1") == Some(gone.sid)) else {
        return Ok(());
    };
    let grp = required_ref(node(g, ca)?, "obj")?;
    let mut dead = child_subtree(g, grp);
    renumber_groups(g, ca)?;
    let root = node_mut(g, 1)?;
    remove_ref(root, "parts", grp, false)?;
    remove_ref(root, "parts", ca, false)?;
    dead.insert(ca);
    g.delete(&dead.into_iter().collect::<Vec<_>>())?;
    remove_graphics_root(ed, gone)
}

/// The graphics t9 entry of the occurrence and the t7 entry that owned it.
fn remove_graphics_root(ed: &mut Editor, gone: &Removed) -> Result<()> {
    let t9 = tables::t9(ed, GRAPHICS)?;
    let start = t9.iter().position(|e| e[0] != 0);
    let start = start.ok_or_else(|| missing("AmGraphicsSegment", "t9 occurrence run"))?;
    tables::t9_remove(ed, GRAPHICS, &[gone.dc_oid], &[(start, t9.len())])?;
    tables::t7_release(ed, GRAPHICS, 0, gone.n_occ, gone.occ_pos, 1).map(|_| ())
}

/// `root` and every node reachable from it over child refs.
fn child_subtree(seg: &EditSegment, root: usize) -> BTreeSet<usize> {
    let (mut set, mut stack) = (BTreeSet::new(), vec![root]);
    while let Some(i) = stack.pop() {
        if !set.insert(i) {
            continue;
        }
        let refs = seg.node(i).map(|n| named_refs(&n.node)).unwrap_or_default();
        stack.extend(
            refs.into_iter()
                .filter(|(k, _, _)| *k == RefKind::Child)
                .map(|(_, t, _)| t),
        );
    }
    set
}

/// Later display groups move down one number (as_del2).
fn renumber_groups(g: &mut EditSegment, ca: usize) -> Result<()> {
    let num = int(node(g, ca)?, "u32_2")?;
    for c2 in g.of_type(T_DISPLAY_GROUP).into_iter().filter(|c| *c != ca) {
        let n = node_mut(g, c2)?;
        let v = int(n, "u32_2")?;
        if v <= num {
            continue;
        }
        set_int(n, "u32_2", v - 1)?;
        let obj = required_ref(n, "obj")?;
        let group = node_mut(g, obj)?;
        if let Some(index) = group.int("indexDC") {
            set_int(group, "indexDC", index - 1)?;
        }
    }
    Ok(())
}

/// DesignViewSegment tree state: one top-level item fewer, its 0 word before the 1 marker
/// goes.
pub(super) fn remove_tree_state_item(dv: &mut EditSegment) -> Result<()> {
    for n in dv.of_type(T_TREE_STATE) {
        let node = node_mut(dv, n)?;
        let mut a0 = tree_state(node);
        let Some(k) = (2..a0.len()).find(|i| a0[*i] == 1) else {
            continue;
        };
        if a0[k - 1] != 0 {
            continue;
        }
        a0[0] -= 1;
        a0.remove(k - 1);
        set_tree_state(node, &a0)?;
    }
    Ok(())
}

/// The tree-state words (`a0`).
pub(super) fn tree_state(node: &DecodedNode) -> Vec<i64> {
    node.get("a0")
        .and_then(FieldValue::as_ints)
        .unwrap_or_default()
}

/// Writes the tree-state words and their count.
pub(super) fn set_tree_state(node: &mut DecodedNode, a0: &[i64]) -> Result<()> {
    set(
        node,
        "a0",
        FieldValue::U16s(a0.iter().map(|v| *v as u16).collect()),
    )?;
    set_int(node, "n", a0.len() as i64)
}

/// The UFRxDoc occurrence record; later records renumber, the file's count drops.
pub(super) fn remove_ufrx_record(ed: &mut Editor, sid: i64) -> Result<()> {
    ufrx_records::edit(ed, |root| {
        let occ = ufrx_records::record_mut(root, "occurrences")?;
        let entries = ufrx_records::list_mut(occ, "entries")?;
        let at = entries
            .iter()
            .position(|o| occ_ids(o).is_some_and(|v| i64::from(v[2]) == sid));
        let at = at.ok_or_else(|| missing("UFRxDoc", format!("occurrence record of id {sid}")))?;
        let ref_id = occ_ids(&entries[at]).map_or(0, |v| v[1]);
        entries.remove(at);
        renumber_records(entries);
        ufrx_records::sync_counts(occ)?;
        change_file_count(root, ref_id, -1).map(|_| ())
    })
}

/// Occurrence records keep 1-based consecutive order numbers.
fn renumber_records(entries: &mut [Record]) {
    for (k, o) in entries.iter_mut().enumerate() {
        let Some(mut v) = occ_ids(o).and_then(|v| <[u32; 5]>::try_from(v).ok()) else {
            continue;
        };
        v[3] = k as u32 + 1;
        set_occ_ids(o, v);
    }
}

/// Adds `delta` to the occurrence count of the referenced file `ref_id`; returns the new
/// count.
pub(super) fn change_file_count(root: &mut Record, ref_id: u32, delta: i64) -> Result<u32> {
    let files = ufrx_records::list_mut(ufrx_records::record_mut(root, "inv_files")?, "files")?;
    let file = files
        .iter_mut()
        .find(|f| f.uint("ref_id") == Some(u64::from(ref_id)));
    let file = file.ok_or_else(|| missing("UFRxDoc", format!("inv_file of ref_id {ref_id}")))?;
    let n = (file.uint("n_occurrences").unwrap_or(0) as i64 + delta).max(0) as u64;
    set_uint(file, "n_occurrences", n)?;
    Ok(n as u32)
}
