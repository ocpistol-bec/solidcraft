//! Deleting a note (dw_note → dw_dim) by unlinking it everywhere it is
//! listed.

use std::collections::BTreeSet;

use super::fields::update_first_word;
use super::fields::{TailList, refs_named, remove_refs, rewrite_tail_list, u32_at, u32_from_end};
use super::locate::{find_note, sheet_prefix};
use super::note_add::HAS_NOTES;
use super::note_doc::{has_annotations, list_ends_at};
use super::usage::bump_usage;
use super::{DOC_SEGMENT, EditSegment, Editor, T_COMPONENT, T_DL_TEXT, T_DOC_NOTE, T_NOTE};
use super::{T_SM_NOTE, T_TEXT, doc_note_text_ref, first_of_type, missing, node_in_mut};
use super::{decoded_of_type, sheet_record};
use crate::error::Result;
use crate::grammar::DecodedNode;
use crate::model::drawing::plain;

/// Deletes the note on `sheet` (1-based) whose plain text is `which` by unlinking it from the
/// sheet component's `objects` (note object and parts), the document DC sheet record's
/// text-cache list and the SM root, and lowering the style usage counts. The records stay as
/// unreferenced blocks: the sheet DC has opaque nodes, so they cannot be renumbered away
/// safely, and Inventor drops unreferenced blocks on its next save. Returns the note object's
/// node index.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::drawing::delete_note(&mut ed, 1, "HELLO NOTE")?;
/// ed.finish()?.save("without-note.idw")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn delete_note(ed: &mut Editor, sheet: usize, which: &str) -> Result<usize> {
    let (note, count) = find_note(ed.document(), sheet, which)?;
    let prefix = sheet_prefix(ed.document(), sheet)?;
    let dc = ed.segment(&format!("{prefix}DCSegment"))?;
    let object = note_object(dc, note.node)?;
    let sheet_name = dc
        .node(1)
        .and_then(|n| n.node.text("name"))
        .unwrap_or_default()
        .to_string();
    unlink_from_component(dc, object, count == 1)?;
    bump_usage(ed, &prefix, -1)?;
    unlink_doc_record(ed.segment(DOC_SEGMENT)?, sheet, &sheet_name, &note.text)?;
    unlink_sm_entry(ed.segment(&format!("{prefix}SMSegment"))?, &note.text)?;
    Ok(object)
}

/// The decoded note object (5EB510C1) whose children include text node `text`.
fn note_object(dc: &EditSegment, text: usize) -> Result<usize> {
    let owns = |k: &usize| {
        dc.node(*k)
            .is_some_and(|n| refs_named(&n.node, "children").contains(&text))
    };
    let found = decoded_of_type(dc, T_NOTE).into_iter().find(owns);
    found.ok_or_else(|| missing(format!("note object owning text #{text}")))
}

/// Drops the note object and its parts from the component's `objects`; the last note
/// clears the component's has-notes flag.
fn unlink_from_component(dc: &mut EditSegment, object: usize, last: bool) -> Result<()> {
    let children = dc
        .node(object)
        .map(|n| refs_named(&n.node, "children"))
        .unwrap_or_default();
    let parts: BTreeSet<usize> = children.into_iter().chain([object]).collect();
    let comp = first_of_type(dc, T_COMPONENT)?;
    let comp = &mut node_in_mut(dc, comp)?.node;
    remove_refs(comp, "objects", &parts)?;
    if last {
        update_first_word(comp, "a0", |a| a & !HAS_NOTES)?;
    }
    Ok(())
}

