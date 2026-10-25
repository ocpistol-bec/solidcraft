//! The sheet SM side of a new note (dw_dim → dw_note).
//!
//! - An entry 5EB510C2, listed by the SM root F4A2F948 `lst0`. Its untyped tail is the
//!   NodeRef of the note's graphics group A79EACCF, then a 1-based index into the M stream's
//!   t9 table: the cross-segment entry naming the DC note object by its object id.
//! - The group holds the A79EACD5 text, nine 41305114 box points and a 48EB8607 → F2FB355D
//!   style; F2FB355D `u32_1` is the 1-based t9 index of the DlApp style object (226 in the
//!   ANSI template).

use super::fields::{append_ref, index_of, int_of, put, refs_named, set_int, set_text};
use super::fields::{shift_floats, u32_from_end};
use super::node_in_mut;
use super::note_doc::Placement;
use super::note_parts::{append_copies, bytes_remap, clone_into, redecode};
use super::xref::{TemplateSegment, xref_add};
use super::{EditSegment, Editor, T_DL_TEXT, T_SM_NOTE, first_of_type, missing, node_in};
use crate::error::Result;

/// SM style record holding the DlApp style's t9 index (F2FB355D).
const T_SM_STYLE: u32 = 0xF2FB_355D;
/// SM box point (41305114).
const T_BOX_POINT: u32 = 0x4130_5114;

/// The template SM nodes a note brings: entry, group, the group's style and the style's
/// parts, then the group's parts.
struct SmParts {
    nodes: Vec<usize>,
    /// Template t9 index of the DC note object (the entry's tail).
    dc_key: usize,
    /// Template t9 index of the DlApp style object.
    app_key: usize,
}

/// The copied SM entry and group.
struct SmCopies {
    entry: usize,
    group: usize,
    all: Vec<usize>,
}

/// Adds the SM records of a note to sheet `prefix`, `dc_note_id` being the DC note object's
/// id. Returns the object id of the new graphics group.
pub(super) fn add_note_sm(
    ed: &mut Editor,
    prefix: &str,
    tpl: &TemplateSegment<'_>,
    dc_note_id: u16,
    text: &str,
    at: &Placement<'_>,
) -> Result<u64> {
    let name = format!("{prefix}SMSegment");
    let parts = template_parts(tpl.seg)?;
    let copies = copy_parts(ed.segment(&name)?, tpl.seg, &parts.nodes)?;
    let dc_key = xref_add(ed, &name, tpl, parts.dc_key, Some(dc_note_id))?;
    let app_key = xref_add(ed, &name, tpl, parts.app_key, None)?;
    let sm = ed.segment(&name)?;
    link_entry(sm, &copies, dc_key, app_key)?;
    place_graphics(sm, &copies.all, text, at)?;
    Ok(node_in(sm, copies.group)?.object_id())
}

/// The template's first note entry and the nodes it brings.
fn template_parts(tsm: &EditSegment) -> Result<SmParts> {
    let entry = first_of_type(tsm, T_SM_NOTE)?;
    let payload = node_in(tsm, entry)?.payload();
    let group = (u32_from_end(&payload, 9) & 0x7FFF_FFFF) as usize;
    let style = index_of(&node_in(tsm, group)?.node, "ref_0");
    let style_parts = refs_named(&node_in(tsm, style)?.node, "lst0");
    let first_part = style_parts
        .first()
        .copied()
        .ok_or_else(|| missing("template SM style record"))?;
    let app_key = int_of(&node_in(tsm, first_part)?.node, "u32_1") as usize;
    let mut nodes = vec![entry, group, style];
    nodes.extend(style_parts);
    nodes.extend(refs_named(&node_in(tsm, group)?.node, "lst0"));
    let dc_key = u32_from_end(&payload, 5) as usize;
    Ok(SmParts {
        nodes,
        dc_key,
        app_key,
    })
}

/// Appends copies of the template parts (with the template's class list) and renumbers
/// the references between them.
fn copy_parts(sm: &mut EditSegment, tsm: &EditSegment, parts: &[usize]) -> Result<SmCopies> {
    let body = &tsm.segment.meta.body;
    sm.adopt_type_table(&body.types, &body.tail.t4)?;
    let (all, map) = append_copies(sm, tsm, parts, clone_into)?;
    all.iter().try_for_each(|k| bytes_remap(sm, *k, &map))?;
    match all[..] {
        [entry, group, ..] => Ok(SmCopies { entry, group, all }),
        _ => Err(missing("copied SM note entry and group")),
    }
}

/// Points the entry's tail and the style records at the new t9 entries, and lists the entry
/// in the SM root.
fn link_entry(
    sm: &mut EditSegment,
    copies: &SmCopies,
    dc_key: usize,
    app_key: usize,
) -> Result<()> {
    let mut payload = node_in(sm, copies.entry)?.payload();
    let at = payload
        .len()
        .checked_sub(5)
        .ok_or_else(|| missing("tail of the copied SM note entry"))?;
    put(&mut payload, at, &(dc_key as u32).to_le_bytes());
    redecode(sm, copies.entry, &payload)?;
    for k in &copies.all {
        let Some(n) = sm.node_mut(*k).filter(|n| n.node.type_id.0 == T_SM_STYLE) else {
            continue;
        };
        set_int(&mut n.node, "u32_1", app_key as i64)?;
    }
    append_ref(&mut node_in_mut(sm, 1)?.node, "lst0", copies.entry)
}

/// The text and the box points of the copied group, moved to the new place.
fn place_graphics(
    sm: &mut EditSegment,
    copies: &[usize],
    text: &str,
    at: &Placement<'_>,
) -> Result<()> {
    let (right, up) = (|v: f64| v + at.dx, |v: f64| v + at.dy);
    for k in copies {
        let n = &mut node_in_mut(sm, *k)?.node;
        match n.type_id.0 {
            T_DL_TEXT => {
                set_text(n, "string", text)?;
                shift_floats(n, "position", [&right, &up])?;
            }
            T_BOX_POINT => shift_floats(n, "a1", [at.wx, &up])?,
            _ => {}
        }
    }
    Ok(())
}
