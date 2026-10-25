//! The Corliss part model as this import's intermediate: parameters, sketches and features in
//! SolidCraft's units (millimetres, degrees), in timeline order.

use std::collections::BTreeMap;

use corliss::model::part::{Feature, FeatureProfile, Part, Sketch};

use solidcraft_doc::sketch::{Constraint, ConstraintKind, Curve, CurveKind, SPoint, Sketch as OurSketch};
use solidcraft_doc::{AxisRef, Dir, Direction, Expr, Extent, Operation, Param, Position};
use solidcraft_geom::{Plane, Vec2, Vec3};

/// Inventor's database length unit, in millimetres.
const CM: f64 = 10.0;
/// Radians to degrees.
const RAD: f64 = 180.0 / std::f64::consts::PI;
/// Sketch positions are matched (and deduplicated) to this much of a millimetre.
const POINT_TOL: f64 = 1e-4;

/// The part as a design: its parameters, its sketches and its features, in timeline order.
pub(crate) struct InvModel {
    pub params: Vec<Param>,
    pub sketches: Vec<BuiltSketch>,
    pub features: Vec<InvFeature>,
    pub warnings: Vec<String>,
}

/// A sketch built for the part: its placement and its SolidCraft sketch, with the file's
/// entity ids resolved into it.
#[derive(Debug)]
pub(crate) struct BuiltSketch {
    pub name: Option<String>,
    pub plane: Plane,
    pub sketch: OurSketch,
    /// `pointAI` → point of `sketch` (the sketch's point nodes, in creation order).
    point_ais: BTreeMap<i64, usize>,
}

#[derive(Debug)]
pub(crate) enum InvFeature {
    Extrude(InvExtrude),
    Revolve(InvRevolve),
    Unsupported { name: Option<String>, kind: &'static str },
}

#[derive(Debug)]
pub(crate) struct InvExtrude {
    pub name: Option<String>,
    /// The profile's sketch, by name.
    pub sketch: Option<String>,
    /// The profile loops, as curve ids of that sketch.
    pub loops: Vec<Vec<String>>,
    pub operation: Operation,
    pub extent: Extent,
}

#[derive(Debug)]
pub(crate) struct InvRevolve {
    pub name: Option<String>,
    pub sketch: Option<String>,
    pub loops: Vec<Vec<String>>,
    pub operation: Operation,
    pub axis: AxisRef,
    pub angle: Expr,
    pub symmetric: bool,
}

/// The sketch's point nodes (centimetres), whose block order is the `pointAI` numbering the
/// file's profile entities use.
const T_POINT2D: u32 = 0xCE52_DF35;
/// The profile entity nodes: `entityAI` (what the profile's loops name) and `typEntity` (2 a
/// line, its endpoints as `point1AI` / `point2AI`).
const T_PROFILE_ENTITY: u32 = 0x9087_4D13;

/// What an Inventor sketch entity is, in the SolidCraft sketch built for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ent {
    /// A point of the sketch (`Sketch::points`).
    Point(usize),
    /// A curve of the sketch (`Sketch::curves`).
    Curve(usize),
}

/// Which unit a parameter carries.
#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Mm,
    Deg,
}

impl Unit {
    fn label(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Deg => "deg",
        }
    }
}

fn num(v: f64) -> String {
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    format!("{v}")
}

/// The value in `unit`'s SolidCraft unit.
fn convert(v: f64, unit: Unit) -> f64 {
    match unit {
        Unit::Mm => v * CM,
        Unit::Deg => v * RAD,
    }
}

/// A feature or dimension's parameter reference: the named parameter when the file drives it,
/// else the value (in `unit`), else nothing.
fn expr_of(name: Option<&str>, value: Option<f64>, unit: Unit) -> Option<Expr> {
    match (name, value) {
        (Some(n), _) => Some(Expr::new(n.to_string())),
        (None, Some(v)) => Some(Expr::new(num(convert(v, unit)))),
        (None, None) => None,
    }
}

