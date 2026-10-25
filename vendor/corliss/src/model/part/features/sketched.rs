//! Sketched features: extrusion, revolution, coil, loft, sweep, emboss and rib (spec 15.3).

use crate::model::part::ParamValue;
use crate::model::view::{floats, node_ref, ref_list};

use super::context::{FeatureProfile, FeatureWire, Slots};
use super::values::{
    FeatureCoilMethod, FeatureEmbossOperation, FeatureExtent, FeatureLoftType, FeatureOperation,
    FeatureRevolveDirection, FeatureRevolveExtent, FeatureSweepOrientation, FeatureValue,
};

/// Body node type.
const T_BODY: u32 = 0x9087_4D47;
/// Open profile node type (rib profiles).
const T_OPEN_PROFILE: u32 = 0xD617_32C1;
/// Parameter node type.
const T_PARAMETER: u32 = 0x9087_4D26;

/// An extrusion.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureExtrusion {
    /// Boolean operation.
    pub operation: Option<FeatureOperation>,
    /// Extent of the first direction.
    pub extent: Option<FeatureExtent>,
    /// Distance parameter.
    pub distance: Option<ParamValue>,
    /// Taper angle parameter.
    pub taper: Option<ParamValue>,
    /// Direction flipped.
    pub flipped: Option<bool>,
    /// Symmetric extent.
    pub symmetric: Option<bool>,
    /// Extrusion direction.
    pub direction: Option<Vec<f64>>,
    /// Point on the from face.
    pub from_face: Option<Vec<f64>>,
    /// Point on the to face.
    pub to_face: Option<Vec<f64>>,
    /// The second direction, when set.
    pub direction_b: Option<FeatureExtrusionSide>,
    /// Result body name.
    pub body: Option<String>,
    /// The extruded profile.
    pub profile: Option<FeatureProfile>,
}

/// The second direction of a two-direction extrusion.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureExtrusionSide {
    /// Distance parameter.
    pub distance: Option<ParamValue>,
    /// Taper parameter.
    pub taper: Option<ParamValue>,
    /// Extent.
    pub extent: Option<FeatureExtent>,
}

/// Operation and result body every revolve, coil, loft and sweep stores.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSolidBase {
    /// Boolean operation.
    pub operation: Option<FeatureOperation>,
    /// Result body: `None` when the body slot is empty, `Some(None)` for an unnamed body.
    pub body: Option<Option<String>>,
}

/// A revolution axis.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureAxis {
    /// Point on the axis.
    pub pos: Vec<f64>,
    /// Axis direction.
    pub dir: Vec<f64>,
}

/// A revolution.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureRevolve {
    /// Operation and body.
    pub base: FeatureSolidBase,
    /// Extent.
    pub extent: Option<FeatureRevolveExtent>,
    /// Angle parameter.
    pub angle: Option<ParamValue>,
    /// Direction.
    pub direction: Option<FeatureRevolveDirection>,
    /// Axis.
    pub axis: Option<FeatureAxis>,
    /// Revolved profile.
    pub profile: Option<FeatureProfile>,
}

/// One end of a coil.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureCoilEnd {
    /// Closed end.
    pub closed: Option<bool>,
    /// Transition angle parameter.
    pub transition: Option<ParamValue>,
    /// Flat angle parameter.
    pub flat: Option<ParamValue>,
}

/// A coil.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureCoil {
    /// Operation and body.
    pub base: FeatureSolidBase,
    /// Coil method.
    pub method: Option<FeatureCoilMethod>,
    /// Rotation flag.
    pub rotation_flag: Option<bool>,
    /// Pitch parameter.
    pub pitch: Option<ParamValue>,
    /// Height parameter.
    pub height: Option<ParamValue>,
    /// Revolutions parameter.
    pub revolutions: Option<ParamValue>,
    /// Taper parameter.
    pub taper: Option<ParamValue>,
    /// Start end.
    pub start: FeatureCoilEnd,
    /// Finish end.
    pub end: FeatureCoilEnd,
    /// Axis.
    pub axis: Option<FeatureAxis>,
    /// Coiled profile.
    pub profile: Option<FeatureProfile>,
}

/// One loft section.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureLoftSection {
    /// The section wire.
    pub wire: FeatureWire,
    /// Weight (impact) parameter.
    pub weight: Option<ParamValue>,
    /// Angle parameter.
    pub angle: Option<ParamValue>,
    /// Direction reversed.
    pub reversed: Option<bool>,
    /// Inferred condition: free when the weight is zero, else direction.
    pub free: bool,
}

