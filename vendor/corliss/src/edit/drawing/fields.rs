//! Field access shared by the drawing edits: the reference engine's field lookup (the n-th
//! field called `name` now or before a rename), values re-encoded in the field's own width, and
//! NodeRef list edits.

use std::collections::BTreeSet;

use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, Field, FieldValue, ListField, RefKind};
use crate::node::NodeRef;

const STRUCTURE: &str = "drawing node field";
/// Meta words of a NodeRef list that had none (capacity 1, 0x10, as the reference writer
/// appends them).
const NEW_LIST_META: [u8; 8] = [1, 0, 0, 0, 0x10, 0, 0, 0];

/// The `nth` field called `name` (or formerly so), in payload order.
fn nth_field<'a>(node: &'a mut DecodedNode, name: &str, nth: usize) -> Result<&'a mut Field> {
    let label = node_label(node);
    let found = node.fields.iter_mut().filter(|f| f.is(name)).nth(nth);
    found.ok_or_else(|| {
        CorlissError::missing(
            "edit",
            STRUCTURE,
            format!("field {name} #{nth} of node {label}"),
        )
    })
}

fn node_label(node: &DecodedNode) -> String {
    node.type_id.to_string()
}

/// Sets the `nth` float field `name` in its own width (f32 or f64).
pub(super) fn set_float(node: &mut DecodedNode, name: &str, nth: usize, v: f64) -> Result<()> {
    let f = nth_field(node, name, nth)?;
    let value = match f.value {
        FieldValue::F64(_) => FieldValue::F64(v),
        FieldValue::F32(_) => FieldValue::F32(v as f32),
        ref other => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "float",
                0,
                format!("{name} = {other:?}"),
                "an f32 or f64 field",
            ));
        }
    };
    f.set(value)
}

/// Sets the first integer (or bool) field `name` in its own width.
pub(super) fn set_int(node: &mut DecodedNode, name: &str, v: i64) -> Result<()> {
    let f = nth_field(node, name, 0)?;
    let value = int_like(&f.value, v)?;
    f.set(value)
}

/// Sets the first text field `name`, keeping its encoding (Text16 or Text8).
pub(super) fn set_text(node: &mut DecodedNode, name: &str, v: &str) -> Result<()> {
    let f = nth_field(node, name, 0)?;
    let value = match f.value {
        FieldValue::Text8(_) | FieldValue::Text8Bytes(_) => FieldValue::Text8(v.to_string()),
        _ => FieldValue::Text(v.to_string()),
    };
    f.set(value)
}

/// `v` in the integer width of `like`.
pub(super) fn int_like(like: &FieldValue, v: i64) -> Result<FieldValue> {
    Ok(match like {
        FieldValue::U8(_) => FieldValue::U8(v as u8),
        FieldValue::I8(_) => FieldValue::I8(v as i8),
        FieldValue::U16(_) => FieldValue::U16(v as u16),
        FieldValue::I16(_) => FieldValue::I16(v as i16),
        FieldValue::U32(_) => FieldValue::U32(v as u32),
        FieldValue::I32(_) => FieldValue::I32(v as i32),
        FieldValue::Bool(_) => FieldValue::Bool(v != 0),
        FieldValue::Ref(_, kind) => FieldValue::Ref(NodeRef(v as u32), *kind),
        other => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "integer",
                0,
                format!("{other:?}"),
                "an integer, bool or NodeRef field",
            ));
        }
    })
}

/// Rewrites the first word of the integer array field `name` (`a0` flags).
pub(super) fn update_first_word(
    node: &mut DecodedNode,
    name: &str,
    f: impl Fn(i64) -> i64,
) -> Result<()> {
    let field = nth_field(node, name, 0)?;
    let value = match &field.value {
        FieldValue::U32s(v) => FieldValue::U32s(with_first(v, |x| f(i64::from(x)) as u32)),
        FieldValue::I32s(v) => FieldValue::I32s(with_first(v, |x| f(i64::from(x)) as i32)),
        FieldValue::U16s(v) => FieldValue::U16s(with_first(v, |x| f(i64::from(x)) as u16)),
        other => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "array",
                0,
                format!("{name} = {other:?}"),
                "a u16, u32 or i32 array",
            ));
        }
    };
    field.set(value)
}

fn with_first<T: Copy>(v: &[T], f: impl Fn(T) -> T) -> Vec<T> {
    let mut out = v.to_vec();
    if let Some(first) = out.first_mut() {
        *first = f(*first);
    }
    out
}

/// The integer value of field `name` (NodeRefs as their raw word), 0 when absent.
pub(super) fn int_of(node: &DecodedNode, name: &str) -> i64 {
    node.int(name).unwrap_or(0)
}