pub(crate) fn read(part: &Part, file: &str) -> Result<InvModel, String> {
    let mut warnings: Vec<String> = Vec::new();
    let parameters = part.parameters();
    let sketches = part.sketches();
    let features = part.features();

    // Which unit each parameter carries: its length or angle uses decide; a parameter used both
    // ways is kept as a length, with a note.
    let mut units: BTreeMap<String, Unit> = BTreeMap::new();
    let use_unit = |units: &mut BTreeMap<String, Unit>, name: Option<&str>, unit: Unit, warnings: &mut Vec<String>| {
        let Some(name) = name else { return };
        match units.get(name) {
            Some(u) if *u != unit => {
                warnings.push(format!("{file}: parameter `{name}` is used as both a length and an angle; it is kept as a length"))
            }
            _ => {
                units.insert(name.to_string(), unit);
            }
        }
    };
    for f in &features {
        match &f.kind {
            corliss::model::part::FeatureKind::Extrusion(e) => {
                use_unit(&mut units, e.distance.as_ref().and_then(|d| d.name.as_deref()), Unit::Mm, &mut warnings);
                use_unit(&mut units, e.taper.as_ref().and_then(|d| d.name.as_deref()), Unit::Deg, &mut warnings);
            }
            corliss::model::part::FeatureKind::Revolve(r) => {
                use_unit(&mut units, r.angle.as_ref().and_then(|d| d.name.as_deref()), Unit::Deg, &mut warnings);
            }
            _ => {}
        }
    }
    for sk in &sketches {
        for d in &sk.dimensions {
            let unit = match d.dimension {
                "angle dimension" | "three-point angle dimension" => Unit::Deg,
                _ => Unit::Mm,
            };
            use_unit(&mut units, d.parameter.as_ref().and_then(|p| p.name.as_deref()), unit, &mut warnings);
        }
    }

    // The parameter table: names and expressions in SolidCraft's units.
    let mut params: Vec<Param> = Vec::new();
    for p in &parameters {
        let unit = units.get(&p.name).copied().unwrap_or(Unit::Mm);
        // An equation that names another parameter is the import's own expression (its
        // coefficients are unitless); a bare number is converted to the import's unit.
        let names = p
            .equation
            .as_deref()
            .map(|e| {
                e.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .filter(|t| !t.is_empty())
                    .any(|t| parameters.iter().any(|q| q.name == t && q.name != p.name))
            })
            .unwrap_or(false);
        let expr = if names {
            p.equation.clone().unwrap_or_else(|| num(convert(p.value.unwrap_or(0.0), unit)))
        } else {
            num(convert(p.value.unwrap_or(0.0), unit))
        };
        params.push(Param { name: p.name.clone(), expr, unit: unit.label().to_string(), comment: String::new(), model: !p.user });
    }

    let mut built: Vec<BuiltSketch> = Vec::new();
    for sk in &sketches {
        built.push(build_sketch(sk, part, file, &mut warnings));
    }

    let mut inv_features: Vec<InvFeature> = Vec::new();
    for f in &features {
        inv_features.push(build_feature(part, f, &built, file, &mut warnings));
    }

    Ok(InvModel { params, sketches: built, features: inv_features, warnings })
}

/// The sketch's placement, from its sketch-to-model matrix (centimetres) or the XY plane.
fn sketch_plane(sk: &Sketch) -> Plane {
    let Some(m) = &sk.matrix else { return Plane::XY };
    let origin = Vec3::new(CM * m[0][3], CM * m[1][3], CM * m[2][3]);
    let x = Vec3::new(m[0][0], m[0][1], m[0][2]);
    let y = Vec3::new(m[1][0], m[1][1], m[1][2]);
    Plane::new(origin, x, y).unwrap_or(Plane::XY)
}

