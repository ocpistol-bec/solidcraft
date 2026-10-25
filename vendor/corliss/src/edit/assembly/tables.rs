//! The cross-segment reference tables an assembly edit keeps in step: M `t6` (referenced
//! segments and their revisions), `t7` (objects owned per revision), `t9` (root entries) and
//! their RSeSegInfo mirrors, and the revision bump that moves them to the session revision.

use crate::database::{RootRecord, SegmentEntry};
use crate::edit::Editor;
use crate::error::{CorlissError, Result};
use crate::guid::Guid;
use crate::segment::Ownership;

const STRUCTURE: &str = "segment tables";

/// The reference engine writes a drawn GUID as `uuid4().bytes` (RFC byte order) where GUIDs
/// are otherwise kept in Windows layout; a drawn GUID is turned the same way so the output
/// matches it byte for byte.
pub(super) fn uuid_bytes(g: Guid) -> Guid {
    let b = *g.as_bytes();
    let order = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    Guid::from_bytes(order.map(|k| b[k]))
}

/// The M `t9` table of segment `name`.
pub(super) fn t9(ed: &mut Editor, name: &str) -> Result<Vec<[u16; 4]>> {
    Ok(ed.segment(name)?.segment.meta.body.tail.t9.clone())
}

fn set_t9(ed: &mut Editor, name: &str, t9: Vec<[u16; 4]>) -> Result<()> {
    ed.segment(name)?.segment.meta.body.tail.t9 = t9;
    Ok(())
}

/// The RSeSegInfo entry of segment `name`.
fn entry<'a>(ed: &'a mut Editor, name: &str) -> Result<&'a mut SegmentEntry> {
    let seg_id = ed
        .segment(name)?
        .segment
        .meta
        .header
        .identity
        .as_ref()
        .map(|i| i.seg_id);
    ed.seg_info_mut()
        .segments
        .iter_mut()
        .find(|e| Some(e.seg_id) == seg_id)
        .ok_or_else(|| {
            CorlissError::missing("edit", STRUCTURE, format!("RSeSegInfo entry of {name}"))
        })
}

/// Root records mirroring `t9` (empty when the segment stores none).
fn roots<'a>(ed: &'a mut Editor, name: &str) -> Result<&'a mut Vec<RootRecord>> {
    Ok(&mut entry(ed, name)?.roots)
}

/// A root record one key further, holding object `value` (`None`: same object).
fn next_root(r: &RootRecord, value: Option<u16>) -> RootRecord {
    let key = ((u32::from(r.root_class) << 16) | u32::from(r.root_key)).wrapping_add(1);
    RootRecord {
        root_key: key as u16,
        root_class: (key >> 16) as u16,
        obj_id_lo: value.unwrap_or(r.obj_id_lo),
        ..*r
    }
}

/// A root record one key back.
fn previous_root(r: &RootRecord) -> RootRecord {
    let key = ((u32::from(r.root_class) << 16) | u32::from(r.root_key)).wrapping_sub(1);
    RootRecord {
        root_key: key as u16,
        root_class: (key >> 16) as u16,
        ..*r
    }
}

/// Records `count` new t9 entries of segment object `obj` under the session revision: t7
/// gains (revision, obj, count) after the last entry of that object. Raising the count of an
/// existing t7 entry instead makes Inventor crash on load.
pub(super) fn t7_add(ed: &mut Editor, name: &str, obj: u16, count: u16) -> Result<()> {
    let g = ed.session_revision();
    let t7 = &mut ed.segment(name)?.segment.meta.body.tail.t7;
    let at = t7
        .iter()
        .rposition(|e| e.segment == obj)
        .unwrap_or(t7.len().saturating_sub(1))
        + 1;
    t7.insert(
        at.min(t7.len()),
        Ownership {
            revision: g,
            segment: obj,
            count,
        },
    );
    Ok(())
}

/// The object index of the last t7 entry of segment `name`.
pub(super) fn last_t7_object(ed: &mut Editor, name: &str) -> Result<u16> {
    let t7 = &ed.segment(name)?.segment.meta.body.tail.t7;
    t7.last()
        .map(|e| e.segment)
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("t7 of {name}")))
}

