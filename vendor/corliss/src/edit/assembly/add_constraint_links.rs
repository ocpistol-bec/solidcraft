//! Linking a copied constraint into the document's DC counters and containers.

use std::collections::{BTreeMap, BTreeSet};

use super::add_constraint_dc::{
    Copies, OccurrenceMap, SOURCE_KEY_BASE, T_GEOMETRY_SOURCE, counterpart,
};
use super::dependents::is_container;
use super::fields::{append_ref, int, list, list_mut, named_refs, node, node_mut, ref_list};
use super::fields::{required_ref, retarget, set, set_int};
use super::lookup::T_LABEL;
use crate::edit::EditSegment;
use crate::error::Result;
use crate::grammar::{DecodedNode, Field, FieldValue, ListField};

/// Labels without an index carry this value.
const NO_INDEX: i64 = 0xFFFF_FFFF;

/// Index values in use outside the copies.
struct UsedIndexes {
    objects: BTreeSet<i64>,
    labels: BTreeSet<i64>,
}

fn has_index(n: &DecodedNode) -> bool {
    n.fields.iter().any(|f| f.name.as_deref() == Some("index"))
}

impl UsedIndexes {
    fn new(dc: &EditSegment, mp: &Copies) -> Self {
        let new: BTreeSet<usize> = mp.values().copied().collect();
        let old = || {
            (1..=dc.nodes.len())
                .filter(|i| !new.contains(i))
                .filter_map(|i| dc.node(i))
                .map(|n| &n.node)
        };
        let objects = old()
            .filter(|n| n.type_id.0 != T_LABEL && has_index(n))
            .filter_map(|n| n.int("index"));
        let labels = old()
            .filter(|n| n.type_id.0 == T_LABEL)
            .filter_map(|n| n.int("index"));
        UsedIndexes {
            objects: objects.collect(),
            labels: labels.collect(),
        }
    }

    /// The template's label index when free, else one past the largest label index.
    fn label(&mut self, v: Option<i64>) -> i64 {
        let top = self
            .labels
            .iter()
            .copied()
            .filter(|x| *x != NO_INDEX)
            .max()
            .unwrap_or(0);
        let v = v
            .filter(|v| !self.labels.contains(v) && *v != NO_INDEX)
            .unwrap_or(top + 1);
        self.labels.insert(v);
        v
    }
}

/// Object indexes come from the component's counter, label indexes from the label counter;
/// the template's own values are kept where they are free in the document (Inventor's
/// counters never hand them out twice), otherwise the next counter value.
pub(super) fn assign_indexes(dc: &mut EditSegment, mp: &Copies, comp: usize) -> Result<()> {
    let mut used = UsedIndexes::new(dc, mp);
    for i in mp.values().copied() {
        let n = node(dc, i)?;
        let v = n.int("index");
        if n.type_id.0 == T_LABEL {
            let v = used.label(v);
            set_int(node_mut(dc, i)?, "index", v)?;
            continue;
        }
        let Some(v) = v.filter(|v| *v != 0 && has_index(n)) else {
            continue;
        };
        let v = object_index(dc, comp, v, &used.objects)?;
        set_int(node_mut(dc, i)?, "index", v)?;
        used.objects.insert(v);
    }
    Ok(())
}

/// `v` when free and below the component counter, else the counter (which moves on).
fn object_index(dc: &mut EditSegment, comp: usize, v: i64, used: &BTreeSet<i64>) -> Result<i64> {
    let counter = int(node(dc, comp)?, "index")?;
    if !used.contains(&v) && v < counter {
        return Ok(v);
    }
    set_int(node_mut(dc, comp)?, "index", counter + 1)?;
    Ok(counter)
}

