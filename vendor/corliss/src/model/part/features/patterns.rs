//! Rectangular, circular, mirror and sketch-driven patterns.

use crate::grammar::{DecodedNode, FieldValue, ListField};
use crate::model::part::ParamValue;
use crate::model::view::{cross, floats, named, node_ref, ref_list, vec3};

use super::FeatureKind;
use super::context::{FeatureContext, Slots, value_of};
use super::values::{FeatureCreationMethod, FeatureValue};

/// RectangularPatternFeature.
pub(super) const T_PATTERN_RECT: u32 = 0x2067_3244;
/// PolarPatternFeature.
pub(super) const T_PATTERN_POLAR: u32 = 0x0697_7131;
/// MirrorFeature.
pub(super) const T_MIRROR: u32 = 0xFAD9_A9B5;
/// Sketch driven pattern (CC90BCDA point set).
pub(super) const T_PATTERN_SKETCH: u32 = 0xA37B_053C;
/// Plane geometry: a mirror plane among the participants.
const T_PLANE: u32 = 0xCE52_DF42;
/// Pattern occurrence results.
const T_PATTERN_RESULTS: u32 = 0x1A26_FF54;
/// Direction along a path curve.
const T_PATH_DIRECTION: u32 = 0xA597_7BAA;
/// Direction vector.
const T_DIRECTION: u32 = 0xCE52_DF40;

/// What every rectangular, circular and mirror pattern stores.
#[derive(Debug, Clone, PartialEq)]
pub struct FeaturePatternBase {
    /// Patterned features (planes excluded).
    pub features: Vec<Option<String>>,
    /// Patterned or affected bodies.
    pub bodies: Vec<Option<String>>,
    /// Creation method.
    pub creation_method: Option<FeatureCreationMethod>,
    /// Patterns solids rather than features.
    pub solids: bool,
    /// Features the occurrences created, when any.
    pub result_features: Option<Vec<Option<String>>>,
    /// Raw (u32, u32) per suppressed occurrence, when any.
    pub suppressed: Option<Vec<FeatureValue>>,
}

/// A direction along a path curve.
#[derive(Debug, Clone, PartialEq)]
pub struct FeaturePatternPath {
    /// Start point.
    pub start: Vec<f64>,
    /// Curve data.
    pub curve: Vec<f64>,
    /// Path flag.
    pub flag: Option<bool>,
}

/// One direction of a rectangular pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct FeaturePatternDirection {
    /// Count parameter.
    pub count: Option<ParamValue>,
    /// Spacing parameter.
    pub spacing: Option<ParamValue>,
    /// Direction vector (flip reverses it; there is no flip flag).
    pub dir: Option<Vec<f64>>,
    /// Direction along a path.
    pub path: Option<FeaturePatternPath>,
    /// Fit distribution (else spacing).
    pub fit: Option<bool>,
    /// Midplane.
    pub midplane: Option<bool>,
    /// Flags 1 and 2.
    pub flags: [Option<bool>; 2],
}

/// A circular pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureCircularPattern {
    /// Common pattern data.
    pub base: FeaturePatternBase,
    /// Count parameter.
    pub count: Option<ParamValue>,
    /// Angle parameter.
    pub angle: Option<ParamValue>,
    /// Axis.
    pub axis: Option<super::FeatureAxis>,
    /// Incremental distribution (else fitted).
    pub incremental: bool,
    /// Midplane.
    pub midplane: Option<bool>,
    /// Fixed orientation (else rotational).
    pub fixed_orientation: bool,
    /// Flag 1.
    pub flag1: Option<bool>,
}

/// A mirror plane.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureMirrorPlane {
    /// Point on the plane.
    pub origin: Vec<f64>,
    /// Plane normal.
    pub normal: [f64; 3],
}

/// A sketch-driven pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSketchPattern {
    /// Patterned features.
    pub features: Vec<Option<String>>,
    /// Bodies.
    pub bodies: Vec<Option<String>>,
    /// Creation method.
    pub creation_method: Option<FeatureCreationMethod>,
    /// Sketch points the occurrences sit on.
    pub points: Vec<Vec<f64>>,
    /// Base point.
    pub base_point: Vec<f64>,
}

