//! Renaming a sheet.

use super::fields::{int_of, replace_text16, set_int, set_text};
use super::locate::sheet_prefix;
use super::{BROWSER_SEGMENT, DOC_SEGMENT, EditSegment, Editor, decoded_of_type, missing};
use super::{node_in_mut, sheet_records};
use crate::error::Result;
use crate::grammar::DecodedNode;

/// Browser sheet entry (`'Sheet:1'`).
const T_BROWSER_SHEET: u32 = 0x79EB_BBEA;
/// Document DC sheet list entry that keeps the sheet name in its raw tail.
const T_SHEET_ENTRY: u32 = 0xA200_FB76;
/// Fields a default-named browser entry carries and a renamed one drops (dw_rename).
const NUMBER_FIELDS: [&str; 4] = ["number", "suffix", "t0", "t1"];

/// Renames sheet `sheet` (1-based; dw_move → dw_rename: 'Sheet' → 'MAIN'): the sheet DC
/// head's `name`, the document DC sheet record 37359718 and its A200FB76 entry (both keep
/// the name, typed or in their raw tails) and the browser entry, which becomes `name:sheet`.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::drawing::rename_sheet(&mut ed, 1, "MAIN")?;
/// ed.finish()?.save("renamed.idw")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn rename_sheet(ed: &mut Editor, sheet: usize, name: &str) -> Result<()> {
    let prefix = sheet_prefix(ed.document(), sheet)?;
    let old = rename_head(ed.segment(&format!("{prefix}DCSegment"))?, name)?;
    let dc = ed.segment(DOC_SEGMENT)?;
    let nth = |list: Vec<usize>| sheet.checked_sub(1).and_then(|k| list.get(k).copied());
    rename_in(dc, nth(sheet_records(dc)), &old, name)?;
    rename_in(dc, nth(decoded_of_type(dc, T_SHEET_ENTRY)), &old, name)?;
    let browser = ed.segment(BROWSER_SEGMENT)?;
    let entry = nth(decoded_of_type(browser, T_BROWSER_SHEET));
    let entry = entry.ok_or_else(|| missing(format!("browser entry of sheet {sheet}")))?;
    rename_browser_entry(
        &mut node_in_mut(browser, entry)?.node,
        &format!("{name}:{sheet}"),
    )
}

/// Sets the sheet DC head's name; returns the old one.
fn rename_head(seg: &mut EditSegment, name: &str) -> Result<String> {
    let head = &mut node_in_mut(seg, 1)?.node;
    let old = head.text("name").unwrap_or_default().to_string();
    set_text(head, "name", name)?;
    Ok(old)
}

/// Renames inside document DC node `index` when the document has one for the sheet.
fn rename_in(seg: &mut EditSegment, index: Option<usize>, old: &str, name: &str) -> Result<()> {
    let Some(node) = index.and_then(|k| seg.node_mut(k)) else {
        return Ok(());
    };
    replace_text16(&mut node.node, old, name)?;
    Ok(())
}

/// A renamed browser entry is no longer default-named: `has_number` 1 makes the browser show
/// its own 'Sheet:n' and goes with the trailing `number`, `suffix` fields (dw_rename).
fn rename_browser_entry(entry: &mut DecodedNode, label: &str) -> Result<()> {
    set_text(entry, "name", label)?;
    if int_of(entry, "has_number") == 0 {
        return Ok(());
    }
    set_int(entry, "has_number", 0)?;
    entry
        .fields
        .retain(|f| !NUMBER_FIELDS.iter().any(|n| f.is(n)));
    Ok(())
}
