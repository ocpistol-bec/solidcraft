//! Node field access with the reference engine's lookup rules: a field matches a name it
//! carries now or carried before a rename, and the first match in payload order wins.

use crate::edit::EditSegment;
use crate::edit::node_ref;
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, Field, FieldValue, ListField, RefKind};
use crate::node::NodeRef;

const STRUCTURE: &str = "assembly node";
const FLAG: u32 = 0x8000_0000;

/// The node at 1-based `index`.
pub(super) fn node(seg: &EditSegment, index: usize) -> Result<&DecodedNode> {
    seg.node(index)
        .map(|n| &n.node)
        .ok_or_else(|| missing_node(seg, index))
}

/// The node at 1-based `index`, mutable.
pub(super) fn node_mut(seg: &mut EditSegment, index: usize) -> Result<&mut DecodedNode> {
    if seg.node(index).is_none() {
        return Err(missing_node(seg, index));
    }
    seg.node_mut(index)
        .map(|n| &mut n.node)
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("node #{index}")))
}

fn missing_node(seg: &EditSegment, index: usize) -> CorlissError {
    let count = seg.nodes.len();
    CorlissError::invalid(
        STRUCTURE,
        "index",
        index,
        index,
        format!("a node index in 1..={count}"),
    )
}

/// The first top-level field that is or was called `name`.
pub(super) fn first_mut<'a>(node: &'a mut DecodedNode, name: &str) -> Result<&'a mut Field> {
    let tid = node.type_id;
    node.fields
        .iter_mut()
        .find(|f| f.is(name))
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("field {name} of {tid}")))
}

/// Sets the first field called `name`.
pub(super) fn set(node: &mut DecodedNode, name: &str, value: FieldValue) -> Result<()> {
    first_mut(node, name)?.set(value)
}

/// Sets the first field called `name` to the integer `v`, in that field's width.
pub(super) fn set_int(node: &mut DecodedNode, name: &str, v: i64) -> Result<()> {
    let field = first_mut(node, name)?;
    let value = same_width(&field.value, v)?;
    field.set(value)
}

/// `v` in the integer type of `like`.
pub(super) fn same_width(like: &FieldValue, v: i64) -> Result<FieldValue> {
    Ok(match like {
        FieldValue::U8(_) => FieldValue::U8(v as u8),
        FieldValue::I8(_) => FieldValue::I8(v as i8),
        FieldValue::U16(_) => FieldValue::U16(v as u16),
        FieldValue::I16(_) => FieldValue::I16(v as i16),
        FieldValue::U32(_) => FieldValue::U32(v as u32),
        FieldValue::I32(_) => FieldValue::I32(v as i32),
        other => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "value",
                0,
                format!("{other:?}"),
                "an integer field",
            ));
        }
    })
}

/// The value of field `name` (last match, as `Parser.get`) as an integer.
pub(super) fn int(node: &DecodedNode, name: &str) -> Result<i64> {
    node.int(name).ok_or_else(|| {
        CorlissError::missing(
            "edit",
            STRUCTURE,
            format!("integer field {name} of {}", node.type_id),
        )
    })
}

/// The index the first ref field called `name` points at.
pub(super) fn ref_of(node: &DecodedNode, name: &str) -> Option<usize> {
    node.fields
        .iter()
        .filter(|f| f.is(name))
        .find_map(|f| match &f.value {
            FieldValue::Ref(r, _) => r.index(),
            _ => None,
        })
}

/// [`ref_of`], failing when the field is absent or null.
pub(super) fn required_ref(node: &DecodedNode, name: &str) -> Result<usize> {
    ref_of(node, name).ok_or_else(|| {
        CorlissError::missing("edit", STRUCTURE, format!("ref {name} of {}", node.type_id))
    })
}

/// The first list field called `name`.
pub(super) fn list<'a>(node: &'a DecodedNode, name: &str) -> Option<&'a ListField> {
    node.fields
        .iter()
        .filter(|f| f.is(name))
        .find_map(|f| match &f.value {
            FieldValue::List(l) => Some(l),
            _ => None,
        })
}

