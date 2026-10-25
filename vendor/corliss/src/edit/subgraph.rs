//! Copying a node together with the nodes only it owns.

use std::collections::{BTreeMap, BTreeSet};

use super::segment::EditSegment;
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodeStatus, RefKind};

const STRUCTURE: &str = "segment edit";

impl EditSegment {
    /// `root` and every node reachable over child refs whose referrers are all in the set.
    pub fn exclusive_subgraph(&self, root: usize) -> Vec<usize> {
        let mut set = BTreeSet::from([root]);
        while let Some(next) = self.next_owned(&set) {
            set.insert(next);
        }
        set.into_iter().collect()
    }

    /// The first child of the set, not in it, whose referrers all lie inside the set.
    fn next_owned(&self, set: &BTreeSet<usize>) -> Option<usize> {
        let children = set
            .iter()
            .filter_map(|i| self.node(*i))
            .flat_map(|n| n.node.refs());
        children
            .filter(|(_, kind)| *kind == RefKind::Child)
            .filter_map(|(r, _)| r.index())
            .find(|t| !set.contains(t) && self.referrers(*t).iter().all(|(r, _)| set.contains(r)))
    }

    /// Appends copies of `root`'s exclusive subgraph with fresh object ids; refs between the
    /// copies point at the copies. Returns old → new indexes.
    pub fn clone_subgraph(&mut self, root: usize) -> Result<BTreeMap<usize, usize>> {
        let olds = self.exclusive_subgraph(root);
        let mut clones = Vec::with_capacity(olds.len());
        for i in &olds {
            let src = self
                .node(*i)
                .cloned()
                .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("node #{i}")))?;
            if src.node.status != DecodeStatus::Full {
                return Err(CorlissError::invalid(
                    STRUCTURE,
                    "status",
                    *i,
                    format!("{:?} node {}", src.node.status, src.node.type_id),
                    "a fully decoded node",
                ));
            }
            clones.push(self.clone_node(&src, false)?);
        }
        let mapping: BTreeMap<usize, usize> = olds
            .iter()
            .zip(clones)
            .map(|(old, c)| (*old, self.append(c)))
            .collect();
        for new in mapping.values() {
            if let Some(n) = self.node_mut(*new) {
                n.node.remap_refs(&|i| mapping.get(&i).copied());
            }
        }
        Ok(mapping)
    }
}