/// The node index a NodeRef or flagged u32 field `name` names (`get(name) & 0x7FFFFFFF`).
pub(super) fn index_of(node: &DecodedNode, name: &str) -> usize {
    (int_of(node, name) & 0x7FFF_FFFF) as usize
}

/// The values of every top-level float field called exactly `name`.
pub(super) fn floats_named(node: &DecodedNode, name: &str) -> Vec<f64> {
    node.fields
        .iter()
        .filter(|f| f.name.as_deref() == Some(name))
        .filter_map(|f| f.value.as_f64())
        .collect()
}

/// Moves the first two float fields called `name` (or formerly so): the k-th through `moves[k]`.
pub(super) fn shift_floats(
    node: &mut DecodedNode,
    name: &str,
    moves: [&dyn Fn(f64) -> f64; 2],
) -> Result<()> {
    for (k, mv) in moves.iter().enumerate() {
        let old = nth_field(node, name, k)?.value.as_f64();
        let old =
            old.ok_or_else(|| CorlissError::invalid(STRUCTURE, "float", k, name, "a float field"))?;
        set_float(node, name, k, mv(old))?;
    }
    Ok(())
}

/// Every non-null NodeRef in `fields` (nested lists included) as (field name, index).
fn named_refs(fields: &[Field], out: &mut Vec<(Option<String>, usize)>) {
    for f in fields {
        match &f.value {
            FieldValue::Ref(r, _) => out.extend(r.index().map(|i| (f.name.clone(), i))),
            FieldValue::List(l) => l.elements.iter().for_each(|el| named_refs(el, out)),
            _ => {}
        }
    }
}

/// Targets of the NodeRefs called exactly `name` anywhere in `node` (`n.refs()` by name).
pub(super) fn refs_named(node: &DecodedNode, name: &str) -> Vec<usize> {
    let mut all = Vec::new();
    named_refs(&node.fields, &mut all);
    all.into_iter()
        .filter(|(n, _)| n.as_deref() == Some(name))
        .map(|(_, i)| i)
        .collect()
}

/// The first list field called `name` (or formerly so).
pub(super) fn list_named<'a>(node: &'a mut DecodedNode, name: &str) -> Result<&'a mut ListField> {
    let label = node_label(node);
    let found = node.fields.iter_mut().find_map(|f| match &mut f.value {
        FieldValue::List(l)
            if f.name.as_deref() == Some(name) || f.aliases.iter().any(|a| a == name) =>
        {
            Some(l)
        }
        _ => None,
    });
    found.ok_or_else(|| {
        CorlissError::missing("edit", STRUCTURE, format!("list {name} of node {label}"))
    })
}

/// Appends a flagged NodeRef to list `name`, flagged like its first element
/// (the reference writer's flagged append).
pub(super) fn append_ref(node: &mut DecodedNode, name: &str, index: usize) -> Result<()> {
    let list = list_named(node, name)?;
    let proto = list
        .elements
        .first()
        .and_then(|el| el.first())
        .map(|f| f.value.clone());
    let (flag, kind) = match proto {
        Some(FieldValue::Ref(r, kind)) => (r.0 & 0x8000_0000, kind),
        _ => (0x8000_0000, RefKind::Child),
    };
    if list.elements.is_empty() && list.meta.is_none() {
        list.meta = Some(NEW_LIST_META.to_vec());
    }
    let value = FieldValue::Ref(NodeRef(index as u32 & 0x7FFF_FFFF | flag), kind);
    list.push(vec![Field::encoded(name, value)?]);
    Ok(())
}

/// The node index of a list element's first field.
pub(super) fn element_index(el: &[Field]) -> usize {
    el.first()
        .and_then(|f| f.value.as_int())
        .map_or(0, |v| (v & 0x7FFF_FFFF) as usize)
}

/// Drops the elements of list `name` whose first NodeRef names one of `indexes`; returns how
/// many went.
pub(super) fn remove_refs(
    node: &mut DecodedNode,
    name: &str,
    indexes: &BTreeSet<usize>,
) -> Result<usize> {
    let list = list_named(node, name)?;
    let before = list.elements.len();
    list.elements
        .retain(|el| !indexes.contains(&element_index(el)));
    Ok(before - list.elements.len())
}

/// A length-prefixed UTF-16 string as the raw tails store it.
pub(super) fn text16_bytes(s: &str) -> Vec<u8> {
    // The count is Python's len(): code points, as the reference writer packs it.
    let mut out = (s.chars().count() as u32).to_le_bytes().to_vec();
    s.encode_utf16().for_each(|u| out.extend(u.to_le_bytes()));
    out
}

