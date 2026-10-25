//! The DC nodes that belong to one constraint and the name-table
//! entries keyed by their creators.

use std::collections::{BTreeSet, HashMap};

use super::fields::{named_refs, set_int};
use crate::edit::EditSegment;
use crate::error::Result;
use crate::grammar::{DecodedNode, FieldValue, RefKind};

/// Container lists a dependent may still be listed in: (class, list name).
pub(super) const DC_CONTAINERS: [(u32, &str); 5] = [
    (0x9087_4D63, "objects"),
    (0x9087_4D63, "parameters"),
    (0x2B48_A42B, "participants"),
    (0x572D_BC7C, "lst0"),
    (0x077D_9583, "lst1"),
];
/// Classes never collected: document, component, occurrences, holders, caches, the name table.
const DC_KEEP: [u32; 9] = [
    0x9087_4D46,
    0x9087_4D63,
    0x9087_4D61,
    0x9087_4D60,
    0xCB07_2B3B,
    0xCCE9_2042,
    0x572D_BC7C,
    0xAE1C_96C9,
    0x077D_9583,
];
pub(super) const NT_ENTRY_IDX: &str = "ntEntry.idx";

/// Whether (class, list name) is a container list.
pub(super) fn is_container(tid: u32, name: &str) -> bool {
    DC_CONTAINERS.iter().any(|(t, n)| *t == tid && *n == name)
}

/// Reference graph of a segment: refs out of and into each node, with the holding field name.
struct Graph {
    tid: Vec<u32>,
    from: HashMap<usize, Vec<(RefKind, usize, String)>>,
    to: HashMap<usize, Vec<(usize, String)>>,
}

impl Graph {
    fn new(seg: &EditSegment) -> Self {
        let n = seg.nodes.len();
        let tid = seg.nodes.iter().map(|x| x.node.type_id.0).collect();
        let (mut from, mut to): (HashMap<_, Vec<_>>, HashMap<_, Vec<_>>) =
            (HashMap::new(), HashMap::new());
        for (k, node) in seg.nodes.iter().enumerate() {
            let refs: Vec<_> = named_refs(&node.node)
                .into_iter()
                .filter(|(_, t, _)| (1..=n).contains(t))
                .collect();
            refs.iter()
                .for_each(|(_, t, nm)| to.entry(*t).or_default().push((k + 1, nm.clone())));
            from.insert(k + 1, refs);
        }
        Graph { tid, from, to }
    }

    fn tid(&self, i: usize) -> u32 {
        self.tid.get(i.wrapping_sub(1)).copied().unwrap_or(0)
    }

    /// Referrers of `i` outside `set` that are not container lists.
    fn outside(&self, i: usize, set: &BTreeSet<usize>) -> Vec<usize> {
        let referrers = self.to.get(&i).map_or(&[][..], Vec::as_slice);
        referrers
            .iter()
            .filter(|(r, nm)| !set.contains(r) && !is_container(self.tid(*r), nm))
            .map(|(r, _)| *r)
            .collect()
    }

    /// Adds `i` and its child subtree (classes outside DC_KEEP) to `set`.
    fn add(&self, i: usize, set: &mut BTreeSet<usize>) {
        let mut stack = vec![i];
        while let Some(x) = stack.pop() {
            if !set.insert(x) {
                continue;
            }
            let refs = self.from.get(&x).map_or(&[][..], Vec::as_slice);
            stack.extend(
                refs.iter()
                    .filter(|(k, t, _)| *k == RefKind::Child && !DC_KEEP.contains(&self.tid(*t)))
                    .map(|(_, t, _)| *t),
            );
        }
    }

    fn touches(&self, i: usize, set: &BTreeSet<usize>) -> bool {
        let into = self
            .to
            .get(&i)
            .is_some_and(|v| v.iter().any(|(r, _)| set.contains(r)));
        into || self
            .from
            .get(&i)
            .is_some_and(|v| v.iter().any(|(_, t, _)| set.contains(t)))
    }

    /// Joins node `i` (with the creators naming it) when everything referring to it lies in
    /// the set or is a container list; returns whether it joined.
    fn try_join(&self, i: usize, set: &mut BTreeSet<usize>) -> bool {
        if set.contains(&i) || DC_KEEP.contains(&self.tid(i)) || i <= 2 {
            return false;
        }
        let out = self.outside(i, set);
        let mut with_i = set.clone();
        with_i.insert(i);
        // a proxy may still be named by its own creator record (90874D56 for planes, B10D8B80
        // for axes): a node that nothing but containers refers to
        let creators: Vec<usize> = out
            .iter()
            .copied()
            .filter(|r| !DC_KEEP.contains(&self.tid(*r)) && self.outside(*r, &with_i).is_empty())
            .collect();
        if !self.touches(i, set) || out.len() != creators.len() {
            return false;
        }
        self.add(i, set);
        creators.iter().for_each(|r| self.add(*r, set));
        true
    }
}

/// Nodes that go with constraint node `start`: its child subtree (label, selections, value),
/// the parameter and its expression nodes, the proxy planes the selections point at with
/// their creator records and geometry sources. A node joins when every reference to it comes
/// from the set or from a container list. Returns the set and the name-table keys
/// (`ntEntry.idx`) its nodes use.
pub(super) fn dependents(seg: &EditSegment, start: usize) -> (BTreeSet<usize>, BTreeSet<i64>) {
    let graph = Graph::new(seg);
    let mut set = BTreeSet::new();
    graph.add(start, &mut set);
    let mut changed = true;
    while changed {
        changed = false;
        for i in 1..=seg.nodes.len() {
            changed |= graph.try_join(i, &mut set);
        }
    }
    let keys = set.iter().flat_map(|i| name_keys(seg, *i)).collect();
    (set, keys)
}

/// The name-table keys node `i` uses (its top-level `ntEntry.idx` fields).
pub(super) fn name_keys(seg: &EditSegment, i: usize) -> Vec<i64> {
    let Some(n) = seg.node(i) else {
        return Vec::new();
    };
    n.node
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some(NT_ENTRY_IDX))
        .filter_map(|f| f.value.as_int())
        .collect()
}

/// The (key, entry node) pairs of a name table (CCE92042), in field order.
pub(super) fn name_table_pairs(fields: &[crate::grammar::Field]) -> Vec<(i64, usize)> {
    fields
        .windows(2)
        .filter(|w| w[0].name.as_deref() == Some("key") && w[1].name.as_deref() == Some("entries"))
        .filter_map(|w| match (&w[0].value, &w[1].value) {
            (k, FieldValue::Ref(r, _)) => Some((k.as_int()?, (r.0 & 0x7FFF_FFFF) as usize)),
            _ => None,
        })
        .collect()
}

/// Recounts a name table's keys (`cnt`).
pub(super) fn recount_names(nt: &mut DecodedNode) -> Result<()> {
    let count = nt
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some("key"))
        .count();
    set_int(nt, "cnt", count as i64)
}

/// Deletes the (key, entries) pairs whose key `drop` selects and recounts the table;
/// returns the entry nodes they named.
pub(super) fn drop_name_pairs(
    nt: &mut DecodedNode,
    drop: &dyn Fn(i64) -> bool,
) -> Result<Vec<usize>> {
    let mut dropped = Vec::new();
    let mut i = 0;
    while i + 1 < nt.fields.len() {
        match name_table_pairs(&nt.fields[i..i + 2]).first() {
            Some((k, entry)) if drop(*k) => {
                dropped.push(*entry);
                nt.fields.drain(i..i + 2);
            }
            _ => i += 1,
        }
    }
    recount_names(nt)?;
    Ok(dropped)
}
