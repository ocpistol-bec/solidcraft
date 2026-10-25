//! Copying an assembly constraint from another assembly (as_place2 / as_delmate -> as_mate):
//! the driver, the browser entry, the UFRxDoc occurrence
//! groups and the RSeSegInfo class lists.

use std::collections::HashMap;

use super::add_constraint_dc::{OccurrenceMap, copy_dc, counterpart};
use super::add_occurrence_rx::{max_of, raise_indexes, set_group_index};
use super::fields::set_int;
use super::fields::{append_ref, find_of_type, list, missing, named_refs, node, node_mut, set};
use super::lookup::{self, BROWSER, BROWSER_CONSTRAINTS, DC, T_BROWSER_OCCURRENCE};
use super::order;
use super::root_classes;
use super::tables;
use super::ufrx_records::{self, GROUP_BASE, group_id, set_uint};
use crate::document::Document;
use crate::edit::{EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::FieldValue;
use crate::guid::Guid;
use crate::ufrx::Record;

const T_ORIGIN_ENTRY: u32 = 0x9180_FF9F;
const TEMPLATE_BROWSER: &str = "template AmBrowserSegment";

/// Copies assembly constraint `name` from `template` (an assembly that places the same part
/// files) into the edited document. `occurrences` maps the template's occurrence labels to
/// the document's, in order. Everything that belongs to the constraint (selections, value
/// parameter, proxy planes, creators with their part face names, geometry sources,
/// name-table entries) is copied and re-linked, missing classes are added to the M type
/// tables, and the browser entry, browser t9 entry, UFRxDoc occurrence groups and
/// RSeSegInfo class lists are written the way Inventor stores them.
///
/// The occurrences keep their unconstrained solver state; Inventor solves the new
/// constraint on Rebuild All.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let template = corliss::Document::open("as_mate.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// let pairs = [("ex_rect20:1", "ex_rect20:1"), ("ex_rect20:2", "ex_rect20:2")];
/// corliss::edit::assembly::add_constraint_from(&mut ed, &template, "Mate:1", &pairs)?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn add_constraint_from(
    ed: &mut Editor,
    template: &Document,
    name: &str,
    occurrences: &[(&str, &str)],
) -> Result<()> {
    let mut tpl = Editor::new(template)?;
    let tnames = lookup::constraint_names(tpl.segment(BROWSER)?);
    let tc = template_constraint(tpl.segment(DC)?, &tnames, name)?;
    let (occ, id_pairs) = occurrence_map(ed, &mut tpl, occurrences)?;
    let copied = copy_dc(ed, tpl.segment(DC)?, tc, &occ)?;
    let tbr = tpl.segment(BROWSER)?;
    let class = copy_browser_entry(ed, tbr, name, occurrences, copied.object_id)?;
    copy_occurrence_groups(ed, template, &id_pairs)?;
    let classes: Vec<Guid> = copied.classes.into_iter().chain([class]).collect();
    merge_class_lists(ed, template, &classes)?;
    tables::bump_revision(ed)
}

/// The template's DC constraint node named `name`.
fn template_constraint(ts: &EditSegment, names: &[String], name: &str) -> Result<usize> {
    let all = lookup::constraints(ts, names)?;
    let found = all.iter().find(|c| c.name == name).map(|c| c.index);
    let expected = format!("a constraint of the template, one of {names:?}");
    found.ok_or_else(|| CorlissError::invalid("template assembly", "constraint", 0, name, expected))
}

/// The occurrence mapping (ids and nodes) and the (template id, document id) pairs in order.
fn occurrence_map(
    ed: &mut Editor,
    tpl: &mut Editor,
    occurrences: &[(&str, &str)],
) -> Result<(OccurrenceMap, Vec<(i64, i64)>)> {
    let mut map = OccurrenceMap {
        ids: HashMap::new(),
        nodes: HashMap::new(),
    };
    let mut pairs = Vec::new();
    for (t, d) in occurrences {
        let (tnode, tid) = lookup::occurrence(tpl.segment(DC)?, t)?;
        let (dnode, did) = lookup::occurrence(ed.segment(DC)?, d)?;
        map.ids.insert(tid, did);
        map.nodes.insert(tnode, dnode);
        pairs.push((tid, did));
    }
    Ok((map, pairs))
}

