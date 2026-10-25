//! The name table (CCE92042) side of copying a constraint: fresh keys for the copied
//! entries, occurrence ids held as plain values, and entries shared with existing ones.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::add_constraint_dc::{Copies, OccurrenceMap, copy_of};
use super::dependents::{
    NT_ENTRY_IDX, drop_name_pairs, name_keys, name_table_pairs, recount_names,
};
use super::fields::{first_of_type, int, list, list_mut, node, node_mut, same_width, set, set_int};
use crate::edit::{EditSegment, node_ref};
use crate::error::Result;
use crate::grammar::{DecodedNode, Field, FieldValue, RefKind};

const T_NAME_TABLE: u32 = 0xCCE9_2042;
/// 0645C2A5 face name: NMxGenTag path / profile numbers and flags, then CoedgeDirection
/// (spec 14.3); files before 2027 keep them as one u32[4] `a1`.
const GEN_TAG: [&str; 4] = [
    "path_number",
    "profile_number",
    "gen_tag_flags",
    "coedge_direction",
];

/// Template name-table key → its entry node.
type EntryOf = BTreeMap<i64, usize>;

/// The ref kind of the template name table's entries.
fn entries_kind(ts: &EditSegment) -> Result<RefKind> {
    let tnt = node(ts, first_of_type(ts, T_NAME_TABLE)?)?;
    let first = tnt
        .fields
        .iter()
        .find(|f| f.name.as_deref() == Some("entries"));
    Ok(match first.map(|f| &f.value) {
        Some(FieldValue::Ref(_, k)) => *k,
        _ => RefKind::Child,
    })
}

/// Lists the copied entries under fresh keys: a template key the document never used (not
/// present, not above `lastIdx`) is kept, as Inventor never reuses a key and a freed one is
/// safe to restore; others take the next key. Returns template key → document key.
pub(super) fn add_name_entries(
    dc: &mut EditSegment,
    ts: &EditSegment,
    ent_of: &EntryOf,
    mp: &Copies,
) -> Result<BTreeMap<i64, i64>> {
    let kind = entries_kind(ts)?;
    let nt = node_mut(dc, first_of_type(dc, T_NAME_TABLE)?)?;
    let mut last = int(nt, "lastIdx")?;
    let present: BTreeSet<i64> = name_table_pairs(&nt.fields).iter().map(|p| p.0).collect();
    let mut newkey = BTreeMap::new();
    for (k, entry) in ent_of {
        let key = if !present.contains(k) && *k <= last {
            *k
        } else {
            last + 1
        };
        last = last.max(key);
        newkey.insert(*k, key);
        push_pair(nt, key, copy_of(mp, *entry)?, kind)?;
    }
    set_int(nt, "lastIdx", last)?;
    recount_names(nt)?;
    Ok(newkey)
}

/// Appends a (key, entries) pair to the name table.
fn push_pair(nt: &mut DecodedNode, key: i64, entry: usize, kind: RefKind) -> Result<()> {
    nt.fields
        .push(Field::encoded("key", FieldValue::U32(key as u32))?);
    nt.fields
        .push(Field::encoded("entries", node_ref(entry, true, kind))?);
    Ok(())
}

/// The occurrence id a creator names in `txt_1`, when it has a name-table key.
fn creator_occurrence(n: &DecodedNode) -> Option<String> {
    let has_key = n
        .fields
        .iter()
        .any(|f| f.name.as_deref() == Some(NT_ENTRY_IDX));
    n.text("txt_1").filter(|_| has_key).map(str::to_string)
}

/// Occurrence ids held as plain values follow the occurrence mapping: a creator's `txt_1` is
/// the occurrence id as text and its name-table entry's `lst0` holds the same id; otherwise
/// Inventor reports "Invalid reference to segment object used" once it solves. The creators'
/// keys move to the document keys.
pub(super) fn follow_occurrence_ids(
    dc: &mut EditSegment,
    mp: &Copies,
    ent_of: &EntryOf,
    newkey: &BTreeMap<i64, i64>,
    occ: &OccurrenceMap,
) -> Result<()> {
    for new in mp.values().copied() {
        let Some(old) = creator_occurrence(node(dc, new)?) else {
            continue;
        };
        let old_id = Some(old.as_str())
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<i64>().ok());
        let ids = old_id.and_then(|o| occ.ids.get(&o).map(|n| (o, *n)));
        if let Some((_, id)) = ids {
            set(
                node_mut(dc, new)?,
                "txt_1",
                FieldValue::Text(id.to_string()),
            )?;
        }
        follow_entries(dc, new, mp, ent_of, newkey, ids)?;
        rekey_creator(node_mut(dc, new)?, &|k| newkey.get(&k).copied())?;
    }
    Ok(())
}

