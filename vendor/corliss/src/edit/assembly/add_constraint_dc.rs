//! Copying a constraint's DC nodes from a template assembly (the AmDcSegment part of copying a
//! constraint): what is copied, the copies and their refs.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::add_constraint_links::{assign_indexes, link_containers, source_keys};
use super::add_constraint_names::{add_name_entries, follow_occurrence_ids, share_names};
use super::dependents;
use super::fields::{first_of_type, for_each_ref_mut, int, missing, named_refs, node, node_mut};
use super::fields::{object_id, required_ref, retarget, set_int};
use super::order;
use crate::edit::{EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::{Field, FieldValue};
use crate::guid::Guid;

pub(super) const T_GEOMETRY_SOURCE: u32 = 0x375E_AEE5;
const T_NAME_TABLE: u32 = 0xCCE9_2042;
/// Geometry-source keys are 0x30 + the occurrence id.
pub(super) const SOURCE_KEY_BASE: i64 = 0x30;

/// How the template's occurrences map onto the document's.
pub(super) struct OccurrenceMap {
    /// Template occurrence id → document occurrence id.
    pub ids: HashMap<i64, i64>,
    /// Template occurrence node → document occurrence node.
    pub nodes: HashMap<usize, usize>,
}

/// What the DC copy produced.
pub(super) struct DcCopied {
    /// Object id of the new constraint node.
    pub object_id: u16,
    /// Classes of the template nodes copied.
    pub classes: Vec<Guid>,
}

/// Template node → copy.
pub(super) type Copies = BTreeMap<usize, usize>;

/// The node of `dst` playing the role of node `i` of `src`: same type, same position among
/// the nodes of that type.
pub(super) fn counterpart(src: &EditSegment, dst: &EditSegment, i: usize) -> Result<usize> {
    let tid = node(src, i)?.type_id.0;
    let k = src.of_type(tid).iter().position(|x| *x == i).unwrap_or(0);
    let found = dst.of_type(tid).get(k).copied();
    let what = format!("{tid:08X} #{i} (position {k})");
    let expected = "a node of the same type at the same position in the document";
    found.ok_or_else(|| CorlissError::invalid("constraint copy", "counterpart", i, what, expected))
}

/// The copy of template node `i`.
pub(super) fn copy_of(mp: &Copies, i: usize) -> Result<usize> {
    mp.get(&i)
        .copied()
        .ok_or_else(|| missing("constraint copy", format!("copy of template node #{i}")))
}

/// Copies template constraint `tc` (everything `dependents` collects, its name-table entries,
/// shared geometry sources reused or copied), links it into the document's containers, name
/// table and counters, and puts the segment in Inventor's order.
pub(super) fn copy_dc(
    ed: &mut Editor,
    ts: &EditSegment,
    tc: usize,
    occ: &OccurrenceMap,
) -> Result<DcCopied> {
    let dc = ed.segment(super::lookup::DC)?;
    let (mut set, ent_of) = template_set(ts, tc)?;
    let (users, reuse) = share_sources(dc, ts, &mut set, occ)?;
    let mp = clone_into(dc, ts, &set)?;
    remap(dc, ts, &mp, occ, reuse)?;
    let comp = required_ref(node(dc, copy_of(&mp, tc)?)?, "ref")?;
    assign_indexes(dc, &mp, comp)?;
    source_keys(dc, &mp, &users, occ)?;
    link_containers(dc, ts, tc, &mp, comp)?;
    let newkey = add_name_entries(dc, ts, &ent_of, &mp)?;
    follow_occurrence_ids(dc, &mp, &ent_of, &newkey, occ)?;
    let object_id = object_id(dc, copy_of(&mp, tc)?);
    share_names(dc, &mp, &ent_of)?;
    order::canonicalize(dc)?;
    let classes = set
        .iter()
        .filter_map(|i| ts.node(*i).map(|n| n.class))
        .collect();
    Ok(DcCopied { object_id, classes })
}

/// The constraint's dependents in the template plus the name-table entries of its creators:
/// (set, key → entry node).
fn template_set(ts: &EditSegment, tc: usize) -> Result<(BTreeSet<usize>, BTreeMap<i64, usize>)> {
    let (mut set, keys) = dependents::dependents(ts, tc);
    let nt = first_of_type(ts, T_NAME_TABLE)?;
    let mut ent_of = BTreeMap::new();
    for (k, entry) in dependents::name_table_pairs(&node(ts, nt)?.fields) {
        if keys.contains(&k) {
            ent_of.insert(k, entry);
            set.insert(entry);
        }
    }
    Ok((set, ent_of))
}

/// Geometry sources (375EAEE5): one per occurrence, shared by every proxy on it (as_flush:
/// Mate:1 and Flush:1 use the same two; u32_0 counts the users). The document's source of
/// the target occurrence is reused when it has one, else the template's is copied. Returns
/// the user count per template source and the reused ones (template → document node).
fn share_sources(
    dc: &mut EditSegment,
    ts: &EditSegment,
    set: &mut BTreeSet<usize>,
    occ: &OccurrenceMap,
) -> Result<(BTreeMap<usize, usize>, HashMap<usize, usize>)> {
    let users = source_users(ts, set)?;
    let mut reuse = HashMap::new();
    for (t, count) in &users {
        match reuse_source(dc, ts, *t, *count, occ)? {
            Some(have) => {
                reuse.insert(*t, have);
                set.remove(t);
            }
            None => {
                set.insert(*t);
            }
        }
    }
    Ok((users, reuse))
}

/// Geometry sources the set's nodes refer to, with the number of distinct users.
fn source_users(ts: &EditSegment, set: &BTreeSet<usize>) -> Result<BTreeMap<usize, usize>> {
    let is_source = |t: usize| {
        ts.node(t)
            .is_some_and(|n| n.node.type_id.0 == T_GEOMETRY_SOURCE)
    };
    let mut users: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for i in set.iter().copied() {
        for (_, t, _) in named_refs(node(ts, i)?)
            .into_iter()
            .filter(|(_, t, _)| *t != i && is_source(*t))
        {
            users.entry(t).or_default().insert(i);
        }
    }
    Ok(users.into_iter().map(|(t, who)| (t, who.len())).collect())
}

/// The document's geometry source of the occurrence template source `t` maps to, its user
/// count raised by `count`; `None` when the document has none.
fn reuse_source(
    dc: &mut EditSegment,
    ts: &EditSegment,
    t: usize,
    count: usize,
    occ: &OccurrenceMap,
) -> Result<Option<usize>> {
    let key = int(node(ts, t)?, "key")?;
    let mapped = occ
        .ids
        .get(&(key - SOURCE_KEY_BASE))
        .copied()
        .unwrap_or(-1000);
    let want = SOURCE_KEY_BASE + mapped;
    let have = super::fields::find_of_type(dc, T_GEOMETRY_SOURCE, |n| n.int("key") == Some(want));
    let Some(have) = have else { return Ok(None) };
    let h = node_mut(dc, have)?;
    let users = int(h, "u32_0")?;
    set_int(h, "u32_0", users + count as i64)?;
    Ok(Some(have))
}

/// Switches to the template's class list and appends copies of the template nodes, keeping
/// their object ids where free.
fn clone_into(dc: &mut EditSegment, ts: &EditSegment, set: &BTreeSet<usize>) -> Result<Copies> {
    let body = &ts.segment.meta.body;
    // the M type table carries a class graph (t4) that must list every class and its
    // bases; adopting the template's whole list covers both
    dc.adopt_type_table(&body.types, &body.tail.t4)?;
    let mut mp = BTreeMap::new();
    for i in set {
        let src = ts
            .node(*i)
            .ok_or_else(|| missing("template AmDcSegment", format!("node #{i}")))?;
        let copy = dc.clone_node(src, true)?;
        mp.insert(*i, dc.append(copy));
    }
    Ok(mp)
}

/// Where the refs of the copies point.
struct Targets<'a> {
    mp: &'a Copies,
    occ: &'a OccurrenceMap,
    /// Template node → document node outside the copies (reused sources, counterparts).
    outside: HashMap<usize, usize>,
}

