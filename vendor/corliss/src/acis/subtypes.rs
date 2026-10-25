//! Subtype definitions and B-spline blocks (spec 13.6.8, Tables 13-24 to 13-29).

use super::cursor::Cursor;
use super::decode::{Decoder, Definition};
use super::geometry::*;
use super::token::Token;
use crate::error::Result;

/// int_cur subtypes whose data starts with the common part (Table 13-27).
const INT_CUR_COMMON: [&str; 9] = [
    "exact_int_cur",
    "int_int_cur",
    "surf_int_cur",
    "par_int_cur",
    "blend_int_cur",
    "off_int_cur",
    "spring_int_cur",
    "off_surf_int_cur",
    "defm_int_cur",
];

/// An int_cur definition: approximation, support surfaces and parameter curves.
pub(crate) fn int_cur(
    d: &Decoder<'_>,
    c: &mut Cursor<'_>,
    name: &str,
    depth: usize,
) -> CurveDefinition {
    let mut def = CurveDefinition {
        subtype: name.to_string(),
        approximation: None,
        fit_tolerance: None,
        surfaces: [None, None],
        pcurves: [None, None],
        helix: None,
    };
    // A layout that does not match leaves the fields read so far; the curve stays usable.
    let _ = if name == "helix_int_cur" {
        helix_int_cur(d, c, &mut def, depth)
    } else if INT_CUR_COMMON.contains(&name) {
        int_cur_common(d, c, &mut def, depth)
    } else {
        Ok(())
    };
    def
}

fn int_cur_common(
    d: &Decoder<'_>,
    c: &mut Cursor<'_>,
    def: &mut CurveDefinition,
    depth: usize,
) -> Result<()> {
    c.long("object_version")?;
    c.enumeration("approx_level")?;
    def.approximation = bs_curve(c, 3)?;
    def.fit_tolerance = Some(c.double("fit_tolerance")?);
    supports(d, c, def, depth)
}

/// Support surfaces and their parameter curves, shared by the common part and helices.
fn supports(
    d: &Decoder<'_>,
    c: &mut Cursor<'_>,
    def: &mut CurveDefinition,
    depth: usize,
) -> Result<()> {
    def.surfaces[0] = Some(Box::new(d.inline_surface(c, depth)?));
    def.surfaces[1] = Some(Box::new(d.inline_surface(c, depth)?));
    def.pcurves[0] = bs_curve(c, 2)?;
    def.pcurves[1] = bs_curve(c, 2)?;
    Ok(())
}

fn helix_int_cur(
    d: &Decoder<'_>,
    c: &mut Cursor<'_>,
    def: &mut CurveDefinition,
    depth: usize,
) -> Result<()> {
    c.long("object_version")?;
    let param_range = c.interval("param_range")?;
    def.helix = Some(helix(c, param_range)?);
    supports(d, c, def, depth)
}

fn helix(c: &mut Cursor<'_>, param_range: super::cursor::Interval) -> Result<Helix> {
    Ok(Helix {
        param_range,
        axis_root: c.position("axis_root")?,
        major_axis: c.vector("major_axis")?,
        minor_axis: c.vector("minor_axis")?,
        pitch_vector: c.vector("pitch_vector")?,
        taper: c.double("taper")?,
        axis_direction: c.vector("axis_direction")?,
    })
}

/// A spl_sur definition: its approximating B-spline surface when the common part is found.
pub(crate) fn spl_sur(
    d: &Decoder<'_>,
    c: &mut Cursor<'_>,
    name: &str,
    depth: usize,
) -> SurfaceDefinition {
    let mut def = SurfaceDefinition {
        subtype: name.to_string(),
        approximation: None,
        fit_tolerance: None,
        helix: None,
    };
    if name == "helix_spl_line" {
        def.helix = helix_spl_line(d, c, depth).ok();
        return def;
    }
    if name == "ortho_spl_sur" {
        let _ = ortho_spl_sur(d, c, &mut def, depth);
        return def;
    }
    let found = if name == "exact_spl_sur" {
        Some(c.i + 1)
    } else {
        find_common(c)
    };
    if let Some(at) = found {
        let mut common = Cursor::at(c.toks, at, c.version);
        let _ = spl_sur_common(&mut common, &mut def);
    }
    def
}