fn build_sketch(sk: &Sketch, part: &Part, file: &str, warnings: &mut Vec<String>) -> BuiltSketch {
    let plane = sketch_plane(sk);

    let mut points: Vec<Vec2> = Vec::new();
    let mut add_point = |p: Vec2| -> usize {
        for (i, q) in points.iter().enumerate() {
            if (q.x - p.x).abs() <= POINT_TOL && (q.y - p.y).abs() <= POINT_TOL {
                return i;
            }
        }
        points.push(p);
        points.len() - 1
    };
    let v2 = |p: [f64; 2]| Vec2::new(CM * p[0], CM * p[1]);

    let mut entities: BTreeMap<i64, Ent> = BTreeMap::new();
    for p in &sk.points {
        if p.projected {
            continue;
        }
        let i = add_point(v2(p.pos));
        entities.insert(p.index as i64, Ent::Point(i));
    }

    let mut curves: Vec<Curve> = Vec::new();
    let mut ids: BTreeMap<&'static str, usize> = BTreeMap::new();
    let add_curve = |curves: &mut Vec<Curve>, ids: &mut BTreeMap<&'static str, usize>, prefix: &'static str, kind: CurveKind| -> usize {
        let n = ids.get(prefix).copied().unwrap_or(0) + 1;
        ids.insert(prefix, n);
        let id = format!("{prefix}{n}");
        curves.push(Curve { id, kind, construction: false, reversed: false, link: None, centerline: false, fixed: false, infinite: false });
        curves.len() - 1
    };

    for l in &sk.lines {
        let a = add_point(v2(l.start));
        let b = add_point(v2(l.end));
        let j = add_curve(&mut curves, &mut ids, "l", CurveKind::Line { a, b });
        curves[j].construction = l.reference;
        entities.insert(l.index as i64, Ent::Curve(j));
    }
    for c in &sk.circles {
        let i = add_point(v2(c.center));
        let r = c.radius.unwrap_or(1.0);
        let j = add_curve(&mut curves, &mut ids, "c", CurveKind::Circle { c: i, r: CM * r });
        let _ = j;
        entities.insert(c.index as i64, Ent::Curve(j));
    }
    for a in &sk.arcs {
        let i = add_point(v2(a.center));
        let sa = add_point(v2(a.start));
        let sb = add_point(v2(a.end));
        let j = add_curve(&mut curves, &mut ids, "a", CurveKind::Arc { c: i, a: sa, b: sb });
        curves[j].reversed = a.sweep < 0.0;
        entities.insert(a.index as i64, Ent::Curve(j));
    }
    for e in &sk.ellipses {
        let c = Vec2::new(CM * e.center[0], CM * e.center[1]);
        let i = add_point(c);
        let major = e.major_radius.unwrap_or(1.0);
        let m = add_point(c + Vec2::new(CM * e.major_dir[0] * major, CM * e.major_dir[1] * major));
        let j = add_curve(&mut curves, &mut ids, "e", CurveKind::Ellipse { c: i, m, r: CM * e.minor_radius.unwrap_or(1.0) });
        let _ = j;
        entities.insert(e.index as i64, Ent::Curve(j));
    }
    for s in &sk.splines {
        let pts: Vec<usize> = s.fit_points.iter().map(|p| add_point(v2(*p))).collect();
        if pts.len() < 2 {
            continue;
        }
        let j = add_curve(&mut curves, &mut ids, "s", CurveKind::Spline { pts, control: false, degree: 3, periodic: false });
        let _ = j;
        entities.insert(s.index as i64, Ent::Curve(j));
    }

    let spoints: Vec<SPoint> =
        points.iter().enumerate().map(|(i, p)| SPoint { id: format!("p{}", i + 1), pos: *p, fixed: false, link: None, exprs: None }).collect();

    let mut constraints: Vec<Constraint> = Vec::new();
    let mut k = 0;
    let mut add_constraint = |constraints: &mut Vec<Constraint>, kind: ConstraintKind, param: Option<String>, driven: bool| {
        k += 1;
        constraints.push(Constraint { id: format!("k{k}"), kind, param, driven, text: None });
    };
    let of = |i: &usize| entities.get(&(*i as i64)).copied();
    let curve_of = |e: Ent| match e {
        Ent::Curve(j) => Some(j),
        Ent::Point(_) => None,
    };

    for c in &sk.constraints {
        if c.entities.is_empty() {
            continue;
        }
        let Some(a) = of(&c.entities[0]) else {
            warnings.push(format!("{file}: a `{}` constraint references an entity the import does not hold; it is not imported", c.constraint));
            continue;
        };
        let b = c.entities.get(1).and_then(of);
        match c.constraint {
            "coincident" => match (a, b) {
                (Ent::Point(p1), Some(Ent::Point(p2))) => add_constraint(&mut constraints, ConstraintKind::Coincident { p: p1, q: p2 }, None, false),
                (Ent::Point(p), Some(Ent::Curve(j))) | (Ent::Curve(j), Some(Ent::Point(p))) => {
                    // A point at the end of a line is the endpoint, not a point on it.
                    let end = match &curves[j].kind {
                        CurveKind::Line { a: pa, b: pb } if points[*pa].dist(points[p]) <= POINT_TOL => Some(*pa),
                        CurveKind::Line { a: _, b: pb } if points[*pb].dist(points[p]) <= POINT_TOL => Some(*pb),
                        _ => None,
                    };
                    match end {
                        Some(q) if q != p => add_constraint(&mut constraints, ConstraintKind::Coincident { p, q }, None, false),
                        Some(_) => {} // the point is the line's own end: nothing to constrain
                        None => add_constraint(&mut constraints, ConstraintKind::PointOnCurve { p, c: j }, None, false),
                    }
                }
                (Ent::Curve(j1), Some(Ent::Curve(j2))) => {
                    // The nearest ends of two lines, where they meet.
                    let ends = |j: usize| match &curves[j].kind {
                        CurveKind::Line { a, b } => Some((*a, *b)),
                        _ => None,
                    };
                    let placed = ends(j1).zip(ends(j2)).and_then(|((a1, b1), (a2, b2))| {
                        let mut best: Option<(usize, usize, f64)> = None;
                        for (p, q) in [(a1, a2), (a1, b2), (b1, a2), (b1, b2)] {
                            let d = points[p].dist(points[q]);
                            if d <= POINT_TOL && best.is_none_or(|(_, _, bd)| d < bd) {
                                best = Some((p, q, d));
                            }
                        }
                        best
                    });
                    match placed {
                        Some((p, q, _)) => add_constraint(&mut constraints, ConstraintKind::Coincident { p, q }, None, false),
                        None => {
                            warnings
                                .push(format!("{file}: a `coincident` constraint between two curves the import cannot place; it is not imported"));
                        }
                    }
                }
                _ => {
                    warnings.push(format!("{file}: a `coincident` constraint the import cannot place; it is not imported"));
                }
            },
            "parallel" | "perpendicular" | "tangent" | "collinear" | "equal length" | "equal radius" | "concentric" | "smooth (G2)" => {
                match (curve_of(a), b.and_then(curve_of)) {
                    (Some(j1), Some(j2)) => {
                        let kind = match c.constraint {
                            "parallel" => ConstraintKind::Parallel { a: j1, b: j2 },
                            "perpendicular" => ConstraintKind::Perpendicular { a: j1, b: j2 },
                            "tangent" => ConstraintKind::Tangent { a: j1, b: j2 },
                            "collinear" => ConstraintKind::Collinear { a: j1, b: j2 },
                            "equal length" | "equal radius" => ConstraintKind::Equal { a: j1, b: j2 },
                            "concentric" => ConstraintKind::Concentric { a: j1, b: j2 },
                            _ => ConstraintKind::Smooth { a: j1, b: j2 },
                        };
                        add_constraint(&mut constraints, kind, None, false);
                    }
                    _ => {
                        warnings.push(format!("{file}: a `{}` constraint the import cannot place; it is not imported", c.constraint));
                    }
                }
            }
            "horizontal" => match a {
                Ent::Curve(j) => add_constraint(&mut constraints, ConstraintKind::Horizontal { l: j }, None, false),
                _ => warnings.push(format!("{file}: a `horizontal` constraint the import cannot place; it is not imported")),
            },
            "vertical" => match a {
                Ent::Curve(j) => add_constraint(&mut constraints, ConstraintKind::Vertical { l: j }, None, false),
                _ => warnings.push(format!("{file}: a `vertical` constraint the import cannot place; it is not imported")),
            },
            "fix" => match a {
                Ent::Point(p) => add_constraint(&mut constraints, ConstraintKind::Fix { p }, None, false),
                _ => warnings.push(format!("{file}: a `fix` constraint the import cannot place; it is not imported")),
            },
            "horizontal alignment" => match (a, b) {
                (Ent::Point(p1), Some(Ent::Point(p2))) => {
                    add_constraint(&mut constraints, ConstraintKind::HorizontalPoints { p: p1, q: p2 }, None, false)
                }
                _ => warnings.push(format!("{file}: a `horizontal alignment` constraint the import cannot place; it is not imported")),
            },
            "vertical alignment" => match (a, b) {
                (Ent::Point(p1), Some(Ent::Point(p2))) => {
                    add_constraint(&mut constraints, ConstraintKind::VerticalPoints { p: p1, q: p2 }, None, false)
                }
                _ => warnings.push(format!("{file}: a `vertical alignment` constraint the import cannot place; it is not imported")),
            },
            "midpoint" => match (a, b) {
                (Ent::Point(p), Some(Ent::Curve(j))) | (Ent::Curve(j), Some(Ent::Point(p))) => {
                    add_constraint(&mut constraints, ConstraintKind::Midpoint { p, l: j }, None, false)
                }
                _ => warnings.push(format!("{file}: a `midpoint` constraint the import cannot place; it is not imported")),
            },
            "symmetric" => match (a, b, c.entities.get(2).and_then(of)) {
                (Ent::Point(p1), Some(Ent::Point(p2)), Some(Ent::Curve(j))) => {
                    add_constraint(&mut constraints, ConstraintKind::Symmetric { p: p1, q: p2, l: j }, None, false)
                }
                _ => warnings.push(format!("{file}: a `symmetric` constraint the import cannot place; it is not imported")),
            },
            other => {
                warnings.push(format!(
                    "{file}: constraint `{other}` is not imported (the part is unchanged; the sketch may need it to be fully constrained)"
                ));
            }
        }
    }

    for d in &sk.dimensions {
        let unit = match d.dimension {
            "angle dimension" | "three-point angle dimension" => Unit::Deg,
            _ => Unit::Mm,
        };
        let driven_param = if d.driven { None } else { d.parameter.as_ref().and_then(|p| p.name.clone()) };
        let value = d.parameter.as_ref().and_then(|p| p.value).map(|v| convert(v, unit));
        let kind = match value {
            Some(v) => dimension_kind(d, &entities, &curves, v),
            None => {
                let computed = dimension_kind(d, &entities, &curves, 0.0);
                match computed {
                    Some(kind) => {
                        let v = kind_value(&kind, &points, &curves);
                        Some(revalue(kind, v))
                    }
                    None => None,
                }
            }
        };
        let Some(kind) = kind else {
            warnings.push(format!("{file}: dimension `{}` is not imported; the sketch keeps the position its geometry came from", d.dimension));
            continue;
        };
        add_constraint(&mut constraints, kind, driven_param, d.driven);
    }

    let mut sketch = OurSketch::default();
    sketch.points = spoints;
    sketch.curves = curves;
    sketch.constraints = constraints;

    // `pointAI` numbering: the sketch's point nodes in creation order (1-based).
    let mut point_ais: BTreeMap<i64, usize> = BTreeMap::new();
    for (i, (_, n)) in part.view().by_type(T_POINT2D).filter(|(_, n)| corliss::model::view::node_ref(n, "sketch") == Some(sk.index)).enumerate() {
        let mut pos = n.fields.iter().filter(|f| f.name.as_deref() == Some("pos")).filter_map(|f| f.value.as_f64());
        let (x, y) = match (pos.next(), pos.next()) {
            (Some(x), Some(y)) => (x, y),
            _ => continue,
        };
        let p = Vec2::new(CM * x, CM * y);
        if let Some(j) = points.iter().position(|q| (q.x - p.x).abs() <= POINT_TOL && (q.y - p.y).abs() <= POINT_TOL) {
            // The `pointAI`s of the file's profile entities are this numbering.
            point_ais.insert(i as i64 + 1, j);
        }
    }

    BuiltSketch { name: sk.name.clone(), plane, sketch, point_ais }
}

