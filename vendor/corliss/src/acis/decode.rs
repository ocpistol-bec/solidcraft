//! Decoding of curve, surface and subtype data (spec 13.6.7–13.6.8).

use super::cursor::Cursor;
use super::geometry::*;
use super::stream::SabStream;
use super::token::Token;
use crate::error::Result;

/// Subtype definitions nest; deeper chains than this are malformed.
const MAX_DEPTH: usize = 32;

/// Which family a subtype belongs to, which selects its layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    IntCur,
    SplSur,
    ParCur,
}

/// A decoded subtype definition.
pub(crate) enum Definition {
    Curve(CurveDefinition),
    Surface(SurfaceDefinition),
    ParCurve {
        bs2: Option<BsCurve>,
        fit_tolerance: Option<f64>,
        surface: Option<Box<Surface>>,
    },
}

/// Decodes geometry of one stream, resolving shared subtype definitions.
pub(crate) struct Decoder<'s> {
    pub stream: &'s SabStream,
    /// (record, token index of the subtype name) of every definition, in stream order [R-11].
    defs: Vec<(usize, usize)>,
    pub version: i64,
}

impl<'s> Decoder<'s> {
    pub fn new(stream: &'s SabStream) -> Self {
        let mut defs = Vec::new();
        for (r, rec) in stream.records.iter().enumerate() {
            for (k, pair) in rec.tokens.windows(2).enumerate() {
                let is_def = matches!(&pair[0], Token::SubtypeStart)
                    && matches!(&pair[1], Token::Ident(n) if n != "ref" && n != "null");
                if is_def {
                    defs.push((r, k + 1));
                }
            }
        }
        Decoder {
            stream,
            defs,
            version: stream.header.restore_version(),
        }
    }