/// The last position of `pat` in `hay`.
pub(super) fn rfind(hay: &[u8], pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || hay.len() < pat.len() {
        return None;
    }
    (0..=hay.len() - pat.len())
        .rev()
        .find(|&k| hay[k..k + pat.len()] == *pat)
}

/// Replaces the last occurrence of text `old` by `new` in a node, scanning fields from the
/// end: inside a raw field's bytes, or a Text16 field equal to `old`.
pub(super) fn replace_text16(node: &mut DecodedNode, old: &str, new: &str) -> Result<bool> {
    let (pat, rep) = (text16_bytes(old), text16_bytes(new));
    for f in node.fields.iter_mut().rev() {
        if matches!(f.value, FieldValue::Raw)
            && let Some(k) = rfind(&f.raw, &pat)
        {
            f.raw.splice(k..k + pat.len(), rep.iter().copied());
            return Ok(true);
        }
        if matches!(&f.value, FieldValue::Text(s) if s == old) {
            f.set(FieldValue::Text(new.to_string()))?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// The u32 at byte `at` of `bytes`.
pub(super) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The u32 `back` bytes before the end of `bytes`, or 0.
pub(super) fn u32_from_end(bytes: &[u8], back: usize) -> u32 {
    bytes
        .len()
        .checked_sub(back)
        .and_then(|at| u32_at(bytes, at))
        .unwrap_or(0)
}

/// What a raw-tail list edit found at one occurrence of the string after the list.
pub(super) enum TailList {
    /// No list ends there: look further.
    Absent,
    /// The list is there but needs no change.
    Unchanged,
    /// The list starts at this offset and becomes these bytes.
    Rebuilt(usize, Vec<u8>),
}

/// Edits the list2 that ends right before the length-prefixed UTF-16 string `before` in a
/// node's raw tail, searching raw fields from the last: `rebuild(raw, k)` sees each field
/// holding the string, `k` being its last position. Returns whether the list changed.
pub(super) fn rewrite_tail_list(
    node: &mut DecodedNode,
    before: &str,
    rebuild: impl Fn(&[u8], usize) -> TailList,
) -> bool {
    let pat = text16_bytes(before);
    for f in node
        .fields
        .iter_mut()
        .rev()
        .filter(|f| matches!(f.value, FieldValue::Raw))
    {
        let Some(k) = rfind(&f.raw, &pat) else {
            continue;
        };
        match rebuild(&f.raw, k) {
            TailList::Absent => continue,
            TailList::Unchanged => return false,
            TailList::Rebuilt(p, bytes) => {
                f.raw.splice(p..k, bytes);
                return true;
            }
        }
    }
    false
}

/// The f64 at byte `at` of `bytes`.
pub(super) fn f64_at(bytes: &[u8], at: usize) -> Option<f64> {
    let b: [u8; 8] = bytes.get(at..at + 8)?.try_into().ok()?;
    Some(f64::from_le_bytes(b))
}

/// Writes `v` at byte `at` (the caller has read the same span).
pub(super) fn put(bytes: &mut [u8], at: usize, v: &[u8]) {
    if let Some(span) = bytes.get_mut(at..at + v.len()) {
        span.copy_from_slice(v);
    }
}

/// The text kept in front of a formatted string's plain text: `\F29;` codes and the like
/// (`old[:len(old) - len(plain(old))]`, only when `old` ends with its plain text).
pub(super) fn format_prefix(old: &str) -> String {
    let plain = crate::model::drawing::plain(old);
    match old.strip_suffix(plain.as_str()) {
        Some(prefix) => prefix.to_string(),
        None => String::new(),
    }
}

/// The leading run of format codes of `old`: codes after the first plain character belong to
/// the text (a reference bug, fixed, took them as part of the prefix).
pub(super) fn leading_codes(old: &str) -> String {
    let mut at = 0;
    while let Some(n) = old
        .get(at..)
        .map(crate::model::drawing::format_code_len)
        .filter(|n| *n > 0)
    {
        at += n;
    }
    old[..at].to_string()
}

#[cfg(test)]
mod tests {
    use super::leading_codes;

    // Regression (a reference bug, fixed): the prefix was len(old) - len(plain(old))
    // characters, which took text when a code sat in the middle.
    #[test]
    fn only_the_leading_run_of_format_codes_is_kept() {
        assert_eq!(leading_codes(r"\F29;\H0.35;HELLO"), r"\F29;\H0.35;");
        assert_eq!(leading_codes(r"\F29;HELLO \H0.35;NOTE"), r"\F29;");
        assert_eq!(leading_codes(r"HELLO \H0.35;NOTE"), "");
        assert_eq!(leading_codes(r"a\b"), "");
    }
}
