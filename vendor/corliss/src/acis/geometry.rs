//! Curves, surfaces and B-splines of an ACIS body (spec 13.6.7–13.6.8). Lengths in cm.

use super::cursor::Interval;

/// A B-spline curve block (`bs3_curve` / `bs2_curve`, spec Table 13-24).
#[derive(Debug, Clone, PartialEq)]
pub struct BsCurve {
    /// True for "nurbs" (weights present), false for "nubs".
    pub rational: bool,
    /// Degree.
    pub degree: usize,
    /// 0 open, 1 closed, 2 periodic.
    pub closure: i32,
    /// Distinct knot values, increasing.
    pub knots: Vec<f64>,
    /// Multiplicity of each knot (end multiplicities are `degree`).
    pub mults: Vec<usize>,
    /// Control points: x y z for bs3, u v for bs2.
    pub poles: Vec<Vec<f64>>,
    /// Weights when rational.
    pub weights: Option<Vec<f64>>,
}

impl BsCurve {
    /// The full clamped knot vector: the stored knots expanded by multiplicity, with the
    /// first and last knot repeated once more (spec 13.6.8).
    pub fn knot_vector(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for (k, m) in self.knots.iter().zip(&self.mults) {
            out.extend(std::iter::repeat_n(*k, *m));
        }
        if let (Some(first), Some(last)) = (self.knots.first(), self.knots.last()) {
            out.insert(0, *first);
            out.push(*last);
        }
        out
    }
}

/// A B-spline surface block (`bs3_surface`, spec Table 13-25).
#[derive(Debug, Clone, PartialEq)]
pub struct BsSurface {
    /// Rational directions ("both", "u", "v"), `None` for "nubs".
    pub rational: Option<String>,
    /// Degree in u.
    pub degree_u: usize,
    /// Degree in v.
    pub degree_v: usize,
    /// u closure: 0 open, 1 closed, 2 periodic.
    pub closure_u: i32,
    /// v closure.
    pub closure_v: i32,
    /// u singularity code.
    pub singularity_u: i32,
    /// v singularity code.
    pub singularity_v: i32,
    /// Distinct u knots.
    pub knots_u: Vec<f64>,
    /// u multiplicities.
    pub mults_u: Vec<usize>,
    /// Distinct v knots.
    pub knots_v: Vec<f64>,
    /// v multiplicities.
    pub mults_v: Vec<usize>,
    /// Control points per u.
    pub count_u: usize,
    /// Control points per v.
    pub count_v: usize,
    /// Control points, v outer, u inner: `poles[v * count_u + u]`.
    pub poles: Vec<[f64; 3]>,
    /// Weights in the same order, when rational.
    pub weights: Option<Vec<f64>>,
}

/// The circle or ellipse data shared by ellipse curves and cone bases (Table 13-20).
#[derive(Debug, Clone, PartialEq)]
pub struct EllipseData {
    /// Centre.
    pub centre: [f64; 3],
    /// Unit normal of the ellipse plane.
    pub normal: [f64; 3],
    /// Major axis; its length is the major radius.
    pub major_axis: [f64; 3],
    /// Minor / major radius.
    pub radius_ratio: f64,
    /// Parameter range (radians from the major axis).
    pub range: Interval,
}

/// Helix data of `helix_int_cur` and `helix_spl_line` subtypes.
#[derive(Debug, Clone, PartialEq)]
pub struct Helix {
    /// Parameter range.
    pub param_range: Interval,
    /// A point of the axis.
    pub axis_root: [f64; 3],
    /// Major axis (radius direction at parameter 0).
    pub major_axis: [f64; 3],
    /// Minor axis.
    pub minor_axis: [f64; 3],
    /// Advance per turn.
    pub pitch_vector: [f64; 3],
    /// Taper.
    pub taper: f64,
    /// Axis direction.
    pub axis_direction: [f64; 3],
}

/// The definition object of an `intcurve` (int_cur family, Tables 13-26/13-27).
#[derive(Debug, Clone, PartialEq)]
pub struct CurveDefinition {
    /// Subtype name (`exact_int_cur`, `int_int_cur` …).
    pub subtype: String,
    /// The approximating 3D B-spline, absent for helices and undecoded subtypes.
    pub approximation: Option<BsCurve>,
    /// Fit tolerance of the approximation (cm).
    pub fit_tolerance: Option<f64>,
    /// Support surfaces.
    pub surfaces: [Option<Box<Surface>>; 2],
    /// Parameter curves on the support surfaces.
    pub pcurves: [Option<BsCurve>; 2],
    /// Helix data of `helix_int_cur`.
    pub helix: Option<Helix>,
}