/// The curve of `sketch` a profile entity names: a line (typEntity 2) by its endpoints'
/// `pointAI`s.
fn profile_curve_id(part: &Part, ai: i64, point_ais: &BTreeMap<i64, usize>, curves: &[Curve]) -> Option<String> {
    let (_, node) = part.view().by_type(T_PROFILE_ENTITY).find(|(_, n)| n.int("entityAI") == Some(ai))?;
    match node.int("typEntity") {
        Some(2) => {
            let a = node.int("point1AI")?;
            let b = node.int("point2AI")?;
            let pa = *point_ais.get(&a)?;
            let pb = *point_ais.get(&b)?;
            curves
                .iter()
                .find(|c| matches!(&c.kind, CurveKind::Line { a: x, b: y } if (*x == pa && *y == pb) || (*x == pb && *y == pa)))
                .map(|c| c.id.clone())
        }
        _ => None,
    }
}

/// The constraint kind a dimension measures, with its value already in millimetres or degrees.
fn dimension_kind(d: &corliss::model::part::SketchDimension, entities: &BTreeMap<i64, Ent>, curves: &[Curve], value: f64) -> Option<ConstraintKind> {
    let ent = |i: &usize| entities.get(&(*i as i64)).copied();
    let curve = |e: Ent| match e {
        Ent::Curve(j) => Some(j),
        Ent::Point(_) => None,
    };
    let e1 = d.entities.first().and_then(ent)?;
    let e2 = d.entities.get(1).and_then(ent);
    match d.dimension {
        "radius dimension" => {
            let c = curve(e1)?;
            Some(ConstraintKind::Radius { c, value })
        }
        "diameter dimension" => {
            let c = curve(e1)?;
            Some(ConstraintKind::Diameter { c, value })
        }
        "ellipse major radius dimension" => {
            let c = curve(e1)?;
            match &curves[c].kind {
                CurveKind::Ellipse { c: i, m, .. } => Some(ConstraintKind::Distance { p: *m, q: *i, value }),
                _ => None,
            }
        }
        "distance dimension" => match (e1, e2) {
            (Ent::Point(p1), Some(Ent::Point(p2))) => Some(ConstraintKind::Distance { p: p1, q: p2, value }),
            (Ent::Point(p), Some(Ent::Curve(j))) | (Ent::Curve(j), Some(Ent::Point(p))) => Some(ConstraintKind::PointLineDistance { p, l: j, value }),
            _ => None,
        },
        "angle dimension" => {
            let a = curve(e1)?;
            let b = curve(e2?)?;
            Some(ConstraintKind::Angle { a, b, value, flip: false })
        }
        "length dimension" | "overall length dimension" => {
            let l = curve(e1)?;
            Some(ConstraintKind::Length { l, value })
        }
        _ => None,
    }
}

