//! Style usage counts in the document DC.
//!
//! DlDocDC 6759D870 `outgoing_style_refs_for_segment` (older grammar: `lst1`) lists (segment
//! id, 388F6C64 usage map) for the sheet segments. A note adds 1 to the sheet DC map's
//! drawing-standard entry (36BC43F4 'Default Standard', dw_dim 5 → dw_note 6) and 1 to the SM
//! map's entry keyed by the SM segment's position in that list (dw_note: key 2 = 1;
//! dw_sheet2, where two segments come first: key 4).

use super::fields::{int_like, list_named};
use super::xref::seg_id_of;
use super::{DOC_SEGMENT, EditSegment, Editor, first_of_type, missing, node_in, node_in_mut};
use crate::error::Result;
use crate::grammar::{DecodedNode, Field, FieldValue, ListField};
use crate::guid::Guid;

/// Document DC root holding the usage map list.
const T_DOC_ROOT: u32 = 0x6759_D870;
/// Drafting standard style (36BC43F4).
const T_STANDARD: u32 = 0x36BC_43F4;
/// Meta of a usage map, (0x10, 4) whatever its size (dw_dim / dw_note 388F6C64).
const MAP_META: [u8; 8] = [0x10, 0, 0, 0, 4, 0, 0, 0];

/// One usage map: the segment it counts for, its position in the list, its node.
struct UsageMap {
    segment: Guid,
    position: usize,
    node: usize,
}

/// Adds `by` to the sheet's style usage: the DC map's standard entry and the SM map's entry.
pub(super) fn bump_usage(ed: &mut Editor, prefix: &str, by: i64) -> Result<()> {
    let dc_id = segment_id(ed, &format!("{prefix}DCSegment"))?;
    let sm_id = segment_id(ed, &format!("{prefix}SMSegment"))?;
    let doc = ed.segment(DOC_SEGMENT)?;
    let maps = usage_maps(doc)?;
    let dc_map = map_of(&maps, dc_id)?;
    let standard = standard_key(doc, dc_map.node)?;
    map_inc(&mut node_in_mut(doc, dc_map.node)?.node, standard, by)?;
    let sm_map = map_of(&maps, sm_id)?;
    map_inc(
        &mut node_in_mut(doc, sm_map.node)?.node,
        sm_map.position as i64,
        by,
    )
}

fn segment_id(ed: &mut Editor, name: &str) -> Result<Guid> {
    seg_id_of(ed.segment(name)?).ok_or_else(|| missing(format!("segment id of {name}")))
}

/// Every usage map entry of the document root, in list order.
fn usage_maps(doc: &EditSegment) -> Result<Vec<UsageMap>> {
    let root = &node_in(doc, first_of_type(doc, T_DOC_ROOT)?)?.node;
    let is_list = |f: &&Field| f.is("outgoing_style_refs_for_segment") || f.is("lst1");
    let lists = root
        .fields
        .iter()
        .filter(is_list)
        .filter_map(|f| match &f.value {
            FieldValue::List(l) => Some(l),
            _ => None,
        });
    Ok(lists
        .flat_map(|l| {
            l.elements
                .iter()
                .enumerate()
                .filter_map(|(k, el)| map_entry(k, el))
        })
        .collect())
}

fn map_entry(position: usize, el: &[Field]) -> Option<UsageMap> {
    let FieldValue::Guid(segment) = el.first()?.value else {
        return None;
    };
    let node = (el.get(1)?.value.as_int()? & 0x7FFF_FFFF) as usize;
    Some(UsageMap {
        segment,
        position,
        node,
    })
}

/// The map of segment `seg_id`; a later entry wins, as in the reference's dict.
fn map_of(maps: &[UsageMap], seg_id: Guid) -> Result<&UsageMap> {
    let found = maps.iter().rev().find(|m| m.segment == seg_id);
    found.ok_or_else(|| missing(format!("usage map of segment {seg_id}")))
}

/// The DC map's key that names the drawing standard (a flagged NodeRef word).
fn standard_key(doc: &EditSegment, map: usize) -> Result<i64> {
    let is_standard = |v: &i64| {
        *v >> 31 != 0
            && doc
                .node((*v & 0x7FFF_FFFF) as usize)
                .is_some_and(|n| n.node.type_id.0 == T_STANDARD)
    };
    let keys = map_list(&node_in(doc, map)?.node)
        .iter()
        .filter_map(|el| el.first()?.value.as_int());
    keys.into_iter()
        .find(is_standard)
        .ok_or_else(|| missing("drawing standard entry of the sheet usage map"))
}

fn map_list(node: &DecodedNode) -> &[Vec<Field>] {
    let list = node.fields.iter().find_map(|f| match &f.value {
        FieldValue::List(l) if f.is("map") => Some(l.elements.as_slice()),
        _ => None,
    });
    list.unwrap_or_default()
}

/// Adds `by` to the entry keyed `key`, dropping it at 0 or below; a missing key gets a new
/// entry shaped like the first one (dw_proj has no entry for the SM key).
fn map_inc(node: &mut DecodedNode, key: i64, by: i64) -> Result<()> {
    let list = list_named(node, "map")?;
    let hit = list
        .elements
        .iter()
        .position(|el| el.first().and_then(|f| f.value.as_int()) == Some(key));
    match hit {
        Some(at) => add_to_entry(list, at, by),
        None => add_entry(list, key, by),
    }
}

fn add_to_entry(list: &mut ListField, at: usize, by: i64) -> Result<()> {
    let count = list.elements.get_mut(at).and_then(|el| el.get_mut(1));
    let count = count.ok_or_else(|| missing(format!("count of usage map entry {at}")))?;
    let value = count.value.as_int().unwrap_or(0) + by;
    count.set(int_like(&count.value, value)?)?;
    if value <= 0 {
        list.elements.remove(at);
    }
    Ok(())
}

fn add_entry(list: &mut ListField, key: i64, by: i64) -> Result<()> {
    let entry = new_entry(list.elements.first(), key, by)?;
    if list.elements.is_empty() && list.meta.is_none() {
        list.meta = Some(MAP_META.to_vec());
    }
    // A map's meta stays (0x10, 4) as it grows, so the element goes in without a refit.
    list.elements.push(entry);
    Ok(())
}

/// A (key, count) entry with the fields of `proto`, or two u32 `map` fields.
fn new_entry(proto: Option<&Vec<Field>>, key: i64, by: i64) -> Result<Vec<Field>> {
    let shaped = |k: usize, v: i64| -> Result<Field> {
        let like = proto.and_then(|p| p.get(k));
        let name = like
            .and_then(|f| f.name.clone())
            .unwrap_or_else(|| "map".to_string());
        let value = like.map_or(Ok(FieldValue::U32(v as u32)), |f| int_like(&f.value, v))?;
        Field::encoded(&name, value)
    };
    Ok(vec![shaped(0, key)?, shaped(1, by)?])
}