/// The definition object of a `spline` surface (spl_sur family, Tables 13-28/13-29).
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceDefinition {
    /// Subtype name (`exact_spl_sur`, `loft_spl_sur` …).
    pub subtype: String,
    /// The approximating B-spline surface, when the common part is found.
    pub approximation: Option<BsSurface>,
    /// Fit tolerance of the approximation (cm).
    pub fit_tolerance: Option<f64>,
    /// Helix and ruling direction of a `helix_spl_line` (a surface swept by a line along a helix).
    pub helix: Option<(Helix, [f64; 3])>,
}

/// A curve.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// A line: point at parameter t = `root + t · direction`.
    Straight {
        /// Point at parameter 0.
        root: [f64; 3],
        /// Unit direction.
        direction: [f64; 3],
        /// Parameter range.
        range: Interval,
    },
    /// A circle or ellipse.
    Ellipse(EllipseData),
    /// A procedural curve with its definition.
    Intcurve {
        /// Sense against the definition.
        reversed: bool,
        /// The definition, `None` for a `null` subtype.
        definition: Option<Box<CurveDefinition>>,
        /// Parameter range.
        range: Interval,
    },
    /// No geometry.
    Null,
}

/// A surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// A plane.
    Plane {
        /// A point of the plane.
        root: [f64; 3],
        /// Unit normal.
        normal: [f64; 3],
        /// u parameter direction.
        u_direction: [f64; 3],
        /// True for "reverse_v".
        reverse_v: bool,
        /// u range.
        u_range: Interval,
        /// v range.
        v_range: Interval,
    },
    /// A cone; a cylinder when `sine_angle` is 0.
    Cone {
        /// Base ellipse.
        base: EllipseData,
        /// Sine of the half angle.
        sine_angle: f64,
        /// Cosine of the half angle.
        cosine_angle: f64,
        /// u parameter scale.
        u_param_scale: f64,
        /// True when u is reversed.
        reversed_u: bool,
        /// u range.
        u_range: Interval,
        /// v range.
        v_range: Interval,
    },
    /// A sphere.
    Sphere {
        /// Centre.
        centre: [f64; 3],
        /// Radius (negative: inside-out).
        radius: f64,
        /// Direction of u = 0 on the equator.
        uv_origin_dir: [f64; 3],
        /// Pole direction.
        pole_dir: [f64; 3],
        /// True for "reverse_v".
        reverse_v: bool,
        /// u range.
        u_range: Interval,
        /// v range.
        v_range: Interval,
    },
    /// A torus.
    Torus {
        /// Centre.
        centre: [f64; 3],
        /// Axis normal.
        normal: [f64; 3],
        /// Major radius.
        major_radius: f64,
        /// Minor radius.
        minor_radius: f64,
        /// Direction of u = 0.
        uv_origin_dir: [f64; 3],
        /// True for "reverse_v".
        reverse_v: bool,
        /// u range.
        u_range: Interval,
        /// v range.
        v_range: Interval,
    },
    /// A procedural or B-spline surface.
    Spline {
        /// Sense against the definition.
        reversed: bool,
        /// The definition, `None` for a `null` subtype.
        definition: Option<Box<SurfaceDefinition>>,
        /// u range.
        u_range: Interval,
        /// v range.
        v_range: Interval,
    },
    /// No geometry.
    Null,
}

impl Surface {
    /// The surface kind as ACIS names it ("plane", "cone" …).
    pub fn kind(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "plane",
            Surface::Cone { .. } => "cone",
            Surface::Sphere { .. } => "sphere",
            Surface::Torus { .. } => "torus",
            Surface::Spline { .. } => "spline",
            Surface::Null => "null_surface",
        }
    }
}

impl Curve {
    /// The curve kind as ACIS names it ("straight", "ellipse" …).
    pub fn kind(&self) -> &'static str {
        match self {
            Curve::Straight { .. } => "straight",
            Curve::Ellipse(_) => "ellipse",
            Curve::Intcurve { .. } => "intcurve",
            Curve::Null => "null_curve",
        }
    }
}

/// A parameter-space curve of a coedge (spec Table 13-23).
#[derive(Debug, Clone, PartialEq)]
pub enum PCurve {
    /// Its own `exp_par_cur` definition.
    Own {
        /// Sense.
        reversed: bool,
        /// The 2D B-spline (u, v), when decoded.
        bs2: Option<BsCurve>,
        /// Fit tolerance.
        fit_tolerance: Option<f64>,
        /// The surface it lies on.
        surface: Option<Box<Surface>>,
    },
    /// Parameter curve 1 or 2 of an intcurve record (sign = sense).
    OfIntcurve {
        /// `def_type`: ±1 or ±2.
        which: i32,
        /// The intcurve record index.
        intcurve: i32,
    },
}
