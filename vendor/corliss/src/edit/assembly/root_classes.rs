//! Root-record class indexes after the RSeSegInfo class lists change (spec 9.2.4 [W-09]:
//! writers that rewrite a segment write indices into the current lists).
//! The remap and the target fix follow a reference bug (fixed): a class-list insertion shifted
//! the root records' class indexes.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::database::SegInfo;
use crate::edit::{EditNode, Editor};
use crate::error::Result;
use crate::guid::Guid;

/// Keeps every root record on the classes it named before `uid_list1` / `uid_list2` changed:
/// an insertion shifts every later index. −1 (target not loaded) and indexes past the old
/// list (a stale segment) are left as they are.
pub(super) fn remap_root_classes(si: &mut SegInfo, old1: &[Guid], old2: &[Guid]) {
    let (new1, new2) = (si.uid_list1.clone(), si.uid_list2.clone());
    for r in si.segments.iter_mut().flat_map(|e| e.roots.iter_mut()) {
        r.root_a = reindex(r.root_a, old1, &new1);
        r.root_b = reindex(r.root_b, old2, &new2);
    }
}

fn reindex(v: i16, old: &[Guid], new: &[Guid]) -> i16 {
    let Some(class) = usize::try_from(v).ok().and_then(|k| old.get(k)) else {
        return v;
    };
    new.iter().position(|g| g == class).map_or(v, |k| k as i16)
}

/// Sets `root_a` of each root record in the segments this edit opened to the `uid_list1`
/// index of the class of the node the record points at (t6 entry → segment, object id →
/// node), appending classes the list lacks. The rule holds for all 19,961 resolvable records
/// of the reference implementation's sample pool; unresolvable records keep their value.
pub(super) fn fix_root_targets(ed: &mut Editor) -> Result<()> {
    let targets = root_targets(ed)?;
    let list = &mut ed.seg_info.uid_list1;
    for (entry, root, class) in &targets {
        if !list.contains(class) {
            list.push(*class);
        }
        let at = list.iter().position(|g| g == class).unwrap_or(0) as i16;
        if let Some(r) = ed
            .seg_info
            .segments
            .get_mut(*entry)
            .and_then(|e| e.roots.get_mut(*root))
        {
            r.root_a = at;
        }
    }
    Ok(())
}

/// (entry, root record, target class) for every resolvable root record of the opened
/// segments, in directory order.
fn root_targets(ed: &Editor) -> Result<Vec<(usize, usize, Guid)>> {
    let mut classes = TargetClasses::default();
    let mut out = Vec::new();
    for (k, entry) in ed.seg_info.segments.iter().enumerate() {
        let Some(seg) = ed.segments.get(&entry.name) else {
            continue;
        };
        let t6 = &seg.segment.meta.body.tail.t6;
        for (j, r) in entry.roots.iter().enumerate() {
            let link = t6.get(usize::from(r.root_class)).filter(|_| r.root_a >= 0);
            let Some(link) = link else { continue };
            if let Some(class) = classes.class_of(ed, link.seg_ref, r.object_id())? {
                out.push((k, j, class));
            }
        }
    }
    Ok(out)
}

/// Node classes by object id, per segment id, read once per segment.
#[derive(Default)]
struct TargetClasses {
    by_segment: HashMap<Guid, HashMap<u64, Guid>>,
}

impl TargetClasses {
    /// The class of object `id` in the segment with id `seg_id`, `None` when not found.
    fn class_of(&mut self, ed: &Editor, seg_id: Guid, id: u64) -> Result<Option<Guid>> {
        let classes = match self.by_segment.entry(seg_id) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(segment_classes(ed, seg_id)?),
        };
        Ok(classes.get(&id).copied())
    }
}

/// Object id → class of every node of segment `seg_id`: the session's nodes when the segment
/// is open, else those of the document.
fn segment_classes(ed: &Editor, seg_id: Guid) -> Result<HashMap<u64, Guid>> {
    let entry = ed.seg_info.segments.iter().find(|e| e.seg_id == seg_id);
    let Some(name) = entry.map(|e| e.name.clone()) else {
        return Ok(HashMap::new());
    };
    let index = |nodes: &[EditNode]| {
        nodes
            .iter()
            .filter_map(|n| Some((wide_object_id(n)?, n.class)))
            .collect()
    };
    match ed.segments.get(&name) {
        Some(seg) => Ok(index(&seg.nodes)),
        None => Ok(index(&ed.open(&name)?.nodes)),
    }
}

/// `(u32 at 0 << 16) | u16 at 4` of the payload: the object id a root record names.
fn wide_object_id(n: &EditNode) -> Option<u64> {
    let p = n.payload();
    let hi = u32::from_le_bytes(p.get(..4)?.try_into().ok()?);
    let lo = u16::from_le_bytes(p.get(4..6)?.try_into().ok()?);
    Some(u64::from(hi) << 16 | u64::from(lo))
}
