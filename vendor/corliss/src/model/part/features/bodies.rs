//! Body and surface operations: combine, split, thicken, sculpt, patch, ruled surface and the
//! direct-edit body moves (spec 15.5).

use crate::model::Matrix4;
use crate::model::part::ParamValue;
use crate::model::view::{node_ref, ref_list, transform};

use super::FeatureKind;
use super::context::{FeatureNaming, Slots};
use super::placed::slot_faces;
use super::sketched::slot_vec;
use super::values::{
    FeatureExtent, FeatureOperation, FeatureRuledType, FeatureSplitMethod, FeatureValue,
};

/// Faces of one thicken/offset set.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFaceOffset {
    /// Faces.
    pub faces: Vec<Option<FeatureNaming>>,
    /// Offset parameter.
    pub offset: Option<ParamValue>,
}

/// A sculpt tool.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSculptTool {
    /// Tool body or surface name.
    pub tool: Option<String>,
    /// Raw side code.
    pub side: Option<FeatureValue>,
}

/// What a sculpt does with its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSculptMode {
    /// Creates a new solid.
    NewSolid,
    /// Adds material.
    Add,
    /// Removes material.
    Remove,
}

/// One boundary-patch loop.
#[derive(Debug, Clone, PartialEq)]
pub struct FeaturePatchLoop {
    /// Weight parameter.
    pub weight: Option<ParamValue>,
    /// (edge, face) reference pairs.
    pub edges: Vec<(Option<FeatureNaming>, Option<FeatureNaming>)>,
}

/// A ruled surface.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureRuledSurface {
    /// Ruled type (normal, tangent, vector).
    pub ruled_type: Option<FeatureRuledType>,
    /// Distance parameter.
    pub distance: Option<ParamValue>,
    /// Angle parameter.
    pub angle: Option<ParamValue>,
    /// Result surface name.
    pub surface: Option<String>,
    /// Direction for the vector type.
    pub vector: Option<Vec<f64>>,
    /// Slots 4, 5, 6 and 9 flags.
    pub flags: [Option<bool>; 4],
}

fn names_in(s: &Slots, k: usize) -> Vec<Option<String>> {
    s.ctx.names(s.node(k), "items")
}

pub(super) fn tagging(s: &Slots) -> FeatureKind {
    FeatureKind::Tagging {
        bodies: names_in(s, 0),
    }
}

pub(super) fn mark(s: &Slots) -> FeatureKind {
    FeatureKind::Mark {
        flags: [s.flag(2), s.flag(4), s.flag(5), s.flag(6)],
    }
}

pub(super) fn delete_face(s: &Slots) -> FeatureKind {
    FeatureKind::DeleteFace {
        faces: slot_faces(s, 0),
        heal: s.flag(1).unwrap_or(false),
    }
}

pub(super) fn replace_face(s: &Slots) -> FeatureKind {
    FeatureKind::ReplaceFace {
        existing: slot_faces(s, 0),
        new: slot_faces(s, 1),
    }
}

pub(super) fn combine(s: &Slots) -> FeatureKind {
    FeatureKind::Combine {
        operation: FeatureOperation::from_value(s.value(2)),
        tool_bodies: names_in(s, 1),
        keep_toolbodies: s.flag(3),
    }
}

pub(super) fn split(s: &Slots) -> FeatureKind {
    FeatureKind::Split {
        method: FeatureSplitMethod::from_value(s.value(0)),
        all_faces: s.flag(1).unwrap_or(false),
        tool: s.name(5),
    }
}

fn face_offset(s: &Slots, e: usize) -> FeatureFaceOffset {
    let en = s.ctx.node(e);
    let faces = en.and_then(|n| node_ref(n, "faces")).unwrap_or(0);
    FeatureFaceOffset {
        faces: if faces != 0 {
            s.ctx.face_list(s.ctx.node(faces))
        } else {
            Vec::new()
        },
        offset: s
            .ctx
            .param(en.and_then(|n| node_ref(n, "offset")).unwrap_or(0)),
    }
}

pub(super) fn thicken(s: &Slots) -> FeatureKind {
    let sets = s.node(0).map(|n| ref_list(n, "lst0")).unwrap_or_default();
    FeatureKind::Thicken {
        operation: FeatureOperation::from_value(s.value(1)),
        distance: s.param(6),
        face_sets: sets.into_iter().map(|e| face_offset(s, e)).collect(),
    }
}

pub(super) fn pattern_body(s: &Slots) -> FeatureKind {
    let id = |k: usize| s.node(k).and_then(|n| n.float("valueModel"));
    FeatureKind::PatternResultBody {
        ids: [id(0), id(1), id(2)],
        body: s.name(3),
    }
}

