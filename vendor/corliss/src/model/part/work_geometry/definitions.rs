//! Work geometry definitions: the node that records which command made a plane, axis or point
//! and from which inputs, as the reference part reader classifies them.

use std::collections::HashMap;

use crate::grammar::{DecodeStatus, DecodedNode, FieldValue};
use crate::guid::TypeId;
use crate::model::part::{ParamValue, Part, T_PARAMETER};
use crate::model::view::{SegmentView, node_ref};

/// Definition TypeID → Inventor command. The definition's first NodeRef is the resulting
/// geometry, the others are its inputs in stored order.
const DEFINITIONS: [(u32, &str); 23] = [
    (0x0D28_D8C0, "plane: offset from plane"),
    (0x74E6_F48A, "plane: angle to plane around edge"),
    (0x5838_B763, "plane: parallel to plane through point"),
    (0xCB6C_0A56, "plane: midplane between two planes"),
    (0x8AFF_BE5A, "plane: two coplanar edges"),
    (0x5838_B762, "plane: normal to axis through point"),
    (0x0942_9289, "axis: intersection of two planes"),
    (0xBCBB_AD85, "axis: parallel to line through point"),
    (0x83D3_1932, "axis: through two points"),
    (0xCE7F_937A, "axis: normal to plane through point"),
    (0x0942_9287, "point: intersection of three planes"),
    (0x0791_0C0B, "point: intersection of plane and line"),
    (0x0791_0C0A, "point: intersection of two lines"),
    (0x0942_928A, "plane: three points"),
    (
        0xB3EA_A9EE,
        "plane: tangent to surface and parallel to plane",
    ),
    (0xDD7D_4B84, "plane: midplane of torus"),
    (0xA76B_22A0, "axis: on line or edge"),
    (0x029D_AD70, "axis: through revolved face"),
    (0x3E71_0428, "axis: through center of circular edge"),
    (T_ON_POINT, "point: on vertex, sketch point or midpoint"),
    (0x1957_3122, "point: center of sphere"),
    (0xE1A2_4899, "point: center of torus"),
    (T_LOOP_CENTER, "point: center of loop of edges"),
];

/// "Point on vertex, sketch point or midpoint": the input point says which of the three it is.
const T_ON_POINT: u32 = 0x0C48_B860;
/// Point at the center of a loop of edges.
const T_LOOP_CENTER: u32 = 0xEFE4_7BB4;
/// Vertex proxy (`point` → the point geometry).
const T_VERTEX: u32 = 0xD8A9_C970;

/// Browser names of the origin geometry, which has no definition node.
const ORIGIN_NAMES: [&str; 7] = [
    "YZ Plane",
    "XZ Plane",
    "XY Plane",
    "X Axis",
    "Y Axis",
    "Z Axis",
    "Center Point",
];

/// How a work plane, axis or point was made.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkGeometryKind {
    /// One of the three origin planes, axes or the center point.
    Origin,
    /// A work point with no definition (placed at fixed coordinates).
    Grounded,
    /// A plane or axis whose definition node is not known.
    Unknown,
    /// Made by an Inventor work-feature command.
    Defined(WorkDefinition),
}

impl WorkGeometryKind {
    /// The reference reader's label: "origin", "grounded", "unknown definition" or the command.
    ///
    /// ```
    /// assert_eq!(corliss::model::part::WorkGeometryKind::Origin.label(), "origin");
    /// ```
    pub fn label(&self) -> &'static str {
        match self {
            WorkGeometryKind::Origin => "origin",
            WorkGeometryKind::Grounded => "grounded",
            WorkGeometryKind::Unknown => "unknown definition",
            WorkGeometryKind::Defined(d) => d.command,
        }
    }
}

