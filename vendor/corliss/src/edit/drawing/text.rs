//! Note text and the sheet display list's cached strings.

use super::fields::{format_prefix, index_of, set_text};
use super::locate::{find_note, sheet_prefix};
use super::{Editor, T_DL_TEXT, node_in_mut};
use crate::error::Result;
use crate::grammar::{DecodeStatus, DecodedNode};

/// Replaces the text of the note on `sheet` (1-based) whose plain text is `which`: the sheet
/// DC record's `name` and `resolved_text`, keeping their leading format codes. Inventor
/// refreshes the document DC copy and the SM text cache on open. Returns the text node index.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::drawing::set_note_text(&mut ed, 1, "HELLO NOTE", "HELLO WRITER")?;
/// ed.finish()?.save("edited.idw")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn set_note_text(ed: &mut Editor, sheet: usize, which: &str, text: &str) -> Result<usize> {
    let (note, _) = find_note(ed.document(), sheet, which)?;
    let prefix = sheet_prefix(ed.document(), sheet)?;
    let seg = ed.segment(&format!("{prefix}DCSegment"))?;
    replace_keeping_codes(
        &mut node_in_mut(seg, note.node)?.node,
        &["name", "resolved_text"],
        text,
    )?;
    Ok(note.node)
}

/// Sets each text field to `text` behind the format codes it had.
pub(super) fn replace_keeping_codes(
    node: &mut DecodedNode,
    names: &[&str],
    text: &str,
) -> Result<()> {
    for name in names {
        let old = node.text(name).unwrap_or_default().to_string();
        set_text(node, name, &format!("{}{text}", format_prefix(&old)))?;
    }
    Ok(())
}

/// Replaces cached display-list text (A79EACD5 `string`) `old` by `new` on a sheet, only in
/// nodes whose `parent` is `parent` when given: Inventor draws title-block fields and view
/// labels from this cache until the sheet is updated.
pub(super) fn dl_replace(
    ed: &mut Editor,
    prefix: &str,
    old: &str,
    new: &str,
    parent: Option<usize>,
) -> Result<()> {
    let dl = ed.segment(&format!("{prefix}DLSegment"))?;
    let hit = |n: &DecodedNode| {
        is_dl_text(n)
            && n.text("string") == Some(old)
            && parent.is_none_or(|p| index_of(n, "parent") == p)
    };
    let hits: Vec<usize> = dl
        .of_type(T_DL_TEXT)
        .into_iter()
        .filter(|k| dl.node(*k).is_some_and(|n| hit(&n.node)))
        .collect();
    for k in hits {
        set_text(&mut node_in_mut(dl, k)?.node, "string", new)?;
    }
    Ok(())
}

/// A decoded display-list text with a `string`.
pub(super) fn is_dl_text(n: &DecodedNode) -> bool {
    n.status != DecodeStatus::NoGrammar && n.get("string").is_some()
}
