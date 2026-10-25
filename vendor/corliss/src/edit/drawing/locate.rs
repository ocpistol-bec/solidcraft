//! Finding what an edit changes: sheet segments by number, and notes and views through the
//! drawing reader on the document as opened (the reference implementation reads the unedited
//! file).

use super::STRUCTURE;
use crate::document::Document;
use crate::error::{CorlissError, Result};
use crate::model::drawing::{DrawingView, Note};
use crate::model::{Drawing, Model};

/// The segment prefix (`DLSheet21` …) of 1-based sheet `sheet`, sheets in creation order.
pub(super) fn sheet_prefix(doc: &Document, sheet: usize) -> Result<String> {
    let prefixes = sheet_prefixes(doc)?;
    let found = sheet.checked_sub(1).and_then(|k| prefixes.get(k));
    found.cloned().ok_or_else(|| {
        CorlissError::invalid(
            STRUCTURE,
            "sheet",
            0,
            sheet,
            format!("a sheet number in 1..={}", prefixes.len()),
        )
    })
}

/// `DLSheet<k>` prefixes of the document's sheet DC segments, by k.
fn sheet_prefixes(doc: &Document) -> Result<Vec<String>> {
    let mut found: Vec<(u64, String)> = doc
        .segments()?
        .iter()
        .filter_map(|l| numbered_sheet(&l.name))
        .collect();
    found.sort();
    Ok(found.into_iter().map(|(_, p)| p).collect())
}

fn numbered_sheet(name: &str) -> Option<(u64, String)> {
    let prefix = name.strip_suffix("DCSegment")?;
    let digits = prefix.strip_prefix("DLSheet")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, prefix.to_string()))
}

/// Runs `read` on the drawing model of `doc`.
pub(super) fn with_drawing<T>(
    doc: &Document,
    read: impl FnOnce(&Drawing<'_>) -> Result<T>,
) -> Result<T> {
    let model = Model::load(doc)?;
    let drawing = Drawing::new(&model)?;
    read(&drawing)
}

/// The note of `sheet` whose plain text is `which`, and how many notes the sheet has.
pub(super) fn find_note(doc: &Document, sheet: usize, which: &str) -> Result<(Note, usize)> {
    let notes = with_drawing(doc, |d| d.notes(sheet))?;
    let count = notes.len();
    let note = notes.into_iter().find(|n| n.text == which);
    let note = note.ok_or_else(|| {
        CorlissError::invalid(
            STRUCTURE,
            "note",
            0,
            which,
            format!("the plain text of a note on sheet {sheet}"),
        )
    })?;
    Ok((note, count))
}

/// The view called `name` on `sheet`, and whether it is the sheet's first view.
pub(super) fn find_view(
    drawing: &Drawing<'_>,
    sheet: usize,
    name: &str,
) -> Result<(DrawingView, bool)> {
    let views = drawing.views(Some(sheet));
    let first = views
        .first()
        .is_some_and(|v| v.name.as_deref() == Some(name));
    let view = views.into_iter().find(|v| v.name.as_deref() == Some(name));
    let view = view.ok_or_else(|| {
        CorlissError::invalid(
            STRUCTURE,
            "view",
            0,
            name,
            format!("the name of a view on sheet {sheet}"),
        )
    })?;
    Ok((view, first))
}
