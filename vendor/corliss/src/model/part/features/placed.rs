//! Placed features on existing geometry: holes, threads, chamfers, shells, drafts (spec 15.4).

use crate::model::Matrix4;
use crate::model::part::ParamValue;
use crate::model::view::{floats, node_ref, ref_list, transform};

use super::context::{FeatureNaming, FeatureThreadInfo, Slots};
use super::sketched::slot_vec;
use super::values::{
    FeatureChamferType, FeatureDraftMode, FeatureExtent, FeatureHoleSeat, FeatureShellDirection,
    FeatureValue,
};

/// A hole or slot.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureHole {
    /// Seat (none, countersink, counterbore).
    pub seat: Option<FeatureHoleSeat>,
    /// Diameter parameter.
    pub diameter: Option<ParamValue>,
    /// Depth parameter.
    pub depth: Option<ParamValue>,
    /// Seat diameter parameter.
    pub seat_diameter: Option<ParamValue>,
    /// Seat depth parameter.
    pub seat_depth: Option<ParamValue>,
    /// Countersink angle parameter.
    pub countersink_angle: Option<ParamValue>,
    /// Drill point angle parameter.
    pub drill_point_angle: Option<ParamValue>,
    /// Tap drill or major diameter parameter.
    pub tap_or_major: Option<ParamValue>,
    /// Extent.
    pub extent: Option<FeatureExtent>,
    /// Hole axis direction.
    pub direction: Option<Vec<f64>>,
    /// Placement transform.
    pub placement: Option<Matrix4>,
    /// Thread table row.
    pub thread: Option<FeatureThreadInfo>,
    /// Taper thread parameters, when either is set.
    pub taper_thread: Option<[Option<ParamValue>; 2]>,
    /// Slot length; `Some` makes the feature a slot.
    pub slot_length: Option<Option<ParamValue>>,
}

/// A cosmetic thread.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureThread {
    /// Threaded faces.
    pub faces: Vec<Option<FeatureNaming>>,
    /// Full length.
    pub full_length: bool,
    /// Length parameter.
    pub length: Option<ParamValue>,
    /// Offset parameter.
    pub offset: Option<ParamValue>,
    /// Thread table row.
    pub thread: Option<FeatureThreadInfo>,
}

/// A chamfer.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureChamfer {
    /// Method.
    pub method: Option<FeatureChamferType>,
    /// Chamfered edges.
    pub edges: Option<Vec<Option<FeatureNaming>>>,
    /// First distance parameter.
    pub distance1: Option<ParamValue>,
    /// Second distance parameter.
    pub distance2: Option<ParamValue>,
    /// Angle parameter.
    pub angle: Option<ParamValue>,
    /// Reference face, when stored.
    pub face: Option<Vec<Option<FeatureNaming>>>,
}

/// A shell.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureShell {
    /// Direction.
    pub direction: Option<FeatureShellDirection>,
    /// Removed faces.
    pub removed_faces: Vec<Option<FeatureNaming>>,
    /// Thickness parameter.
    pub thickness: Option<ParamValue>,
    /// Raw approximation settings (slots 7 and 8).
    pub approximation: [Option<FeatureValue>; 2],
}

/// A face draft.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFaceDraft {
    /// Pull direction.
    pub pull_direction: Vec<f64>,
    /// Drafted faces.
    pub faces: Vec<Option<FeatureNaming>>,
    /// Fixed edges.
    pub fixed_edges: Option<Vec<Option<FeatureNaming>>>,
    /// Draft angle parameter.
    pub angle: Option<ParamValue>,
    /// Mode (`None` for an unnamed code).
    pub mode: Option<FeatureDraftMode>,
    /// Fixed plane face reference.
    pub fixed_plane: Option<FeatureNaming>,
    /// Parting tool name.
    pub parting_tool: Option<String>,
}

/// A direct-edit move of faces.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureMoveFace {
    /// Moved faces.
    pub faces: Vec<Option<FeatureNaming>>,
    /// Raw move type.
    pub mode: Option<FeatureValue>,
    /// Move transform.
    pub transform: Option<Matrix4>,
}

/// An unwrap of faces.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureUnwrap {
    /// Unwrapped faces.
    pub faces: Vec<Option<FeatureNaming>>,
    /// Result body.
    pub body: Option<String>,
    /// Merge result.
    pub merge_result: Option<bool>,
    /// Automatic face chain.
    pub auto_face_chain: Option<bool>,
    /// Placement transform.
    pub transform: Option<Matrix4>,
}

