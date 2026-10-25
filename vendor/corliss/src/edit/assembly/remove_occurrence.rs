//! Deleting an occurrence (as_place3 -> as_del2): the driver
//! and the AmDc / AmRx records.

use super::fields::{find_of_type, first_of_type, int, int_list, missing, node, node_mut};
use super::fields::{object_id, ref_list, ref_of, remove_ref, required_ref, set, set_int};
use super::lookup::{self, DC, DESIGN_VIEW, PLACEMENT_RECORDS, RX, T_CACHE, T_LABEL, T_OCCURRENCE};
use super::remove_constraint::still_used;
use super::remove_occurrence_parts::{
    remove_browser, remove_graphics, remove_tree_state_item, remove_ufrx_record,
};
use super::tables;
use crate::edit::{EditSegment, Editor};
use crate::error::Result;
use crate::grammar::{DecodedNode, FieldValue};

pub(super) const T_RX_ROOT: u32 = 0xEC60_C64B;
pub(super) const T_COMPONENT_DEF: u32 = 0x08B9_DB46;
pub(super) const T_RX_PATH: u32 = 0xA20D_9DC5;

/// The occurrence being removed and where it sat.
pub(super) struct Removed {
    pub sid: i64,
    pub dc_oid: u16,
    pub n_occ: usize,
    pub occ_pos: usize,
}

/// Deletes occurrence `name` the way Inventor does (as_place3 -> as_del2): its AmDc records
/// (occurrence, label, holder, cache, placement pair), AmRx records (placement, FDCD32FE,
/// E5DDE747, A20D9DC5), browser entry, AmGraphics display group, design-view item and UFRxDoc
/// record, with every list, key, index counter and cross-segment t9 table updated. Refuses
/// when anything else (a constraint, pattern, joint …) still references the occurrence.
/// Returns the occurrence id.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::assembly::remove_occurrence(&mut ed, "ex_rect20:2")?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn remove_occurrence(ed: &mut Editor, name: &str) -> Result<i64> {
    let dc = ed.segment(DC)?;
    let (occ, sid) = lookup::occurrence(dc, name)?;
    let occs = dc.of_type(T_OCCURRENCE);
    let occ_pos = occs.iter().position(|i| *i == occ).unwrap_or(0);
    let gone = Removed {
        sid,
        dc_oid: object_id(dc, occ),
        n_occ: occs.len(),
        occ_pos,
    };
    remove_dc(dc, occ, sid, name)?;
    let rx_ids = remove_rx(ed.segment(RX)?, sid)?;
    remove_dc_tables(ed, &gone, &rx_ids)?;
    remove_browser(ed, &gone)?;
    remove_graphics(ed, &gone)?;
    remove_tree_state_item(ed.segment(DESIGN_VIEW)?)?;
    remove_ufrx_record(ed, sid)?;
    tables::bump_revision(ed)?;
    Ok(sid)
}

/// The AmDc records: unlinked from the component, document, root label and caches; the
/// remaining occurrences and holders re-keyed; then deleted.
fn remove_dc(dc: &mut EditSegment, occ: usize, sid: i64, name: &str) -> Result<()> {
    let o = node(dc, occ)?;
    let (comp, holder) = (required_ref(o, "ref")?, required_ref(o, "value")?);
    let cache = required_ref(node(dc, holder)?, "ref_1")?;
    let label = find_of_type(dc, T_LABEL, |n| ref_of(n, "owner") == Some(occ));
    let label = label.ok_or_else(|| missing("AmDcSegment", format!("label of {name}")))?;
    let recs = placement_records(dc, sid);
    unlink_from_component(dc, comp, occ, holder)?;
    unlink_from_lists(dc, occ, &recs)?;
    rekey(dc, occ)?;
    let dead: Vec<usize> = [occ, label, holder, cache]
        .into_iter()
        .chain(recs)
        .collect();
    let deleted = dc.delete(&dead).map(|_| ());
    deleted.map_err(|e| still_used("assembly occurrence", occ, format!("{name}: {e}")))
}

/// The AmDc placement records of occurrence id `sid`.
fn placement_records(dc: &EditSegment, sid: i64) -> Vec<usize> {
    let is_mine = |n: &DecodedNode| int_list(n, "lst0") == [sid];
    let records = PLACEMENT_RECORDS.iter().flat_map(|t| dc.of_type(*t));
    records
        .filter(|i| node(dc, *i).is_ok_and(is_mine))
        .collect()
}

