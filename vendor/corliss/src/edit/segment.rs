//! An editable segment: its nodes as typed fields, plus what is needed to rebuild it.

use std::collections::{BTreeSet, HashMap};

use super::idset;
use crate::document::SegmentLocation;
use crate::error::{CorlissError, Result};
use crate::grammar::{self, DecodeContext, DecodedNode, FieldValue, RefKind};
use crate::guid::Guid;
use crate::segment::{Segment, TypeEntry};

const STRUCTURE: &str = "segment edit";

/// One node of an editable segment with its block framing.
#[derive(Debug, Clone, PartialEq)]
pub struct EditNode {
    /// Class GUID of the node (its type-table entry).
    pub class: Guid,
    /// Original block header, `None` for nodes built from scratch.
    pub header: Option<u32>,
    /// Block trailer bytes (empty: written as a single 0 byte in 2015+ files).
    pub trailer: Vec<u8>,
    /// The decoded node.
    pub node: DecodedNode,
}

impl EditNode {
    /// The payload the node encodes.
    pub fn payload(&self) -> Vec<u8> {
        self.node.payload()
    }

    /// The u16 object id of the common header, or 0 for nodes without one.
    pub fn object_id(&self) -> u64 {
        match self.node.get("hdr.u16_0") {
            Some(v) => v.as_int().unwrap_or(0) as u64,
            None => 0,
        }
    }

    /// The wide object id `(u32 << 16) | u16` of drawing document DC nodes (spec 11.4).
    pub fn wide_id(&self) -> Option<u64> {
        let hi = self.node.get("hdr.u32_0")?.as_int()? as u64;
        (hi < 0x10000).then(|| hi << 16 | self.object_id())
    }
}

/// A segment opened for editing.
pub struct EditSegment {
    /// Where the segment is stored.
    pub location: SegmentLocation,
    /// Segment type, which selects the node grammars.
    pub segment_type: String,
    /// The M and B streams as read; the commit rebuilds them.
    pub segment: Segment,
    /// Nodes in block order; NodeRef index k names `nodes[k - 1]`.
    pub nodes: Vec<EditNode>,
    /// Object ids handed out in this session.
    pub new_ids: Vec<u64>,
    /// Distance kept from the allocation set's top for the first new id (drawings, spec 11.4).
    pub id_gap: u64,
    pub(crate) original_classes: Vec<Guid>,
    pub(crate) ctx: DecodeContext,
    taken: Option<BTreeSet<u64>>,
}

impl EditSegment {
    pub(crate) fn new(
        location: SegmentLocation,
        segment_type: String,
        segment: Segment,
        decoded: Vec<DecodedNode>,
        ctx: DecodeContext,
    ) -> Self {
        let nodes = edit_nodes(&segment, decoded);
        let original_classes = nodes.iter().map(|n| n.class).collect();
        EditSegment {
            location,
            segment_type,
            segment,
            nodes,
            new_ids: Vec::new(),
            id_gap: 0,
            original_classes,
            ctx,
            taken: None,
        }
    }

    /// The node at 1-based `index`.
    pub fn node(&self, index: usize) -> Option<&EditNode> {
        index.checked_sub(1).and_then(|k| self.nodes.get(k))
    }

    /// The node at 1-based `index`, mutable.
    pub fn node_mut(&mut self, index: usize) -> Option<&mut EditNode> {
        index.checked_sub(1).and_then(|k| self.nodes.get_mut(k))
    }

