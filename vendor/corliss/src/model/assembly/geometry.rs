//! Geometry proxies that constraint selections reference (part coordinates, cm).

use super::Assembly;
use crate::grammar::DecodedNode;
use crate::model::view::{Matrix4, cross, floats, transform, vec3};

/// Plane copy: `origin`, in-plane axes `u` and `v`.
const T_PLANE: u32 = 0xCE52_DF42;
/// Direction vector `dir`.
const T_VECTOR: u32 = 0xCE52_DF40;
/// Circle: `pos`, `normal`, `radius` (or `r`).
const T_CIRCLE: u32 = 0x9E43_716A;
/// Cylinder surface.
const T_CYLINDER: u32 = 0x4116_DA9E;
/// Axis.
const T_AXIS: u32 = 0x8EF0_6C89;
/// Point.
const T_POINT: u32 = 0xCE52_DF3E;
/// Joint origin: a 4 × 4 frame.
const T_JOINT_ORIGIN: u32 = 0x55CE_9B23;

/// The geometry a constraint selection names.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A plane through `origin` with face normal `normal` (u × v of the stored axes).
    Plane {
        /// A point of the plane.
        origin: Vec<f64>,
        /// Face normal.
        normal: [f64; 3],
    },
    /// A direction.
    Vector {
        /// The direction.
        direction: Vec<f64>,
    },
    /// A circle (circular edge).
    Circle {
        /// Centre.
        center: Vec<f64>,
        /// Plane normal.
        normal: Vec<f64>,
        /// Radius.
        radius: Option<f64>,
    },
    /// A cylindrical face.
    Cylinder {
        /// Radius.
        radius: Option<f64>,
        /// A point on the axis.
        axis_point: Option<Vec<f64>>,
        /// Axis direction.
        axis: Option<Vec<f64>>,
    },
    /// A joint origin frame (column 3 = origin, column 2 = joint Z).
    JointOrigin {
        /// The frame, row-major.
        frame: Option<Matrix4>,
    },
    /// An axis (no further fields read).
    Axis,
    /// A point (no further fields read).
    Point,
    /// A proxy node of another type.
    Other {
        /// Its TypeID.
        type_id: u32,
    },
}

impl Assembly<'_> {
    /// The geometry proxy at AmDc `index`.
    pub(super) fn geometry(&self, index: Option<usize>) -> Option<Geometry> {
        let g = self.node(index)?;
        Some(match g.type_id.0 {
            T_PLANE => Geometry::Plane {
                origin: floats(g, "origin"),
                normal: cross(vec3(g, "u"), vec3(g, "v")),
            },
            T_VECTOR => Geometry::Vector {
                direction: floats(g, "dir"),
            },
            T_CIRCLE => circle(g),
            T_JOINT_ORIGIN => Geometry::JointOrigin {
                frame: transform(g, "transformation"),
            },
            T_CYLINDER => cylinder(g),
            T_AXIS => Geometry::Axis,
            T_POINT => Geometry::Point,
            type_id => Geometry::Other { type_id },
        })
    }
}

fn circle(g: &DecodedNode) -> Geometry {
    let radius = g.float("radius").or_else(|| g.float("r"));
    Geometry::Circle {
        center: floats(g, "pos"),
        normal: floats(g, "normal"),
        radius,
    }
}

/// 2027 grammars store the RDx cylinder fields; older files a u32/f64 run `a2`
/// (radius, axis point, axis).
fn cylinder(g: &DecodedNode) -> Geometry {
    if let Some(radius) = g.float("radius") {
        let array = |name: &str| g.get(name).and_then(|v| v.as_f64s());
        return Geometry::Cylinder {
            radius: Some(radius),
            axis_point: array("origin"),
            axis: array("axis_of_symmetry"),
        };
    }
    legacy_cylinder(g)
}

/// The cylinder of an older file: `a2` = radius, axis point, axis.
fn legacy_cylinder(g: &DecodedNode) -> Geometry {
    let a2 = g
        .get("a2")
        .and_then(|v| v.as_f64s())
        .filter(|a| !a.is_empty());
    let slice = |from: usize, to: usize| {
        a2.as_ref()
            .map(|a| a.iter().skip(from).take(to - from).copied().collect())
    };
    Geometry::Cylinder {
        radius: a2.as_ref().and_then(|a| a.first().copied()),
        axis_point: slice(1, 4),
        axis: slice(4, 7),
    }
}
