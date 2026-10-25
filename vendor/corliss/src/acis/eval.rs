//! Evaluation of ACIS curves and surfaces: points on curves, distances to surfaces.

use super::geometry::{BsCurve, Curve, EllipseData, Surface};

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// The point of an ellipse at angle `t` (radians from the major axis).
pub fn ellipse_point(e: &EllipseData, t: f64) -> [f64; 3] {
    let minor = scale(cross(e.normal, e.major_axis), e.radius_ratio);
    add(
        e.centre,
        add(scale(e.major_axis, t.cos()), scale(minor, t.sin())),
    )
}

/// The point of a curve at parameter `t`; intcurves use their approximating B-spline.
///
/// ```
/// use corliss::acis::{Curve, Interval, eval::curve_point};
/// let line = Curve::Straight { root: [1.0, 0.0, 0.0], direction: [0.0, 1.0, 0.0], range: Interval::default() };
/// assert_eq!(curve_point(&line, 2.0), Some([1.0, 2.0, 0.0]));
/// ```
pub fn curve_point(c: &Curve, t: f64) -> Option<[f64; 3]> {
    match c {
        Curve::Straight {
            root, direction, ..
        } => Some(add(*root, scale(*direction, t))),
        Curve::Ellipse(e) => Some(ellipse_point(e, t)),
        Curve::Intcurve {
            reversed,
            definition,
            ..
        } => {
            let bs = definition.as_ref()?.approximation.as_ref()?;
            let p = nurbs_point(bs, if *reversed { -t } else { t })?;
            Some([p[0], p[1], *p.get(2)?])
        }
        Curve::Null => None,
    }
}

/// Evaluates a B-spline curve at `t` with de Boor's algorithm; `t` is clamped to the knot range.
pub fn nurbs_point(bs: &BsCurve, t: f64) -> Option<Vec<f64>> {
    let p = bs.degree;
    let knots = bs.knot_vector();
    let n = bs.poles.len();
    if n == 0 || knots.len() < n + p + 1 {
        return None;
    }
    let t = t.clamp(knots[p], knots[n]);
    let span = (p..n).rev().find(|k| knots[*k] <= t).unwrap_or(p);
    let dim = bs.poles[0].len();
    let w = |k: usize| {
        bs.weights
            .as_ref()
            .and_then(|w| w.get(k))
            .copied()
            .unwrap_or(1.0)
    };
    let mut d: Vec<Vec<f64>> = (0..=p)
        .map(|j| {
            let k = span - p + j;
            let mut h: Vec<f64> = bs.poles[k].iter().map(|x| x * w(k)).collect();
            h.push(w(k));
            h
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = span - p + j;
            let denom = knots[i + p + 1 - r] - knots[i];
            let a = if denom == 0.0 {
                0.0
            } else {
                (t - knots[i]) / denom
            };
            d[j] = d[j]
                .iter()
                .zip(&d[j - 1])
                .map(|(x, y)| a * x + (1.0 - a) * y)
                .collect();
        }
    }
    let h = &d[p];
    let wt = h[dim];
    Some(h[..dim].iter().map(|x| x / wt).collect())
}

/// The distance from `p` to an analytic surface; `None` for splines and null surfaces.
pub fn surface_distance(s: &Surface, p: [f64; 3]) -> Option<f64> {
    match s {
        Surface::Plane { root, normal, .. } => {
            Some(dot(sub(p, *root), *normal).abs() / norm(*normal))
        }
        Surface::Cone {
            base,
            sine_angle,
            cosine_angle,
            ..
        } => Some(cone_distance(base, *sine_angle, *cosine_angle, p)),
        Surface::Sphere { centre, radius, .. } => {
            Some((norm(sub(p, *centre)) - radius.abs()).abs())
        }
        Surface::Torus {
            centre,
            normal,
            major_radius,
            minor_radius,
            ..
        } => {
            let q = sub(p, *centre);
            let n = scale(*normal, 1.0 / norm(*normal));
            let h = dot(q, n);
            let rho = norm(sub(q, scale(n, h)));
            Some(((rho - major_radius).hypot(h) - minor_radius.abs()).abs())
        }
        Surface::Spline { .. } | Surface::Null => None,
    }
}

/// Distance to a circular cone or cylinder whose base circle is `base` (spec Table 13-22).
fn cone_distance(base: &EllipseData, sine: f64, cosine: f64, p: [f64; 3]) -> f64 {
    let axis = scale(base.normal, 1.0 / norm(base.normal));
    let q = sub(p, base.centre);
    let h = dot(q, axis);
    let rho = norm(sub(q, scale(axis, h)));
    let r0 = norm(base.major_axis);
    if sine == 0.0 {
        return (rho - r0).abs();
    }
    // The generator through the base circle leans by the half angle: r(h) = r0 + h·tan.
    let radius = r0 + h * sine / cosine;
    ((rho - radius) * cosine).abs()
}
