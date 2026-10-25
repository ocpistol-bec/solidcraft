//! The masked 4x4 placement encoding (spec "Payload encodings"): encoding, replacing and
//! decoding a node's placement.

use super::fields::missing;
use crate::error::Result;
use crate::grammar::{DecodedNode, Field, FieldValue};

/// A 4x4 placement, row-major, translation in column 3 (cm).
pub type Placement = [[f64; 4]; 4];

/// AmRx placement records write the marker for the identity placement only.
const T_RX_PLACEMENT: u32 = 0x2327_92BC;
const MARKER: u32 = 0x203;
const PREFIX: &str = "transformation.";

/// The identity placement.
pub const IDENTITY: Placement = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Per cell j the d1 / d2 bits mark 1 (d1), 0 (d2), -1 (both); other cells follow as f64
/// in cell order.
fn masks(m: &Placement) -> (u16, u16, Vec<(usize, f64)>) {
    let (mut d1, mut d2, mut values) = (0u16, 0u16, Vec::new());
    for j in 0..16 {
        let (v, bit) = (m[j / 4][j % 4], 1u16 << j);
        if v == 0.0 || v == -1.0 {
            d2 |= bit;
        }
        if v == 1.0 || v == -1.0 {
            d1 |= bit;
        }
        if v != 0.0 && v != 1.0 && v != -1.0 {
            values.push((j, v));
        }
    }
    (d1, d2, values)
}

/// The `transformation.*` fields encoding `m`, with the marker word when `marker`.
fn encode(m: &Placement, marker: bool) -> Result<Vec<Field>> {
    let (d1, d2, values) = masks(m);
    let mut out = Vec::new();
    if marker {
        out.push(Field::encoded(
            &format!("{PREFIX}marker"),
            FieldValue::U32(MARKER),
        )?);
    }
    out.push(Field::encoded(&format!("{PREFIX}d1"), FieldValue::U16(d1))?);
    out.push(Field::encoded(&format!("{PREFIX}d2"), FieldValue::U16(d2))?);
    for (j, v) in values {
        out.push(Field::encoded(
            &format!("{PREFIX}m{j}"),
            FieldValue::F64(v),
        )?);
    }
    Ok(out)
}

/// Whether the field is called `transformation.<suffix>` for a suffix starting `start`.
fn is_part(f: &Field, start: &str) -> bool {
    f.name
        .as_deref()
        .and_then(|n| n.strip_prefix(PREFIX))
        .is_some_and(|s| s.starts_with(start))
}

/// Replaces the node's `transformation.*` fields with the encoding of `m`. The marker word
/// stays as the node had it, except on AmRx placements: written for the identity, dropped
/// otherwise.
pub(super) fn replace(node: &mut DecodedNode, m: &Placement) -> Result<()> {
    let first = node.fields.iter().position(|f| is_part(f, ""));
    let last = node.fields.iter().rposition(|f| is_part(f, ""));
    let (Some(first), Some(last)) = (first, last) else {
        return Err(missing(
            "placement",
            format!("{PREFIX}* of {}", node.type_id),
        ));
    };
    let had_marker = node.fields[first..=last]
        .iter()
        .any(|f| is_part(f, "marker"));
    let marker = match node.type_id.0 {
        T_RX_PLACEMENT => *m == IDENTITY,
        _ => had_marker,
    };
    let new = encode(m, marker)?;
    node.fields.splice(first..=last, new);
    Ok(())
}

/// The integer `transformation.<suffix>` word.
fn word(node: &DecodedNode, suffix: &str) -> Option<i64> {
    node.fields
        .iter()
        .find(|f| is_part(f, suffix))?
        .value
        .as_int()
}

/// The placement a node stores, if it has one.
pub(super) fn decode(node: &DecodedNode) -> Option<Placement> {
    let (d1, d2) = (word(node, "d1")?, word(node, "d2").unwrap_or(0));
    let mut values = node.fields.iter().filter(|f| is_part(f, "m"));
    let mut values = values.by_ref().filter_map(|f| f.value.as_f64());
    let mut m = [[0.0; 4]; 4];
    for j in 0..16 {
        let bit = 1i64 << j;
        m[j / 4][j % 4] = match (d2 & bit != 0, d1 & bit != 0) {
            (false, false) => values.next().unwrap_or(0.0),
            (false, true) => 1.0,
            (true, false) => 0.0,
            (true, true) => -1.0,
        };
    }
    Some(m)
}
