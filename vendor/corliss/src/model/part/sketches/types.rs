//! The records [`Part::sketches`](crate::model::Part::sketches) returns. Positions are sketch
//! coordinates (cm) unless a field says otherwise.

use crate::model::part::ParamValue;
use crate::model::view::Matrix4;

/// A 2D sketch.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sketch {
    /// Node index of the sketch.
    pub index: usize,
    /// Browser name ("Sketch1").
    pub name: Option<String>,
    /// Browser name of the plane it is placed on.
    pub plane: Option<String>,
    /// Sketch (x, y, 0, 1) → model; columns are the x axis, y axis, normal and origin.
    pub matrix: Option<Matrix4>,
    /// Sketch normal in model coordinates.
    pub normal: Option<[f64; 3]>,
    /// Sketch points.
    pub points: Vec<SketchPoint>,
    /// Line segments.
    pub lines: Vec<SketchLine>,
    /// Full circles.
    pub circles: Vec<SketchCircle>,
    /// Circular arcs.
    pub arcs: Vec<SketchArc>,
    /// Ellipses.
    pub ellipses: Vec<SketchEllipse>,
    /// Splines.
    pub splines: Vec<SketchSpline>,
    /// Sketch texts.
    pub texts: Vec<SketchText>,
    /// Sketch patterns whose occurrences use this sketch's entities.
    pub patterns: Vec<SketchPattern>,
    /// Geometric constraints.
    pub constraints: Vec<SketchConstraint>,
    /// Dimensions, which drive (or are driven by) a parameter.
    pub dimensions: Vec<SketchDimension>,
}

/// A sketch point.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPoint {
    /// Node index.
    pub index: usize,
    /// Position.
    pub pos: [f64; 2],
    /// Position in model coordinates (`None` when the sketch has no matrix).
    pub model: Option<[f64; 3]>,
    /// True for a point projected from other geometry.
    pub projected: bool,
}

/// A line segment.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchLine {
    /// Node index.
    pub index: usize,
    /// Start point.
    pub start: [f64; 2],
    /// End point.
    pub end: [f64; 2],
    /// Stored midpoint.
    pub mid: [f64; 2],
    /// Stored unit direction.
    pub dir: [f64; 2],
    /// True for construction (reference) geometry.
    pub reference: bool,
}

/// A full circle.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchCircle {
    /// Node index.
    pub index: usize,
    /// Center.
    pub center: [f64; 2],
    /// Radius.
    pub radius: Option<f64>,
}

/// A circular arc, counter-clockwise from `start` to `end`.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchArc {
    /// Node index.
    pub index: usize,
    /// Center.
    pub center: [f64; 2],
    /// Radius.
    pub radius: Option<f64>,
    /// Start point.
    pub start: [f64; 2],
    /// End point.
    pub end: [f64; 2],
    /// Angle of `start` about the center (rad).
    pub start_angle: f64,
    /// Counter-clockwise sweep in [0, 2π) (rad).
    pub sweep: f64,
}

/// An ellipse.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchEllipse {
    /// Node index.
    pub index: usize,
    /// Center.
    pub center: [f64; 2],
    /// Unit major-axis direction.
    pub major_dir: [f64; 2],
    /// Major radius.
    pub major_radius: Option<f64>,
    /// Minor radius.
    pub minor_radius: Option<f64>,
}

/// A spline through fit points.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchSpline {
    /// Node index.
    pub index: usize,
    /// Fit points, in stored order.
    pub fit_points: Vec<[f64; 2]>,
    /// Control points.
    pub poles: Vec<Vec<f64>>,
    /// Knot vector.
    pub knots: Vec<f64>,
}

/// A sketch text.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchText {
    /// Node index.
    pub index: usize,
    /// The formatted text ("\\F6;Hello").
    pub text: Option<String>,
    /// Anchor point.
    pub anchor: [f64; 3],
    /// Two opposite corners of the text box.
    pub box_corners: [[f64; 3]; 2],
}

/// A sketch pattern (circular or rectangular).
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPattern {
    /// Node index of the pattern feature.
    pub index: usize,
    /// Browser name.
    pub name: Option<String>,
    /// "circular" or "rectangular".
    pub pattern: &'static str,
    /// Number of entities in each occurrence, in occurrence order.
    pub occurrences: Vec<usize>,
}

/// A geometric constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchConstraint {
    /// Node index.
    pub index: usize,
    /// The constraint ("coincident", "tangent", ...).
    pub constraint: &'static str,
    /// Constrained nodes, in stored order.
    pub entities: Vec<usize>,
}

/// A dimension: a constraint whose value is a parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchDimension {
    /// Node index.
    pub index: usize,
    /// The dimension ("distance dimension", "radius dimension", ...).
    pub dimension: &'static str,
    /// Dimensioned nodes, in stored order.
    pub entities: Vec<usize>,
    /// The parameter holding its value.
    pub parameter: Option<ParamValue>,
    /// True for a driven (reference) dimension.
    pub driven: bool,
}