/// The value a dimension constraint carries.
fn kind_value(kind: &ConstraintKind, points: &[Vec2], curves: &[Curve]) -> f64 {
    match kind {
        ConstraintKind::Distance { p, q, .. } => points[*p].dist(points[*q]),
        ConstraintKind::Radius { c, .. } => radius_of(*c, points, curves),
        ConstraintKind::Diameter { c, .. } => 2.0 * radius_of(*c, points, curves),
        ConstraintKind::Length { l, .. } => match &curves[*l].kind {
            CurveKind::Line { a, b } => points[*a].dist(points[*b]),
            _ => 0.0,
        },
        ConstraintKind::Angle { a, b, .. } => angle_between(*a, *b, points, curves),
        ConstraintKind::PointLineDistance { p, l, .. } => point_line_distance(*p, *l, points, curves),
        _ => 0.0,
    }
}

fn revalue(kind: ConstraintKind, value: f64) -> ConstraintKind {
    match kind {
        ConstraintKind::Distance { p, q, .. } => ConstraintKind::Distance { p, q, value },
        ConstraintKind::Radius { c, .. } => ConstraintKind::Radius { c, value },
        ConstraintKind::Diameter { c, .. } => ConstraintKind::Diameter { c, value },
        ConstraintKind::Length { l, .. } => ConstraintKind::Length { l, value },
        ConstraintKind::Angle { a, b, flip, .. } => ConstraintKind::Angle { a, b, value, flip },
        ConstraintKind::PointLineDistance { p, l, .. } => ConstraintKind::PointLineDistance { p, l, value },
        other => other,
    }
}

