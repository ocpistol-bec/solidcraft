//! UFRxDoc occurrence records and occurrence groups an assembly edit adds or drops
//! (`ufrxdoc.sync_counts` for the counts it changes).

use crate::edit::Editor;
use crate::error::{CorlissError, Result};
use crate::ufrx::{Record, UfrxDoc, Value};

const STRUCTURE: &str = "UFRxDoc";
/// Occurrence group ids are 0x30 + the occurrence id.
pub(super) const GROUP_BASE: i64 = 0x30;

/// The nested record `name` of `rec`, mutable.
pub(super) fn record_mut<'a>(rec: &'a mut Record, name: &str) -> Result<&'a mut Record> {
    match rec.get_mut(name) {
        Some(Value::Record(r)) => Ok(r),
        _ => Err(CorlissError::missing(
            "edit",
            STRUCTURE,
            format!("record {name}"),
        )),
    }
}

/// The list field `name` of `rec`, mutable.
pub(super) fn list_mut<'a>(rec: &'a mut Record, name: &str) -> Result<&'a mut Vec<Record>> {
    match rec.get_mut(name) {
        Some(Value::List(items)) => Ok(items),
        _ => Err(CorlissError::missing(
            "edit",
            STRUCTURE,
            format!("list {name}"),
        )),
    }
}

/// Sets an existing u32 / u16 field, keeping its width.
pub(super) fn set_uint(rec: &mut Record, name: &str, v: u64) -> Result<()> {
    match rec.get_mut(name) {
        Some(Value::U32(x)) => *x = v as u32,
        Some(Value::U16(x)) => *x = v as u16,
        Some(Value::U8(x)) => *x = v as u8,
        Some(Value::U64(x)) => *x = v,
        other => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "field",
                0,
                format!("{name} = {other:?}"),
                "an unsigned integer field",
            ));
        }
    }
    Ok(())
}

/// The five `occ_ids` of an occurrence record (type, file ref id, id, order, instance).
pub(super) fn occ_ids(rec: &Record) -> Option<&[u32]> {
    match rec.get("occ_ids") {
        Some(Value::U32s(v)) if v.len() == 5 => Some(v),
        _ => None,
    }
}

/// Replaces an occurrence record's `occ_ids`.
pub(super) fn set_occ_ids(rec: &mut Record, ids: [u32; 5]) {
    if let Some(v) = rec.get_mut("occ_ids") {
        *v = Value::U32s(ids.to_vec());
    }
}

/// The `occurrences` record count fields follow their lists.
pub(super) fn sync_counts(occ: &mut Record) -> Result<()> {
    let (entries, groups) = (occ.list("entries").len(), occ.list("groups").len());
    set_uint(occ, "count", entries as u64)?;
    set_uint(occ, "n_groups", groups as u64)
}

/// Applies `change` to the document's `occurrences` record and writes the stream back.
pub(super) fn edit_occurrences(
    ed: &mut Editor,
    change: impl FnOnce(&mut Record) -> Result<()>,
) -> Result<()> {
    edit(ed, |root| {
        let occ = record_mut(root, "occurrences")?;
        change(occ)?;
        sync_counts(occ)
    })
}

/// Applies `change` to the whole UFRxDoc record and writes the stream back.
pub(super) fn edit(ed: &mut Editor, change: impl FnOnce(&mut Record) -> Result<()>) -> Result<()> {
    let mut u: UfrxDoc = ed.document().ufrx_doc()?;
    change(&mut u.root)?;
    ed.document_mut().write_ufrx_doc(&u)
}

/// The occurrence group id (`id` of a group record).
pub(super) fn group_id(group: &Record) -> Option<i64> {
    group.uint("id").map(|v| v as i64)
}