/// Geometry-source keys follow the occurrence id (0x30 + id); the user count is the number
/// of copied nodes that use the source.
pub(super) fn source_keys(
    dc: &mut EditSegment,
    mp: &Copies,
    users: &BTreeMap<usize, usize>,
    occ: &OccurrenceMap,
) -> Result<()> {
    for (t, i) in mp {
        let n = node_mut(dc, *i)?;
        if n.type_id.0 != T_GEOMETRY_SOURCE {
            continue;
        }
        let Some(key) = n.int("key") else { continue };
        if let Some(id) = occ.ids.get(&(key - SOURCE_KEY_BASE)) {
            set_int(n, "key", SOURCE_KEY_BASE + id)?;
        }
        let count = match users.get(t).copied().filter(|c| *c > 0) {
            Some(c) => c as i64,
            None => int(n, "u32_0")?,
        };
        set_int(n, "u32_0", count)?;
    }
    Ok(())
}

/// The copies go into the same container lists the template uses, in the same order.
pub(super) fn link_containers(
    dc: &mut EditSegment,
    ts: &EditSegment,
    tc: usize,
    mp: &Copies,
    comp: usize,
) -> Result<()> {
    let tcomp = required_ref(node(ts, tc)?, "ref")?;
    for j in ref_list(node(ts, tcomp)?, "objects") {
        if let Some(new) = mp.get(&j) {
            append_ref(node_mut(dc, comp)?, "objects", *new, None, None)?;
        }
    }
    if let Some(tlist) = list(node(ts, tcomp)?, "parameters") {
        link_parameters(dc, tlist, mp, comp)?;
    }
    for t in (1..=ts.nodes.len()).filter(|t| !mp.contains_key(t)) {
        link_other_containers(dc, ts, t, mp)?;
    }
    Ok(())
}

/// Lists the copies in the counterpart of template container `t` (the root label for #2).
fn link_other_containers(
    dc: &mut EditSegment,
    ts: &EditSegment,
    t: usize,
    mp: &Copies,
) -> Result<()> {
    let tn = node(ts, t)?;
    for (_, target, field) in named_refs(tn) {
        let Some(new) = mp.get(&target) else { continue };
        if !is_container(tn.type_id.0, &field) || field == "objects" || field == "parameters" {
            continue;
        }
        let dst = if t != 2 { counterpart(ts, dc, t)? } else { 2 };
        append_ref(node_mut(dc, dst)?, &field, *new, None, list(tn, &field))?;
    }
    Ok(())
}

/// Parameters of the copies join the component's name map as the next free `d<k>`.
fn link_parameters(
    dc: &mut EditSegment,
    tlist: &ListField,
    mp: &Copies,
    comp: usize,
) -> Result<()> {
    for el in &tlist.elements {
        let Some(new) = element_target(el).and_then(|j| mp.get(&j)).copied() else {
            continue;
        };
        let plst = list_mut(node_mut(dc, comp)?, "parameters")?;
        let name = next_parameter_name(plst);
        let entry = renamed_entry(el, new, &name)?;
        if plst.meta.is_none() {
            // an empty list carries no meta word; a filled one must (as_mate: 17, 16)
            plst.meta = tlist.meta.clone();
        }
        plst.elements.push(entry);
        set(node_mut(dc, new)?, "name", FieldValue::Text(name))?;
    }
    Ok(())
}

/// The node a name-map entry points at.
fn element_target(el: &[Field]) -> Option<usize> {
    el.iter().find_map(|f| match f.value {
        FieldValue::Ref(r, _) => Some((r.0 & 0x7FFF_FFFF) as usize),
        _ => None,
    })
}

/// `d<k>` with the smallest k the name map does not hold.
fn next_parameter_name(plst: &ListField) -> String {
    let is_taken = |name: &str| {
        plst.elements
            .iter()
            .flatten()
            .any(|f| matches!(&f.value, FieldValue::Text(s) if s == name))
    };
    (0..)
        .map(|k| format!("d{k}"))
        .find(|n| !is_taken(n))
        .unwrap_or_default()
}

/// A name-map entry pointing at `new` under `name`.
fn renamed_entry(el: &[Field], new: usize, name: &str) -> Result<Vec<Field>> {
    let mut entry = el.to_vec();
    for f in &mut entry {
        match f.value {
            FieldValue::Ref(..) => retarget(f, new)?,
            FieldValue::Text(_) => f.set(FieldValue::Text(name.to_string()))?,
            _ => {}
        }
    }
    Ok(entry)
}
