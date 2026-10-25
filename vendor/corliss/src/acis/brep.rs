//! The live B-rep graph of a SAB stream: bodies, lumps, shells, faces, loops, coedges, edges
//! and vertices (spec 13.6.6).

use super::geometry::{Curve, PCurve, Surface};

/// A transform record (spec Table 13-19).
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    /// Rows of the 3 × 3 rotation-and-scale matrix.
    pub rotation: [[f64; 3]; 3],
    /// Translation (cm).
    pub translation: [f64; 3],
    /// Uniform scale.
    pub scale: f64,
    /// Rotation flag.
    pub rotate: bool,
    /// Reflection flag.
    pub reflect: bool,
    /// Shear flag.
    pub shear: bool,
}

/// A vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// Record index.
    pub record: usize,
    /// Entity id, when set.
    pub entity_id: Option<i32>,
    /// Position (cm).
    pub point: [f64; 3],
}

/// An edge; vertices index [`Brep::vertices`].
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Record index.
    pub record: usize,
    /// Entity id, when set.
    pub entity_id: Option<i32>,
    /// Start vertex.
    pub start: Option<usize>,
    /// Curve parameter at the start vertex.
    pub start_param: f64,
    /// End vertex.
    pub end: Option<usize>,
    /// Curve parameter at the end vertex.
    pub end_param: f64,
    /// The curve.
    pub curve: Curve,
    /// Edge direction against the curve.
    pub reversed: bool,
    /// "convex", "concave", "tangent", "unknown" …
    pub convexity: String,
}

/// A coedge: one use of an edge in a loop or wire; `edge` indexes [`Brep::edges`].
#[derive(Debug, Clone, PartialEq)]
pub struct Coedge {
    /// Record index.
    pub record: usize,
    /// The edge.
    pub edge: Option<usize>,
    /// Sense relative to the edge.
    pub reversed: bool,
    /// Parameter-space curve, when stored.
    pub pcurve: Option<PCurve>,
}

/// A loop: a cycle of coedges bounding a face.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    /// Coedges in cycle order.
    pub coedges: Vec<Coedge>,
}

/// A face.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// Record index.
    pub record: usize,
    /// Entity id, when set (the id Inventor's naming uses).
    pub entity_id: Option<i32>,
    /// The surface.
    pub surface: Surface,
    /// True when the face normal is opposite to the surface normal.
    pub reversed: bool,
    /// True for a double-sided face.
    pub double_sided: bool,
    /// Bounding loops.
    pub loops: Vec<Loop>,
}

/// A wire: coedges not bounding a face (sketch sections).
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    /// Coedges in order.
    pub coedges: Vec<Coedge>,
}

/// A shell.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shell {
    /// Faces.
    pub faces: Vec<Face>,
    /// Wires.
    pub wires: Vec<Wire>,
}

/// A lump: one connected solid or sheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lump {
    /// Shells.
    pub shells: Vec<Shell>,
}

/// A body.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Record index.
    pub record: usize,
    /// Entity id, when set (CC0F7521 wrappers refer to bodies by it).
    pub entity_id: Option<i32>,
    /// Lumps.
    pub lumps: Vec<Lump>,
    /// Transform, when stored.
    pub transform: Option<Transform>,
}

/// The live B-rep of one SAB stream.
///
/// ```no_run
/// # let sab: Vec<u8> = Vec::new();
/// let stream = corliss::acis::SabStream::parse(&sab)?;
/// let brep = corliss::acis::Brep::read(&stream)?;
/// for body in &brep.bodies {
///     let faces: usize = body.lumps.iter().flat_map(|l| &l.shells).map(|s| s.faces.len()).sum();
///     println!("body with {faces} faces, {} edges in the stream", brep.edges.len());
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Brep {
    /// Bodies, in record order.
    pub bodies: Vec<Body>,
    /// Edges shared by the coedges of all bodies.
    pub edges: Vec<Edge>,
    /// Vertices shared by the edges.
    pub vertices: Vec<Vertex>,
}