/// The browser entry: a copy of the template's, after the last constraint in the shared
/// index counter, referenced where the template references it, with its t9 entry. Returns
/// its class.
fn copy_browser_entry(
    ed: &mut Editor,
    tbr: &EditSegment,
    name: &str,
    occurrences: &[(&str, &str)],
    c_oid: u16,
) -> Result<Guid> {
    let tb = template_entry(tbr, name)?;
    let br = ed.segment(BROWSER)?;
    let (nb, class) = clone_entry(br, tbr, tb, name)?;
    number_entry(br, nb)?;
    link_entry(br, tbr, tb, nb, occurrences)?;
    store_entry(br, nb)?;
    tables::t9_add_browser(ed, BROWSER, c_oid)?;
    Ok(class)
}

/// The template's browser entry of constraint `name`.
fn template_entry(tbr: &EditSegment, name: &str) -> Result<usize> {
    let is_entry = |k: &usize| {
        tbr.node(*k).is_some_and(|n| {
            BROWSER_CONSTRAINTS.contains(&n.node.type_id.0) && n.node.text("name") == Some(name)
        })
    };
    let found = (1..=tbr.nodes.len()).find(is_entry);
    found.ok_or_else(|| missing(TEMPLATE_BROWSER, format!("entry of {name}")))
}

/// Appends a copy of template entry `tb` (its object id kept where free) under the
/// template's class list; returns its index and class.
fn clone_entry(
    br: &mut EditSegment,
    tbr: &EditSegment,
    tb: usize,
    name: &str,
) -> Result<(usize, Guid)> {
    let body = &tbr.segment.meta.body;
    br.adopt_type_table(&body.types, &body.tail.t4)?;
    let src = tbr
        .node(tb)
        .cloned()
        .ok_or_else(|| missing(TEMPLATE_BROWSER, format!("node #{tb}")))?;
    let copy = br.clone_node(&src, true)?;
    let nb = br.append(copy);
    set(
        node_mut(br, nb)?,
        "name",
        FieldValue::Text(name.to_string()),
    )?;
    Ok((nb, src.class))
}

/// The shared index counter runs occurrences, then constraints in creation order: a new
/// constraint goes after the last existing one (taking the slot after the occurrences
/// instead renumbered Mate:1 onto the new entry's slot and crashed Rebuild All for an
/// Angle).
fn number_entry(br: &mut EditSegment, nb: usize) -> Result<()> {
    let is_peer = |i: &usize| {
        *i != nb
            && br.node(*i).is_some_and(|n| {
                let tid = n.node.type_id.0;
                tid == T_BROWSER_OCCURRENCE || BROWSER_CONSTRAINTS.contains(&tid)
            })
    };
    let peers: Vec<usize> = (1..=br.nodes.len()).filter(is_peer).collect();
    let newi = max_of(br, &peers, "index")? + 1;
    raise_indexes(br, nb, newi)?;
    set_int(node_mut(br, nb)?, "index", newi)?;
    set_group_index(br, nb, &peers)
}

/// Lists the new entry wherever the template lists its entry: in the mapped occurrence
/// entries and in the counterparts of the other referrers.
fn link_entry(
    br: &mut EditSegment,
    tbr: &EditSegment,
    tb: usize,
    nb: usize,
    occurrences: &[(&str, &str)],
) -> Result<()> {
    for t in 1..=tbr.nodes.len() {
        let tn = node(tbr, t)?;
        for (_, _, field) in named_refs(tn).into_iter().filter(|(_, to, _)| *to == tb) {
            let dst = entry_target(br, tbr, t, occurrences)?;
            append_ref(node_mut(br, dst)?, &field, nb, None, list(tn, &field))?;
        }
    }
    Ok(())
}

