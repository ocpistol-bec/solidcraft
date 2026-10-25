//! The live B-rep graph: bodies, lumps, shells, faces, loops, coedges, edges and vertices
//! (spec 13.6.6), walked from the `body` records [R-07].

use std::collections::HashMap;

use super::brep::*;
use super::cursor::Cursor;
use super::decode::{Decoder, Definition, Family};
use super::geometry::{Curve, PCurve, Surface};
use super::stream::SabStream;
use crate::error::{CorlissError, Result};

impl Brep {
    /// Walks every live `body` record of `stream`.
    pub fn read(stream: &SabStream) -> Result<Self> {
        let mut w = Walker {
            d: Decoder::new(stream),
            brep: Brep::default(),
            edges: HashMap::new(),
            vertices: HashMap::new(),
        };
        let bodies: Vec<usize> = (0..stream.records.len())
            .filter(|k| is_live(stream, *k, "body"))
            .collect();
        for record in bodies {
            let body = w.body(record)?;
            w.brep.bodies.push(body);
        }
        Ok(w.brep)
    }
}

fn is_live(stream: &SabStream, k: usize, class: &str) -> bool {
    stream
        .records
        .get(k)
        .is_some_and(|r| r.live && r.class == class)
}

/// Builds the graph, sharing edges and vertices by record.
struct Walker<'s> {
    d: Decoder<'s>,
    brep: Brep,
    edges: HashMap<usize, usize>,
    vertices: HashMap<usize, usize>,
}

/// Index of a non-null pointer.
fn idx(p: i32) -> Option<usize> {
    usize::try_from(p).ok()
}

