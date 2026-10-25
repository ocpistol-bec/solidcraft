//! Drawing document edits, verified against controlled Inventor 2027 saves.
//! Lengths are in cm.
//!
//! - [`set_note_text`]: the sheet DC note (`name`, `resolved_text`); Inventor refreshes the
//!   document DC copy and the SM text cache on open.
//! - [`rename_sheet`]: sheet DC head, document DC sheet records, browser entry.
//! - [`move_view`] / [`set_view_scale`]: the sheet DC view records and the view's placement
//!   transform in the sheet SM segment.
//! - [`add_note`] / [`delete_note`]: a general note across the sheet DC, the sheet SM, the
//!   document DC and the cross-segment tables.
//!
//! ```no_run
//! let doc = corliss::Document::open("drawing.idw")?;
//! let mut ed = corliss::edit::Editor::new(&doc)?;
//! corliss::edit::drawing::rename_sheet(&mut ed, 1, "MAIN")?;
//! ed.finish()?.save("edited.idw")?;
//! # Ok::<(), corliss::CorlissError>(())
//! ```

mod fields;
mod locate;
mod note_add;
mod note_delete;
mod note_doc;
mod note_link;
mod note_parts;
mod note_sm;
mod placement;
mod sheet;
mod text;
mod usage;
mod view;
mod xref;

pub use note_add::add_note;
pub use note_delete::delete_note;
pub use sheet::rename_sheet;
pub use text::set_note_text;
pub use view::{move_view, set_view_scale};

use super::{EditNode, EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::DecodeStatus;
use fields::int_of;

const STRUCTURE: &str = "drawing edit";
/// The document-level DC segment.
const DOC_SEGMENT: &str = "DlDocDCSegment";
/// The browser segment.
const BROWSER_SEGMENT: &str = "DlBrowserSegment";
/// Text node (1FBB3C01): notes, title-block fields, view labels.
const T_TEXT: u32 = 0x1FBB_3C01;
/// Display-list / SM cached text (A79EACD5).
const T_DL_TEXT: u32 = 0xA79E_ACD5;
/// Sheet component, the RDxFragment (90874D63).
const T_COMPONENT: u32 = 0x9087_4D63;
/// Document DC sheet record (37359718).
const T_SHEET_RECORD: u32 = 0x3735_9718;
/// Document DC note record: text reference and box (B1089D42).
const T_DOC_NOTE: u32 = 0xB108_9D42;
/// Sheet SM note entry (5EB510C2).
const T_SM_NOTE: u32 = 0x5EB5_10C2;
/// Note object (5EB510C1).
const T_NOTE: u32 = 0x5EB5_10C1;

/// An error for something an edit needs and the document lacks.
fn missing(what: impl Into<String>) -> CorlissError {
    CorlissError::missing("edit", STRUCTURE, what)
}

/// Node `k` of `seg`.
fn node_in(seg: &EditSegment, k: usize) -> Result<&EditNode> {
    seg.node(k)
        .ok_or_else(|| missing(format!("node #{k} of {}", seg.location.name)))
}

/// Node `k` of `seg`, mutable.
fn node_in_mut(seg: &mut EditSegment, k: usize) -> Result<&mut EditNode> {
    let name = seg.location.name.clone();
    seg.node_mut(k)
        .ok_or_else(|| missing(format!("node #{k} of {name}")))
}

/// The first node of type `tid`.
fn first_of_type(seg: &EditSegment, tid: u32) -> Result<usize> {
    let found = seg.of_type(tid).first().copied();
    found.ok_or_else(|| missing(format!("a {tid:08X} node in {}", seg.location.name)))
}

/// Indexes of the nodes of type `tid` a grammar decoded.
fn decoded_of_type(seg: &EditSegment, tid: u32) -> Vec<usize> {
    let decoded = |k: &usize| {
        seg.node(*k)
            .is_some_and(|n| n.node.status != DecodeStatus::NoGrammar)
    };
    seg.of_type(tid).into_iter().filter(decoded).collect()
}

/// The document DC sheet records: the 37359718 nodes with `id` 0, in sheet order.
fn sheet_records(dc: &EditSegment) -> Vec<usize> {
    let is_sheet = |k: &usize| dc.node(*k).is_some_and(|n| n.node.int("id") == Some(0));
    decoded_of_type(dc, T_SHEET_RECORD)
        .into_iter()
        .filter(is_sheet)
        .collect()
}

/// The document DC record of 1-based sheet `sheet`.
fn sheet_record(dc: &EditSegment, sheet: usize) -> Result<usize> {
    let records = sheet_records(dc);
    let found = sheet.checked_sub(1).and_then(|k| records.get(k)).copied();
    found.ok_or_else(|| {
        missing(format!(
            "document DC record of sheet {sheet} ({} records)",
            records.len()
        ))
    })
}

/// The document DC record's text reference (`text`, `t0` before the full grammar).
fn doc_note_text_ref(n: &EditNode) -> usize {
    let name = if n.node.get("text").is_some() {
        "text"
    } else {
        "t0"
    };
    (int_of(&n.node, name) & 0x7FFF_FFFF) as usize
}