/// Reads the pattern feature `n` at `index`.
pub(super) fn read_pattern(ctx: &FeatureContext, index: usize, n: &DecodedNode) -> FeatureKind {
    if n.type_id.0 == T_PATTERN_SKETCH {
        return FeatureKind::SketchDrivenPattern(sketch_pattern(ctx, n));
    }
    let s = Slots::new(ctx, index, plist(n));
    let base = pattern_base(&s, n);
    match n.type_id.0 {
        T_PATTERN_RECT => rectangular(&s, n, base),
        T_PATTERN_POLAR => FeatureKind::CircularPattern(circular(&s, n, base)),
        _ => mirror(&s, n, base),
    }
}

/// The separate `properties` NodeRef fields of a pattern.
fn plist(n: &DecodedNode) -> Vec<usize> {
    let refs = n
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some("properties"));
    refs.filter_map(|f| match f.value {
        FieldValue::Ref(r, _) => Some(r.index().unwrap_or(0)),
        _ => None,
    })
    .collect()
}

fn ref_flag(s: &Slots, n: &DecodedNode, name: &str) -> Option<bool> {
    node_ref(n, name).and_then(|r| s.ctx.flag(r))
}

fn pattern_base(s: &Slots, n: &DecodedNode) -> FeaturePatternBase {
    let parts = ref_list(n, "participants");
    let not_plane = parts.iter().filter(|&&f| s.ctx.tid(f) != Some(T_PLANE));
    let features: Vec<Option<String>> = not_plane.map(|&f| s.ctx.name(f)).collect();
    let solids = if n.type_id.0 == T_PATTERN_POLAR {
        Some(features.is_empty())
    } else {
        s.flag(6)
    };
    let body_slot = if solids == Some(true) { 10 } else { 9 };
    let results = result_features(s);
    FeaturePatternBase {
        features,
        bodies: s.ctx.names(s.node(body_slot), "items"),
        creation_method: FeatureCreationMethod::from_value(
            s.node(2).and_then(|n| value_of(n, "valueModel")),
        ),
        solids: solids.unwrap_or(false),
        result_features: (!results.is_empty()).then_some(results),
        suppressed: s.node(1).map(suppressed).filter(|v| !v.is_empty()),
    }
}

/// Names of the features in the occurrence results' nested lists.
fn result_features(s: &Slots) -> Vec<Option<String>> {
    let Some(res) = s.node(8).filter(|r| r.type_id.0 == T_PATTERN_RESULTS) else {
        return Vec::new();
    };
    let lists = res
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some("lst0"));
    let refs = lists
        .filter_map(|f| list_of(&f.value))
        .flat_map(nested_first_refs);
    refs.map(|r| s.ctx.name(r)).collect()
}

fn list_of(v: &FieldValue) -> Option<&ListField> {
    match v {
        FieldValue::List(l) => Some(l),
        _ => None,
    }
}

/// The first value of every element of the lists nested in `l`'s elements, as node indexes.
fn nested_first_refs(l: &ListField) -> Vec<usize> {
    let inner = l
        .elements
        .iter()
        .flatten()
        .filter_map(|f| list_of(&f.value));
    let firsts = inner.flat_map(|i| i.elements.iter().filter_map(|y| y.first()));
    firsts
        .map(|y| (y.value.as_int().unwrap_or(0) & 0x7FFF_FFFF) as usize)
        .collect()
}