/// End of the browser's first t9 run (keys 0, 1, 2 …).
pub(super) fn first_run_end(t9: &[[u16; 4]]) -> Result<usize> {
    (1..t9.len())
        .find(|i| u32::from(t9[*i][0]) != u32::from(t9[i - 1][0]) + 1)
        .ok_or_else(|| {
            CorlissError::invalid(
                STRUCTURE,
                "t9",
                0,
                format!("{} consecutive keys", t9.len()),
                "a t9 whose first run ends",
            )
        })
}

/// Start of the second occurrence run of the AmDc t9 (where the key drops).
pub(super) fn second_run_start(t9: &[[u16; 4]]) -> Result<usize> {
    (1..t9.len())
        .find(|i| t9[*i][0] < t9[i - 1][0])
        .ok_or_else(|| {
            CorlissError::invalid(
                STRUCTURE,
                "t9",
                0,
                format!("{t9:?}"),
                "a t9 with a second run (an assembly with occurrences)",
            )
        })
}

/// AmBrowserSegment t9: a first run (key 0, 1, 2 …) whose values are the AmDc object ids of
/// the document and its occurrences, then key-0 entries, then a final entry whose key is the
/// length of the first run. A new object appends (next key, 0, 0, its AmDc object id) to the
/// first run and bumps the final key; the last t7 count and the root records follow.
pub(super) fn t9_add_browser(ed: &mut Editor, name: &str, dc_oid: u16) -> Result<()> {
    let mut t9 = t9(ed, name)?;
    let cut = first_run_end(&t9)?;
    let a = t9[cut - 1];
    t9.insert(cut, [a[0] + 1, a[1], a[2], dc_oid]);
    if let Some(last) = t9.last_mut() {
        last[0] += 1;
    }
    set_t9(ed, name, t9)?;
    t7_add(ed, name, 0, 1)?;
    let roots = roots(ed, name)?;
    if let (Some(prev), Some(_)) = (roots.get(cut - 1).copied(), roots.last()) {
        roots.insert(cut, next_root(&prev, Some(dc_oid)));
        let k = roots.len() - 1;
        roots[k] = next_root(&roots[k], None);
    }
    Ok(())
}

/// AmDcSegment t9 lists AmRx object ids (a cross-segment reference table): after fixed head
/// entries come two runs with one (key, 0, 0, id) entry per occurrence, first the AmRx
/// placements, then their FDCD32FE records. A new occurrence appends to both runs, and t7 gets
/// an entry under the session revision counting the 2 new entries. Without this the file
/// fails with "Error loading segment AmDcSegment".
pub(super) fn t9_add_occurrence(
    ed: &mut Editor,
    name: &str,
    rx_ids: Option<(u16, u16)>,
) -> Result<()> {
    let mut t9 = t9(ed, name)?;
    let cut = second_run_start(&t9)?;
    let (a, b) = (t9[cut - 1], t9[t9.len() - 1]);
    let (va, vb) = rx_ids.unwrap_or((a[3] + 3, b[3] + 3));
    t9.insert(cut, [a[0] + 1, a[1], a[2], va]);
    t9.push([b[0] + 1, b[1], b[2], vb]);
    set_t9(ed, name, t9)?;
    let obj = last_t7_object(ed, name)?;
    t7_add(ed, name, obj, 2)?;
    let roots = roots(ed, name)?;
    if let (Some(prev), Some(last)) = (roots.get(cut - 1).copied(), roots.last().copied()) {
        roots.insert(cut, next_root(&prev, Some(va)));
        roots.push(next_root(&last, Some(vb)));
    }
    Ok(())
}

/// Appends (next key, 0, 0, `value`) after the last t9 entry, and its root record.
pub(super) fn t9_append(ed: &mut Editor, name: &str, value: u16) -> Result<()> {
    let mut t9 = t9(ed, name)?;
    let last = *t9
        .last()
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("t9 of {name}")))?;
    t9.push([last[0] + 1, last[1], last[2], value]);
    set_t9(ed, name, t9)?;
    let roots = roots(ed, name)?;
    if let Some(last) = roots.last().copied() {
        roots.push(next_root(&last, Some(value)));
    }
    Ok(())
}