/// The document node playing template browser node `t`'s role: the mapped occurrence's entry
/// for occurrence entries, else the counterpart.
fn entry_target(
    br: &EditSegment,
    tbr: &EditSegment,
    t: usize,
    occurrences: &[(&str, &str)],
) -> Result<usize> {
    let tn = node(tbr, t)?;
    if tn.type_id.0 != T_BROWSER_OCCURRENCE {
        return counterpart(tbr, br, t);
    }
    let from = tn.text("name");
    let mapped = occurrences
        .iter()
        .find(|(f, _)| Some(*f) == from)
        .map(|(_, to)| *to);
    let found = find_of_type(br, T_BROWSER_OCCURRENCE, |n| n.text("name") == mapped);
    found.ok_or_else(|| missing("AmBrowserSegment", format!("entry of {mapped:?}")))
}

/// Stored before the first origin entry after the occurrence entries (as_mate).
fn store_entry(br: &mut EditSegment, nb: usize) -> Result<()> {
    let last_occ = br
        .of_type(T_BROWSER_OCCURRENCE)
        .into_iter()
        .max()
        .unwrap_or(0);
    let is_origin = |i: &usize| {
        br.node(*i)
            .is_some_and(|n| n.node.type_id.0 == T_ORIGIN_ENTRY)
    };
    let before = (last_occ + 1..=br.nodes.len()).find(is_origin);
    order::move_before(br, nb, before).map(|_| ())
}

/// UFRxDoc occurrence groups of the template's occurrences, renumbered to the document's
/// occurrence ids, for occurrences that have none yet; groups stay sorted by id.
fn copy_occurrence_groups(
    ed: &mut Editor,
    template: &Document,
    id_pairs: &[(i64, i64)],
) -> Result<()> {
    let tu = template.ufrx_doc()?;
    let occurrences = tu.root.record("occurrences");
    let tgroups = occurrences
        .map(|o| o.list("groups").to_vec())
        .unwrap_or_default();
    ufrx_records::edit_occurrences(ed, |occ| {
        let groups = ufrx_records::list_mut(occ, "groups")?;
        add_groups(groups, &tgroups, id_pairs)?;
        groups.sort_by_key(|g| group_id(g).unwrap_or(0));
        Ok(())
    })
}

/// Copies of the template groups of each mapped occurrence that has no group yet.
fn add_groups(groups: &mut Vec<Record>, tgroups: &[Record], id_pairs: &[(i64, i64)]) -> Result<()> {
    for (tid, did) in id_pairs {
        let (from, to) = (GROUP_BASE + tid, GROUP_BASE + did);
        let Some(src) = tgroups.iter().find(|g| group_id(g) == Some(from)) else {
            continue;
        };
        if groups.iter().any(|g| group_id(g) == Some(to)) {
            continue;
        }
        let mut copy = src.clone();
        set_uint(&mut copy, "id", to as u64)?;
        groups.push(copy);
    }
    Ok(())
}

/// RSeSegInfo's two class lists ("meta types"): as_mate lists 4E86F047 in uid_list1,
/// as_delmate does not, and without it Inventor reports "Error in reading meta types in
/// file segment AmDcSegment". The template's entries for the classes copied here are taken
/// over at the template's position.
///
/// An insertion shifts every later index, so the root records are then put back on their
/// classes, and those of the opened segments onto their targets' classes (spec 9.2.4).
fn merge_class_lists(ed: &mut Editor, template: &Document, classes: &[Guid]) -> Result<()> {
    let theirs = template.seg_info()?;
    let si = ed.seg_info_mut();
    let (old1, old2) = (si.uid_list1.clone(), si.uid_list2.clone());
    merge_classes(&mut si.uid_list1, &theirs.uid_list1, classes);
    merge_classes(&mut si.uid_list2, &theirs.uid_list2, classes);
    root_classes::remap_root_classes(si, &old1, &old2);
    root_classes::fix_root_targets(ed)
}

/// Inserts each used class of `theirs` that `mine` lacks after the nearest class before it
/// that both lists hold (at the front when none).
fn merge_classes(mine: &mut Vec<Guid>, theirs: &[Guid], used: &[Guid]) {
    for (j, g) in theirs.iter().enumerate() {
        if !used.contains(g) || mine.contains(g) {
            continue;
        }
        let prev = theirs[..j].iter().rev().find(|x| mine.contains(x));
        let at = prev
            .and_then(|p| mine.iter().position(|x| x == p))
            .map_or(0, |p| p + 1);
        mine.insert(at, *g);
    }
}
