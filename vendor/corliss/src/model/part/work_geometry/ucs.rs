//! User coordinate systems.

use std::collections::{BTreeMap, HashMap};

use crate::grammar::{DecodedNode, FieldValue};
use crate::model::part::{ParamValue, Part};
use crate::model::view::{Matrix4, node_ref, transform, vec3};

/// UCS feature: transformation, its own planes, axes and center point.
const T_UCS: u32 = 0x4ACA_204D;
/// UCS values: the defining points and the six offset / angle parameters.
const T_UCS_VALUES: u32 = 0x9C8C_1297;

/// The UCS feature's NodeRef fields naming its own work geometry.
const UCS_GEOMETRY: [&str; 7] = [
    "planeXY", "planeXZ", "planeYZ", "axisX", "axisY", "axisZ", "origin",
];

/// A user coordinate system.
#[derive(Debug, Clone, PartialEq)]
pub struct UserCoordinateSystem {
    /// Node index of the UCS feature.
    pub index: usize,
    /// Browser name ("UCS1").
    pub name: Option<String>,
    /// Node index of its Transformation node, when it has one.
    pub transformation: Option<usize>,
    /// UCS → model matrix (row-major, translation in column 3, cm); `None` when the
    /// Transformation node stores no matrix.
    pub transform: Option<Matrix4>,
    /// Browser names of its planes, axes and center point, keyed `planeXY`, `planeXZ`,
    /// `planeYZ`, `axisX`, `axisY`, `axisZ`, `origin`.
    pub geometry: BTreeMap<&'static str, Option<String>>,
    /// The defining points and parameters, when the values node exists.
    pub values: Option<UcsValues>,
}

/// How a UCS is placed: three points and six parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct UcsValues {
    /// Parameters keyed by role (`XOffset`, `YOffset`, `ZOffset`, `XAngle`, `YAngle`, `ZAngle`).
    pub parameters: BTreeMap<String, Option<ParamValue>>,
    /// Origin, x-axis and y-axis points (model cm); `None` for a vertex without a point.
    pub defining_points: Vec<Option<[f64; 3]>>,
}

impl Part<'_> {
    /// User coordinate systems, in block order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// for ucs in part.user_coordinate_systems() {
    ///     println!("{:?} {:?}", ucs.name, ucs.transform);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn user_coordinate_systems(&self) -> Vec<UserCoordinateSystem> {
        let values: HashMap<Option<usize>, &DecodedNode> = self
            .view()
            .by_type(T_UCS_VALUES)
            .map(|(_, v)| (node_ref(v, "ucs"), v))
            .collect();
        self.view()
            .by_type(T_UCS)
            .map(|(index, n)| {
                self.user_coordinate_system(index, n, values.get(&Some(index)).copied())
            })
            .collect()
    }

    fn user_coordinate_system(
        &self,
        index: usize,
        n: &DecodedNode,
        values: Option<&DecodedNode>,
    ) -> UserCoordinateSystem {
        let transformation = node_ref(n, "transformation");
        let matrix_node = transformation.and_then(|t| self.view().node(t));
        UserCoordinateSystem {
            index,
            name: self.name(index),
            transformation,
            transform: matrix_node.and_then(|t| transform(t, "transformation")),
            geometry: UCS_GEOMETRY
                .iter()
                .map(|&k| (k, node_ref(n, k).and_then(|r| self.name(r))))
                .collect(),
            values: values.map(|v| self.ucs_values(v)),
        }
    }

    fn ucs_values(&self, v: &DecodedNode) -> UcsValues {
        let parameters = v
            .fields
            .iter()
            .filter_map(|f| {
                let role = f.name.as_deref()?.strip_prefix("refParam")?;
                let target = match f.value {
                    FieldValue::Ref(r, _) => r.index(),
                    _ => None,
                };
                Some((role.to_string(), target.and_then(|t| self.param(t))))
            })
            .collect();
        let points = ["origin_point", "x_point", "y_point"]
            .iter()
            .filter_map(|k| node_ref(v, k));
        UcsValues {
            parameters,
            defining_points: points.map(|vertex| self.vertex_position(vertex)).collect(),
        }
    }

    /// The position of the point a vertex proxy names.
    fn vertex_position(&self, vertex: usize) -> Option<[f64; 3]> {
        let point = self
            .view()
            .node(vertex)
            .and_then(|n| node_ref(n, "point"))?;
        self.view().node(point).map(|p| vec3(p, "pos"))
    }
}