impl Targets<'_> {
    /// Another copy, a mapped occurrence, a reused source, else the document's counterpart.
    fn target(&mut self, ts: &EditSegment, dc: &EditSegment, idx: usize) -> Result<usize> {
        if let Some(t) = self.mp.get(&idx).or_else(|| self.occ.nodes.get(&idx)) {
            return Ok(*t);
        }
        if let Some(t) = self.outside.get(&idx) {
            return Ok(*t);
        }
        let t = counterpart(ts, dc, idx)?;
        self.outside.insert(idx, t);
        Ok(t)
    }

    fn retarget(&mut self, ts: &EditSegment, dc: &EditSegment, f: &mut Field) -> Result<()> {
        let FieldValue::Ref(r, _) = f.value else {
            return Ok(());
        };
        let Some(idx) = r.index() else { return Ok(()) };
        let t = self.target(ts, dc, idx)?;
        retarget(f, t)
    }
}

/// Points every ref of the copies at its document target.
fn remap(
    dc: &mut EditSegment,
    ts: &EditSegment,
    mp: &Copies,
    occ: &OccurrenceMap,
    reuse: HashMap<usize, usize>,
) -> Result<()> {
    let mut targets = Targets {
        mp,
        occ,
        outside: reuse,
    };
    for new in mp.values().copied() {
        let mut fields = std::mem::take(&mut node_mut(dc, new)?.fields);
        let done = for_each_ref_mut(&mut fields, &mut |f| targets.retarget(ts, dc, f));
        node_mut(dc, new)?.fields = fields;
        done?;
    }
    Ok(())
}
