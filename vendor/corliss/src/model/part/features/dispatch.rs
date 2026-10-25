//! Telling apart the features stored as 90874D91 nodes by the node types in their slots.
//!
//! Most solid, surface, sheet metal and direct-edit features share the extrusion node type;
//! the reference reader recognises each by its slot signature, first match wins (spec 15.3).

use super::context::Slots;
use super::{FeatureKind, bodies, placed, sheet_metal, sketched};

const T_LOFT_SECTIONS: u32 = 0x312F_9E50;
const T_SWEEP_PATH: u32 = 0xA477_243B;
const T_BODY_SET: u32 = 0x0E68_70AE;
const T_MARK_SET: u32 = 0x1462_D052;
const T_HOLE_SEAT: u32 = 0x43CD_7C11;
const T_FACE_SET: u32 = 0x24BC_B2F1;
const T_THREAD_MARK: u32 = 0x4C47_8499;
const T_MOVE_TYPE: u32 = 0xEF82_79FB;
const T_BOOLEAN: u32 = 0xD5DA_AA83;
const T_OPERATION: u32 = 0x729A_BE28;
const T_SPLIT_METHOD: u32 = 0xB8E1_9019;
const T_THICKEN_FACES: u32 = 0xEFF2_257A;
const T_FILLET_CONST: u32 = 0x1B48_E9DA;
const T_FILLET_VAR: u32 = 0xCA7A_A850;
const T_FILLET_FACES: u32 = 0xD524_C30A;
const T_CHAMFER_TYPE: u32 = 0x7DAA_0032;
const T_SHELL_DIR: u32 = 0xB3A1_69E4;
const T_DIRECTION: u32 = 0xCE52_DF40;
const T_VALUE: u32 = 0xBB1D_D5DF;
const T_BODY: u32 = 0x9087_4D47;
const T_PLATE: u32 = 0xFC20_3F47;
const T_PLATE_MARK: u32 = 0x5E50_B969;
const T_SEAM: u32 = 0xAFD8_A8E0;
const T_EDGE_SET: u32 = 0x9087_4D51;
const T_BEND_A: u32 = 0xA96B_5992;
const T_BEND_B: u32 = 0x0455_B440;
const T_CORNER: u32 = 0x8677_CE83;
const T_HEM: u32 = 0xB310_B8E5;
const T_FLAT_A: u32 = 0xD776_DFD1;
const T_FLAT_B: u32 = 0xCE59_B7F5;
const T_PATCH: u32 = 0x8B2B_8D96;
const T_PARAMETER: u32 = 0x9087_4D26;
const T_SCULPT: u32 = 0x5FB2_5A7E;
const T_RULED_A: u32 = 0x671C_E131;
const T_RULED_TYPE: u32 = 0x9C38_036E;
const T_TRIM: u32 = 0xEA68_0672;
const T_EXTEND: u32 = 0x34FA_B548;
const T_MOVE_BODIES: u32 = 0x0800_FE29;
const T_COPY_FLAG: u32 = 0x9087_4D28;
const T_PROFILE_PROXY: u32 = 0x2294_7391;
const T_EMBOSS_MARK: u32 = 0x1FBB_3C01;
const T_AXIS3D: u32 = 0x8EF0_6C89;
const T_COIL_METHOD: u32 = 0x4FB1_0CB8;

type Matches = fn(&Slots) -> bool;
type Reads = fn(&Slots) -> FeatureKind;

