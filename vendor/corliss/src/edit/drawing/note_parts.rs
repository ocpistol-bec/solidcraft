//! Copying template nodes into a drawing segment and renumbering their references.

use std::collections::BTreeMap;

use super::fields::{index_of, put, refs_named, u32_at};
use super::{EditNode, EditSegment, node_in, node_in_mut};
use crate::error::Result;
use crate::grammar::{DecodeStatus, Field, FieldValue};
use crate::node::NodeRef;

/// Template index → new index.
pub(super) type IndexMap = BTreeMap<usize, usize>;

/// Note part carrying a style (2ED478DF).
const T_SYMBOL: u32 = 0x2ED4_78DF;

/// The note object and everything only it uses (dw_note): its parts (children), the
/// 614A01F1 'TextGeneralNote' link (`next`), that link's 086E7238 preset entry (`next`; `t5`
/// before the full 614A01F1 grammar) and the 48C5F41A style of the 2ED478DF part.
pub(super) fn note_group(seg: &EditSegment, note: usize) -> Result<Vec<usize>> {
    let n = &node_in(seg, note)?.node;
    let mut out = vec![note];
    out.extend(refs_named(n, "children"));
    let link = index_of(n, "next");
    out.push(link);
    let l = &node_in(seg, link)?.node;
    out.push(index_of(
        l,
        if l.get("t5").is_some() { "t5" } else { "next" },
    ));
    let symbols = out
        .iter()
        .filter_map(|k| seg.node(*k))
        .filter(|n| n.node.type_id.0 == T_SYMBOL);
    let styles: Vec<usize> = symbols.map(|n| index_of(&n.node, "style")).collect();
    out.extend(styles);
    Ok(out)
}

/// A copy of template node `src` with a fresh object id in `seg`: a node
/// no grammar decoded gets its id written at byte 4, as the reference does.
pub(super) fn clone_into(seg: &mut EditSegment, src: &EditNode) -> Result<EditNode> {
    if src.node.status != DecodeStatus::NoGrammar {
        return seg.clone_node(src, false);
    }
    let id = seg.alloc_id(None)?;
    let mut payload = src.payload();
    put(&mut payload, 4, &(id as u16).to_le_bytes());
    let mut copy = src.clone();
    copy.node = seg.decode_payload(src.node.type_id.0, &payload)?;
    Ok(copy)
}

/// How a template node becomes a node of the target segment.
pub(super) type CopyNode = fn(&mut EditSegment, &EditNode) -> Result<EditNode>;

/// Appends a copy of each template node `sources[k]` made by `copy`; returns the copies in
/// order and the template → copy map.
pub(super) fn append_copies(
    seg: &mut EditSegment,
    tpl: &EditSegment,
    sources: &[usize],
    copy: CopyNode,
) -> Result<(Vec<usize>, IndexMap)> {
    let (mut copies, mut map) = (Vec::new(), IndexMap::new());
    for src in sources {
        let node = copy(seg, node_in(tpl, *src)?)?;
        let new = seg.append(node);
        map.insert(*src, new);
        copies.push(new);
    }
    Ok((copies, map))
}

/// A wide-id copy (document DC).
pub(super) fn clone_wide_into(seg: &mut EditSegment, src: &EditNode) -> Result<EditNode> {
    seg.clone_wide(src)
}

/// Re-decodes node `index` from `payload` (after a byte-level edit).
pub(super) fn redecode(seg: &mut EditSegment, index: usize, payload: &[u8]) -> Result<()> {
    let tid = node_in(seg, index)?.node.type_id.0;
    let decoded = seg.decode_payload(tid, payload)?;
    node_in_mut(seg, index)?.node = decoded;
    Ok(())
}

/// Renumbers flagged NodeRef and u32 words through `map`, lists included.
pub(super) fn remap_flagged(fields: &mut [Field], map: &IndexMap) -> Result<()> {
    remap_fields(fields, &|v| flagged_target(v, map))
}

fn flagged_target(v: &FieldValue, map: &IndexMap) -> Option<FieldValue> {
    match v {
        FieldValue::Ref(r, kind) if r.is_flagged() => map
            .get(&((r.0 & 0x7FFF_FFFF) as usize))
            .map(|n| FieldValue::Ref(NodeRef(*n as u32 | 0x8000_0000), *kind)),
        FieldValue::U32(w) if *w >> 31 != 0 => map
            .get(&((*w & 0x7FFF_FFFF) as usize))
            .map(|n| FieldValue::U32(*n as u32 | 0x8000_0000)),
        _ => None,
    }
}

/// Replaces each field value (lists walked) that `target` maps.
fn remap_fields(
    fields: &mut [Field],
    target: &dyn Fn(&FieldValue) -> Option<FieldValue>,
) -> Result<()> {
    for f in fields {
        if let FieldValue::List(l) = &mut f.value {
            l.elements
                .iter_mut()
                .try_for_each(|el| remap_fields(el, target))?;
        } else if let Some(v) = target(&f.value) {
            f.set(v)?;
        }
    }
    Ok(())
}