impl Walker<'_> {
    /// A cursor at record `k`, which must have class `class`.
    fn open(&self, k: usize, class: &'static str) -> Result<Cursor<'_>> {
        match self.d.stream.records.get(k) {
            Some(r) if r.class == class => Ok(self.d.cursor(k)),
            Some(r) => Err(CorlissError::invalid(
                "SAB record",
                "class",
                k,
                &r.class,
                class,
            )),
            None => Err(CorlissError::invalid(
                "SAB record",
                "pointer",
                k,
                k,
                "a record index",
            )),
        }
    }

    /// Follows a `next` chain from `first`, guarding against cycles.
    fn chain(
        &self,
        first: i32,
        class: &'static str,
        next: impl Fn(&mut Cursor<'_>) -> Result<i32>,
    ) -> Result<Vec<usize>> {
        let mut out = Vec::new();
        let mut cur = idx(first);
        while let Some(k) = cur {
            if out.contains(&k) || out.len() > self.d.stream.records.len() {
                break;
            }
            out.push(k);
            cur = idx(next(&mut self.open(k, class)?)?);
        }
        Ok(out)
    }

    fn body(&mut self, record: usize) -> Result<Body> {
        let mut c = self.open(record, "body")?;
        let entity_id = c.entity(true)?;
        let lump = c.ptr("lump")?;
        c.ptr("wire")?;
        let transform = idx(c.ptr("transform")?)
            .map(|t| self.transform(t))
            .transpose()?;
        let lumps = self.chain(lump, "lump", |c| {
            c.entity(true)?;
            c.ptr("next_lump")
        })?;
        let lumps = lumps
            .into_iter()
            .map(|k| self.lump(k))
            .collect::<Result<_>>()?;
        Ok(Body {
            record,
            entity_id,
            lumps,
            transform,
        })
    }

    fn transform(&self, k: usize) -> Result<Transform> {
        let mut c = self.open(k, "transform")?;
        c.entity(false)?;
        let rotation = [c.vector("row0")?, c.vector("row1")?, c.vector("row2")?];
        Ok(Transform {
            rotation,
            translation: c.vector("translation")?,
            scale: c.double("scale")?,
            rotate: c.logical("rotate")?,
            reflect: c.logical("reflect")?,
            shear: c.logical("shear")?,
        })
    }

    fn lump(&mut self, k: usize) -> Result<Lump> {
        let mut c = self.open(k, "lump")?;
        c.entity(true)?;
        c.ptr("next_lump")?;
        let first = c.ptr("shell")?;
        let shells = self.chain(first, "shell", |c| {
            c.entity(true)?;
            c.ptr("next_shell")
        })?;
        Ok(Lump {
            shells: shells
                .into_iter()
                .map(|s| self.shell(s))
                .collect::<Result<_>>()?,
        })
    }

    fn shell(&mut self, k: usize) -> Result<Shell> {
        let mut c = self.open(k, "shell")?;
        c.entity(true)?;
        c.ptr("next_shell")?;
        c.ptr("subshell")?;
        let face = c.ptr("face")?;
        let wire = if self.d.version >= 10_700 {
            c.ptr("wire")?
        } else {
            -1
        };
        let faces = self.chain(face, "face", |c| {
            c.entity(true)?;
            c.ptr("next_face")
        })?;
        let wires = self.chain(wire, "wire", |c| {
            c.entity(true)?;
            c.ptr("next_wire")
        })?;
        Ok(Shell {
            faces: faces
                .into_iter()
                .map(|f| self.face(f))
                .collect::<Result<_>>()?,
            wires: wires
                .into_iter()
                .map(|w| self.wire(w))
                .collect::<Result<_>>()?,
        })
    }

    fn face(&mut self, record: usize) -> Result<Face> {
        let mut c = self.open(record, "face")?;
        let entity_id = c.entity(true)?;
        c.ptr("next_face")?;
        let first_loop = c.ptr("loop")?;
        c.ptr("shell")?;
        c.ptr("subshell")?;
        let surface = self.surface(c.ptr("surface")?)?;
        let reversed = c.logical("reversed")?;
        let double_sided = self.d.version >= 10_500 && c.logical("double_sided")?;
        let loops = self.chain(first_loop, "loop", |c| {
            c.entity(true)?;
            c.ptr("next_loop")
        })?;
        let loops = loops
            .into_iter()
            .map(|l| self.face_loop(l))
            .collect::<Result<_>>()?;
        Ok(Face {
            record,
            entity_id,
            surface,
            reversed,
            double_sided,
            loops,
        })
    }

    fn face_loop(&mut self, k: usize) -> Result<Loop> {
        let mut c = self.open(k, "loop")?;
        c.entity(true)?;
        c.ptr("next_loop")?;
        let first = c.ptr("coedge")?;
        Ok(Loop {
            coedges: self.coedge_cycle(first)?,
        })
    }

    fn wire(&mut self, k: usize) -> Result<Wire> {
        let mut c = self.open(k, "wire")?;
        c.entity(true)?;
        c.ptr("next_wire")?;
        let first = c.ptr("coedge")?;
        Ok(Wire {
            coedges: self.coedge_cycle(first)?,
        })
    }

    /// The coedges from `first` along `next_coedge` until the cycle closes or ends.
    fn coedge_cycle(&mut self, first: i32) -> Result<Vec<Coedge>> {
        let ks = self.chain(first, "coedge", |c| {
            c.entity(true)?;
            c.ptr("next_coedge")
        })?;
        ks.into_iter().map(|k| self.coedge(k)).collect()
    }

    fn coedge(&mut self, record: usize) -> Result<Coedge> {
        let mut c = self.open(record, "coedge")?;
        c.entity(true)?;
        c.ptr("next_coedge")?;
        c.ptr("prev_coedge")?;
        c.ptr("partner_coedge")?;
        let edge = c.ptr("edge")?;
        let reversed = c.logical("reversed")?;
        c.ptr("loop")?;
        if self.d.version >= 2_170_003 {
            c.long("reserved_58")?;
        }
        let pcurve = idx(c.ptr("pcurve")?).map(|p| self.pcurve(p)).transpose()?;
        let edge = idx(edge).map(|e| self.edge(e)).transpose()?;
        Ok(Coedge {
            record,
            edge,
            reversed,
            pcurve,
        })
    }

    fn edge(&mut self, record: usize) -> Result<usize> {
        if let Some(&k) = self.edges.get(&record) {
            return Ok(k);
        }
        let mut c = self.open(record, "edge")?;
        let entity_id = c.entity(true)?;
        let start = c.ptr("start_vertex")?;
        let start_param = if self.d.version >= 50_000 {
            c.double("start_param")?
        } else {
            0.0
        };
        let end = c.ptr("end_vertex")?;
        let end_param = if self.d.version >= 50_000 {
            c.double("end_param")?
        } else {
            0.0
        };
        c.ptr("coedge")?;
        let curve = self.curve(c.ptr("curve")?)?;
        let reversed = c.logical("reversed")?;
        let convexity = if self.d.version >= 50_000 {
            c.string("convexity")?
        } else {
            String::new()
        };
        let start = idx(start).map(|v| self.vertex(v)).transpose()?;
        let end = idx(end).map(|v| self.vertex(v)).transpose()?;
        let edge = Edge {
            record,
            entity_id,
            start,
            start_param,
            end,
            end_param,
            curve,
            reversed,
            convexity,
        };
        self.brep.edges.push(edge);
        self.edges.insert(record, self.brep.edges.len() - 1);
        Ok(self.brep.edges.len() - 1)
    }

    fn vertex(&mut self, record: usize) -> Result<usize> {
        if let Some(&k) = self.vertices.get(&record) {
            return Ok(k);
        }
        let mut c = self.open(record, "vertex")?;
        let entity_id = c.entity(true)?;
        c.ptr("edge")?;
        if self.d.version >= 2_110_001 {
            c.long("edge_end_info")?;
        }
        let point = match idx(c.ptr("point")?) {
            Some(p) => self.point(p)?,
            None => [0.0; 3],
        };
        self.brep.vertices.push(Vertex {
            record,
            entity_id,
            point,
        });
        self.vertices.insert(record, self.brep.vertices.len() - 1);
        Ok(self.brep.vertices.len() - 1)
    }

    fn point(&self, k: usize) -> Result<[f64; 3]> {
        let mut c = self.open(k, "point")?;
        c.entity(true)?;
        c.position("position")
    }

    /// The curve record at pointer `p` (`Curve::Null` for a null pointer).
    fn curve(&self, p: i32) -> Result<Curve> {
        let Some(k) = idx(p) else {
            return Ok(Curve::Null);
        };
        let class = self
            .d
            .stream
            .records
            .get(k)
            .map_or("", |r| r.class.as_str());
        let Some(kind) = class.strip_suffix("-curve") else {
            return Err(CorlissError::invalid(
                "SAB record",
                "curve",
                k,
                class,
                "a …-curve record",
            ));
        };
        let mut c = self.d.cursor(k);
        c.entity(true)?;
        self.d.curve_data(&mut c, kind, 0)
    }

    /// The surface record at pointer `p`.
    fn surface(&self, p: i32) -> Result<Surface> {
        let Some(k) = idx(p) else {
            return Ok(Surface::Null);
        };
        let class = self
            .d
            .stream
            .records
            .get(k)
            .map_or("", |r| r.class.as_str());
        let Some(kind) = class.strip_suffix("-surface") else {
            return Err(CorlissError::invalid(
                "SAB record",
                "surface",
                k,
                class,
                "a …-surface record",
            ));
        };
        let mut c = self.d.cursor(k);
        c.entity(true)?;
        self.d.surface_data(&mut c, kind, 0)
    }

    fn pcurve(&self, k: usize) -> Result<PCurve> {
        let mut c = self.open(k, "pcurve")?;
        c.entity(true)?;
        let which = c.long("def_type")?;
        if which != 0 {
            return Ok(PCurve::OfIntcurve {
                which,
                intcurve: c.ptr("intcurve")?,
            });
        }
        let reversed = c.logical("reversed")?;
        Ok(match self.d.subtype(&mut c, Family::ParCur, 0)? {
            Some(Definition::ParCurve {
                bs2,
                fit_tolerance,
                surface,
            }) => PCurve::Own {
                reversed,
                bs2,
                fit_tolerance,
                surface,
            },
            _ => PCurve::Own {
                reversed,
                bs2: None,
                fit_tolerance: None,
                surface: None,
            },
        })
    }
}