/// Slot signatures in the reference reader's order.
const RULES: &[(Matches, Reads)] = &[
    (
        |s| s.is(0, T_LOFT_SECTIONS),
        |s| FeatureKind::Loft(sketched::read_loft(s)),
    ),
    (
        |s| s.is(1, T_SWEEP_PATH),
        |s| FeatureKind::Sweep(sketched::read_sweep(s)),
    ),
    (|s| s.filled() == 1 && s.is(0, T_BODY_SET), bodies::tagging),
    (|s| s.is(0, T_MARK_SET), bodies::mark),
    (
        |s| s.is(0, T_HOLE_SEAT),
        |s| FeatureKind::Hole(Box::new(placed::read_hole(s))),
    ),
    (
        |s| s.is(0, T_FACE_SET) && s.is(10, T_THREAD_MARK),
        |s| FeatureKind::Thread(placed::read_thread(s)),
    ),
    (
        |s| s.is(0, T_FACE_SET) && s.is(1, T_MOVE_TYPE),
        |s| FeatureKind::MoveFace(placed::read_move_face(s)),
    ),
    (
        |s| s.is(0, T_FACE_SET) && s.is(3, T_BOOLEAN) && s.filled() == 4,
        bodies::delete_face,
    ),
    (
        |s| s.is(0, T_BOOLEAN) && s.is(1, T_BODY_SET) && s.is(2, T_OPERATION),
        bodies::combine,
    ),
    (|s| s.is(0, T_SPLIT_METHOD), bodies::split),
    (|s| s.is(0, T_THICKEN_FACES), bodies::thicken),
    (is_fillet, |s| FeatureKind::Fillet(placed::read_fillet(s))),
    (
        |s| s.is(4, T_CHAMFER_TYPE),
        |s| FeatureKind::Chamfer(placed::read_chamfer(s)),
    ),
    (
        |s| s.is(0, T_SHELL_DIR),
        |s| FeatureKind::Shell(placed::read_shell(s)),
    ),
    (
        |s| s.is(0, T_DIRECTION) && s.is(1, T_FACE_SET),
        |s| FeatureKind::FaceDraft(placed::read_draft(s)),
    ),
    (is_pattern_body, bodies::pattern_body),
    (
        |s| s.is(0, T_PLATE) && s.is(2, T_PLATE_MARK),
        sheet_metal::plate,
    ),
    (
        |s| s.is(0, T_SEAM) && s.is(5, T_EDGE_SET),
        sheet_metal::corner_seam,
    ),
    (
        |s| s.is(0, T_EDGE_SET) && (s.is(3, T_BEND_A) || s.is(3, T_BEND_B)),
        sheet_metal::bend,
    ),
    (|s| s.is(0, T_CORNER), sheet_metal::corner),
    (|s| s.is(3, T_HEM), sheet_metal::hem),
    (
        |s| s.is(7, T_FLAT_A) && s.is(8, T_FLAT_B),
        sheet_metal::flat_pattern,
    ),
    (|s| s.is(1, T_PATCH), bodies::patch),
    (
        |s| s.is(0, T_BODY_SET) && s.is(1, T_BOOLEAN) && s.is(4, T_PARAMETER),
        bodies::stitch,
    ),
    (|s| s.is(0, T_SCULPT), bodies::sculpt),
    (
        |s| s.is(1, T_RULED_A) && s.is(2, T_RULED_TYPE),
        bodies::ruled,
    ),
    (|s| s.is(0, T_TRIM), bodies::trim),
    (|s| s.is(1, T_EXTEND), bodies::extend),
    (
        |s| s.is(0, T_FACE_SET) && s.is(1, T_FACE_SET) && s.is(2, T_BOOLEAN),
        bodies::replace_face,
    ),
    (|s| s.is(1, T_MOVE_BODIES), bodies::move_bodies),
    (
        |s| s.is(0, T_BODY) && s.is(1, T_BODY) && s.is(2, T_COPY_FLAG),
        bodies::copy_object,
    ),
    (is_bend_part, sheet_metal::bend_part),
    (
        |s| s.is(0, T_FACE_SET),
        |s| FeatureKind::Unwrap(placed::read_unwrap(s)),
    ),
    (
        |s| s.filled() <= 3 && s.is(0, T_BODY) && s.is(1, T_BODY),
        bodies::base_body,
    ),
    (
        |s| s.is(13, T_EMBOSS_MARK),
        |s| FeatureKind::Emboss(sketched::read_emboss(s)),
    ),
    (
        |s| s.is(0, T_PROFILE_PROXY) && s.is(1, T_PROFILE_PROXY),
        |s| FeatureKind::Rib(sketched::read_rib(s)),
    ),
    (|s| s.is(2, T_AXIS3D), read_revolution),
];

fn is_fillet(s: &Slots) -> bool {
    s.is(0, T_FILLET_CONST) || s.is(1, T_FILLET_VAR) || s.is(10, T_FILLET_FACES)
}

fn is_pattern_body(s: &Slots) -> bool {
    s.is(0, T_VALUE) && s.is(1, T_VALUE) && s.is(2, T_VALUE) && s.is(3, T_BODY)
}

fn is_bend_part(s: &Slots) -> bool {
    s.is(0, T_PROFILE_PROXY) && s.is(1, T_PARAMETER) && s.is(2, T_PARAMETER)
}

fn read_revolution(s: &Slots) -> FeatureKind {
    if s.is(5, T_COIL_METHOD) {
        return FeatureKind::Coil(Box::new(sketched::read_coil(s)));
    }
    FeatureKind::Revolve(sketched::read_revolve(s))
}

/// Reads the feature a 90874D91 node stores; an extrusion when no other signature matches.
pub(super) fn read_slotted(s: &Slots) -> FeatureKind {
    match RULES.iter().find(|(matches, _)| matches(s)) {
        Some((_, read)) => read(s),
        None => FeatureKind::Extrusion(sketched::read_extrusion(s)),
    }
}