/// `helix_spl_line`: ranges, the helix, its support surfaces and curves, the line direction.
fn helix_spl_line(d: &Decoder<'_>, c: &mut Cursor<'_>, depth: usize) -> Result<(Helix, [f64; 3])> {
    c.long("object_version")?;
    c.interval("u_range")?;
    c.interval("v_range")?;
    let param_range = c.interval("param_range")?;
    let h = helix(c, param_range)?;
    let mut supports_of = CurveDefinition {
        subtype: String::new(),
        approximation: None,
        fit_tolerance: None,
        surfaces: [None, None],
        pcurves: [None, None],
        helix: None,
    };
    supports(d, c, &mut supports_of, depth)?;
    Ok((h, c.vector("line_direction")?))
}

/// `ortho_spl_sur` (Table 13-29): the defining surface and curve, a 2D curve and a scale
/// factor, then the common part; `sense` follows it.
fn ortho_spl_sur(
    d: &Decoder<'_>,
    c: &mut Cursor<'_>,
    def: &mut SurfaceDefinition,
    depth: usize,
) -> Result<()> {
    c.long("object_version")?;
    d.inline_surface(c, depth + 1)?;
    d.inline_curve(c, depth + 1)?;
    bs_curve(c, 2)?;
    c.double("scale_factor")?;
    spl_sur_common(c, def)
}

fn spl_sur_common(c: &mut Cursor<'_>, def: &mut SurfaceDefinition) -> Result<()> {
    c.enumeration("approx_level")?;
    def.approximation = bs_surface(c)?;
    def.fit_tolerance = Some(c.double("fit_tolerance")?);
    Ok(())
}

/// Locates the spl_sur common part after a subtype-specific prefix whose layout is open
/// (OI-196): the last depth-0 `approx_level` enum followed by a B-spline surface block.
fn find_common(c: &Cursor<'_>) -> Option<usize> {
    let end = c.close_of_subtype();
    let mut depth = 0i32;
    let mut best = None;
    for j in c.i..end.saturating_sub(3) {
        match &c.toks[j] {
            Token::SubtypeStart => depth += 1,
            Token::SubtypeEnd => depth -= 1,
            Token::Enum(_) if depth == 0 && starts_surface_block(&c.toks[j + 1..]) => {
                best = Some(j)
            }
            _ => {}
        }
    }
    best
}

fn starts_surface_block(t: &[Token]) -> bool {
    matches!(t, [Token::Ident(f), Token::Long(_), Token::Long(_), ..] if f == "nubs" || f == "nurbs")
}

/// A par_cur definition (`exp_par_cur`: 2D B-spline, tolerance, surface).
pub(crate) fn par_cur(d: &Decoder<'_>, c: &mut Cursor<'_>, name: &str, depth: usize) -> Definition {
    let mut out = Definition::ParCurve {
        bs2: None,
        fit_tolerance: None,
        surface: None,
    };
    if name != "exp_par_cur" {
        return out;
    }
    let read = (|| -> Result<_> {
        let bs2 = bs_curve(c, 2)?;
        let tol = c.double("fit_tolerance")?;
        Ok((bs2, tol, d.inline_surface(c, depth)?))
    })();
    if let Ok((bs2, tol, surface)) = read {
        out = Definition::ParCurve {
            bs2,
            fit_tolerance: Some(tol),
            surface: Some(Box::new(surface)),
        };
    }
    out
}

/// Distinct knots and their multiplicities; returns them with the multiplicity sum.
fn knots(c: &mut Cursor<'_>, n: i32) -> Result<(Vec<f64>, Vec<usize>, usize)> {
    let mut values = Vec::new();
    let mut mults = Vec::new();
    for _ in 0..n.max(0) {
        values.push(c.double("knot")?);
        mults.push(usize::try_from(c.long("multiplicity")?).unwrap_or(0));
    }
    let sum = mults.iter().sum();
    Ok((values, mults, sum))
}