/// Renumbers a copied node's references through `map`: typed NodeRefs
/// (flagged or not - SM refs are unflagged), flagged values of untyped u32 / i32 fields, and
/// flagged words inside raw fields (e.g. the SM entry's group ref). Floats, text, GUIDs and
/// other typed fields are never rewritten, so a value that only looks like a NodeRef stays
/// (a reference bug, fixed: a byte scan over the whole payload rewrote such values).
pub(super) fn bytes_remap(seg: &mut EditSegment, index: usize, map: &IndexMap) -> Result<()> {
    let node = node_in_mut(seg, index)?;
    let payload = if node.node.status == DecodeStatus::NoGrammar {
        // No grammar: the payload is one raw run after the common header.
        remap_raw(&node.payload(), map, 6)
    } else {
        remap_typed(&mut node.node.fields, map)?;
        node.payload()
    };
    redecode(seg, index, &payload)
}

/// Raw fields get their flagged words remapped; the other fields go through `typed_target`.
fn remap_typed(fields: &mut [Field], map: &IndexMap) -> Result<()> {
    for f in fields {
        match &mut f.value {
            FieldValue::List(l) => l
                .elements
                .iter_mut()
                .try_for_each(|el| remap_typed(el, map))?,
            FieldValue::Raw => f.raw = remap_raw(&f.raw, map, 0),
            other => {
                if let Some(v) = typed_target(other, map) {
                    f.set(v)?;
                }
            }
        }
    }
    Ok(())
}

/// The remapped value of a NodeRef (flag kept) or of a flagged u32 / i32 field (arrays
/// included); `None` when nothing changes.
fn typed_target(v: &FieldValue, map: &IndexMap) -> Option<FieldValue> {
    let word = |w: u32| flagged_word(w, map).unwrap_or(w);
    let out = match v {
        FieldValue::Ref(r, kind) => {
            let new = r.index().and_then(|i| map.get(&i))?;
            FieldValue::Ref(NodeRef(*new as u32 | (r.0 & 0x8000_0000)), *kind)
        }
        FieldValue::U32(w) => FieldValue::U32(word(*w)),
        FieldValue::I32(w) => FieldValue::I32(word(*w as u32) as i32),
        FieldValue::U32s(ws) => FieldValue::U32s(ws.iter().map(|w| word(*w)).collect()),
        FieldValue::I32s(ws) => {
            FieldValue::I32s(ws.iter().map(|w| word(*w as u32) as i32).collect())
        }
        _ => return None,
    };
    (out != *v).then_some(out)
}

/// Flagged words of a raw byte run, left to right from `start`; a match takes its 4 bytes,
/// so matches never overlap.
fn remap_raw(bytes: &[u8], map: &IndexMap, start: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut o = start;
    while o + 4 <= out.len() {
        match u32_at(&out, o).and_then(|v| flagged_word(v, map)) {
            Some(w) => {
                put(&mut out, o, &w.to_le_bytes());
                o += 4;
            }
            None => o += 1,
        }
    }
    out
}

/// `0x80000000 | map[i]` for a flagged word `0x80000000 | i` whose `i` is mapped elsewhere.
fn flagged_word(v: u32, map: &IndexMap) -> Option<u32> {
    if v >> 31 == 0 {
        return None;
    }
    let new = 0x8000_0000 | *map.get(&((v & 0x7FFF_FFFF) as usize))? as u32;
    (new != v).then_some(new)
}

#[cfg(test)]
mod tests {
    use super::{IndexMap, remap_raw, remap_typed};
    use crate::grammar::{Field, FieldValue};

    fn map() -> IndexMap {
        IndexMap::from([(5, 9)])
    }

    #[test]
    fn flagged_words_in_raw_runs_are_remapped_without_overlap() {
        // 0x80000005 at 1, then bytes that only overlap a match.
        let raw = [0xAA, 5, 0, 0, 0x80, 5, 0, 0, 0x80];
        assert_eq!(
            remap_raw(&raw, &map(), 0),
            [0xAA, 9, 0, 0, 0x80, 9, 0, 0, 0x80]
        );
        assert_eq!(
            remap_raw(&raw, &map(), 2)[1],
            5,
            "bytes before the start stay"
        );
    }

    // Regression (a reference bug, fixed): the byte scan rewrote typed values whose
    // bytes looked like a flagged NodeRef.
    #[test]
    fn typed_values_that_look_like_noderefs_stay() {
        let lookalike = f64::from_le_bytes([5, 0, 0, 0x80, 0, 0, 0xF0, 0x3F]);
        let mut fields = vec![
            Field::encoded("x", FieldValue::F64(lookalike)).unwrap(),
            Field::encoded("text", FieldValue::Text("\u{5}\u{8000}".into())).unwrap(),
            Field::encoded("word", FieldValue::U32(0x8000_0005)).unwrap(),
        ];
        let before: Vec<Vec<u8>> = fields.iter().map(|f| f.raw.clone()).collect();
        remap_typed(&mut fields, &map()).unwrap();
        assert_eq!(fields[0].raw, before[0]);
        assert_eq!(fields[1].raw, before[1]);
        assert_eq!(fields[2].value, FieldValue::U32(0x8000_0009));
    }
}
