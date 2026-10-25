//! Changing a constraint's value, shifting planar constraints as the reference writer does.

use super::fields::{has_named, missing, node, node_mut, ref_list, ref_of, required_ref, set};
use super::lookup::{self, BROWSER, DC, GROUNDED, RX, T_CONSTRAINT};
use super::occurrence::set_occurrence_transform;
use super::transform::{self, Placement};
use crate::edit::{EditSegment, Editor};
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, FieldValue};

const T_FORMULA: u32 = 0xF8A7_7A04;
const T_SELECTION: u32 = 0xB91F_CE52;
const T_PLANE: u32 = 0xCE52_DF42;
/// Constraint styles whose offset moves a planar face: mate and flush.
const PLANAR_STYLES: [i64; 2] = [1, 2];

/// Sets value `slot` of constraint or joint `name` (0 = offset / angle / gap) in database
/// units (cm, rad): the parameter, its formula value and the browser entry. With `solve` a
/// planar mate / flush offset change also moves the occurrence on the free side by the
/// difference along the grounded face's world normal, when that occurrence takes part in no
/// other constraint (otherwise Inventor re-solves on Rebuild). Returns the parameter name.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::assembly::set_constraint_value(&mut ed, "Mate:1", 2.0, 0, true)?;
/// ed.finish()?.save("edited.iam")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn set_constraint_value(
    ed: &mut Editor,
    name: &str,
    value: f64,
    slot: usize,
    solve: bool,
) -> Result<String> {
    let k = lookup::constraint_position(&lookup::constraint_names(ed.segment(BROWSER)?), name)?;
    let dc = ed.segment(DC)?;
    let c = dc.of_type(T_CONSTRAINT).get(k).copied();
    let c = c.ok_or_else(|| missing("AmDcSegment", format!("constraint node of {name}")))?;
    let (param, old) = set_parameter(dc, c, slot, value)?;
    let style = node(dc, c)?.int("style");
    if slot == 0 {
        set_browser_offset(ed.segment(BROWSER)?, k, value)?;
    }
    if solve && slot == 0 && style.is_some_and(|s| PLANAR_STYLES.contains(&s)) {
        shift_planar(ed, c, value - old)?;
    }
    Ok(param)
}

/// The parameter behind value `slot` of constraint `c`.
fn value_parameter(dc: &EditSegment, c: usize, slot: usize) -> Result<usize> {
    let values = ref_list(node(dc, c)?, "values");
    let Some(v) = values.get(slot).copied() else {
        let expected = format!("a value slot below {}", values.len());
        return Err(CorlissError::invalid(
            "assembly constraint",
            "values",
            c,
            slot,
            expected,
        ));
    };
    required_ref(node(dc, v)?, "parameter")
}

/// The value parameter, its formula: returns the parameter's name and its previous value.
fn set_parameter(dc: &mut EditSegment, c: usize, slot: usize, value: f64) -> Result<(String, f64)> {
    let param = node_mut(dc, value_parameter(dc, c, slot)?)?;
    let old = param.float("valueModel").unwrap_or(0.0);
    set(param, "valueNominal", FieldValue::F64(value))?;
    set(param, "valueModel", FieldValue::F64(value))?;
    let name = param.text("name").unwrap_or_default().to_string();
    let Some(f) = ref_of(param, "formula") else {
        return Ok((name, old));
    };
    let formula = node_mut(dc, f)?;
    if formula.type_id.0 == T_FORMULA {
        set(formula, "value", FieldValue::F64(value))?;
    }
    Ok((name, old))
}

/// The browser entry repeats the value as `offset` where it has that field.
fn set_browser_offset(browser: &mut EditSegment, k: usize, value: f64) -> Result<()> {
    let Some(e) = lookup::browser_constraints(browser).get(k).copied() else {
        return Ok(());
    };
    let entry = node_mut(browser, e)?;
    if has_named(entry, "offset") {
        set(entry, "offset", FieldValue::F64(value))?;
    }
    Ok(())
}