fn radius_of(c: usize, points: &[Vec2], curves: &[Curve]) -> f64 {
    match &curves[c].kind {
        CurveKind::Circle { r, .. } => *r,
        CurveKind::Arc { c, a, .. } => points[*a].dist(points[*c]),
        _ => 0.0,
    }
}

fn line_dir(l: usize, points: &[Vec2], curves: &[Curve]) -> Option<Vec2> {
    let v = match &curves[l].kind {
        CurveKind::Line { a, b } => points[*b] - points[*a],
        _ => return None,
    };
    v.normalized()
}

fn angle_between(a: usize, b: usize, points: &[Vec2], curves: &[Curve]) -> f64 {
    match (line_dir(a, points, curves), line_dir(b, points, curves)) {
        (Some(da), Some(db)) => f64::atan2(da.cross(db), da.dot(db)),
        _ => 0.0,
    }
}

fn point_line_distance(p: usize, l: usize, points: &[Vec2], curves: &[Curve]) -> f64 {
    let Some(d) = line_dir(l, points, curves) else { return 0.0 };
    let a = match &curves[l].kind {
        CurveKind::Line { a, .. } => *a,
        _ => return 0.0,
    };
    let ap = points[p] - points[a];
    let closest = points[a] + d * (ap.x * d.x + ap.y * d.y);
    points[p].dist(closest)
}