/// Removes the t9 entries whose id is in `values` from each occurrence run `(start, end)`,
/// re-keys the later entries of that run and keeps the root records in step. Returns the
/// removed positions and the occurrence's position in the first run that held it.
pub(super) fn t9_remove(
    ed: &mut Editor,
    name: &str,
    values: &[u16],
    runs: &[(usize, usize)],
) -> Result<(Vec<usize>, Option<usize>)> {
    let mut t9 = t9(ed, name)?;
    let mut roots = roots(ed, name)?.clone();
    let (mut gone, mut occ_pos) = (Vec::new(), None);
    for (a, b) in runs.iter().copied() {
        let Some(k) = rekey_after_hit(&mut t9, &mut roots, a, b, values) else {
            continue;
        };
        occ_pos = occ_pos.or(Some(k));
        gone.push(a + k);
    }
    for p in gone.iter().rev() {
        t9.remove(*p);
        if *p < roots.len() {
            roots.remove(*p);
        }
    }
    set_t9(ed, name, t9)?;
    *self::roots(ed, name)? = roots;
    Ok((gone, occ_pos))
}

/// Finds the run `a..b` entry holding one of `values` and moves the later entries of the
/// run (and their root records) one key back; returns its position in the run.
fn rekey_after_hit(
    t9: &mut [[u16; 4]],
    roots: &mut [RootRecord],
    a: usize,
    b: usize,
    values: &[u16],
) -> Option<usize> {
    let k = t9.get(a..b)?.iter().position(|e| values.contains(&e[3]))?;
    for j in a + k + 1..b.min(t9.len()) {
        t9[j][0] = t9[j][0].wrapping_sub(1);
        if let Some(r) = roots.get_mut(j) {
            *r = previous_root(r);
        }
    }
    Some(k)
}

/// Lowers the final t9 key of the browser (the length of its first run) and its root record.
pub(super) fn shorten_browser_run(ed: &mut Editor, name: &str) -> Result<()> {
    let mut t9 = t9(ed, name)?;
    if let Some(last) = t9.last_mut() {
        last[0] = last[0].wrapping_sub(1);
    }
    set_t9(ed, name, t9)?;
    let roots = roots(ed, name)?;
    if let Some(last) = roots.last_mut() {
        *last = previous_root(last);
    }
    Ok(())
}

/// Takes `per_occ` entries away from the t7 entry of object `obj` that owns the occurrence at
/// position `occ_pos` of `n_occ`; t7 entries own the occurrences from the last one backwards,
/// and an entry that drops to 0 goes.
pub(super) fn t7_release(
    ed: &mut Editor,
    name: &str,
    obj: u16,
    n_occ: usize,
    occ_pos: usize,
    per_occ: u16,
) -> Result<bool> {
    let t7 = &mut ed.segment(name)?.segment.meta.body.tail.t7;
    let Some(i) = owner(t7, obj, n_occ, occ_pos, per_occ) else {
        return Ok(false);
    };
    t7[i].count -= per_occ;
    if t7[i].count == 0 {
        t7.remove(i);
    }
    Ok(true)
}

/// The t7 entry of object `obj` that owns the occurrence at `occ_pos` of `n_occ`, each
/// entry owning `count / per_occ` occurrences from the last one backwards.
fn owner(t7: &[Ownership], obj: u16, n_occ: usize, occ_pos: usize, per_occ: u16) -> Option<usize> {
    let (pos, mut occ) = (occ_pos as i64, n_occ as i64 - 1);
    for i in (0..t7.len()).rev().filter(|i| t7[*i].segment == obj) {
        let take = i64::from(t7[i].count / per_occ.max(1));
        if occ - take < pos && pos <= occ {
            return Some(i);
        }
        occ -= take;
        if occ < 0 {
            return None;
        }
    }
    None
}

/// Moves the whole document to the session revision ([`Editor::session_revision`]; it is not
/// added to RSeDbRevisionInfo: a record appended there makes Inventor report "Invalid
/// reference to segment object used", an unlisted revision loads, W5f), as every Inventor
/// save does: each
/// segment's RSeSegInfo revision and every t6 link. A t6 entry naming a revision the
/// referenced segment does not have makes Inventor report "Invalid reference to segment
/// object used" as soon as it follows that reference, so every segment is touched.
pub(super) fn bump_revision(ed: &mut Editor) -> Result<()> {
    let g = ed.session_revision();
    let names: Vec<String> = ed
        .seg_info()
        .segments
        .iter()
        .map(|e| e.name.clone())
        .collect();
    for name in names {
        ed.segment(&name)?
            .segment
            .meta
            .body
            .tail
            .t6
            .iter_mut()
            .for_each(|link| link.revision = g);
        entry(ed, &name)?.revision = g;
    }
    Ok(())
}