    /// Indexes of the nodes of TypeID `tid`.
    pub fn of_type(&self, tid: u32) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.node.type_id.0 == tid)
            .map(|(k, _)| k + 1)
            .collect()
    }

    /// Nodes that reference node `index`: (referrer, kind).
    pub fn referrers(&self, index: usize) -> Vec<(usize, RefKind)> {
        let mut out = Vec::new();
        for (k, n) in self.nodes.iter().enumerate() {
            for (r, kind) in n.node.refs() {
                if r.index() == Some(index) {
                    out.push((k + 1, kind));
                }
            }
        }
        out
    }

    /// Appends a node at the end; existing indexes do not move. Returns its index.
    pub fn append(&mut self, node: EditNode) -> usize {
        self.nodes.push(node);
        self.nodes.len()
    }

    /// Removes nodes; every remaining NodeRef is renumbered. Fails when a remaining node still
    /// references a removed one (callers unlink first). Returns old → new indexes.
    pub fn delete(&mut self, indexes: &[usize]) -> Result<HashMap<usize, usize>> {
        let dead: BTreeSet<usize> = indexes.iter().copied().collect();
        self.check_unlinked(&dead)?;
        let mut map = HashMap::new();
        let mut kept = Vec::new();
        for (k, n) in std::mem::take(&mut self.nodes).into_iter().enumerate() {
            if !dead.contains(&(k + 1)) {
                kept.push(n);
                map.insert(k + 1, kept.len());
            }
        }
        self.nodes = kept;
        self.remap(&map);
        Ok(map)
    }

    fn check_unlinked(&self, dead: &BTreeSet<usize>) -> Result<()> {
        for (k, n) in self
            .nodes
            .iter()
            .enumerate()
            .filter(|(k, _)| !dead.contains(&(k + 1)))
        {
            if let Some((r, _)) = n
                .node
                .refs()
                .into_iter()
                .find(|(r, _)| r.index().is_some_and(|i| dead.contains(&i)))
            {
                let found = format!(
                    "node #{} references deleted #{}",
                    k + 1,
                    r.index().unwrap_or(0)
                );
                return Err(CorlissError::invalid(
                    STRUCTURE,
                    "refs",
                    k + 1,
                    found,
                    "no reference to a deleted node",
                ));
            }
        }
        Ok(())
    }

    /// Puts the nodes in `order` (a permutation of 1..=n) and renumbers every NodeRef.
    pub fn reorder(&mut self, order: &[usize]) -> Result<HashMap<usize, usize>> {
        let mut sorted = order.to_vec();
        sorted.sort_unstable();
        if sorted != (1..=self.nodes.len()).collect::<Vec<_>>() {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "order",
                0,
                format!("{} indexes", order.len()),
                "a permutation of the nodes",
            ));
        }
        let map: HashMap<usize, usize> = order
            .iter()
            .enumerate()
            .map(|(new, old)| (*old, new + 1))
            .collect();
        let mut old: Vec<Option<EditNode>> = std::mem::take(&mut self.nodes)
            .into_iter()
            .map(Some)
            .collect();
        self.nodes = order
            .iter()
            .filter_map(|o| old.get_mut(o - 1).and_then(Option::take))
            .collect();
        self.remap(&map);
        Ok(map)
    }

    fn remap(&mut self, map: &HashMap<usize, usize>) {
        let f = |i: usize| map.get(&i).copied();
        self.nodes.iter_mut().for_each(|n| n.node.remap_refs(&f));
    }

    /// Decodes a payload of TypeID `tid` with this segment's grammars.
    pub fn decode_payload(&self, tid: u32, payload: &[u8]) -> Result<DecodedNode> {
        let nodes = grammar::decode_segment(
            &self.segment_type,
            self.ctx,
            &[(crate::TypeId(tid), payload)],
        )?;
        nodes
            .into_iter()
            .next()
            .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "decoded node"))
    }

    /// A copy of node `src` with a fresh object id, not linked anywhere.
    /// With `keep_id` the source's id is kept when no node of the segment uses it.
    pub fn clone_node(&mut self, src: &EditNode, keep_id: bool) -> Result<EditNode> {
        let mut copy = src.clone();
        copy.node.error = None;
        if src.node.field("hdr.u16_0").is_none() {
            return Ok(copy);
        }
        let want = keep_id.then(|| src.object_id());
        let id = self.alloc_id(want)?;
        copy.node.set("hdr.u16_0", FieldValue::U16(id as u16))?;
        Ok(copy)
    }

    /// A copy of `src` with a fresh wide id (drawing document DC).
    pub fn clone_wide(&mut self, src: &EditNode) -> Result<EditNode> {
        let mut copy = src.clone();
        let id = self.alloc_wide_id()?;
        copy.node
            .set("hdr.u16_0", FieldValue::U16((id & 0xFFFF) as u16))?;
        copy.node
            .set("hdr.u32_0", FieldValue::U32((id >> 16) as u32))?;
        Ok(copy)
    }

    /// The id space of the M stream (highest id handed out).
    pub fn id_space(&self) -> u64 {
        self.segment
            .meta
            .header
            .identity
            .as_ref()
            .map_or(0, |i| i.id_space & 0xFFFF_FFFF)
    }

    /// Ids the allocation set marks used, including those past the id space (spec 11.4).
    pub fn taken_ids(&mut self) -> &BTreeSet<u64> {
        if self.taken.is_none() {
            let decoded = self
                .t5_bitset()
                .and_then(|(first, b)| idset::decode(first, &b).ok());
            self.taken = Some(decoded.unwrap_or_default());
        }
        self.taken.get_or_insert_with(BTreeSet::new)
    }

    fn t5_bitset(&self) -> Option<(u32, Vec<u8>)> {
        let t5 = self.segment.meta.body.tail.t5.as_ref()?;
        // The t5 split is located by the id space it records: the last sec2 N.
        let recorded = self.segment.meta.body.sec2.last().map_or(0, |e| e.1);
        let split = idset::T5::split(t5, recorded)?;
        Some((split.first_type, split.bitset))
    }

    /// Next object id for a new node; `want` when no node uses it yet.
    pub fn alloc_id(&mut self, want: Option<u64>) -> Result<u64> {
        if let Some(w) = want.filter(|w| *w > 0 && *w <= 0xFFFF && !self.new_ids.contains(w))
            && self.nodes.iter().all(|n| n.object_id() != w)
        {
            self.new_ids.push(w);
            return Ok(w);
        }
        if self.id_space() > 0xFFFF {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "id_space",
                0,
                self.id_space(),
                "an id space within 16 bits (use alloc_wide_id)",
            ));
        }
        let id = self.next_free_id();
        if id > 0xFFFF {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "object id",
                0,
                id,
                "an id below 0x10000",
            ));
        }
        self.new_ids.push(id);
        Ok(id)
    }

    /// The id after every id in use, past the allocation set's top plus `id_gap` for the
    /// session's first id, and not marked in the allocation set.
    fn next_free_id(&mut self) -> u64 {
        let mut id = self
            .nodes
            .iter()
            .map(EditNode::object_id)
            .chain(self.new_ids.iter().copied())
            .fold(self.id_space(), u64::max)
            + 1;
        let first_gap = self.new_ids.is_empty() && self.id_gap > 0;
        let taken = self.taken_ids().clone();
        if first_gap {
            id = taken.iter().map(|t| t + 1).fold(id, u64::max) + self.id_gap;
        }
        while taken.contains(&id) {
            id += 1;
        }
        id
    }

    /// Next wide object id from the id space top.
    pub fn alloc_wide_id(&mut self) -> Result<u64> {
        let mut id = self.new_ids.iter().copied().fold(self.id_space(), u64::max) + 1;
        let taken = self.taken_ids().clone();
        while taken.contains(&id) {
            id += 1;
        }
        self.new_ids.push(id);
        Ok(id)
    }

    /// Switches to a donor's class list and graph when this segment's classes are a subset of
    /// it, keeping each class counter at the larger of the two.
    pub fn adopt_type_table(&mut self, donor: &[TypeEntry], t4: &[u16]) -> Result<()> {
        let body = &mut self.segment.meta.body;
        let own: HashMap<Guid, TypeEntry> = body.types.iter().map(|t| (t.class, *t)).collect();
        let theirs: HashMap<Guid, TypeEntry> = donor.iter().map(|t| (t.class, *t)).collect();
        if donor.iter().all(|t| own.contains_key(&t.class)) {
            raise_counters(&mut body.types, &theirs);
            return Ok(());
        }
        if let Some(missing) = own.keys().find(|g| !theirs.contains_key(g)) {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "types",
                0,
                missing,
                "a donor class list that holds every class of the segment",
            ));
        }
        body.types = donor
            .iter()
            .map(|t| match own.get(&t.class) {
                Some(o) => TypeEntry {
                    instance_counter: o.instance_counter.max(t.instance_counter),
                    ..*o
                },
                None => *t,
            })
            .collect();
        body.tail.t4 = t4.to_vec();
        Ok(())
    }
}

/// The decoded nodes paired with their blocks' class, header and trailer.
fn edit_nodes(segment: &Segment, decoded: Vec<DecodedNode>) -> Vec<EditNode> {
    let class_of = |k: usize| {
        segment
            .meta
            .body
            .types
            .get(k)
            .map(|t| t.class)
            .unwrap_or_default()
    };
    let blocks = segment.blocks();
    let framed = blocks.iter().zip(decoded);
    framed
        .map(|(b, node)| EditNode {
            class: class_of(b.type_index()),
            header: Some(b.header),
            trailer: b.trailer.clone(),
            node,
        })
        .collect()
}

/// Keeps each class counter at least the donor's: copied nodes carry instance serials handed
/// out by the donor's counters, and a counter below them crashes Inventor on load (W10).
fn raise_counters(types: &mut [TypeEntry], donor: &HashMap<Guid, TypeEntry>) {
    for t in types {
        if let Some(d) = donor.get(&t.class) {
            t.instance_counter = d.instance_counter.max(t.instance_counter);
        }
    }
}