/// The first list field called `name`, mutable.
pub(super) fn list_mut<'a>(node: &'a mut DecodedNode, name: &str) -> Result<&'a mut ListField> {
    let tid = node.type_id;
    node.fields
        .iter_mut()
        .filter(|f| f.is(name))
        .find_map(|f| match &mut f.value {
            FieldValue::List(l) => Some(l),
            _ => None,
        })
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("list {name} of {tid}")))
}

/// Indexes of a NodeRef list, null entries kept as 0 (as the reference reader lists them).
pub(super) fn ref_list(node: &DecodedNode, name: &str) -> Vec<usize> {
    let Some(l) = list(node, name) else {
        return Vec::new();
    };
    l.elements
        .iter()
        .filter_map(|el| match el.first().map(|f| &f.value) {
            Some(FieldValue::Ref(r, _)) => Some(r.index().unwrap_or(0)),
            _ => None,
        })
        .collect()
}

/// The raw integer values of the first field of each element of list `name`.
pub(super) fn int_list(node: &DecodedNode, name: &str) -> Vec<i64> {
    let Some(l) = list(node, name) else {
        return Vec::new();
    };
    l.elements
        .iter()
        .filter_map(|el| el.first().and_then(|f| f.value.as_int()))
        .collect()
}

/// Points the first ref field called one of `names` at `index`; the flag is kept, or set when
/// the field was null.
pub(super) fn set_ref(node: &mut DecodedNode, names: &[&str], index: usize) -> Result<()> {
    let tid = node.type_id;
    let field = node
        .fields
        .iter_mut()
        .find(|f| matches!(f.value, FieldValue::Ref(..)) && names.iter().any(|n| f.is(n)))
        .ok_or_else(|| {
            CorlissError::missing("edit", STRUCTURE, format!("ref {names:?} of {tid}"))
        })?;
    let FieldValue::Ref(old, kind) = field.value else {
        return Ok(());
    };
    let flag = if old.0 != 0 { old.0 & FLAG } else { FLAG };
    field.set(FieldValue::Ref(
        NodeRef(index as u32 & 0x7FFF_FFFF | flag),
        kind,
    ))
}

/// Re-points a ref value at `index`, keeping its flag.
pub(super) fn retarget(field: &mut Field, index: usize) -> Result<()> {
    let FieldValue::Ref(old, kind) = field.value else {
        return Ok(());
    };
    field.set(FieldValue::Ref(
        NodeRef(index as u32 & 0x7FFF_FFFF | (old.0 & FLAG)),
        kind,
    ))
}

/// Element kind and flag new entries of `lst` take, and the meta word for an empty list.
fn element_proto(lst: &ListField, like: Option<&ListField>) -> (RefKind, bool, Option<Vec<u8>>) {
    let proto_of = |l: &ListField| match l
        .elements
        .first()
        .and_then(|el| el.first())
        .map(|f| &f.value)
    {
        Some(FieldValue::Ref(r, k)) => Some((*k, r.is_flagged())),
        _ => None,
    };
    if !lst.elements.is_empty() {
        let (kind, flag) = proto_of(lst).unwrap_or((RefKind::Child, true));
        return (kind, flag, None);
    }
    if let Some((kind, flag)) = like.and_then(proto_of) {
        return (kind, flag, like.and_then(|l| l.meta.clone()));
    }
    let meta = [1u32.to_le_bytes(), 0x10u32.to_le_bytes()].concat();
    (RefKind::Child, true, Some(meta))
}

/// Appends (or inserts at `pos`) a NodeRef to list `name`; the capacity word follows the count
/// (as the reference writer does). An empty list takes element kind, flag and meta from `like`.
pub(super) fn append_ref(
    node: &mut DecodedNode,
    name: &str,
    index: usize,
    pos: Option<usize>,
    like: Option<&ListField>,
) -> Result<()> {
    let lst = list_mut(node, name)?;
    let (kind, flag, meta) = element_proto(lst, like);
    if lst.meta.is_none() {
        lst.meta = meta;
    }
    let field_name = lst
        .elements
        .first()
        .and_then(|el| el.first())
        .and_then(|f| f.name.clone())
        .unwrap_or_else(|| name.to_string());
    let element = vec![Field::encoded(&field_name, node_ref(index, flag, kind))?];
    match pos {
        Some(at) => lst.insert(at, element),
        None => lst.push(element),
    }
    Ok(())
}