/// A `bs3_curve` (dim 3) or `bs2_curve` (dim 2) block; `None` for "nullbs".
pub(crate) fn bs_curve(c: &mut Cursor<'_>, dim: usize) -> Result<Option<BsCurve>> {
    let form = c.ident("form")?;
    if form == "nullbs" {
        return Ok(None);
    }
    let rational = form == "nurbs";
    let degree = usize::try_from(c.long("degree")?).unwrap_or(0);
    let closure = c.enumeration("closure")?;
    let n = c.long("n_knots")?;
    let (knots, mults, sum) = knots(c, n)?;
    let count = (sum + 1).saturating_sub(degree);
    let mut poles = Vec::with_capacity(count);
    let mut weights = rational.then(Vec::new);
    for _ in 0..count {
        poles.push(
            (0..dim)
                .map(|_| c.double("pole"))
                .collect::<Result<Vec<_>>>()?,
        );
        if let Some(w) = weights.as_mut() {
            w.push(c.double("weight")?);
        }
    }
    Ok(Some(BsCurve {
        rational,
        degree,
        closure,
        knots,
        mults,
        poles,
        weights,
    }))
}

/// A `bs3_surface` block; `None` for "nullbs".
pub(crate) fn bs_surface(c: &mut Cursor<'_>) -> Result<Option<BsSurface>> {
    let form = c.ident("form")?;
    if form == "nullbs" {
        return Ok(None);
    }
    let degree_u = usize::try_from(c.long("degree_u")?).unwrap_or(0);
    let degree_v = usize::try_from(c.long("degree_v")?).unwrap_or(0);
    let rational = rational_dirs(c, form)?;
    let (closure_u, closure_v) = (c.enumeration("closure_u")?, c.enumeration("closure_v")?);
    let (singularity_u, singularity_v) = (
        c.enumeration("singularity_u")?,
        c.enumeration("singularity_v")?,
    );
    let (nku, nkv) = (c.long("n_knots_u")?, c.long("n_knots_v")?);
    let (knots_u, mults_u, su) = knots(c, nku)?;
    let (knots_v, mults_v, sv) = knots(c, nkv)?;
    let (count_u, count_v) = (
        (su + 1).saturating_sub(degree_u),
        (sv + 1).saturating_sub(degree_v),
    );
    let (poles, weights) = net(c, count_u * count_v, rational.is_some())?;
    Ok(Some(BsSurface {
        rational,
        degree_u,
        degree_v,
        closure_u,
        closure_v,
        singularity_u,
        singularity_v,
        knots_u,
        mults_u,
        knots_v,
        mults_v,
        count_u,
        count_v,
        poles,
        weights,
    }))
}

/// The rational directions of a "nurbs" surface: an optional identifier, "both" when absent.
fn rational_dirs(c: &mut Cursor<'_>, form: &str) -> Result<Option<String>> {
    if form != "nurbs" {
        return Ok(None);
    }
    match c.peek() {
        Some(Token::Ident(s)) if matches!(s.as_str(), "both" | "u" | "v" | "none") => {
            let s = c.ident("rational")?.to_string();
            Ok((s != "none").then_some(s))
        }
        _ => Ok(Some("both".into())),
    }
}

/// Control points and, when rational, their weights.
type ControlNet = (Vec<[f64; 3]>, Option<Vec<f64>>);

fn net(c: &mut Cursor<'_>, n: usize, rational: bool) -> Result<ControlNet> {
    let mut poles = Vec::with_capacity(n);
    let mut weights = rational.then(Vec::new);
    for _ in 0..n {
        poles.push([c.double("x")?, c.double("y")?, c.double("z")?]);
        if let Some(w) = weights.as_mut() {
            w.push(c.double("w")?);
        }
    }
    Ok((poles, weights))
}