/// Moves the free side of a planar mate / flush by `delta` along the grounded side's world
/// face normal (plane proxy normal = u x v, part coordinates).
fn shift_planar(ed: &mut Editor, c: usize, delta: f64) -> Result<()> {
    if delta == 0.0 {
        return Ok(());
    }
    let dc = ed.segment(DC)?;
    let Some((fixed, moving)) = grounded_and_free(dc, c)? else {
        return Ok(());
    };
    let label = lookup::labels(dc).remove(&moving).unwrap_or_default();
    let fixed_oid = lookup::occurrence_id(dc, fixed.0)?;
    let moving_oid = lookup::occurrence_id(dc, moving)?;
    let rx = ed.segment(RX)?;
    let rf = placement(rx, fixed_oid)?;
    let mut mm = placement(rx, moving_oid)?;
    for (r, row) in mm.iter_mut().take(3).enumerate() {
        let world = (0..3).fold(0.0, |acc, k| acc + rf[r][k] * fixed.1[k]);
        row[3] += delta * world;
    }
    set_occurrence_transform(ed, &label, &mm).map(|_| ())
}

/// The grounded side and the free occurrence of a planar constraint, when there is exactly
/// one of each and the free occurrence takes part in no other constraint.
fn grounded_and_free(dc: &EditSegment, c: usize) -> Result<Option<(Side, usize)>> {
    let Some(sides) = planar_sides(dc, c)? else {
        return Ok(None);
    };
    let flags = |o: usize| node(dc, o).ok().and_then(|n| n.int("flags")).unwrap_or(0);
    let (grounded, free): (Vec<_>, Vec<_>) = sides
        .into_iter()
        .partition(|(o, _)| flags(*o) & GROUNDED != 0);
    let ([fixed], [(moving, _)]) = (&grounded[..], &free[..]) else {
        return Ok(None);
    };
    if used_elsewhere(dc, c, *moving)? {
        return Ok(None);
    }
    Ok(Some((*fixed, *moving)))
}

/// An occurrence and the world-independent normal of the face selected on it.
type Side = (usize, [f64; 3]);

/// (occurrence, face normal) of each selection, or `None` when a selection is not an
/// occurrence + plane proxy pair.
fn planar_sides(dc: &EditSegment, c: usize) -> Result<Option<Vec<Side>>> {
    let mut sides = Vec::new();
    for sel in ref_list(node(dc, c)?, "selection") {
        let m = node(dc, sel)?;
        if m.type_id.0 != T_SELECTION {
            return Ok(None);
        }
        let (Some(occ), Some(g)) = (
            ref_of(m, "ref_1"),
            ref_of(m, "dir").and_then(|g| dc.node(g)),
        ) else {
            return Ok(None);
        };
        if g.node.type_id.0 != T_PLANE {
            return Ok(None);
        }
        sides.push((occ, cross(vec3(&g.node, "u"), vec3(&g.node, "v"))));
    }
    Ok(Some(sides))
}

/// Whether occurrence `occ` takes part in a constraint other than `c`.
fn used_elsewhere(dc: &EditSegment, c: usize, occ: usize) -> Result<bool> {
    for other in dc.of_type(T_CONSTRAINT).into_iter().filter(|o| *o != c) {
        for sel in ref_list(node(dc, other)?, "selection") {
            let m = node(dc, sel)?;
            if ["ref_1", "ref_0", "occurrence"]
                .iter()
                .any(|n| ref_of(m, n) == Some(occ))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn placement(rx: &EditSegment, oid: i64) -> Result<Placement> {
    let p = lookup::rx_placement(rx, oid)?;
    transform::decode(node(rx, p)?).ok_or_else(|| {
        CorlissError::missing(
            "edit",
            "AmRxSegment",
            format!("transformation of occurrence id {oid}"),
        )
    })
}

/// The three values of a vector field, read as the reference reader does: fields called `name`
/// now, else those that had the name.
fn vec3(n: &DecodedNode, name: &str) -> [f64; 3] {
    let now: Vec<f64> = n
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some(name))
        .filter_map(|f| f.value.as_f64())
        .collect();
    let values = if now.is_empty() {
        n.fields
            .iter()
            .filter(|f| f.is(name))
            .filter_map(|f| f.value.as_f64())
            .collect()
    } else {
        now
    };
    [0, 1, 2].map(|k| values.get(k).copied().unwrap_or(0.0))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