fn suppressed(n: &DecodedNode) -> Vec<FeatureValue> {
    let numbered = |name: &str| {
        name.strip_prefix("suppressed.")
            .is_some_and(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
    };
    let fields = n
        .fields
        .iter()
        .filter(|f| f.name.as_deref().is_some_and(numbered));
    fields.map(|f| FeatureValue::from_field(&f.value)).collect()
}

fn path_direction(s: &Slots, dn: &DecodedNode) -> FeaturePatternPath {
    let curve = match named(dn, "edge").first().map(|f| &f.value) {
        Some(v @ (FieldValue::F64s(_) | FieldValue::F32s(_))) => v.as_f64s().unwrap_or_default(),
        _ => floats(dn, "edge"),
    };
    FeaturePatternPath {
        start: floats(dn, "point"),
        curve,
        flag: ref_flag(s, dn, "parameter"),
    }
}

fn rect_direction(
    s: &Slots,
    n: &DecodedNode,
    slots: [usize; 3],
    flag: &str,
) -> FeaturePatternDirection {
    let [cnt, sp, dv] = slots;
    let f: Vec<Option<bool>> = (0..4)
        .map(|k| ref_flag(s, n, &format!("{flag}{k}")))
        .collect();
    let dn = s.node(dv);
    FeaturePatternDirection {
        count: s.param(cnt),
        spacing: s.param(sp),
        dir: dn
            .filter(|d| d.type_id.0 == T_DIRECTION)
            .map(|d| floats(d, "dir")),
        path: dn
            .filter(|d| d.type_id.0 == T_PATH_DIRECTION)
            .map(|d| path_direction(s, d)),
        fit: f[0],
        midplane: f[3],
        flags: [f[1], f[2]],
    }
}

fn rectangular(s: &Slots, n: &DecodedNode, base: FeaturePatternBase) -> FeatureKind {
    let new_solid = base.solids.then(|| s.flag(11));
    FeatureKind::RectangularPattern {
        direction_a: rect_direction(s, n, [26, 28, 30], "flagA"),
        direction_b: rect_direction(s, n, [27, 29, 31], "flagB"),
        new_solid,
        base,
    }
}

fn circular(s: &Slots, n: &DecodedNode, base: FeaturePatternBase) -> FeatureCircularPattern {
    let ax = node_ref(n, "axis").and_then(|r| s.ctx.node(r));
    FeatureCircularPattern {
        base,
        count: s.ctx.param(node_ref(n, "count").unwrap_or(0)),
        angle: s.ctx.param(node_ref(n, "angle").unwrap_or(0)),
        axis: ax.map(|a| super::FeatureAxis {
            pos: floats(a, "pos"),
            dir: floats(a, "dir"),
        }),
        incremental: s.flag(3).unwrap_or(false),
        midplane: ref_flag(s, n, "midplane"),
        fixed_orientation: ref_flag(s, n, "fixed_orientation").unwrap_or(false),
        flag1: ref_flag(s, n, "flag1"),
    }
}

fn mirror(s: &Slots, n: &DecodedNode, base: FeaturePatternBase) -> FeatureKind {
    let parts = ref_list(n, "participants");
    let planes = parts.into_iter().filter(|&f| s.ctx.tid(f) == Some(T_PLANE));
    let pl = node_ref(n, "plane").and_then(|r| s.ctx.node(r));
    FeatureKind::Mirror {
        plane_tool: planes.map(|f| s.ctx.name(f)).collect(),
        plane: pl.map(|p| FeatureMirrorPlane {
            origin: floats(p, "origin"),
            normal: cross(vec3(p, "u"), vec3(p, "v")),
        }),
        flags: [s.flag(6), s.flag(7)],
        base,
    }
}

fn sketch_pattern(ctx: &FeatureContext, n: &DecodedNode) -> FeatureSketchPattern {
    let linked = |name: &str| node_ref(n, name).and_then(|r| ctx.node(r));
    let points = linked("points")
        .map(|p| ref_list(p, "points"))
        .unwrap_or_default();
    let point = |q: usize| ctx.node(q).map(|q| floats(q, "pos")).unwrap_or_default();
    let base = named(n, "base")
        .first()
        .and_then(|f| f.value.as_f64s())
        .unwrap_or_default();
    FeatureSketchPattern {
        features: ctx.names(Some(n), "participants"),
        bodies: ctx.names(linked("bodies"), "items"),
        creation_method: FeatureCreationMethod::from_value(
            linked("creation").and_then(|c| value_of(c, "valueModel")),
        ),
        points: points.into_iter().map(point).collect(),
        base_point: base,
    }
}