pub(super) fn move_bodies(s: &Slots) -> FeatureKind {
    let refs = s
        .node(1)
        .map(|n| ref_list(n, "transformations"))
        .unwrap_or_default();
    let transforms: Vec<Option<Matrix4>> = refs
        .into_iter()
        .map(|t| s.ctx.node(t).and_then(|n| transform(n, "transformation")))
        .collect();
    FeatureKind::MoveBodies {
        bodies: names_in(s, 0),
        transforms,
    }
}

pub(super) fn copy_object(s: &Slots) -> FeatureKind {
    let flag = s.flag(2).unwrap_or(false);
    FeatureKind::CopyObject {
        fit_mesh_face: s.ctx.has_meshes && flag,
        source_ref: s.node(0).and_then(|n| node_ref(n, "ref_0")),
        result: s.name(1),
        flag,
    }
}

pub(super) fn base_body(s: &Slots) -> FeatureKind {
    let source = s.ctx.base_sources.get(&s.index).cloned().flatten();
    FeatureKind::BaseBody {
        body: s.name(1),
        source,
    }
}

pub(super) fn stitch(s: &Slots) -> FeatureKind {
    FeatureKind::Stitch {
        surfaces: names_in(s, 0),
        tolerance: s.param(4),
        flags: [s.flag(3), s.flag(5)],
    }
}

fn sculpt_tool(s: &Slots, it: usize) -> FeatureSculptTool {
    let tn = s.ctx.node(it);
    let r = |name: &str| tn.and_then(|n| node_ref(n, name)).unwrap_or(0);
    FeatureSculptTool {
        tool: s.ctx.name(r("body")),
        side: s.ctx.value(r("operation")),
    }
}

pub(super) fn sculpt(s: &Slots) -> FeatureKind {
    let items = s.node(0).map(|n| ref_list(n, "items")).unwrap_or_default();
    let mode = if s.flag(3).unwrap_or(false) {
        FeatureSculptMode::NewSolid
    } else if s.flag(1).unwrap_or(false) {
        FeatureSculptMode::Add
    } else {
        FeatureSculptMode::Remove
    };
    FeatureKind::Sculpt {
        tools: items.into_iter().map(|i| sculpt_tool(s, i)).collect(),
        mode,
    }
}

pub(super) fn ruled(s: &Slots) -> FeatureKind {
    // The type is the payload's last u16; the grammar does not name it.
    let payload = s.node(2).map(|n| n.payload()).unwrap_or_default();
    let tail = payload
        .len()
        .checked_sub(2)
        .map(|k| u16::from_le_bytes([payload[k], payload[k + 1]]));
    let code = FeatureValue::Int(i64::from(tail.unwrap_or(0)));
    FeatureKind::RuledSurface(FeatureRuledSurface {
        ruled_type: FeatureRuledType::from_value(Some(code)),
        distance: s.param(3),
        angle: s.param(13),
        surface: s.name(10),
        vector: slot_vec(s, 7, "dir"),
        flags: [s.flag(4), s.flag(5), s.flag(6), s.flag(9)],
    })
}

pub(super) fn trim(s: &Slots) -> FeatureKind {
    FeatureKind::Trim {
        tool: s.name(3),
        surface: s.name(5),
    }
}

pub(super) fn extend(s: &Slots) -> FeatureKind {
    FeatureKind::Extend {
        edges: s.ctx.edges(s.at(0)),
        extent: FeatureExtent::from_value(s.value(2)),
        distance: s.param(3),
        surface: s.name(7),
    }
}

fn patch_loop(s: &Slots, c: usize) -> FeaturePatchLoop {
    let cn = s.ctx.node(c);
    let r = |n: Option<&crate::grammar::DecodedNode>, name: &str| {
        n.and_then(|n| node_ref(n, name)).unwrap_or(0)
    };
    let pair = |e: usize| {
        let en = s.ctx.node(e);
        (
            s.ctx.facerefs(r(en, "ref_1")),
            s.ctx.facerefs(r(en, "ref_2")),
        )
    };
    let edges = cn.map(|n| ref_list(n, "lst0")).unwrap_or_default();
    FeaturePatchLoop {
        weight: s.ctx.param(r(cn, "ref_1")),
        edges: edges.into_iter().map(pair).collect(),
    }
}

pub(super) fn patch(s: &Slots) -> FeatureKind {
    let mut loops = Vec::new();
    for lp in s.node(1).map(|n| ref_list(n, "lst0")).unwrap_or_default() {
        let curves = s
            .ctx
            .node(lp)
            .map(|n| ref_list(n, "lst0"))
            .unwrap_or_default();
        loops.extend(curves.into_iter().map(|c| patch_loop(s, c)));
    }
    FeatureKind::BoundaryPatch {
        loops,
        parameter: s.param(3),
        surface: s.name(4),
    }
}