/// The name-table entries of creator `new` (before rekeying) hold the occurrence id too.
fn follow_entries(
    dc: &mut EditSegment,
    new: usize,
    mp: &Copies,
    ent_of: &EntryOf,
    newkey: &BTreeMap<i64, i64>,
    ids: Option<(i64, i64)>,
) -> Result<()> {
    let Some((old, id)) = ids else { return Ok(()) };
    for key in name_keys(dc, new)
        .into_iter()
        .filter(|k| newkey.contains_key(k))
    {
        if let Some(entry) = ent_of.get(&key) {
            replace_occurrence_id(node_mut(dc, copy_of(mp, *entry)?)?, old, id)?;
        }
    }
    Ok(())
}

/// The entry's occurrence-id list: `old` becomes `new`.
fn replace_occurrence_id(entry: &mut DecodedNode, old: i64, new: i64) -> Result<()> {
    let lst = list_mut(entry, "lst0")?;
    for f in lst
        .elements
        .iter_mut()
        .flatten()
        .filter(|f| f.value.as_int() == Some(old))
    {
        let value = same_width(&f.value, new)?;
        f.set(value)?;
    }
    Ok(())
}

/// Moves each `ntEntry.idx` of a creator through `map` (unmapped keys stay).
fn rekey_creator(n: &mut DecodedNode, map: &dyn Fn(i64) -> Option<i64>) -> Result<()> {
    for f in n
        .fields
        .iter_mut()
        .filter(|f| f.name.as_deref() == Some(NT_ENTRY_IDX))
    {
        let Some(new) = f.value.as_int().and_then(map) else {
            continue;
        };
        let value = same_width(&f.value, new)?;
        f.set(value)?;
    }
    Ok(())
}

/// What makes two name-table entries the same name: the face name (gen tag) and the
/// occurrence and feature lists.
fn signature(n: &DecodedNode) -> Vec<Vec<i64>> {
    let tag = match n.get("a1").and_then(FieldValue::as_ints) {
        Some(a1) => a1,
        None => GEN_TAG.iter().map(|k| n.int(k).unwrap_or(-1)).collect(),
    };
    let ints = |f: &Field| {
        f.value
            .as_ints()
            .unwrap_or_else(|| f.value.as_int().into_iter().collect())
    };
    let values = |name: &str| -> Vec<i64> {
        let elements = list(n, name)
            .map(|l| l.elements.as_slice())
            .unwrap_or_default();
        elements.iter().flatten().flat_map(ints).collect()
    };
    vec![tag, values("lst0"), values("lst1")]
}

/// A name-table entry is per (occurrence, face): when the document already has one for the
/// same face of the same occurrence, the new creator uses its key (as_angle: Flush:1 and
/// Angle:1 share key 10) and the copied entry goes again.
pub(super) fn share_names(dc: &mut EditSegment, mp: &Copies, ent_of: &EntryOf) -> Result<()> {
    let nt = first_of_type(dc, T_NAME_TABLE)?;
    let new_entries: BTreeSet<usize> = ent_of
        .values()
        .map(|e| copy_of(mp, *e))
        .collect::<Result<_>>()?;
    let shared = shared_keys(dc, nt, &new_entries)?;
    if shared.is_empty() {
        return Ok(());
    }
    for new in mp.values().copied() {
        rekey_creator(node_mut(dc, new)?, &|k| shared.get(&k).copied())?;
    }
    let gone = drop_name_pairs(node_mut(dc, nt)?, &|k| shared.contains_key(&k))?;
    dc.delete(&gone).map(|_| ())
}

/// New key → the existing key naming the same (occurrence, face).
fn shared_keys(
    dc: &EditSegment,
    nt: usize,
    new_entries: &BTreeSet<usize>,
) -> Result<BTreeMap<i64, i64>> {
    let pairs = name_table_pairs(&node(dc, nt)?.fields);
    let mut existing: HashMap<Vec<Vec<i64>>, i64> = HashMap::new();
    for (k, e) in pairs.iter().filter(|(_, e)| !new_entries.contains(e)) {
        existing.entry(signature(node(dc, *e)?)).or_insert(*k);
    }
    let mut shared = BTreeMap::new();
    for (k, e) in pairs.iter().filter(|(_, e)| new_entries.contains(e)) {
        if let Some(old) = existing.get(&signature(node(dc, *e)?)) {
            shared.insert(*k, *old);
        }
    }
    Ok(shared)
}