/// Removes the entries of list `name` that point at `index`. The capacity word is a
/// high-water mark and stays, except with `shrink_meta` where it follows the count when it
/// equalled it. Returns whether one was removed.
pub(super) fn remove_ref(
    node: &mut DecodedNode,
    name: &str,
    index: usize,
    shrink_meta: bool,
) -> Result<bool> {
    let lst = list_mut(node, name)?;
    let old = lst.elements.len();
    let hits = |el: &Vec<Field>| {
        el.iter().any(
            |f| matches!(f.value, FieldValue::Ref(r, _) if (r.0 & 0x7FFF_FFFF) as usize == index),
        )
    };
    lst.elements.retain(|el| !hits(el));
    if lst.elements.len() == old {
        return Ok(false);
    }
    let Some(meta) = lst.meta.as_mut().filter(|m| m.len() >= 4) else {
        return Ok(true);
    };
    if shrink_meta && u32::from_le_bytes([meta[0], meta[1], meta[2], meta[3]]) as usize == old {
        meta[..4].copy_from_slice(&(lst.elements.len() as u32).to_le_bytes());
    }
    Ok(true)
}

/// Overwrites the first field of each element of list `name` with `values`, in order.
pub(super) fn set_int_list(node: &mut DecodedNode, name: &str, values: &[i64]) -> Result<()> {
    let lst = list_mut(node, name)?;
    for (el, v) in lst.elements.iter_mut().zip(values) {
        let Some(f) = el.first_mut() else { continue };
        let value = same_width(&f.value, *v)?;
        f.set(value)?;
    }
    Ok(())
}

/// Every non-null NodeRef of the node with its kind and the (current) name of the field that
/// holds it, lists included.
pub(super) fn named_refs(node: &DecodedNode) -> Vec<(RefKind, usize, String)> {
    let mut out = Vec::new();
    collect(&node.fields, &mut out);
    out
}

fn collect(fields: &[Field], out: &mut Vec<(RefKind, usize, String)>) {
    for f in fields {
        match &f.value {
            FieldValue::Ref(r, k) => {
                if let Some(i) = r.index() {
                    out.push((*k, i, f.name.clone().unwrap_or_default()));
                }
            }
            FieldValue::List(l) => l.elements.iter().for_each(|el| collect(el, out)),
            _ => {}
        }
    }
}

/// Whether a top-level field is called exactly `name` now (no rename lookup).
pub(super) fn has_named(node: &DecodedNode, name: &str) -> bool {
    node.fields.iter().any(|f| f.name.as_deref() == Some(name))
}

/// Calls `f` on every NodeRef field of `fields`, lists included, in payload order.
pub(super) fn for_each_ref_mut(
    fields: &mut [Field],
    f: &mut dyn FnMut(&mut Field) -> Result<()>,
) -> Result<()> {
    for field in fields {
        match &mut field.value {
            FieldValue::Ref(..) => f(field)?,
            FieldValue::List(l) => {
                for el in &mut l.elements {
                    for_each_ref_mut(el, f)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A "missing" error of an edit on `structure`.
pub(super) fn missing(structure: &'static str, what: impl Into<String>) -> CorlissError {
    CorlissError::missing("edit", structure, what)
}

/// The first node of type `tid`.
pub(super) fn first_of_type(seg: &EditSegment, tid: u32) -> Result<usize> {
    let found = seg.of_type(tid).first().copied();
    found.ok_or_else(|| {
        missing(
            STRUCTURE,
            format!("a {tid:08X} node in {}", seg.segment_type),
        )
    })
}

/// The first node of type `tid` that satisfies `keep`.
pub(super) fn find_of_type(
    seg: &EditSegment,
    tid: u32,
    keep: impl Fn(&DecodedNode) -> bool,
) -> Option<usize> {
    let mut nodes = seg.of_type(tid).into_iter();
    nodes.find(|i| seg.node(*i).is_some_and(|n| keep(&n.node)))
}

/// The u16 object id of node `index` (0 when it has none).
pub(super) fn object_id(seg: &EditSegment, index: usize) -> u16 {
    seg.node(index).map_or(0, |n| n.object_id()) as u16
}