/// `transform3d(node(k))` when slot `k` holds a node.
pub(super) fn slot_transform(s: &Slots, k: usize) -> Option<Matrix4> {
    s.node(k).and_then(|n| transform(n, "transformation"))
}

/// Face references of slot `k`'s face set.
pub(super) fn slot_faces(s: &Slots, k: usize) -> Vec<Option<FeatureNaming>> {
    s.ctx.face_list(s.node(k))
}

pub(super) fn read_hole(s: &Slots) -> FeatureHole {
    let taper = (s.at(22) != 0 || s.at(23) != 0).then(|| [s.param(22), s.param(23)]);
    FeatureHole {
        seat: FeatureHoleSeat::from_value(s.value(0)),
        diameter: s.param(1),
        depth: s.param(2),
        seat_diameter: s.param(3),
        seat_depth: s.param(4),
        countersink_angle: s.param(5),
        drill_point_angle: s.param(6),
        tap_or_major: s.param(20),
        extent: FeatureExtent::from_value(s.value(9)),
        direction: slot_vec(s, 16, "dir"),
        placement: slot_transform(s, 8),
        thread: s.ctx.thread_info(s.index),
        taper_thread: taper,
        slot_length: (s.at(30) != 0).then(|| s.param(30)),
    }
}

pub(super) fn read_thread(s: &Slots) -> FeatureThread {
    FeatureThread {
        faces: slot_faces(s, 0),
        full_length: s.flag(1).unwrap_or(false),
        length: s.param(3),
        offset: s.param(4),
        thread: s.ctx.thread_info(s.index),
    }
}

pub(super) fn read_chamfer(s: &Slots) -> FeatureChamfer {
    FeatureChamfer {
        method: FeatureChamferType::from_value(s.value(4)),
        edges: s.ctx.edges(s.at(0)),
        distance1: s.param(2),
        distance2: s.param(3),
        angle: s.param(10),
        face: s.node(1).map(|n| s.ctx.face_list(Some(n))),
    }
}

pub(super) fn read_shell(s: &Slots) -> FeatureShell {
    FeatureShell {
        direction: FeatureShellDirection::from_value(s.value(0)),
        removed_faces: slot_faces(s, 1),
        thickness: s.param(3),
        approximation: [s.value(7), s.value(8)],
    }
}

pub(super) fn read_draft(s: &Slots) -> FeatureFaceDraft {
    let mode = FeatureDraftMode::from_value(s.value(15)).filter(|m| m.label().is_some());
    FeatureFaceDraft {
        pull_direction: s.node(0).map(|n| floats(n, "dir")).unwrap_or_default(),
        faces: slot_faces(s, 1),
        fixed_edges: s.ctx.edges(s.at(2)),
        angle: s.param(3),
        mode,
        fixed_plane: s.ctx.facerefs(s.at(10)),
        parting_tool: s.name(11),
    }
}

pub(super) fn read_move_face(s: &Slots) -> FeatureMoveFace {
    FeatureMoveFace {
        faces: slot_faces(s, 0),
        mode: s.value(1),
        transform: slot_transform(s, 12),
    }
}

pub(super) fn read_unwrap(s: &Slots) -> FeatureUnwrap {
    FeatureUnwrap {
        faces: slot_faces(s, 0),
        body: s.name(1),
        merge_result: s.flag(2),
        auto_face_chain: s.flag(8),
        transform: slot_transform(s, 10),
    }
}

/// One edge set of a fillet.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureFilletSet {
    /// Constant radius edges.
    Constant {
        /// Edges.
        edges: Option<Vec<Option<FeatureNaming>>>,
        /// Radius parameter.
        radius: Option<ParamValue>,
        /// G1 continuity.
        g1: Option<bool>,
    },
    /// Variable radius edges.
    Variable {
        /// Edges.
        edges: Option<Vec<Option<FeatureNaming>>>,
        /// Radius points.
        radii: Vec<FeatureFilletPoint>,
    },
    /// Face or full round fillet.
    Faces {
        /// Face references per side (`facesSide1`, `facesCenter`, `facesSide2`), as stored.
        faces: Vec<(&'static str, Vec<Option<FeatureNaming>>)>,
        /// Radius parameter.
        radius: Option<ParamValue>,
    },
}

