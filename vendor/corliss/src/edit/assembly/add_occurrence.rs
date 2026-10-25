//! Adding an occurrence of an already placed part (as_place2 -> as_place3): the driver and the
//! AmDc records.

use super::add_occurrence_graphics::{add_graphics, add_tree_state_item, add_ufrx_record};
use super::add_occurrence_rx::{add_browser, add_rx};
use super::fields::{append_ref, find_of_type, int, list_mut, missing, node, node_mut, object_id};
use super::fields::{
    ref_list, ref_of, required_ref, retarget, set, set_int, set_int_list, set_ref,
};
use super::lookup::{self, DC, DESIGN_VIEW, GROUNDED, PLACEMENT_RECORDS, RX};
use super::lookup::{T_CACHE, T_LABEL, T_OCCURRENCE};
use super::order;
use super::remove_occurrence::number_occurrences;
use super::tables;
use super::transform::{self, Placement};
use crate::edit::{EditSegment, Editor};
use crate::error::Result;
use crate::grammar::{DecodedNode, FieldValue};
use crate::guid::Guid;

/// Labels without an object index carry this value.
const NO_INDEX: i64 = 0xFFFF_FFFF;

/// The new occurrence's DC identity.
struct DcAdded {
    /// Its object id.
    object_id: u16,
    /// Its object index (component counter).
    index: i64,
}

/// The four AmDc records of an occurrence: occurrence, label, holder, cache.
struct Records {
    occ: usize,
    label: usize,
    holder: usize,
    cache: usize,
}

/// Adds an instance of the part occurrence `source` places, at placement `m` (cm), named
/// `name` (default: the next free `<part>:<n>`). Clones the source occurrence's records in
/// AmDc (occurrence + label, holder + cache, placement pair), AmRx (placement, FDCD32FE,
/// E5DDE747, A20D9DC5), the browser entry, the AmGraphics display group (without it the
/// occurrence loads but is not drawn), the design-view item and the UFRxDoc record, and links
/// them the way Inventor does (as_place3). Returns the label and the occurrence id.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// let mut m = corliss::edit::assembly::IDENTITY;
/// m[2][3] = 6.0;
/// let (label, id) = corliss::edit::assembly::add_occurrence(&mut ed, "ex_rect20:2", &m, None)?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn add_occurrence(
    ed: &mut Editor,
    source: &str,
    m: &Placement,
    name: Option<&str>,
) -> Result<(String, i64)> {
    let (src, sid) = lookup::occurrence(ed.segment(DC)?, source)?;
    let (oid, taken) = next_occurrence(ed)?;
    let name = name.map_or_else(|| free_label(source, &taken), str::to_string);
    let added = add_dc(ed, src, oid, &name, m)?;
    let rx_ids = add_rx(ed.segment(RX)?, sid, oid, added.index, m)?;
    tables::t9_add_occurrence(ed, DC, Some(rx_ids))?;
    add_browser(ed, source, &name, oid, added.object_id)?;
    add_graphics(ed, sid, oid, m, added.object_id)?;
    add_tree_state_item(ed.segment(DESIGN_VIEW)?)?;
    add_ufrx_record(ed, sid, oid)?;
    tables::bump_revision(ed)?;
    Ok((name, oid))
}

/// The next occurrence id (past UFRxDoc's highest and every occurrence's) and the labels
/// in use.
fn next_occurrence(ed: &mut Editor) -> Result<(i64, Vec<String>)> {
    let ufrx = ed.document().ufrx_doc()?;
    let header = ufrx.root.record("header2");
    let recorded = header
        .and_then(|h| h.uint("max_occurrence_id"))
        .unwrap_or(0) as i64;
    let dc = ed.segment(DC)?;
    let labels = lookup::labels(dc);
    let occs = dc.of_type(T_OCCURRENCE);
    let ids = occs.iter().map(|o| lookup::occurrence_id(dc, *o));
    let top = ids
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(recorded, i64::max);
    let taken = occs.iter().filter_map(|o| labels.get(o).cloned()).collect();
    Ok((top + 1, taken))
}

/// `<part>:<k>` with the smallest k not yet used.
fn free_label(source: &str, taken: &[String]) -> String {
    let base = source.rsplit_once(':').map_or(source, |(b, _)| b);
    (1..)
        .map(|k| format!("{base}:{k}"))
        .find(|n| !taken.contains(n))
        .unwrap_or_default()
}

