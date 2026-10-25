//! Deleting an assembly constraint.

use std::collections::BTreeSet;

use super::dependents;
use super::fields::set_int;
use super::fields::{first_of_type, missing, named_refs, node, node_mut, object_id, remove_ref};
use super::lookup::{self, BROWSER, BROWSER_CONSTRAINTS, DC, is_browser_indexed};
use super::tables;
use super::ufrx_records::{self, GROUP_BASE};
use crate::edit::{EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::DecodedNode;

const T_NAME_TABLE: u32 = 0xCCE9_2042;

/// Deletes assembly constraint `name` the way Inventor does (as_mate -> as_delmate): the DC
/// constraint with every node that belongs only to it, its name-table entries, its browser
/// entry (and the references to it), the browser t9 entry naming it, and the UFRxDoc
/// occurrence groups of occurrences no constraint uses any more.
///
/// The reference also clears the AmRx solver state (`u8_0`, `a0`, `v1`) of occurrences left
/// unconstrained, but the 2027 grammar names none of those fields, so it never applies; Corliss
/// does not implement it.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::assembly::remove_constraint(&mut ed, "Mate:1")?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn remove_constraint(ed: &mut Editor, name: &str) -> Result<()> {
    let names = lookup::constraint_names(ed.segment(BROWSER)?);
    let dc = ed.segment(DC)?;
    let (c, keep) = constraint_and_kept_groups(dc, &names, name)?;
    let c_oid = object_id(dc, c);
    delete_dc_nodes(dc, c, name)?;
    remove_browser_entry(ed, name, c_oid)?;
    ufrx_records::edit_occurrences(ed, |occ| {
        let groups = ufrx_records::list_mut(occ, "groups")?;
        groups.retain(|g| ufrx_records::group_id(g).is_some_and(|id| keep.contains(&id)));
        Ok(())
    })?;
    tables::bump_revision(ed)
}

/// The constraint node named `name`, and the UFRxDoc group ids (0x30 + occurrence id) of
/// the occurrences other constraints still use.
fn constraint_and_kept_groups(
    dc: &EditSegment,
    names: &[String],
    name: &str,
) -> Result<(usize, BTreeSet<i64>)> {
    let all = lookup::constraints(dc, names)?;
    let Some(info) = all.iter().find(|c| c.name == name) else {
        let expected = format!("a constraint name, one of {names:?}");
        return Err(CorlissError::invalid(
            "assembly",
            "constraint",
            0,
            name,
            expected,
        ));
    };
    let others = all.iter().filter(|c| c.name != name);
    let used: BTreeSet<usize> = others.flat_map(|c| c.occurrences.iter().copied()).collect();
    let ids = used
        .iter()
        .map(|o| lookup::occurrence_id(dc, *o).map(|id| GROUP_BASE + id));
    Ok((info.index, ids.collect::<Result<_>>()?))
}

/// Unlinks and deletes the constraint's DC nodes and its name-table entries.
fn delete_dc_nodes(dc: &mut EditSegment, c: usize, name: &str) -> Result<()> {
    let (mut set, mut keys) = dependents::dependents(dc, c);
    unlink(dc, &set, name)?;
    // entries shared with creators of other constraints stay (as_angle key 10)
    for i in (1..=dc.nodes.len()).filter(|i| !set.contains(i)) {
        for k in dependents::name_keys(dc, i) {
            keys.remove(&k);
        }
    }
    let nt = node_mut(dc, first_of_type(dc, T_NAME_TABLE)?)?;
    set.extend(dependents::drop_name_pairs(nt, &|k| keys.contains(&k))?);
    let dead: Vec<usize> = set.into_iter().collect();
    dc.delete(&dead).map(|_| ())
}

/// Removes the set's nodes from container lists; anything else still pointing in is an error.
fn unlink(dc: &mut EditSegment, set: &BTreeSet<usize>, name: &str) -> Result<()> {
    for i in (1..=dc.nodes.len()).filter(|i| !set.contains(i)) {
        let n = node_mut(dc, i)?;
        let tid = n.type_id.0;
        for (_, target, field) in named_refs(n)
            .into_iter()
            .filter(|(_, t, _)| set.contains(t))
        {
            if !dependents::is_container(tid, &field) && !(i == 2 && field == "participants") {
                let found = format!("#{i} {tid:08X}.{field} still uses #{target} of {name}");
                return Err(still_used("assembly constraint", i, found));
            }
            remove_ref(n, &field, target, false)?;
        }
    }
    Ok(())
}

/// A delete refused because `found` still references what goes.
pub(super) fn still_used(structure: &'static str, at: usize, found: String) -> CorlissError {
    let expected = "constraints, patterns or joints that use it removed first";
    CorlissError::invalid(structure, "refs", at, found, expected)
}

/// The browser entry, references to it, the shared index counter and the t9 entry naming
/// the DC constraint.
fn remove_browser_entry(ed: &mut Editor, name: &str, c_oid: u16) -> Result<()> {
    let br = ed.segment(BROWSER)?;
    let b = browser_entry(br, name)?;
    let bi = node(br, b)?.int("index").unwrap_or(0);
    for i in (1..=br.nodes.len()).filter(|i| *i != b) {
        let n = node_mut(br, i)?;
        for (_, _, field) in named_refs(n).into_iter().filter(|(_, t, _)| *t == b) {
            remove_ref(n, &field, b, false)?;
        }
        if bi != 0 && is_browser_indexed(n.type_id.0) {
            lower_index(n, bi)?;
        }
    }
    br.delete(&[b])?;
    remove_browser_root(ed, c_oid)
}

/// The browser entry of constraint `name`.
fn browser_entry(br: &EditSegment, name: &str) -> Result<usize> {
    let is_entry = |k: &usize| {
        br.node(*k).is_some_and(|n| {
            BROWSER_CONSTRAINTS.contains(&n.node.type_id.0) && n.node.text("name") == Some(name)
        })
    };
    let found = (1..=br.nodes.len()).find(is_entry);
    found.ok_or_else(|| missing("AmBrowserSegment", format!("entry of {name}")))
}

/// The browser t9 entry naming DC object `c_oid` (when the first run has one) and the t7
/// entry that owned it.
fn remove_browser_root(ed: &mut Editor, c_oid: u16) -> Result<()> {
    let t9 = tables::t9(ed, BROWSER)?;
    let end1 = tables::first_run_end(&t9)?;
    if !t9[..end1].iter().any(|e| e[3] == c_oid) {
        return Ok(());
    }
    let (_, pos) = tables::t9_remove(ed, BROWSER, &[c_oid], &[(0, end1)])?;
    tables::shorten_browser_run(ed, BROWSER)?;
    tables::t7_release(ed, BROWSER, 0, end1, pos.unwrap_or(0), 1).map(|_| ())
}

/// Entries after a removed one move down one in the shared index counter.
pub(super) fn lower_index(n: &mut DecodedNode, removed: i64) -> Result<()> {
    let iv = n.int("index").unwrap_or(0);
    if iv != 0 && iv > removed {
        set_int(n, "index", iv - 1)?;
    }
    Ok(())
}
