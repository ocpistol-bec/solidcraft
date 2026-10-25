//! Origin and work geometry (spec clause 15.2), after the reference part reader: work planes,
//! work axes, work points and user coordinate systems.
//!
//! Planes, axes and points are the plain geometry nodes; what made them (offset from a plane,
//! intersection of two planes, ...) is a separate definition node whose first NodeRef names the
//! geometry, see [`WorkGeometryKind`].

mod definitions;
mod ucs;

pub use definitions::{WorkDefinition, WorkGeometryKind};
pub use ucs::{UcsValues, UserCoordinateSystem};

use super::Part;
use crate::grammar::{DecodedNode, FieldValue};
use crate::model::view::{cross, vec3};
use definitions::DefinitionMap;

/// Plane geometry: origin planes and work planes.
const T_PLANE: u32 = 0xCE52_DF42;
/// 3D line: origin axes, work axes and 3D sketch lines.
const T_LINE3D: u32 = 0x8EF0_6C89;
/// 3D point: the center point, work points and 3D sketch points.
const T_POINT3D: u32 = 0xCE52_DF3E;

/// A work plane (or origin plane) in model coordinates (cm).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkPlane {
    /// Node index of the plane geometry.
    pub index: usize,
    /// Browser name ("XY Plane", "Work Plane1").
    pub name: Option<String>,
    /// A point on the plane.
    pub origin: [f64; 3],
    /// First in-plane axis.
    pub u: [f64; 3],
    /// Second in-plane axis.
    pub v: [f64; 3],
    /// `u × v`.
    pub normal: [f64; 3],
    /// How the plane is defined.
    pub kind: WorkGeometryKind,
}

/// A work axis (or origin axis) in model coordinates (cm).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkAxis {
    /// Node index of the line geometry.
    pub index: usize,
    /// Browser name ("X Axis", "Work Axis1").
    pub name: Option<String>,
    /// The stored point on the axis.
    pub origin: [f64; 3],
    /// `origin` plus the stored (unnormalized) direction vector.
    pub segment_end: [f64; 3],
    /// Unit direction.
    pub direction: [f64; 3],
    /// How the axis is defined.
    pub kind: WorkGeometryKind,
}

/// A work point (or the center point) in model coordinates (cm).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkPoint {
    /// Node index of the point geometry.
    pub index: usize,
    /// Browser name ("Center Point", "Work Point1").
    pub name: Option<String>,
    /// Position.
    pub position: [f64; 3],
    /// How the point is defined; an undefined point is [`WorkGeometryKind::Grounded`].
    pub kind: WorkGeometryKind,
}

impl Part<'_> {
    /// Origin planes and work planes, in block order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// for plane in part.work_planes() {
    ///     println!("{:?}: normal {:?} ({})", plane.name, plane.normal, plane.kind.label());
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn work_planes(&self) -> Vec<WorkPlane> {
        let defs = DefinitionMap::new(self.view());
        self.view()
            .by_type(T_PLANE)
            .map(|(index, n)| {
                let name = self.name(index);
                let (u, v) = (vec3(n, "u"), vec3(n, "v"));
                let kind = defs.kind(self, index, name.as_deref(), false);
                WorkPlane {
                    index,
                    name,
                    origin: vec3(n, "origin"),
                    u,
                    v,
                    normal: cross(u, v),
                    kind,
                }
            })
            .collect()
    }

    /// Origin axes and work axes (3D sketch lines excluded), in block order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// let axes = part.work_axes();
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn work_axes(&self) -> Vec<WorkAxis> {
        let defs = DefinitionMap::new(self.view());
        self.view()
            .by_type(T_LINE3D)
            .filter(|(_, n)| !in_sketch(n))
            .map(|(index, n)| {
                let name = self.name(index);
                let kind = defs.kind(self, index, name.as_deref(), false);
                let (origin, dir) = (vec3(n, "pos"), vec3(n, "dir"));
                let end = [origin[0] + dir[0], origin[1] + dir[1], origin[2] + dir[2]];
                WorkAxis {
                    index,
                    name,
                    origin,
                    segment_end: end,
                    direction: unit(dir),
                    kind,
                }
            })
            .collect()
    }

    /// The center point and work points (3D sketch points excluded), in block order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// let points = part.work_points();
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn work_points(&self) -> Vec<WorkPoint> {
        let defs = DefinitionMap::new(self.view());
        self.view()
            .by_type(T_POINT3D)
            .filter(|(_, n)| !in_sketch(n))
            .map(|(index, n)| {
                let name = self.name(index);
                let kind = defs.kind(self, index, name.as_deref(), true);
                WorkPoint {
                    index,
                    name,
                    position: vec3(n, "pos"),
                    kind,
                }
            })
            .collect()
    }
}

/// True for geometry owned by a 3D sketch: those lines and points are not work features.
fn in_sketch(n: &DecodedNode) -> bool {
    n.get("sketch")
        .and_then(FieldValue::as_int)
        .is_some_and(|v| v != 0)
}

/// `v / |v|`; a zero vector stays zero rather than becoming NaN.
fn unit(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let len = if len == 0.0 { 1.0 } else { len };
    [v[0] / len, v[1] / len, v[2] / len]
}
