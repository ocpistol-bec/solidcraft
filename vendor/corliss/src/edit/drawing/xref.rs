//! Cross-segment references copied from a template and the session revision every segment
//! moves to.
//!
//! A t9 entry is (key, ., ., object id) with key the index of the t7 entry (revision,
//! referenced segment = t6 index, count) that owns it; RSeSegInfo mirrors t9 in its root
//! records. Records hold 1-based t9 indexes (SM 31C56502 `u32_1`, F2FB355D `u32_1`, DC
//! 0675182E `t1`), so new entries are appended and no existing index moves.

use super::{EditSegment, Editor, STRUCTURE, missing};
use crate::database::{RootRecord, SegInfo, SegmentEntry, SegmentReference};
use crate::error::{CorlissError, Result};
use crate::guid::Guid;
use crate::segment::{MTail, Ownership, SegmentLink};

/// A template segment with its directory, the source of copied references.
pub(super) struct TemplateSegment<'t> {
    pub(super) seg: &'t EditSegment,
    pub(super) info: &'t SegInfo,
}

impl TemplateSegment<'_> {
    fn tail(&self) -> &MTail {
        &self.seg.segment.meta.body.tail
    }

    /// Template t9 entry `tk` (1-based).
    fn t9(&self, tk: usize) -> Result<[u16; 4]> {
        let t9 = &self.tail().t9;
        let found = tk.checked_sub(1).and_then(|k| t9.get(k)).copied();
        let expected = format!("a template t9 index in 1..={}", t9.len());
        found.ok_or_else(|| CorlissError::invalid(STRUCTURE, "t9 index", 0, tk, expected))
    }

    /// The referenced segment and its t6 index behind t7 entry `key`.
    fn referenced(&self, key: u16) -> Result<(Guid, u16)> {
        let obj = t7_segment(&self.tail().t7, key)?;
        Ok((t6_ref(&self.tail().t6, obj)?, obj))
    }

    fn entry(&self) -> Result<&SegmentEntry> {
        let seg_id = seg_id_of(self.seg);
        let found = self.info.segments.iter().find(|e| Some(e.seg_id) == seg_id);
        found.ok_or_else(|| missing("template segment directory entry"))
    }
}

fn t7_segment(t7: &[Ownership], key: u16) -> Result<u16> {
    let found = t7.get(usize::from(key)).map(|o| o.segment);
    let expected = format!("a t7 index below {}", t7.len());
    found.ok_or_else(|| CorlissError::invalid(STRUCTURE, "t9 key", 0, key, expected))
}

fn t6_ref(t6: &[SegmentLink], obj: u16) -> Result<Guid> {
    let found = t6.get(usize::from(obj)).map(|l| l.seg_ref);
    let expected = format!("a t6 index below {}", t6.len());
    found.ok_or_else(|| CorlissError::invalid(STRUCTURE, "t7 segment", 0, obj, expected))
}

/// The segment id an M stream records.
pub(super) fn seg_id_of(seg: &EditSegment) -> Option<Guid> {
    seg.segment.meta.header.identity.as_ref().map(|i| i.seg_id)
}

/// An opened segment with its directory entry and the document-wide class registries.
struct Target<'e> {
    seg: &'e mut EditSegment,
    entry: &'e mut SegmentEntry,
    uid_list1: &'e mut Vec<Guid>,
    uid_list2: &'e mut Vec<Guid>,
}

/// Segment `name`, opened, borrowed together with its directory entry.
fn target<'e>(ed: &'e mut Editor, name: &str) -> Result<Target<'e>> {
    ed.segment(name)?;
    let seg = ed
        .segments
        .get_mut(name)
        .ok_or_else(|| missing(name.to_string()))?;
    let seg_id = seg_id_of(seg);
    let info = &mut ed.seg_info;
    let entry = info.segments.iter_mut().find(|e| Some(e.seg_id) == seg_id);
    let entry = entry.ok_or_else(|| missing(format!("directory entry of {name}")))?;
    let (uid_list1, uid_list2) = (&mut info.uid_list1, &mut info.uid_list2);
    Ok(Target {
        seg,
        entry,
        uid_list1,
        uid_list2,
    })
}

