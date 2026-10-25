//! A view's placement transform in the sheet SM segment (8A6D1381): a sparse 4x4 matrix,
//! scale on the diagonal and centre in m3 / m7. Inventor draws the cached view geometry
//! through it, so a view edit that skips it leaves the view where it was.

use super::decoded_of_type;
use super::{EditSegment, STRUCTURE};
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, Field, FieldValue};

/// SM view placement node.
const T_PLACEMENT: u32 = 0x8A6D_1381;
const PREFIX: &str = "placement";

/// The SM placement node of the view called `view`.
pub(super) fn placement_of(sm: &EditSegment, view: &str) -> Result<usize> {
    let found = decoded_of_type(sm, T_PLACEMENT).into_iter().find(|k| {
        sm.node(*k)
            .is_some_and(|n| n.node.text("name") == Some(view))
    });
    found.ok_or_else(|| {
        CorlissError::missing("edit", STRUCTURE, format!("SM placement of view {view}"))
    })
}

/// The 16 cells: stored ones, else 1 where `d1` marks the cell, else 0 (`get_transform`).
pub(super) fn transform(node: &DecodedNode) -> Result<[f64; 16]> {
    let d1 = node
        .int(&format!("{PREFIX}.d1"))
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "placement.d1"))?;
    let mut m = [0.0; 16];
    for (j, cell) in m.iter_mut().enumerate() {
        *cell = if d1 & (1 << j) != 0 { 1.0 } else { 0.0 };
    }
    for f in &node.fields {
        if let (Some(j), Some(v)) = (cell_number(f), f.value.as_f64()) {
            m[j] = v;
        }
    }
    Ok(m)
}

/// j of a field called `placement.m<j>`.
fn cell_number(f: &Field) -> Option<usize> {
    let name = f.name.as_deref()?;
    name.strip_prefix(PREFIX)?
        .strip_prefix(".m")?
        .parse()
        .ok()
        .filter(|j: &usize| *j < 16)
}

/// Re-encodes the sparse transform: `d1` marks cells equal to 1, `d2` cells equal to 0,
/// the rest follow as f64 (dw_proj: d1 0x8000, d2 0x7B56 for scale 2; dw_scale: 0x8421 /
/// 0x7B56 for scale 1) (`set_transform`).
pub(super) fn set_transform(node: &mut DecodedNode, m: &[f64; 16]) -> Result<()> {
    let d1_name = format!("{PREFIX}.d1");
    let start = node
        .fields
        .iter()
        .position(|f| f.name.as_deref() == Some(d1_name.as_str()));
    let start = start.ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "placement.d1"))?;
    let mut end = (start + 2).min(node.fields.len());
    while node.fields.get(end).is_some_and(|f| {
        f.name
            .as_deref()
            .is_some_and(|n| n.starts_with("placement.m"))
    }) {
        end += 1;
    }
    node.fields.splice(start..end, encode_cells(m)?);
    Ok(())
}

fn encode_cells(m: &[f64; 16]) -> Result<Vec<Field>> {
    let (mut d1, mut d2) = (0u16, 0u16);
    let mut cells = Vec::new();
    for (j, v) in m.iter().enumerate() {
        if *v == 1.0 {
            d1 |= 1 << j;
        } else if *v == 0.0 {
            d2 |= 1 << j;
        } else {
            cells.push(Field::encoded(
                &format!("{PREFIX}.m{j}"),
                FieldValue::F64(*v),
            )?);
        }
    }
    let mut out = vec![
        Field::encoded(&format!("{PREFIX}.d1"), FieldValue::U16(d1))?,
        Field::encoded(&format!("{PREFIX}.d2"), FieldValue::U16(d2))?,
    ];
    out.extend(cells);
    Ok(out)
}