    pub fn cursor(&self, record: usize) -> Cursor<'s> {
        let toks = self
            .stream
            .records
            .get(record)
            .map_or(&[][..], |r| r.tokens.as_slice());
        Cursor::new(toks, self.version)
    }

    /// Curve data without the ENTITY part, of curve `kind`.
    pub fn curve_data(&self, c: &mut Cursor<'_>, kind: &str, depth: usize) -> Result<Curve> {
        Ok(match kind {
            "straight" => Curve::Straight {
                root: c.position("root_point")?,
                direction: c.vector("direction")?,
                range: self.range(c, "subset_range")?,
            },
            "ellipse" => Curve::Ellipse(self.ellipse(c)?),
            "intcurve" => {
                let reversed = c.logical("reversed")?;
                let definition = match self.subtype(c, Family::IntCur, depth)? {
                    Some(Definition::Curve(d)) => Some(Box::new(d)),
                    _ => None,
                };
                Curve::Intcurve {
                    reversed,
                    definition,
                    range: self.range(c, "subset_range")?,
                }
            }
            "null_curve" => Curve::Null,
            other => return Err(c.mismatch("curve", other, "a curve kind")),
        })
    }

    fn ellipse(&self, c: &mut Cursor<'_>) -> Result<EllipseData> {
        Ok(EllipseData {
            centre: c.position("centre")?,
            normal: c.vector("normal")?,
            major_axis: c.vector("major_axis")?,
            radius_ratio: c.double("radius_ratio")?,
            range: self.range(c, "subset_range")?,
        })
    }

    /// An interval, present from V ≥ 10,600 (spec Table 13-7).
    fn range(&self, c: &mut Cursor<'_>, field: &'static str) -> Result<super::cursor::Interval> {
        if self.version < 10_600 {
            return Ok(Default::default());
        }
        c.interval(field)
    }

    /// Surface data without the ENTITY part, of surface `kind`.
    pub fn surface_data(&self, c: &mut Cursor<'_>, kind: &str, depth: usize) -> Result<Surface> {
        Ok(match kind {
            "plane" => Surface::Plane {
                root: c.position("root_point")?,
                normal: c.vector("normal")?,
                u_direction: if self.version >= 10_300 {
                    c.vector("u_direction")?
                } else {
                    [0.0; 3]
                },
                reverse_v: c.logical("reverse_v")?,
                u_range: self.range(c, "u_range")?,
                v_range: self.range(c, "v_range")?,
            },
            "cone" => self.cone(c)?,
            "sphere" => Surface::Sphere {
                centre: c.position("centre")?,
                radius: c.double("radius")?,
                uv_origin_dir: c.vector("uv_origin_dir")?,
                pole_dir: if self.version >= 10_300 {
                    c.vector("pole_dir")?
                } else {
                    [0.0; 3]
                },
                reverse_v: c.logical("reverse_v")?,
                u_range: self.range(c, "u_range")?,
                v_range: self.range(c, "v_range")?,
            },
            "torus" => self.torus(c)?,
            "spline" => self.spline(c, depth)?,
            "null_surface" => Surface::Null,
            other => return Err(c.mismatch("surface", other, "a surface kind")),
        })
    }

    fn cone(&self, c: &mut Cursor<'_>) -> Result<Surface> {
        Ok(Surface::Cone {
            base: self.ellipse(c)?,
            sine_angle: c.double("sine_angle")?,
            cosine_angle: c.double("cosine_angle")?,
            u_param_scale: if self.version >= 40_000 {
                c.double("u_param_scale")?
            } else {
                1.0
            },
            reversed_u: self.version >= 10_300 && c.logical("reversed_u")?,
            u_range: self.range(c, "u_range")?,
            v_range: self.range(c, "v_range")?,
        })
    }

    fn torus(&self, c: &mut Cursor<'_>) -> Result<Surface> {
        Ok(Surface::Torus {
            centre: c.position("centre")?,
            normal: c.vector("normal")?,
            major_radius: c.double("major_radius")?,
            minor_radius: c.double("minor_radius")?,
            uv_origin_dir: c.vector("uv_origin_dir")?,
            reverse_v: c.logical("reverse_v")?,
            u_range: self.range(c, "u_range")?,
            v_range: self.range(c, "v_range")?,
        })
    }

    fn spline(&self, c: &mut Cursor<'_>, depth: usize) -> Result<Surface> {
        let reversed = c.logical("reversed")?;
        let definition = match self.subtype(c, Family::SplSur, depth)? {
            Some(Definition::Surface(d)) => Some(Box::new(d)),
            _ => None,
        };
        Ok(Surface::Spline {
            reversed,
            definition,
            u_range: self.range(c, "u_range")?,
            v_range: self.range(c, "v_range")?,
        })
    }

    /// An inline `surface` composite.
    pub fn inline_surface(&self, c: &mut Cursor<'_>, depth: usize) -> Result<Surface> {
        let kind = c.ident("surface kind")?;
        self.surface_data(c, kind, depth)
    }

    /// An inline `curve` composite.
    pub fn inline_curve(&self, c: &mut Cursor<'_>, depth: usize) -> Result<Curve> {
        let kind = c.ident("curve kind")?;
        self.curve_data(c, kind, depth)
    }

    /// A subtype block (spec 13.6.3): a definition, a `ref` to an earlier one, or `null`.
    pub fn subtype(
        &self,
        c: &mut Cursor<'_>,
        family: Family,
        depth: usize,
    ) -> Result<Option<Definition>> {
        c.expect("subtype_start", &Token::SubtypeStart)?;
        let name = c.ident("subtype")?;
        let def = match name {
            "null" => None,
            "ref" => {
                let n = c.long("ref_index")?;
                self.resolve(n, family, depth)
            }
            _ => self.definition(&mut c.clone(), name, family, depth),
        };
        c.i = c.close_of_subtype();
        c.expect("subtype_end", &Token::SubtypeEnd)?;
        Ok(def)
    }

    fn resolve(&self, n: i32, family: Family, depth: usize) -> Option<Definition> {
        let &(record, at) = self.defs.get(usize::try_from(n).ok()?)?;
        let toks = &self.stream.records.get(record)?.tokens;
        let name = toks.get(at)?.ident()?;
        let mut c = Cursor::at(toks, at + 1, self.version);
        self.definition(&mut c, name, family, depth)
    }

    /// A definition body; a layout this crate does not follow gives a definition without data.
    fn definition(
        &self,
        c: &mut Cursor<'_>,
        name: &str,
        family: Family,
        depth: usize,
    ) -> Option<Definition> {
        if depth > MAX_DEPTH {
            return None;
        }
        let depth = depth + 1;
        Some(match family {
            Family::IntCur => Definition::Curve(super::subtypes::int_cur(self, c, name, depth)),
            Family::SplSur => Definition::Surface(super::subtypes::spl_sur(self, c, name, depth)),
            Family::ParCur => super::subtypes::par_cur(self, c, name, depth),
        })
    }
}
