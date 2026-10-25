//! The document DC side of a new note (dw_dim → dw_note): a
//! B1089D42 record (text reference and box) with copies of the note text (1FBB3C01) and style
//! (48C5F41A), listed by the sheet's 37359718 record. New ids continue the wide id space.

use super::fields::{TailList, rewrite_tail_list, shift_floats, u32_at};
use super::fields::{append_ref, floats_named, index_of, leading_codes, set_float, set_text};
use super::note_add::CopiedNote;
use super::note_parts::{append_copies, bytes_remap, clone_wide_into};
use super::usage::bump_usage;
use super::{DOC_SEGMENT, EditSegment, Editor, T_DOC_NOTE, T_TEXT, doc_note_text_ref, missing};
use super::{node_in, node_in_mut, sheet_record};
use crate::error::Result;
use crate::grammar::{DecodedNode, FieldValue};
use crate::model::drawing::plain;

/// Note style (48C5F41A).
const T_STYLE: u32 = 0x48C5_F41A;
/// List framing: code 2, 0x3000.
pub(super) const LIST2: [u8; 4] = [2, 0, 0, 0x30];

/// Where the new note goes relative to the template's: `wx` maps a template x (the text box
/// widens with the text), `dx` and `dy` shift.
pub(super) struct Placement<'a> {
    pub(super) dx: f64,
    pub(super) dy: f64,
    pub(super) wx: &'a dyn Fn(f64) -> f64,
}

/// Adds the document DC records of a note whose template text reads `template_text`.
pub(super) fn add_note_doc(
    ed: &mut Editor,
    note: &CopiedNote,
    tdc: &EditSegment,
    template_text: &str,
    text: &str,
    at: &Placement<'_>,
) -> Result<()> {
    let rec = template_record(tdc, &plain(template_text))?;
    let sources = [
        rec,
        doc_note_text_ref(node_in(tdc, rec)?),
        style_of(tdc, rec)?,
    ];
    let (b, t) = copy_records(ed.segment(DOC_SEGMENT)?, tdc, &sources)?;
    let sheet_head = ed.segment(&note.dc_name())?.node(1);
    let sheet_name = sheet_head
        .and_then(|n| n.node.text("name"))
        .unwrap_or_default()
        .to_string();
    let dc = ed.segment(DOC_SEGMENT)?;
    move_box(&mut node_in_mut(dc, b)?.node, at)?;
    retext(&mut node_in_mut(dc, t)?.node, text, at)?;
    let record = sheet_record(dc, note.sheet)?;
    raw_list_add(&mut node_in_mut(dc, record)?.node, &sheet_name, b)?;
    bump_usage(ed, &note.prefix, 1)
}

/// Appends wide-id copies of the template record, its text and its style with their
/// references renumbered; returns the copied record and text.
fn copy_records(
    dc: &mut EditSegment,
    tdc: &EditSegment,
    sources: &[usize],
) -> Result<(usize, usize)> {
    let (copies, map) = append_copies(dc, tdc, sources, clone_wide_into)?;
    copies.iter().try_for_each(|k| bytes_remap(dc, *k, &map))?;
    match copies[..] {
        [b, t, _] => Ok((b, t)),
        _ => Err(missing("copied document DC note records")),
    }
}

/// The last template B1089D42 whose text copy reads `plain_text`.
fn template_record(tdc: &EditSegment, plain_text: &str) -> Result<usize> {
    let is_text = |t: &super::EditNode| {
        t.node.type_id.0 == T_TEXT && plain(t.node.text("name").unwrap_or_default()) == plain_text
    };
    let reads = |k: &usize| {
        tdc.node(*k)
            .and_then(|n| tdc.node(doc_note_text_ref(n)))
            .is_some_and(is_text)
    };
    let found = tdc.of_type(T_DOC_NOTE).into_iter().rfind(reads);
    found.ok_or_else(|| {
        missing(format!(
            "template document DC note record reading {plain_text:?}"
        ))
    })
}

/// The style (48C5F41A) a B1089D42 record names by its typed `style` ref.
fn style_of(tdc: &EditSegment, rec: usize) -> Result<usize> {
    let style = index_of(&node_in(tdc, rec)?.node, "style");
    let is_style = tdc.node(style).is_some_and(|n| n.node.type_id.0 == T_STYLE);
    if style == 0 || !is_style {
        return Err(missing(format!(
            "48C5F41A style ref of template note record #{rec}"
        )));
    }
    Ok(style)
}

