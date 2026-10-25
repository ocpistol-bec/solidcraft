//! Sketch geometry entities: points, lines, circles, arcs, ellipses and splines.

use std::f64::consts::TAU;

use super::types::{
    Sketch, SketchArc, SketchCircle, SketchEllipse, SketchLine, SketchPoint, SketchSpline,
};
use crate::grammar::{DecodedNode, Field, FieldValue};
use crate::model::part::Part;
use crate::model::view::{apply, node_ref, ref_list, vec2};

const T_POINT2D: u32 = 0xCE52_DF35;
/// Midpoint + unit direction; endpoints via `points`.
const T_LINE2D: u32 = 0xCE52_DF3A;
/// Circle (`points` = [0, 0]) or arc (counter-clockwise from points[0] to points[1]).
const T_ARC2D: u32 = 0xCE52_DF3B;
/// Center ref, `dA` = unit major-axis direction, `a` / `b` = major / minor radius.
const T_ELLIPSE2D: u32 = 0x4507_D460;
/// Fit points, knots, 2D poles.
const T_SPLINE2D: u32 = 0xF937_2FD4;

/// `flags2` bit of a projected point.
const FLAG_PROJECTED: i64 = 0x40;
/// `flags2` bit of construction geometry.
const FLAG_REFERENCE: i64 = 0x8_0000;

impl Part<'_> {
    /// Files sketch entity node `index` under the matching list of `sketch`.
    pub(super) fn add_sketch_entity(&self, sketch: &mut Sketch, index: usize) {
        let Some(n) = self.view().node(index) else {
            return;
        };
        match n.type_id.0 {
            T_POINT2D => sketch.points.push(sketch_point(sketch, index, n)),
            T_LINE2D => sketch.lines.extend(self.sketch_line(index, n)),
            T_ARC2D => self.add_sketch_arc(sketch, index, n),
            T_ELLIPSE2D => sketch.ellipses.push(self.sketch_ellipse(index, n)),
            T_SPLINE2D => sketch.splines.push(self.sketch_spline(index, n)),
            _ => self.add_sketch_constraint(sketch, index, n),
        }
    }

    /// The `pos` of the point node `index`.
    fn point2(&self, index: usize) -> Option<[f64; 2]> {
        self.view().node(index).map(|p| vec2(p, "pos"))
    }

    /// The `pos` of the point a NodeRef field names.
    fn ref_point2(&self, n: &DecodedNode, field: &str) -> Option<[f64; 2]> {
        node_ref(n, field).and_then(|i| self.point2(i))
    }

    fn sketch_line(&self, index: usize, n: &DecodedNode) -> Option<SketchLine> {
        let ends: Vec<[f64; 2]> = ref_list(n, "points")
            .into_iter()
            .filter_map(|r| self.point2(r))
            .collect();
        let (&start, &end) = (ends.first()?, ends.get(1)?);
        let reference = flags2(n) & FLAG_REFERENCE != 0;
        Some(SketchLine {
            index,
            start,
            end,
            mid: vec2(n, "pos"),
            dir: vec2(n, "dir"),
            reference,
        })
    }

    /// A circle has no end points; an arc has two.
    fn add_sketch_arc(&self, sketch: &mut Sketch, index: usize, n: &DecodedNode) {
        let center = self.ref_point2(n, "center").unwrap_or_default();
        let radius = n.float("r");
        let ends: Vec<usize> = ref_list(n, "points")
            .into_iter()
            .filter(|&p| p != 0)
            .collect();
        if ends.is_empty() {
            sketch.circles.push(SketchCircle {
                index,
                center,
                radius,
            });
            return;
        }
        let [Some(start), Some(end)] =
            [ends.first(), ends.get(1)].map(|e| e.and_then(|&i| self.point2(i)))
        else {
            return;
        };
        let (start_angle, sweep) = arc_angles(center, start, end);
        sketch.arcs.push(SketchArc {
            index,
            center,
            radius,
            start,
            end,
            start_angle,
            sweep,
        });
    }

    fn sketch_ellipse(&self, index: usize, n: &DecodedNode) -> SketchEllipse {
        SketchEllipse {
            index,
            center: self.ref_point2(n, "center").unwrap_or_default(),
            major_dir: vec2(n, "dA"),
            major_radius: n.float("a"),
            minor_radius: n.float("b"),
        }
    }

    fn sketch_spline(&self, index: usize, n: &DecodedNode) -> SketchSpline {
        let fit_points = ref_list(n, "points")
            .into_iter()
            .filter_map(|r| self.point2(r))
            .collect();
        let poles = fields_named(n, &["poles"])
            .map(|f| field_floats(&f.value))
            .collect();
        let knots = fields_named(n, &["knots", "a3"])
            .filter_map(|f| field_floats(&f.value).first().copied())
            .collect();
        SketchSpline {
            index,
            fit_points,
            poles,
            knots,
        }
    }
}

/// The angle of `start` about `center` and the counter-clockwise sweep to `end`, in [0, 2π).
fn arc_angles(center: [f64; 2], start: [f64; 2], end: [f64; 2]) -> (f64, f64) {
    let start_angle = (start[1] - center[1]).atan2(start[0] - center[0]);
    let end_angle = (end[1] - center[1]).atan2(end[0] - center[0]);
    (start_angle, (end_angle - start_angle).rem_euclid(TAU))
}

/// Top-level fields whose current name is one of `names` (exact names, not earlier ones).
fn fields_named<'n>(n: &'n DecodedNode, names: &'n [&str]) -> impl Iterator<Item = &'n Field> {
    n.fields
        .iter()
        .filter(|f| f.name.as_deref().is_some_and(|x| names.contains(&x)))
}

fn sketch_point(sketch: &Sketch, index: usize, n: &DecodedNode) -> SketchPoint {
    let pos = vec2(n, "pos");
    let model = sketch.matrix.map(|m| apply(&m, [pos[0], pos[1], 0.0]));
    SketchPoint {
        index,
        pos,
        model,
        projected: flags2(n) & FLAG_PROJECTED != 0,
    }
}

/// The geometry flags word.
fn flags2(n: &DecodedNode) -> i64 {
    n.int("flags2").unwrap_or(0)
}

/// The values of one float field, scalar or array.
fn field_floats(value: &FieldValue) -> Vec<f64> {
    value
        .as_f64s()
        .or_else(|| value.as_f64().map(|x| vec![x]))
        .unwrap_or_default()
}