/// Unlinks the first document DC note record whose text copy reads `text` from the sheet
/// record's list.
fn unlink_doc_record(
    doc: &mut EditSegment,
    sheet: usize,
    sheet_name: &str,
    text: &str,
) -> Result<()> {
    let record = sheet_record(doc, sheet)?;
    let is_text = |t: &super::EditNode| {
        t.node.type_id.0 == T_TEXT && plain(t.node.text("name").unwrap_or_default()) == text
    };
    let reads = |k: &usize| {
        doc.node(*k)
            .and_then(|b| doc.node(doc_note_text_ref(b)))
            .is_some_and(is_text)
    };
    let candidates: Vec<usize> = doc.of_type(T_DOC_NOTE).into_iter().filter(reads).collect();
    let rec = &mut node_in_mut(doc, record)?.node;
    for b in candidates {
        if raw_list_remove(rec, sheet_name, b)? {
            break;
        }
    }
    Ok(())
}

/// Removes NodeRef `index` from the `annotations` list, or from the list2 right before the
/// length-prefixed UTF-16 string `before` in the raw tail (inverse of `raw_list_add`).
fn raw_list_remove(node: &mut DecodedNode, before: &str, index: usize) -> Result<bool> {
    if has_annotations(node) {
        return Ok(remove_refs(node, "annotations", &BTreeSet::from([index]))? > 0);
    }
    let shrink = |raw: &[u8], k: usize| {
        let Some(p) = filled_list_before(raw, k) else {
            return TailList::Absent;
        };
        match shrunk_list(&raw[p..k], index) {
            Some(bytes) => TailList::Rebuilt(p, bytes),
            None => TailList::Unchanged,
        }
    };
    Ok(rewrite_tail_list(node, before, shrink))
}

/// The start of the non-empty list2 that ends at `k` (scanning back from `k - 8`).
fn filled_list_before(b: &[u8], k: usize) -> Option<usize> {
    let filled = |p: &usize| list_ends_at(b, *p, k) && u32_at(b, p + 4).is_some_and(|n| n > 0);
    (0..=k.checked_sub(8)?).rev().find(filled)
}

/// A non-empty list2 without the elements naming `index`; `None` when none does. An emptied
/// list loses its meta words.
fn shrunk_list(list: &[u8], index: usize) -> Option<Vec<u8>> {
    let n = u32_at(list, 4)? as usize;
    let els: Vec<u32> = (0..n).filter_map(|i| u32_at(list, 16 + 4 * i)).collect();
    let keep: Vec<u32> = els
        .iter()
        .copied()
        .filter(|e| (e & 0x7FFF_FFFF) as usize != index)
        .collect();
    if keep.len() == n {
        return None;
    }
    let mut out = list.get(..4)?.to_vec();
    out.extend((keep.len() as u32).to_le_bytes());
    if !keep.is_empty() {
        out.extend(list.get(8..16)?);
        keep.iter().for_each(|e| out.extend(e.to_le_bytes()));
    }
    Some(out)
}

/// Unlinks from the SM root the first note entry whose graphics group holds the text.
fn unlink_sm_entry(sm: &mut EditSegment, text: &str) -> Result<()> {
    let Some(entry) = sm
        .of_type(T_SM_NOTE)
        .into_iter()
        .find(|e| shows_text(sm, *e, text))
    else {
        return Ok(());
    };
    remove_refs(
        &mut node_in_mut(sm, 1)?.node,
        "lst0",
        &BTreeSet::from([entry]),
    )?;
    Ok(())
}

/// True when SM note entry `entry`'s graphics group (the NodeRef 9 bytes from its end)
/// holds an A79EACD5 text reading `text`.
fn shows_text(sm: &EditSegment, entry: usize, text: &str) -> bool {
    let payload = sm.node(entry).map(|n| n.payload()).unwrap_or_default();
    let group = (u32_from_end(&payload, 9) & 0x7FFF_FFFF) as usize;
    let kids = sm
        .node(group)
        .map(|g| refs_named(&g.node, "lst0"))
        .unwrap_or_default();
    let reads =
        |k: &super::EditNode| k.node.type_id.0 == T_DL_TEXT && k.node.text("string") == Some(text);
    kids.iter().filter_map(|k| sm.node(*k)).any(reads)
}