/// A loft.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureLoft {
    /// Operation and body.
    pub base: FeatureSolidBase,
    /// Loft type.
    pub loft_type: Option<FeatureLoftType>,
    /// Closed loop.
    pub closed_loop: Option<bool>,
    /// Merge tangent faces.
    pub merge_tangent_faces: Option<bool>,
    /// Sections.
    pub sections: Vec<FeatureLoftSection>,
}

/// A sweep.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSweep {
    /// Operation and body.
    pub base: FeatureSolidBase,
    /// Taper parameter.
    pub taper: Option<ParamValue>,
    /// Twist parameter (the parameter created right after the taper).
    pub twist: Option<ParamValue>,
    /// Orientation.
    pub orientation: Option<FeatureSweepOrientation>,
    /// Path wire.
    pub path: FeatureWire,
    /// Swept profile.
    pub profile: Option<FeatureProfile>,
}

/// An emboss or engrave.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEmboss {
    /// Emboss or engrave.
    pub operation: Option<FeatureEmbossOperation>,
    /// From the sketch plane.
    pub from_plane: Option<bool>,
    /// Depth parameter.
    pub depth: Option<ParamValue>,
    /// Taper parameter.
    pub taper: Option<ParamValue>,
    /// Direction.
    pub direction: Option<Vec<f64>>,
    /// Direction flipped.
    pub flipped: Option<bool>,
    /// Wrap to face.
    pub wrap_to_face: Option<bool>,
    /// Faces wrapped onto.
    pub wrap_faces: Option<Vec<Option<super::FeatureNaming>>>,
    /// Slots 4 and 12 flags.
    pub flags: [Option<bool>; 2],
    /// Embossed profile.
    pub profile: Option<FeatureProfile>,
}

/// A rib or web.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureRib {
    /// Normal to the sketch plane (else parallel).
    pub normal: bool,
    /// Thickness parameter.
    pub thickness: Option<ParamValue>,
    /// Raw extent code.
    pub extent: Option<FeatureValue>,
    /// Extent distance parameter.
    pub extent_distance: Option<ParamValue>,
    /// Symmetric thickness.
    pub symmetric_thickness: Option<bool>,
    /// Direction.
    pub direction: Option<Vec<f64>>,
    /// Taper parameter.
    pub taper: Option<ParamValue>,
    /// Boss parameters.
    pub boss: [Option<ParamValue>; 3],
    /// Slots 2 and 10 flags.
    pub flags: [Option<bool>; 2],
    /// The open profile the rib follows, when one names it.
    pub profile: Option<FeatureProfile>,
}

/// The vector field `name` of the node in slot `k`, when the slot holds a node.
pub(super) fn slot_vec(s: &Slots, k: usize, name: &str) -> Option<Vec<f64>> {
    s.node(k).map(|n| floats(n, name))
}

pub(super) fn read_extrusion(s: &Slots) -> FeatureExtrusion {
    let direction_b = (s.at(27) != 0).then(|| FeatureExtrusionSide {
        distance: s.param(27),
        taper: s.param(28),
        extent: FeatureExtent::from_value(s.value(29)),
    });
    FeatureExtrusion {
        operation: FeatureOperation::from_value(s.value(0)),
        extent: FeatureExtent::from_value(s.value(6)),
        distance: s.param(4),
        taper: s.param(5),
        flipped: s.flag(3),
        symmetric: s.flag(7),
        direction: slot_vec(s, 2, "dir"),
        from_face: slot_vec(s, 17, "pos"),
        to_face: slot_vec(s, 18, "pos"),
        direction_b,
        body: s.ctx.name(extrusion_body(s)),
        profile: s.ctx.profile_of_proxy(s.at(1)),
    }
}

fn extrusion_body(s: &Slots) -> usize {
    if s.is(16, T_BODY) {
        return s.at(16);
    }
    s.node(26)
        .map(|n| ref_list(n, "items").first().copied().unwrap_or(0))
        .unwrap_or(0)
}

fn solid_base(s: &Slots, op_slot: usize, body_slot: usize) -> FeatureSolidBase {
    let body = s.node(body_slot).map(|b| {
        if b.type_id.0 == T_BODY {
            return s.name(body_slot);
        }
        ref_list(b, "items").first().and_then(|&i| s.ctx.name(i))
    });
    FeatureSolidBase {
        operation: FeatureOperation::from_value(s.value(op_slot)),
        body,
    }
}

fn axis(s: &Slots) -> Option<FeatureAxis> {
    s.node(2).map(|ax| FeatureAxis {
        pos: floats(ax, "pos"),
        dir: floats(ax, "dir"),
    })
}

