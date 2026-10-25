//! Block order: where Inventor stores new nodes (the canonical order, with a new node grouped
//! next to the nodes of its type).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::edit::EditSegment;
use crate::error::Result;

/// Flagged (bit 31) NodeRef targets of node `index`, in payload order.
fn flagged_refs(seg: &EditSegment, index: usize) -> Vec<usize> {
    let Some(n) = seg.node(index) else {
        return Vec::new();
    };
    n.node
        .refs()
        .into_iter()
        .filter(|(r, _)| r.is_flagged())
        .filter_map(|(r, _)| r.index())
        .collect()
}

/// Inventor's block order: a breadth-first walk from node 1 over the flagged refs in payload
/// order; nodes not reached follow in their current order.
pub(super) fn canonical_order(seg: &EditSegment) -> Vec<usize> {
    let n = seg.nodes.len();
    let (mut seen, mut order, mut queue) = (HashSet::new(), Vec::with_capacity(n), VecDeque::new());
    if n >= 1 {
        seen.insert(1);
        queue.push_back(1);
    }
    while let Some(x) = queue.pop_front() {
        order.push(x);
        for y in flagged_refs(seg, x) {
            if y <= n && seen.insert(y) {
                queue.push_back(y);
            }
        }
    }
    order.extend((1..=n).filter(|i| !seen.contains(i)));
    order
}

/// Puts the segment in canonical order; returns old → new indexes.
pub(super) fn canonicalize(seg: &mut EditSegment) -> Result<HashMap<usize, usize>> {
    let order = canonical_order(seg);
    seg.reorder(&order)
}

/// Moves node `index` right after node `after` (or to the end when `after` is `None`);
/// returns old → new indexes.
pub(super) fn move_after(
    seg: &mut EditSegment,
    index: usize,
    after: Option<usize>,
) -> Result<HashMap<usize, usize>> {
    let mut order: Vec<usize> = (1..=seg.nodes.len()).filter(|i| *i != index).collect();
    let at = after
        .and_then(|a| order.iter().position(|i| *i == a))
        .map_or(order.len(), |p| p + 1);
    order.insert(at, index);
    seg.reorder(&order)
}

/// Moves each node of `new` right after the last other node of its type that is not itself
/// still waiting to be placed; returns where the nodes ended up.
pub(super) fn group_with_type(seg: &mut EditSegment, new: &[usize]) -> Result<Vec<usize>> {
    let mut placed: Vec<usize> = new.to_vec();
    let mut pending: Vec<usize> = new.to_vec();
    for k in 0..placed.len() {
        let nn = placed[k];
        let tid = seg.node(nn).map(|n| n.node.type_id);
        let last = (1..=seg.nodes.len())
            .filter(|i| !pending.contains(i) && seg.node(*i).map(|n| n.node.type_id) == tid)
            .max();
        let Some(last) = last else { continue };
        let map = move_after(seg, nn, Some(last))?;
        let follow = |i: &usize| map.get(i).copied().unwrap_or(*i);
        placed = placed.iter().map(follow).collect();
        pending = pending
            .iter()
            .map(follow)
            .filter(|i| *i != placed[k])
            .collect();
    }
    Ok(placed)
}

/// Moves node `index` right before node `before` (or to the end when `before` is `None`);
/// returns old → new indexes.
pub(super) fn move_before(
    seg: &mut EditSegment,
    index: usize,
    before: Option<usize>,
) -> Result<HashMap<usize, usize>> {
    let mut order: Vec<usize> = (1..=seg.nodes.len()).filter(|i| *i != index).collect();
    let at = before
        .and_then(|b| order.iter().position(|i| *i == b))
        .unwrap_or(order.len());
    order.insert(at, index);
    seg.reorder(&order)
}