/// A radius point of a variable fillet.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFilletPoint {
    /// Point position.
    pub point: Vec<f64>,
    /// Radius parameter.
    pub radius: Option<ParamValue>,
}

/// A corner setback of a fillet.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFilletSetback {
    /// Edges meeting at the corner.
    pub edges: Option<Vec<Option<FeatureNaming>>>,
    /// Setback distance parameters.
    pub distances: Vec<Option<ParamValue>>,
    /// Per-distance flags.
    pub flags: Vec<bool>,
}

/// A fillet.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFillet {
    /// Fillet type.
    pub fillet_type: Option<super::values::FeatureFilletType>,
    /// Edge and face sets.
    pub sets: Vec<FeatureFilletSet>,
    /// Corner setbacks.
    pub setbacks: Vec<FeatureFilletSetback>,
    /// Body name.
    pub body: Option<String>,
}

fn sub_ref(s: &Slots, index: usize, name: &str) -> usize {
    s.ctx
        .node(index)
        .and_then(|n| node_ref(n, name))
        .unwrap_or(0)
}

fn sub_list(s: &Slots, index: usize, name: &str) -> Vec<usize> {
    s.ctx
        .node(index)
        .map(|n| ref_list(n, name))
        .unwrap_or_default()
}

fn constant_set(s: &Slots, si: usize) -> FeatureFilletSet {
    let g1 = sub_ref(s, si, "continuityG1");
    FeatureFilletSet::Constant {
        edges: s.ctx.edges(sub_ref(s, si, "edges")),
        radius: s.ctx.param(sub_ref(s, si, "radius")),
        g1: (g1 != 0).then(|| s.ctx.flag(g1).unwrap_or(false)),
    }
}

fn variable_set(s: &Slots, si: usize) -> FeatureFilletSet {
    let radii = sub_ref(s, si, "radii");
    let mut points = sub_list(s, radii, "extrema");
    points.extend(sub_list(s, radii, "additional"));
    let point = |e: usize| FeatureFilletPoint {
        point: s
            .ctx
            .node(sub_ref(s, e, "point"))
            .map(|p| floats(p, "pos"))
            .unwrap_or_default(),
        radius: s.ctx.param(sub_ref(s, e, "radius")),
    };
    FeatureFilletSet::Variable {
        edges: s.ctx.edges(sub_ref(s, si, "edges")),
        radii: points.into_iter().map(point).collect(),
    }
}

fn faces_set(s: &Slots) -> FeatureFilletSet {
    let mut faces = Vec::new();
    for key in ["facesSide1", "facesCenter", "facesSide2"] {
        let r = sub_ref(s, s.at(10), key);
        if r != 0 {
            let refs = sub_list(s, r, "lst0")
                .into_iter()
                .map(|e| s.ctx.facerefs(sub_ref(s, e, "ref_1")));
            faces.push((key, refs.collect()));
        }
    }
    FeatureFilletSet::Faces {
        faces,
        radius: s.param(12),
    }
}

fn fillet_sets(s: &Slots) -> Vec<FeatureFilletSet> {
    let mut sets: Vec<FeatureFilletSet> = sub_list(s, s.at(0), "sets")
        .into_iter()
        .map(|i| constant_set(s, i))
        .collect();
    sets.extend(
        sub_list(s, s.at(1), "sets")
            .into_iter()
            .map(|i| variable_set(s, i)),
    );
    if s.node(10).is_some() {
        sets.push(faces_set(s));
    }
    sets
}

fn setback(s: &Slots) -> FeatureFilletSetback {
    let corner = s.at(9);
    FeatureFilletSetback {
        edges: s.ctx.edges(sub_ref(s, corner, "edgeProxies")),
        distances: sub_list(s, corner, "radii")
            .into_iter()
            .map(|r| s.ctx.param(r))
            .collect(),
        flags: sub_list(s, corner, "booleans")
            .into_iter()
            .map(|b| s.ctx.flag(b).unwrap_or(false))
            .collect(),
    }
}

pub(super) fn read_fillet(s: &Slots) -> FeatureFillet {
    let body = s
        .node(15)
        .map(|n| ref_list(n, "items").first().copied().unwrap_or(0));
    FeatureFillet {
        fillet_type: super::values::FeatureFilletType::from_value(s.value(11)),
        sets: fillet_sets(s),
        setbacks: s.node(9).map(|_| setback(s)).into_iter().collect(),
        body: body.and_then(|b| s.ctx.name(b)),
    }
}
