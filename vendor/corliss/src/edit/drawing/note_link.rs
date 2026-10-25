//! The references of a copied note across segments (second half of adding a note): the SM
//! graphics, the t9 entries between the sheet DC, the SM and the styles, and the note's
//! variable number.

use super::fields::{int_of, set_int};
use super::note_add::{CopiedNote, NoteTemplate};
use super::note_doc::Placement;
use super::note_sm::add_note_sm;
use super::xref::{TemplateSegment, xref_add, xref_map};
use super::{EditSegment, Editor, T_COMPONENT, first_of_type, missing, node_in, node_in_mut};
use crate::error::Result;
use crate::grammar::DecodedNode;

/// Note part naming the SM graphics group through the t9 table (0675182E).
const T_NOTE_PART: u32 = 0x0675_182E;
/// The note's link to the drafting standard (614A01F1).
const T_STANDARD_LINK: u32 = 0x614A_01F1;

/// Adds the SM graphics and links the copied note to them and to the styles.
pub(super) fn link_note(
    ed: &mut Editor,
    tpl: &NoteTemplate,
    note: &CopiedNote,
    text: &str,
    at: &Placement<'_>,
) -> Result<()> {
    let dc_name = note.dc_name();
    let tdc = tpl.sheet("DCSegment")?;
    let note_id = node_in(ed.segment(&dc_name)?, note.note()?)?.object_id() as u16;
    let group_id = add_note_sm(
        ed,
        &note.prefix,
        &tpl.sheet("SMSegment")?,
        note_id,
        text,
        at,
    )?;
    link_graphics(ed, &tdc, note, group_id as u16)?;
    link_standards(ed, &tdc, note)?;
    adjust_variable_number(ed.segment(&dc_name)?, tdc.seg, note.note()?)
}

/// 0675182E `graphics_xref` (`t1` before the full grammar) is a 1-based index into the sheet
/// DC's t9 table: the entry naming the note's SM graphics group by its object id (dw_note:
/// 19 → (12, 0, 0, 303)). A dangling index fails the sheet DC load.
fn link_graphics(
    ed: &mut Editor,
    tdc: &TemplateSegment<'_>,
    note: &CopiedNote,
    group_id: u16,
) -> Result<()> {
    let dc_name = note.dc_name();
    let (part, tpart) = note.first_of(ed.segment(&dc_name)?, T_NOTE_PART)?;
    let tpart = &node_in(tdc.seg, tpart)?.node;
    let field = if tpart.get("graphics_xref").is_some() {
        "graphics_xref"
    } else {
        "t1"
    };
    let key = xref_add(
        ed,
        &dc_name,
        tdc,
        int_of(tpart, field) as usize,
        Some(group_id),
    )?;
    set_int(
        &mut node_in_mut(ed.segment(&dc_name)?, part)?.node,
        field,
        key as i64,
    )
}

/// Sheet DC fields holding 1-based t9 indexes (dw_proj → dw_dim shifted them all by one when
/// an entry was inserted ahead): each is mapped to the entry naming the same object in the
/// target, added when missing. The note brings one: 614A01F1 `standard_xref`, the sheet
/// copy's reference to the drafting standard 36BC43F4.
fn link_standards(ed: &mut Editor, tdc: &TemplateSegment<'_>, note: &CopiedNote) -> Result<()> {
    let dc_name = note.dc_name();
    for k in &note.copies {
        let dc = ed.segment(&dc_name)?;
        let Some(n) = dc.node(*k).filter(|n| n.node.type_id.0 == T_STANDARD_LINK) else {
            continue;
        };
        let tk = int_of(&n.node, "standard_xref") as usize;
        let key = xref_map(ed, &dc_name, tdc, tk)?;
        set_int(
            &mut node_in_mut(ed.segment(&dc_name)?, *k)?.node,
            "standard_xref",
            key as i64,
        )?;
    }
    Ok(())
}

/// 5EB510C1 `u32_3` follows the sheet fragment's next variable number (2 per view or
/// dimension: dw_proj 15 / dw_dim 17), which notes do not change: Inventor's note on dw_proj
/// (dw_note0) has 4 = 15 - 11, the one on dw_dim (dw_note) 6 = 17 - 11.
fn adjust_variable_number(dc: &mut EditSegment, tdc: &EditSegment, note: usize) -> Result<()> {
    let own = next_variable(&node_in(dc, first_of_type(dc, T_COMPONENT)?)?.node)?;
    let template = next_variable(&node_in(tdc, first_of_type(tdc, T_COMPONENT)?)?.node)?;
    let n = &mut node_in_mut(dc, note)?.node;
    let v = int_of(n, "u32_3");
    set_int(n, "u32_3", v + own - template)
}

/// RDxFragment (90874D63) next variable number; older files keep it as `a1[0]`.
fn next_variable(frag: &DecodedNode) -> Result<i64> {
    if let Some(v) = frag.int("next_variable_number") {
        return Ok(v);
    }
    let a1 = frag
        .get("a1")
        .and_then(|v| v.as_ints())
        .and_then(|v| v.first().copied());
    a1.ok_or_else(|| missing("next variable number of the sheet component"))
}
