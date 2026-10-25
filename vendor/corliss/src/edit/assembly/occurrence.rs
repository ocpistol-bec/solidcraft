//! Grounding and moving occurrences.

use super::fields::{int, missing, node, node_mut, set_int};
use super::lookup::{
    self, BROWSER, DC, GRAPHICS, GROUNDED, PLACEMENT_RECORDS, RX, T_BROWSER_OCCURRENCE,
    T_OCCURRENCE, T_RX_PLACEMENT,
};
use super::transform::{self, Placement};
use crate::edit::{EditSegment, Editor};
use crate::error::Result;
use crate::grammar::DecodedNode;

/// AmGraphics display group records of an occurrence (u32_1 / occurrence = occurrence id).
const GRAPHICS_GROUPS: [u32; 2] = [0xCA71_63A2, 0xB9D0_D007];
/// Browser status word bits: 0x20 pinned (grounded), 0x2000_0000 free.
const BROWSER_PINNED: i64 = 0x20;
const BROWSER_FREE: i64 = 0x2000_0000;

/// Grounds (or frees) occurrence `name`: the DC object flag 0x02000000 and the browser
/// entry's status word (0x30 grounded, 0x20000010 free).
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::assembly::set_grounded(&mut ed, "ex_rect20:2", true)?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn set_grounded(ed: &mut Editor, name: &str, grounded: bool) -> Result<()> {
    let dc = ed.segment(DC)?;
    let (occ, _) = lookup::occurrence(dc, name)?;
    let n = node_mut(dc, occ)?;
    let flags = int(n, "flags")?;
    let flags = if grounded {
        flags | GROUNDED
    } else {
        flags & !GROUNDED
    };
    set_int(n, "flags", flags)?;
    let browser = ed.segment(BROWSER)?;
    for e in browser.of_type(T_BROWSER_OCCURRENCE) {
        let entry = node_mut(browser, e)?;
        if entry.text("name") == Some(name) {
            set_browser_status(entry, grounded)?;
        }
    }
    Ok(())
}

/// The browser entry's status word (u32_1): 0x20 pinned when grounded, 0x20000000 when free.
fn set_browser_status(entry: &mut DecodedNode, grounded: bool) -> Result<()> {
    let v = int(entry, "u32_1")?;
    let status = match grounded {
        true => (v & !BROWSER_FREE) | BROWSER_PINNED,
        false => (v | BROWSER_FREE) & !BROWSER_PINNED,
    };
    set_int(entry, "u32_1", status)
}

/// Moves occurrence `name` to placement `m` (row-major, translation in column 3, cm) in all
/// three places Inventor keeps it: the AmRx placement, the AmGraphics display group and the
/// AmDc placement record pair. Returns the occurrence id.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// let mut m = corliss::edit::assembly::IDENTITY;
/// m[2][3] = 3.0;
/// corliss::edit::assembly::set_occurrence_transform(&mut ed, "ex_rect20:2", &m)?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn set_occurrence_transform(ed: &mut Editor, name: &str, m: &Placement) -> Result<i64> {
    let (occ, oid) = lookup::occurrence(ed.segment(DC)?, name)?;
    let rx = ed.segment(RX)?;
    let hits = matching(rx, &[T_RX_PLACEMENT], |n| n.int("u32_1") == Some(oid));
    if hits.is_empty() {
        return Err(missing(
            "AmRxSegment",
            format!("placement of occurrence id {oid}"),
        ));
    }
    replace_all(rx, &hits, m)?;
    if ed.seg_info().entry(GRAPHICS).is_some() {
        let g = ed.segment(GRAPHICS)?;
        let is_mine =
            |n: &DecodedNode| n.int("u32_1") == Some(oid) || n.int("occurrence") == Some(oid);
        let groups = matching(g, &GRAPHICS_GROUPS, is_mine);
        replace_all(g, &groups, m)?;
    }
    move_dc_records(ed.segment(DC)?, occ, m)?;
    Ok(oid)
}

/// The AmDc placement record pair at the occurrence's position.
fn move_dc_records(dc: &mut EditSegment, occ: usize, m: &Placement) -> Result<()> {
    let pos = dc
        .of_type(T_OCCURRENCE)
        .iter()
        .position(|i| *i == occ)
        .unwrap_or(0);
    let records = PLACEMENT_RECORDS
        .iter()
        .filter_map(|tid| dc.of_type(*tid).get(pos).copied());
    let records: Vec<usize> = records.collect();
    replace_all(dc, &records, m)
}

/// Nodes of the given types, in type order then node order, that satisfy `keep`.
fn matching(seg: &EditSegment, types: &[u32], keep: impl Fn(&DecodedNode) -> bool) -> Vec<usize> {
    types
        .iter()
        .flat_map(|t| seg.of_type(*t))
        .filter(|i| node(seg, *i).is_ok_and(&keep))
        .collect()
}

fn replace_all(seg: &mut EditSegment, nodes: &[usize], m: &Placement) -> Result<()> {
    for i in nodes {
        transform::replace(node_mut(seg, *i)?, m)?;
    }
    Ok(())
}