pub(super) fn read_revolve(s: &Slots) -> FeatureRevolve {
    FeatureRevolve {
        base: solid_base(s, 0, 17),
        extent: FeatureRevolveExtent::from_value(s.value(3)),
        angle: s.param(4),
        direction: FeatureRevolveDirection::from_value(s.value(5)),
        axis: axis(s),
        profile: s.ctx.profile_of_proxy(s.at(1)),
    }
}

fn coil_end(s: &Slots, first: usize) -> FeatureCoilEnd {
    FeatureCoilEnd {
        closed: s.flag(first),
        transition: s.param(first + 1),
        flat: s.param(first + 2),
    }
}

pub(super) fn read_coil(s: &Slots) -> FeatureCoil {
    FeatureCoil {
        base: solid_base(s, 0, 19),
        method: FeatureCoilMethod::from_value(s.value(5)),
        rotation_flag: s.flag(3),
        pitch: s.param(6),
        height: s.param(7),
        revolutions: s.param(8),
        taper: s.param(9),
        start: coil_end(s, 10),
        end: coil_end(s, 13),
        axis: axis(s),
        profile: s.ctx.profile_of_proxy(s.at(1)),
    }
}

fn loft_section(s: &Slots, index: usize) -> FeatureLoftSection {
    let sn = s.ctx.node(index);
    let r = |name: &str| sn.and_then(|n| node_ref(n, name)).unwrap_or(0);
    let weight = s.ctx.param(r("impact"));
    let free = weight
        .as_ref()
        .is_some_and(|w| w.value.is_none_or(|v| v == 0.0));
    let rev = r("directionReversed");
    FeatureLoftSection {
        wire: s.ctx.sections(sn),
        angle: s.ctx.param(r("angle")),
        reversed: (rev != 0).then(|| s.ctx.flag(rev).unwrap_or(false)),
        weight,
        free,
    }
}

pub(super) fn read_loft(s: &Slots) -> FeatureLoft {
    let body_slot = if s.at(6) != 0 { 6 } else { 13 };
    let items = s.node(0).map(|n| ref_list(n, "items")).unwrap_or_default();
    FeatureLoft {
        base: solid_base(s, 1, body_slot),
        loft_type: FeatureLoftType::from_value(s.value(10)),
        closed_loop: s.flag(2),
        merge_tangent_faces: s.flag(9),
        sections: items.into_iter().map(|i| loft_section(s, i)).collect(),
    }
}

pub(super) fn read_sweep(s: &Slots) -> FeatureSweep {
    let taper = s.at(3);
    let follows =
        taper != 0 && taper + 1 < s.ctx.node_count() && s.ctx.tid(taper + 1) == Some(T_PARAMETER);
    FeatureSweep {
        base: solid_base(s, 2, 15),
        taper: s.param(3),
        twist: if follows {
            s.ctx.param(taper + 1)
        } else {
            None
        },
        orientation: FeatureSweepOrientation::from_value(s.value(8)),
        path: s.ctx.sections(s.node(1)),
        profile: s.ctx.profile_of_proxy(s.at(0)),
    }
}

pub(super) fn read_emboss(s: &Slots) -> FeatureEmboss {
    FeatureEmboss {
        operation: FeatureEmbossOperation::from_value(s.value(0)),
        from_plane: s.flag(7),
        depth: s.param(5),
        taper: s.param(6),
        direction: slot_vec(s, 2, "dir"),
        flipped: s.flag(3),
        wrap_to_face: s.flag(8),
        wrap_faces: s.node(9).map(|n| s.ctx.face_list(Some(n))),
        flags: [s.flag(4), s.flag(12)],
        profile: s.ctx.profile_of_proxy(s.at(1)),
    }
}

pub(super) fn read_rib(s: &Slots) -> FeatureRib {
    let normal = s.flag(5).unwrap_or(false);
    let (thick, dist) = if normal { (6, 3) } else { (3, 6) };
    FeatureRib {
        normal,
        thickness: s.param(thick),
        extent: s.value(4),
        extent_distance: s.param(dist),
        symmetric_thickness: s.flag(12),
        direction: slot_vec(s, 7, "dir"),
        taper: s.param(11),
        boss: [s.param(13), s.param(14), s.param(15)],
        flags: [s.flag(2), s.flag(10)],
        profile: rib_profile(s),
    }
}

/// The last open profile whose proxy is one of the rib's profile slots.
fn rib_profile(s: &Slots) -> Option<FeatureProfile> {
    let slots = [s.at(0), s.at(1)];
    let names = |r: Option<usize>| r.is_some_and(|r| slots.contains(&r));
    let open = s.ctx.part.view().by_type(T_OPEN_PROFILE);
    let matching =
        open.filter(|(_, op)| names(node_ref(op, "proxy1")) || names(node_ref(op, "proxy2")));
    matching.last().and_then(|(i, _)| s.ctx.profile(i))
}