/// Appends a copy of node `i` with a fresh object id; returns its index.
pub(super) fn append_clone(seg: &mut EditSegment, i: usize) -> Result<usize> {
    let src = seg
        .node(i)
        .cloned()
        .ok_or_else(|| missing("assembly", format!("node #{i}")))?;
    let copy = seg.clone_node(&src, false)?;
    Ok(seg.append(copy))
}

/// The source occurrence's records.
fn source_records(dc: &EditSegment, src: usize) -> Result<(usize, Records)> {
    let s = node(dc, src)?;
    let (comp, holder) = (required_ref(s, "ref")?, required_ref(s, "value")?);
    let cache = required_ref(node(dc, holder)?, "ref_1")?;
    let label = find_of_type(dc, T_LABEL, |n| ref_of(n, "owner") == Some(src));
    let label =
        label.ok_or_else(|| missing("AmDcSegment", format!("label of occurrence #{src}")))?;
    Ok((
        comp,
        Records {
            occ: src,
            label,
            holder,
            cache,
        },
    ))
}

/// The AmDc records: object ids in Inventor's allocation order (as_place3): holder, cache,
/// occurrence, label, then the placement records.
fn add_dc(ed: &mut Editor, src: usize, oid: i64, name: &str, m: &Placement) -> Result<DcAdded> {
    let dc = ed.segment(DC)?;
    let (comp, from) = source_records(dc, src)?;
    let new = clone_records(dc, &from)?;
    let index = link_records(dc, comp, &new, oid, name)?;
    let g = tables::uuid_bytes(ed.new_guid());
    let dc = ed.segment(DC)?;
    set_cache_uids(node_mut(dc, new.cache)?, g)?;
    list_in_containers(dc, comp, new.occ, new.holder)?;
    add_placement_records(dc, src, oid, m)?;
    let map = order::canonicalize(dc)?;
    let occ = map.get(&new.occ).copied().unwrap_or(new.occ);
    Ok(DcAdded {
        object_id: object_id(dc, occ),
        index,
    })
}

/// Copies of the four records, appended in Inventor's allocation order: holder, cache,
/// occurrence, label.
fn clone_records(dc: &mut EditSegment, from: &Records) -> Result<Records> {
    let holder = append_clone(dc, from.holder)?;
    let cache = append_clone(dc, from.cache)?;
    let occ = append_clone(dc, from.occ)?;
    let label = append_clone(dc, from.label)?;
    Ok(Records {
        occ,
        label,
        holder,
        cache,
    })
}

/// Object indexes, refs, flags, label and keys of the new records; returns the occurrence's
/// object index.
fn link_records(
    dc: &mut EditSegment,
    comp: usize,
    new: &Records,
    oid: i64,
    name: &str,
) -> Result<i64> {
    let next = int(node(dc, comp)?, "index")?;
    set_int(node_mut(dc, new.occ)?, "index", next + 1)?;
    set_int(node_mut(dc, new.holder)?, "index", next)?;
    set_int(node_mut(dc, comp)?, "index", next + 2)?;
    link_occurrence(node_mut(dc, new.occ)?, new)?;
    link_label(dc, new, name)?;
    link_holder(node_mut(dc, new.holder)?, new, oid)?;
    rekey(dc, new.occ)?;
    let counter = int(node(dc, comp)?, "u32_0")?;
    set_int(node_mut(dc, comp)?, "u32_0", counter + 1)?;
    Ok(next + 1)
}

/// The occurrence points at its label and holder and starts free (not grounded).
fn link_occurrence(o: &mut DecodedNode, new: &Records) -> Result<()> {
    set_ref(o, &["next"], new.label)?;
    set_ref(o, &["value"], new.holder)?;
    let flags = int(o, "flags")?;
    set_int(o, "flags", flags & !GROUNDED)
}

/// The label names the new occurrence and takes the next label index.
fn link_label(dc: &mut EditSegment, new: &Records, name: &str) -> Result<()> {
    let l = node_mut(dc, new.label)?;
    set_ref(l, &["owner"], new.occ)?;
    set(l, "label", FieldValue::Text(name.to_string()))?;
    let labels = dc
        .of_type(T_LABEL)
        .into_iter()
        .filter_map(|i| dc.node(i)?.node.int("index"));
    let top = labels.filter(|v| *v != NO_INDEX).max().unwrap_or(0);
    set_int(node_mut(dc, new.label)?, "index", top + 1)
}