/// Appends to segment `name` a copy of template t9 entry `tk` naming object `value` (or the
/// template's object): t7 gains (session revision, referenced segment, 1), t9 the entry, and
/// the directory the template's root record with key and id replaced. Returns the new
/// 1-based t9 index.
pub(super) fn xref_add(
    ed: &mut Editor,
    name: &str,
    tpl: &TemplateSegment<'_>,
    tk: usize,
    value: Option<u16>,
) -> Result<usize> {
    let rev = ed.session_revision();
    let [tkey, a1, a2, tval] = tpl.t9(tk)?;
    let (seg_ref, tobj) = tpl.referenced(tkey)?;
    let val = value.unwrap_or(tval);
    let mut t = target(ed, name)?;
    let obj = link_index(&mut t, tpl, (seg_ref, tobj), rev)?;
    let root = copied_root(tpl, tk, &mut t, obj, val)?;
    let tail = &mut t.seg.segment.meta.body.tail;
    tail.t7.push(Ownership {
        revision: rev,
        segment: obj,
        count: 1,
    });
    tail.t9.push([root.root_key, a1, a2, val]);
    t.entry.roots.push(root);
    Ok(tail.t9.len())
}

/// The t6 index of segment `link.0` in the target, linked first when missing.
fn link_index(
    t: &mut Target<'_>,
    tpl: &TemplateSegment<'_>,
    link: (Guid, u16),
    rev: Guid,
) -> Result<u16> {
    let t6 = &t.seg.segment.meta.body.tail.t6;
    match t6.iter().position(|l| l.seg_ref == link.0) {
        Some(k) => Ok(k as u16),
        None => add_link(t, tpl, link, rev),
    }
}

/// The first reference to a segment (dw_proj SM → DlApp): a t6 link and a directory
/// reference copied from the template's (`link` = segment, template t6 index), both on the
/// session revision. Only the last reference record is followed by the root count, which
/// the commit rederives.
fn add_link(
    t: &mut Target<'_>,
    tpl: &TemplateSegment<'_>,
    link: (Guid, u16),
    rev: Guid,
) -> Result<u16> {
    let (seg_ref, tobj) = link;
    let tref = tpl.entry()?.refs.get(usize::from(tobj)).cloned();
    let tref = tref.ok_or_else(|| missing(format!("template segment reference {tobj}")))?;
    let t6 = &mut t.seg.segment.meta.body.tail.t6;
    t6.push(SegmentLink {
        seg_ref,
        revision: rev,
    });
    add_reference(t.entry, tref, seg_ref, rev);
    Ok((t6.len() - 1) as u16)
}

/// The template's directory reference `tref` as the entry's new last reference to `seg_ref`.
fn add_reference(entry: &mut SegmentEntry, tref: SegmentReference, seg_ref: Guid, rev: Guid) {
    entry.refs.iter_mut().skip(1).for_each(|r| r.ref_state = 0);
    let mut copied = tref;
    copied.ref_state = 0;
    copied.ref_revision = rev;
    copied.ref_seg_id = seg_ref;
    copied.ref_count = 0;
    entry.refs.push(copied);
    entry.n_root_records = 0;
}

/// The template's root record for `tk`, keyed by the t7 entry about to be appended, naming
/// `val`, with its class indexes mapped by GUID: an index past the registry makes Inventor
/// refuse the file or crash, so missing classes are registered.
fn copied_root(
    tpl: &TemplateSegment<'_>,
    tk: usize,
    t: &mut Target<'_>,
    obj: u16,
    val: u16,
) -> Result<RootRecord> {
    let roots = &tpl.entry()?.roots;
    let root = tk.checked_sub(1).and_then(|k| roots.get(k)).copied();
    let mut root = root.ok_or_else(|| missing(format!("template root record {tk}")))?;
    root.root_key = t.seg.segment.meta.body.tail.t7.len() as u16;
    root.root_class = obj;
    root.obj_id_lo = val;
    root.root_a = class_index(&tpl.info.uid_list1, root.root_a, t.uid_list1)?;
    root.root_b = class_index(&tpl.info.uid_list2, root.root_b, t.uid_list2)?;
    Ok(root)
}