/// B1089D42 box corners (x, y, z) in sheet cm, or the older grammar's t1/t2 and t5/t6.
fn move_box(b: &mut DecodedNode, at: &Placement<'_>) -> Result<()> {
    let up = |v: f64| v + at.dy;
    if b.get("box_corner1").is_some() {
        shift_floats(b, "box_corner1", [at.wx, &up])?;
        return shift_floats(b, "box_corner2", [at.wx, &up]);
    }
    let moves: [(&str, &dyn Fn(f64) -> f64); 4] =
        [("t1", at.wx), ("t5", at.wx), ("t2", &up), ("t6", &up)];
    for (name, f) in moves {
        let v = b
            .float(name)
            .ok_or_else(|| missing(format!("{name} of the note box")))?;
        set_float(b, name, 0, f(v))?;
    }
    Ok(())
}

/// The document DC text copy: new text behind the template's format codes, moved by (dx, dy).
fn retext(t: &mut DecodedNode, text: &str, at: &Placement<'_>) -> Result<()> {
    let old = t.text("name").unwrap_or_default().to_string();
    set_text(t, "name", &format!("{}{text}", leading_codes(&old)))?;
    if floats_named(t, "loc").len() < 2 {
        return Ok(());
    }
    shift_floats(t, "loc", [&|v| v + at.dx, &|v| v + at.dy])
}

/// Appends flagged NodeRef `index` to the list2 right before the length-prefixed UTF-16
/// string `before` in a node's raw tail (DlDocDC sheet record 37359718: the sheet's note
/// text caches, then the sheet name); with the full grammar that list is `annotations`.
fn raw_list_add(node: &mut DecodedNode, before: &str, index: usize) -> Result<()> {
    if has_annotations(node) {
        return append_ref(node, "annotations", index);
    }
    let grow = |raw: &[u8], k: usize| match grown_list(raw, k, index) {
        Some((p, bytes)) => TailList::Rebuilt(p, bytes),
        None => TailList::Absent,
    };
    if rewrite_tail_list(node, before, grow) {
        return Ok(());
    }
    Err(missing(format!(
        "list before {before:?} in node {}",
        node.type_id
    )))
}

/// True when the sheet record's grammar types its annotation list.
pub(super) fn has_annotations(node: &DecodedNode) -> bool {
    node.fields
        .iter()
        .any(|f| f.name.as_deref() == Some("annotations") && matches!(f.value, FieldValue::List(_)))
}

/// The list2 ending at `k` (scanning back from `k - 8`), rebuilt with `index` appended: its
/// start and new bytes. An empty list gets capacity 1 and the usual second meta word 0x10.
fn grown_list(b: &[u8], k: usize, index: usize) -> Option<(usize, Vec<u8>)> {
    let p = (0..=k.checked_sub(8)?)
        .rev()
        .find(|p| list_ends_at(b, *p, k))?;
    let n = u32_at(b, p + 4)? as usize;
    let (els, meta) = match n {
        0 => (
            &b[k..k],
            [0u32.to_le_bytes(), 0x10u32.to_le_bytes()].concat(),
        ),
        _ => (&b[p + 16..k], b[p + 8..p + 16].to_vec()),
    };
    let cap = (u32_at(&meta, 0)? as usize).max(n + 1) as u32;
    let mut out = LIST2.to_vec();
    out.extend(((n + 1) as u32).to_le_bytes());
    out.extend(cap.to_le_bytes());
    out.extend(&meta[4..]);
    out.extend(els);
    out.extend((0x8000_0000 | index as u32).to_le_bytes());
    Some((p, out))
}

/// True when a list2 framed at `p` ends exactly at `k`.
pub(super) fn list_ends_at(b: &[u8], p: usize, k: usize) -> bool {
    if b.get(p..p + 4) != Some(&LIST2[..]) {
        return false;
    }
    let Some(n) = u32_at(b, p + 4).map(|n| n as usize) else {
        return false;
    };
    p + 8 + if n > 0 { 8 + 4 * n } else { 0 } == k
}