/// The component's object list loses the holder and the occurrence; its a0 goes (256, 0) ->
/// (0, 0) when Inventor deletes one of its occurrences (as_del2; meaning unknown, mirrored).
fn unlink_from_component(
    dc: &mut EditSegment,
    comp: usize,
    occ: usize,
    holder: usize,
) -> Result<()> {
    let c = node_mut(dc, comp)?;
    remove_ref(c, "objects", holder, false)?;
    remove_ref(c, "objects", occ, false)?;
    let Some(FieldValue::U16s(a0)) = c.get("a0").cloned() else {
        return Ok(());
    };
    let zeroed: Vec<u16> = std::iter::once(0).chain(a0.into_iter().skip(1)).collect();
    set(c, "a0", FieldValue::U16s(zeroed))
}

/// The document's occurrence list, the root label's participants and the caches' record
/// lists.
fn unlink_from_lists(dc: &mut EditSegment, occ: usize, recs: &[usize]) -> Result<()> {
    remove_ref(node_mut(dc, 1)?, "lst1", occ, false)?;
    let root_label = node_mut(dc, 2)?;
    if root_label.type_id.0 == T_LABEL {
        remove_ref(root_label, "participants", occ, false)?;
    }
    let has_list = |n: &DecodedNode| {
        n.fields
            .iter()
            .any(|f| f.name.as_deref() == Some("lst0") && matches!(f.value, FieldValue::List(_)))
    };
    for cache in dc.of_type(T_CACHE) {
        let n = node_mut(dc, cache)?;
        if has_list(n) {
            recs.iter()
                .try_for_each(|r| remove_ref(n, "lst0", *r, false).map(|_| ()))?;
        }
    }
    Ok(())
}

/// Occurrence keys stay consecutive from the first one, the holders' keys follow them.
fn rekey(dc: &mut EditSegment, occ: usize) -> Result<()> {
    let occs = dc.of_type(T_OCCURRENCE);
    let keys = occs
        .iter()
        .map(|i| int(node(dc, *i)?, "u32_0"))
        .collect::<Result<Vec<_>>>()?;
    let base = keys.into_iter().min().unwrap_or(0);
    let rest: Vec<usize> = occs.into_iter().filter(|i| *i != occ).collect();
    number_occurrences(dc, &rest, base)
}

/// Keys `base`, `base + 1` … for `occs`, then for their holders.
pub(super) fn number_occurrences(dc: &mut EditSegment, occs: &[usize], base: i64) -> Result<()> {
    for (k, o) in occs.iter().enumerate() {
        set_int(node_mut(dc, *o)?, "u32_0", base + k as i64)?;
    }
    for (k, o) in occs.iter().enumerate() {
        let holder = required_ref(node(dc, *o)?, "value")?;
        set_int(
            node_mut(dc, holder)?,
            "u32_0",
            base + (occs.len() + k) as i64,
        )?;
    }
    Ok(())
}

/// The AmRx records; returns the object ids of the placement and its FDCD32FE record.
fn remove_rx(rx: &mut EditSegment, sid: i64) -> Result<[u16; 2]> {
    let p = lookup::rx_placement(rx, sid)?;
    let f = required_ref(node(rx, p)?, "ref_0")?;
    let e = required_ref(node(rx, f)?, "ref_0")?;
    let ids = [p, f].map(|i| object_id(rx, i));
    let root = first_of_type(rx, T_RX_ROOT)?;
    remove_ref(node_mut(rx, root)?, "lst0", p, false)?;
    let mut dead = vec![p, f, e];
    dead.extend(remove_path_records(rx, sid)?);
    rx.delete(&dead).map(|_| ids)
}

/// Unlists each component definition's A20D9DC5 path record of occurrence `sid`; returns
/// them.
fn remove_path_records(rx: &mut EditSegment, sid: i64) -> Result<Vec<usize>> {
    let mut dead = Vec::new();
    let is_mine = |n: &DecodedNode| n.type_id.0 == T_RX_PATH && int_list(n, "lst0") == [sid];
    for comp_def in rx.of_type(T_COMPONENT_DEF) {
        for k in ref_list(node(rx, comp_def)?, "lst0") {
            if rx.node(k).is_some_and(|n| is_mine(&n.node)) {
                remove_ref(node_mut(rx, comp_def)?, "lst0", k, false)?;
                dead.push(k);
            }
        }
    }
    Ok(dead)
}

/// The AmDc t9 runs naming the AmRx records, and the t7 entry that owned them.
fn remove_dc_tables(ed: &mut Editor, gone: &Removed, rx_ids: &[u16; 2]) -> Result<()> {
    let t9 = tables::t9(ed, DC)?;
    let cut = tables::second_run_start(&t9)?;
    let head = cut.saturating_sub(gone.n_occ);
    tables::t9_remove(ed, DC, rx_ids, &[(head, cut), (cut, t9.len())])?;
    let obj = tables::last_t7_object(ed, DC)?;
    tables::t7_release(ed, DC, obj, gone.n_occ, gone.occ_pos, 2).map(|_| ())
}