/// The index of template class `k` in `registry`, registering it when missing. −1 (target
/// not loaded when the template was saved) names no class and is kept. An index past the
/// template's list is stale (a segment its last save did not rewrite, spec 9.2.4): the class
/// cannot be known, so it is refused (a reference bug, fixed, read it as another class).
fn class_index(template: &[Guid], k: i16, registry: &mut Vec<Guid>) -> Result<i16> {
    if k < 0 {
        return Ok(k);
    }
    let guid = template.get(k as usize).copied();
    let expected = format!("an index into {} template classes", template.len());
    let guid =
        guid.ok_or_else(|| CorlissError::invalid(STRUCTURE, "root class", 0, k, expected))?;
    if !registry.contains(&guid) {
        registry.push(guid);
    }
    Ok(registry.iter().position(|g| *g == guid).unwrap_or_default() as i16)
}

/// The target t9 index (1-based) of the object template entry `tk` names, added when missing.
pub(super) fn xref_map(
    ed: &mut Editor,
    name: &str,
    tpl: &TemplateSegment<'_>,
    tk: usize,
) -> Result<usize> {
    let [tkey, _, _, tval] = tpl.t9(tk)?;
    let (tref, _) = tpl.referenced(tkey)?;
    let tail = &ed.segment(name)?.segment.meta.body.tail;
    for (i, [k, _, _, v]) in tail.t9.iter().enumerate() {
        if *v == tval && t6_ref(&tail.t6, t7_segment(&tail.t7, *k)?)? == tref {
            return Ok(i + 1);
        }
    }
    xref_add(ed, name, tpl, tk, None)
}

/// Moves the whole document to the session revision, as every Inventor save does: each
/// directory entry and each segment's t6 links. A t6 link naming a revision the referenced
/// segment does not have makes Inventor report "Invalid reference to segment object used"
/// as soon as it follows that reference, so every segment is touched.
pub(super) fn bump_revision(ed: &mut Editor) -> Result<()> {
    let rev = ed.session_revision();
    let names: Vec<String> = ed
        .seg_info()
        .segments
        .iter()
        .map(|e| e.name.clone())
        .collect();
    for name in &names {
        let t = target(ed, name)?;
        t.seg
            .segment
            .meta
            .body
            .tail
            .t6
            .iter_mut()
            .for_each(|l| l.revision = rev);
        t.entry.revision = rev;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::class_index;
    use crate::guid::Guid;

    fn guid(k: u8) -> Guid {
        Guid::from_bytes([k; 16])
    }

    // Regression (a reference bug, fixed): -1 was read as the template's last class.
    #[test]
    fn an_unloaded_target_keeps_minus_one_and_registers_nothing() {
        let mut registry = vec![guid(1)];
        assert_eq!(
            class_index(&[guid(1), guid(2)], -1, &mut registry).unwrap(),
            -1
        );
        assert_eq!(registry, [guid(1)]);
    }

    #[test]
    fn a_template_class_is_registered_once() {
        let mut registry = vec![guid(1)];
        assert_eq!(
            class_index(&[guid(1), guid(2)], 1, &mut registry).unwrap(),
            1
        );
        assert_eq!(
            class_index(&[guid(1), guid(2)], 1, &mut registry).unwrap(),
            1
        );
        assert_eq!(registry, [guid(1), guid(2)]);
    }

    #[test]
    fn a_stale_index_past_the_template_list_is_refused() {
        let err = class_index(&[guid(1)], 3, &mut Vec::new()).unwrap_err();
        assert!(err.to_string().contains('3'), "{err}");
    }
}