/// A work feature's definition node.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkDefinition {
    /// TypeID of the definition node.
    pub type_id: TypeId,
    /// The Inventor command ("plane: offset from plane").
    pub command: &'static str,
    /// Browser names of the non-parameter inputs, in stored order.
    pub inputs: Vec<Option<String>>,
    /// The first parameter input (offset distance, angle), if any.
    pub parameter: Option<ParamValue>,
}

/// Geometry index → (definition node TypeID, input node indexes).
pub(super) struct DefinitionMap {
    by_result: HashMap<usize, (u32, Vec<usize>)>,
}

impl DefinitionMap {
    /// Indexes every definition node of the segment by the geometry it defines; a later
    /// definition of the same geometry replaces an earlier one.
    pub(super) fn new(view: &SegmentView<'_>) -> Self {
        let mut by_result = HashMap::new();
        let decoded = view
            .nodes()
            .filter(|(_, n)| n.status != DecodeStatus::NoGrammar);
        for (_, n) in decoded.filter(|(_, n)| command_of(n.type_id.0).is_some()) {
            if let Some((&result, inputs)) = definition_refs(n).split_first() {
                by_result.insert(result, (n.type_id.0, inputs.to_vec()));
            }
        }
        DefinitionMap { by_result }
    }

    /// The kind of geometry node `index` called `name`; `grounded` for points, which may be
    /// placed without a definition.
    pub(super) fn kind(
        &self,
        part: &Part<'_>,
        index: usize,
        name: Option<&str>,
        grounded: bool,
    ) -> WorkGeometryKind {
        if name.is_some_and(|n| ORIGIN_NAMES.contains(&n)) {
            return WorkGeometryKind::Origin;
        }
        match self.by_result.get(&index) {
            Some((tid, inputs)) => WorkGeometryKind::Defined(self.definition(part, *tid, inputs)),
            None if grounded => WorkGeometryKind::Grounded,
            None => WorkGeometryKind::Unknown,
        }
    }

    /// The definition record: parameters are reported apart from the geometric inputs.
    fn definition(&self, part: &Part<'_>, tid: u32, inputs: &[usize]) -> WorkDefinition {
        let is_param = |i: &usize| part.view().type_of(*i) == Some(T_PARAMETER);
        WorkDefinition {
            type_id: TypeId(tid),
            command: self.refined_command(part.view(), tid, inputs),
            inputs: inputs
                .iter()
                .filter(|i| !is_param(i))
                .map(|&i| part.name(i))
                .collect(),
            parameter: inputs
                .iter()
                .find(|i| is_param(i))
                .and_then(|&i| part.param(i)),
        }
    }

    /// The command of definition `tid`; a point on a point is resolved by what its input is.
    fn refined_command(&self, view: &SegmentView<'_>, tid: u32, inputs: &[usize]) -> &'static str {
        let command = command_of(tid).unwrap_or("unknown definition");
        let (T_ON_POINT, Some(&src)) = (tid, inputs.last()) else {
            return command;
        };
        if self
            .by_result
            .get(&src)
            .is_some_and(|(sub, _)| *sub == T_LOOP_CENTER)
        {
            return "point: center of loop of edges";
        }
        if view
            .by_type(T_VERTEX)
            .any(|(_, v)| node_ref(v, "point") == Some(src))
        {
            return "point: on vertex";
        }
        "point: on point (midpoint or sketch point)"
    }
}

/// The non-null NodeRefs of a definition node except the `next` / `ref` bookkeeping: the
/// defined geometry first, then the inputs.
fn definition_refs(n: &DecodedNode) -> Vec<usize> {
    n.fields
        .iter()
        .filter(|f| !matches!(f.name.as_deref(), Some("next" | "ref")))
        .filter_map(|f| match f.value {
            FieldValue::Ref(r, _) => r.index(),
            _ => None,
        })
        .collect()
}

/// The command a definition TypeID stands for.
fn command_of(tid: u32) -> Option<&'static str> {
    DEFINITIONS.iter().find(|(t, _)| *t == tid).map(|(_, c)| *c)
}