/// The holder points at the new cache and occurrence and carries the occurrence id.
fn link_holder(h: &mut DecodedNode, new: &Records, oid: i64) -> Result<()> {
    set_ref(h, &["def_state", "ref_1"], new.cache)?;
    let first = list_mut(h, "lst1")?
        .elements
        .first_mut()
        .and_then(|el| el.first_mut());
    if let Some(f) = first {
        retarget(f, new.occ)?;
    }
    set_int(h, "u32_1", oid)
}

/// Inventor numbers the occurrences consecutively from the first occurrence key, then the
/// holders in the same order (as_place3: 4,5,6 / 7,8,9); free-form keys break loading
/// ("Error loading segment AmDcSegment").
fn rekey(dc: &mut EditSegment, new: usize) -> Result<()> {
    let occs = dc.of_type(T_OCCURRENCE);
    let old = occs.iter().filter(|i| **i != new);
    let keys = old
        .map(|i| int(node(dc, *i)?, "u32_0"))
        .collect::<Result<Vec<_>>>()?;
    number_occurrences(dc, &occs, keys.into_iter().min().unwrap_or(0))
}

fn set_cache_uids(cache: &mut DecodedNode, g: Guid) -> Result<()> {
    for f in ["uid1", "uid2", "uid3", "uid4"] {
        set(cache, f, FieldValue::Guid(g))?;
    }
    Ok(())
}

/// The component's objects, the document's occurrence list and the root label's
/// participants (after the last occurrence already listed, as_place3).
fn list_in_containers(dc: &mut EditSegment, comp: usize, occ: usize, holder: usize) -> Result<()> {
    let c = node_mut(dc, comp)?;
    append_ref(c, "objects", holder, None, None)?;
    append_ref(c, "objects", occ, None, None)?;
    append_ref(node_mut(dc, 1)?, "lst1", occ, None, None)?;
    if node(dc, 2)?.type_id.0 != T_LABEL {
        return Ok(());
    }
    let occs: Vec<usize> = dc
        .of_type(T_OCCURRENCE)
        .into_iter()
        .filter(|i| *i != occ)
        .collect();
    let parts = ref_list(node(dc, 2)?, "participants");
    let last = parts.iter().rposition(|p| occs.contains(p));
    let at = last.map_or(parts.len(), |p| p + 1);
    append_ref(node_mut(dc, 2)?, "participants", occ, Some(at), None)
}

/// Copies of the source's placement record pair for occurrence id `oid`, listed in the root
/// cache.
fn add_placement_records(dc: &mut EditSegment, src: usize, oid: i64, m: &Placement) -> Result<()> {
    let pos = dc
        .of_type(T_OCCURRENCE)
        .iter()
        .position(|i| *i == src)
        .unwrap_or(0);
    let root_cache = root_cache(dc)?;
    for tid in PLACEMENT_RECORDS {
        let rec = dc.of_type(tid).get(pos).copied();
        let rec = rec.ok_or_else(|| missing("AmDcSegment", format!("{tid:08X} record {pos}")))?;
        let ni = append_clone(dc, rec)?;
        let r = node_mut(dc, ni)?;
        set_int_list(r, "lst0", &[oid])?;
        transform::replace(r, m)?;
        append_ref(node_mut(dc, root_cache)?, "lst0", ni, None, None)?;
    }
    Ok(())
}

/// The cache that lists the placement records (the first CB072B3B with a filled `lst0`).
fn root_cache(dc: &EditSegment) -> Result<usize> {
    let has_entries = |n: &DecodedNode| {
        let filled = |f: &&crate::grammar::Field| matches!(&f.value, FieldValue::List(l) if !l.elements.is_empty());
        n.fields
            .iter()
            .filter(|f| f.name.as_deref() == Some("lst0"))
            .any(|f| filled(&f))
    };
    let found = find_of_type(dc, T_CACHE, has_entries);
    found.ok_or_else(|| missing("AmDcSegment", "root cache (CB072B3B with records)"))
}