fn build_feature(part: &Part, f: &Feature, built: &[BuiltSketch], file: &str, warnings: &mut Vec<String>) -> InvFeature {
    let unsupported = || InvFeature::Unsupported { name: f.name.clone(), kind: f.kind.type_name() };
    let mut profile_of = |profile: &Option<FeatureProfile>| -> Option<Vec<Vec<String>>> {
        let profile = profile.as_ref()?;
        if profile.loops.is_empty() {
            warnings.push(format!("{file}: feature `{}` has no profile; it is not imported", f.name.as_deref().unwrap_or("?")));
            return None;
        }
        let sketch = built.iter().find(|s| s.name.as_deref() == profile.sketch.as_deref())?;
        profile
            .loops
            .iter()
            .map(|loop_| {
                loop_
                    .iter()
                    .map(|id| id.and_then(|id| profile_curve_id(part, id, &sketch.point_ais, &sketch.sketch.curves)))
                    .collect::<Option<Vec<String>>>()
            })
            .collect::<Option<Vec<_>>>()
    };
    match &f.kind {
        corliss::model::part::FeatureKind::Extrusion(e) => {
            let loops = match profile_of(&e.profile) {
                Some(loops) => loops,
                None => return unsupported(),
            };
            let operation = match operation_of(e.operation.as_ref()) {
                Some(op) => op,
                None => {
                    warnings.push(format!("{file}: feature `{}` is a surface operation; it is not imported", f.name.as_deref().unwrap_or("?")));
                    return unsupported();
                }
            };
            let symmetric = e.symmetric.unwrap_or(false) || matches!(e.extent, Some(corliss::model::part::FeatureExtent::DistanceSymmetric));
            let through = matches!(e.extent, Some(corliss::model::part::FeatureExtent::ThroughAll));
            let direction = if e.flipped.unwrap_or(false) { Direction::Negative } else { Direction::Positive };
            if e.direction.as_ref().is_some_and(|d| d.len() == 3 && d[2] < 0.0) && !symmetric && !through {
                warnings.push(format!(
                    "{file}: feature `{}` extrudes against its sketch normal; the import keeps the normal's side",
                    f.name.as_deref().unwrap_or("?")
                ));
            }
            let distance = expr_of(e.distance.as_ref().and_then(|d| d.name.as_deref()), e.distance.as_ref().and_then(|d| d.value), Unit::Mm)
                .unwrap_or(Expr::new("0"));
            let extent = if through {
                Extent {
                    distance: Expr::new("0"),
                    convention: Default::default(),
                    direction,
                    distance2: None,
                    start_offset: None,
                    through_all: true,
                    taper: None,
                    to: None,
                    to_offset: None,
                    from: None,
                }
            } else {
                let mut extent = Extent {
                    distance,
                    convention: Default::default(),
                    direction,
                    distance2: None,
                    start_offset: None,
                    through_all: false,
                    taper: None,
                    to: None,
                    to_offset: None,
                    from: None,
                };
                if symmetric {
                    extent.direction = Direction::Symmetric;
                }
                if let Some(taper) = e.taper.as_ref().and_then(|t| expr_of(t.name.as_deref(), t.value, Unit::Deg)).filter(|t| t != &Expr::new("0")) {
                    extent.taper = Some(taper);
                }
                if e.extent.as_ref().is_some_and(|ext| {
                    !matches!(ext, corliss::model::part::FeatureExtent::Distance | corliss::model::part::FeatureExtent::DistanceSymmetric)
                }) {
                    warnings.push(format!(
                        "{file}: feature `{}`'s extent is not a plain distance; it is imported as one",
                        f.name.as_deref().unwrap_or("?")
                    ));
                }
                extent
            };
            InvFeature::Extrude(InvExtrude {
                name: f.name.clone(),
                sketch: e.profile.as_ref().and_then(|p| p.sketch.clone()),
                loops,
                operation,
                extent,
            })
        }
        corliss::model::part::FeatureKind::Revolve(r) => {
            let loops = match profile_of(&r.profile) {
                Some(loops) => loops,
                None => return unsupported(),
            };
            let operation = match operation_of(r.base.operation.as_ref()) {
                Some(op) => op,
                None => return unsupported(),
            };
            let axis = match &r.axis {
                Some(a) => AxisRef::Line {
                    origin: Position(Vec3::new(CM * a.pos[0], CM * a.pos[1], CM * a.pos[2])),
                    dir: Dir(Vec3::new(a.dir[0], a.dir[1], a.dir[2])),
                },
                None => return unsupported(),
            };
            let full = matches!(r.extent, Some(corliss::model::part::FeatureRevolveExtent::Full) | None);
            let angle = if full {
                Expr::new("360")
            } else {
                expr_of(r.angle.as_ref().and_then(|a| a.name.as_deref()), r.angle.as_ref().and_then(|a| a.value), Unit::Deg)
                    .unwrap_or(Expr::new("360"))
            };
            let symmetric = matches!(r.direction, Some(corliss::model::part::FeatureRevolveDirection::Symmetric));
            InvFeature::Revolve(InvRevolve {
                name: f.name.clone(),
                sketch: r.profile.as_ref().and_then(|p| p.sketch.clone()),
                loops,
                operation,
                axis,
                angle,
                symmetric,
            })
        }
        _ => unsupported(),
    }
}

fn operation_of(op: Option<&corliss::model::part::FeatureOperation>) -> Option<Operation> {
    match op? {
        corliss::model::part::FeatureOperation::NewSolid => Some(Operation::NewBody),
        corliss::model::part::FeatureOperation::Join => Some(Operation::Join),
        corliss::model::part::FeatureOperation::Cut => Some(Operation::Cut),
        corliss::model::part::FeatureOperation::Intersect => Some(Operation::Intersect),
        _ => None,
    }
}
