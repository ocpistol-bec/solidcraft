//! The feature kinds and their typed definitions.

use crate::model::Matrix4;
use crate::model::part::ParamValue;

use super::bodies::{
    FeatureFaceOffset, FeaturePatchLoop, FeatureRuledSurface, FeatureSculptMode, FeatureSculptTool,
};
use super::context::{FeatureNaming, FeatureWire};
use super::patterns::{
    FeatureCircularPattern, FeatureMirrorPlane, FeaturePatternBase, FeaturePatternDirection,
    FeatureSketchPattern,
};
use super::placed::{
    FeatureChamfer, FeatureFaceDraft, FeatureFillet, FeatureHole, FeatureMoveFace, FeatureShell,
    FeatureThread, FeatureUnwrap,
};
use super::sheet_metal::FeatureSheetMetalGroup;
use super::sketched::{
    FeatureCoil, FeatureEmboss, FeatureExtrusion, FeatureLoft, FeatureRevolve, FeatureRib,
    FeatureSweep,
};
use super::sources::{FeatureDecalImage, FeatureFinishParameter};
use super::values::{FeatureExtent, FeatureOperation, FeatureSplitMethod, FeatureValue};

/// Edge references of an edge set (`None` when the slot is empty).
type Edges = Option<Vec<Option<FeatureNaming>>>;

/// What a feature is, with its definition.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureKind {
    /// Derived part (255D7ED7).
    DerivedPart {
        /// Source file name.
        file: Option<String>,
        /// Scale parameter.
        scale: Option<ParamValue>,
        /// Placement transform.
        transform: Option<Matrix4>,
    },
    /// Imported reference file (9C9E3CB3).
    Import {
        /// Source file path.
        file: Option<String>,
    },
    /// Decal (7F936BAA).
    Decal {
        /// Sketch holding the image.
        sketch: Option<String>,
        /// Images.
        images: Vec<FeatureDecalImage>,
        /// Target face reference.
        face: Option<FeatureNaming>,
        /// Wrap to face.
        wrap_to_face: Option<bool>,
    },
    /// Finish (CBA26FC8).
    Finish {
        /// Finished faces, when found.
        faces: Option<Vec<Option<FeatureNaming>>>,
        /// Appearance, area and comment parameters, when found.
        parameters: Option<Vec<FeatureFinishParameter>>,
    },
    /// Sheet metal punch tool (an inserted punch iFeature).
    PunchTool {
        /// Punch parameters.
        parameters: Vec<Option<ParamValue>>,
        /// Placement transform.
        transform: Option<Matrix4>,
    },
    /// Sheet metal face, flange or contour flange grouping its member features.
    SheetMetalGroup {
        /// Which command.
        group: FeatureSheetMetalGroup,
        /// Member feature names.
        members: Vec<Option<String>>,
    },
    /// Rectangular pattern.
    RectangularPattern {
        /// Common pattern data.
        base: FeaturePatternBase,
        /// First direction.
        direction_a: FeaturePatternDirection,
        /// Second direction.
        direction_b: FeaturePatternDirection,
        /// New solid flag, stored for solid patterns only.
        new_solid: Option<Option<bool>>,
    },
    /// Circular pattern.
    CircularPattern(FeatureCircularPattern),
    /// Mirror.
    Mirror {
        /// Common pattern data.
        base: FeaturePatternBase,
        /// Mirror plane features among the participants.
        plane_tool: Vec<Option<String>>,
        /// Mirror plane.
        plane: Option<FeatureMirrorPlane>,
        /// Slots 6 and 7 flags.
        flags: [Option<bool>; 2],
    },
    /// Sketch-driven pattern.
    SketchDrivenPattern(FeatureSketchPattern),
    /// Mesh feature container.
    MeshFeature {
        /// Mesh names.
        meshes: Vec<Option<String>>,
    },
    /// Repaired geometry.
    RepairBodies,
    /// Loft.
    Loft(FeatureLoft),
    /// Sweep.
    Sweep(FeatureSweep),
    /// Body tagging.
    Tagging {
        /// Tagged bodies.
        bodies: Vec<Option<String>>,
    },
    /// Mark.
    Mark {
        /// Slots 2, 4, 5 and 6 flags.
        flags: [Option<bool>; 4],
    },
    /// Hole or slot.
    Hole(Box<FeatureHole>),
    /// Cosmetic thread.
    Thread(FeatureThread),
    /// Direct-edit face move.
    MoveFace(FeatureMoveFace),
    /// Delete face.
    DeleteFace {
        /// Deleted faces.
        faces: Vec<Option<FeatureNaming>>,
        /// Heal the gap.
        heal: bool,
    },
    /// Combine bodies.
    Combine {
        /// Boolean operation.
        operation: Option<FeatureOperation>,
        /// Tool bodies.
        tool_bodies: Vec<Option<String>>,
        /// Keep tool bodies.
        keep_toolbodies: Option<bool>,
    },
    /// Split.
    Split {
        /// Method.
        method: Option<FeatureSplitMethod>,
        /// Split all faces.
        all_faces: bool,
        /// Split tool.
        tool: Option<String>,
    },
    /// Thicken or offset faces.
    Thicken {
        /// Boolean operation.
        operation: Option<FeatureOperation>,
        /// Distance parameter.
        distance: Option<ParamValue>,
        /// Face sets.
        face_sets: Vec<FeatureFaceOffset>,
    },
    /// Fillet.
    Fillet(FeatureFillet),
    /// Chamfer.
    Chamfer(FeatureChamfer),
    /// Shell.
    Shell(FeatureShell),
    /// Face draft.
    FaceDraft(FeatureFaceDraft),
    /// A body a pattern created.
    PatternResultBody {
        /// Occurrence ids.
        ids: [Option<f64>; 3],
        /// Body name.
        body: Option<String>,
    },
    /// Sheet metal plate.
    SheetMetalPlate {
        /// Thickness parameter.
        thickness: Option<ParamValue>,
        /// Direction.
        direction: Option<Vec<f64>>,
        /// Feature dimensions.
        dimensions: Vec<ParamValue>,
        /// Boolean operation.
        operation: Option<FeatureOperation>,
    },
    /// Corner seam.
    CornerSeam {
        /// Raw gap type.
        gap_type: Option<FeatureValue>,
        /// Thickness parameter.
        thickness: Option<ParamValue>,
        /// Gap parameter.
        gap: Option<ParamValue>,
        /// Overlap parameter.
        overlap: Option<ParamValue>,
        /// Seamed edges.
        edges: Edges,
        /// Relief size parameter.
        relief_size: Option<ParamValue>,
        /// Raw relief type.
        relief_type: Option<FeatureValue>,
        /// Percent parameter.
        percent: Option<ParamValue>,
    },
    /// Sheet metal bend.
    SheetMetalBend {
        /// Radius parameter.
        radius: Option<ParamValue>,
        /// Thickness parameter.
        thickness: Option<ParamValue>,
        /// Bent edges.
        edges: Edges,
    },
    /// Sheet metal corner.
    SheetMetalCorner {
        /// Gap parameter.
        gap: Option<ParamValue>,
        /// Relief size parameter.
        relief_size: Option<ParamValue>,
        /// Raw relief type.
        relief_type: Option<FeatureValue>,
        /// Raw gap type.
        gap_type: Option<FeatureValue>,
    },
    /// Hem.
    Hem {
        /// Thickness parameter.
        thickness: Option<ParamValue>,
        /// Hemmed edges.
        edges: Edges,
        /// Feature dimensions.
        dimensions: Vec<ParamValue>,
    },
    /// Flat pattern.
    FlatPattern {
        /// Thickness parameter.
        thickness: Option<ParamValue>,
        /// Flat body name.
        flat_body: Option<String>,
        /// Folded body name.
        folded_body: Option<String>,
    },
    /// Boundary patch.
    BoundaryPatch {
        /// Loops.
        loops: Vec<FeaturePatchLoop>,
        /// Parameter.
        parameter: Option<ParamValue>,
        /// Result surface.
        surface: Option<String>,
    },
    /// Stitch surfaces.
    Stitch {
        /// Stitched surfaces.
        surfaces: Vec<Option<String>>,
        /// Tolerance parameter.
        tolerance: Option<ParamValue>,
        /// Slots 3 and 5 flags.
        flags: [Option<bool>; 2],
    },
    /// Sculpt.
    Sculpt {
        /// Tools.
        tools: Vec<FeatureSculptTool>,
        /// Mode.
        mode: FeatureSculptMode,
    },
    /// Ruled surface.
    RuledSurface(FeatureRuledSurface),
    /// Trim surface.
    Trim {
        /// Cutting tool.
        tool: Option<String>,
        /// Trimmed surface.
        surface: Option<String>,
    },
    /// Extend surface.
    Extend {
        /// Extended edges.
        edges: Edges,
        /// Extent.
        extent: Option<FeatureExtent>,
        /// Distance parameter.
        distance: Option<ParamValue>,
        /// Extended surface.
        surface: Option<String>,
    },
    /// Replace face.
    ReplaceFace {
        /// Replaced faces.
        existing: Vec<Option<FeatureNaming>>,
        /// New faces.
        new: Vec<Option<FeatureNaming>>,
    },
    /// Direct-edit move or scale of bodies.
    MoveBodies {
        /// Moved bodies.
        bodies: Vec<Option<String>>,
        /// Transforms.
        transforms: Vec<Option<Matrix4>>,
    },
    /// Copy object, or fit mesh face when the part has a mesh and the flag is set.
    CopyObject {
        /// Fit mesh face rather than copy object.
        fit_mesh_face: bool,
        /// Source reference node.
        source_ref: Option<usize>,
        /// Result body.
        result: Option<String>,
        /// Slot 2 flag.
        flag: bool,
    },
    /// Bend part.
    BendPart {
        /// Radius parameter.
        radius: Option<ParamValue>,
        /// Angle parameter.
        angle: Option<ParamValue>,
        /// Slots 4–7 flags.
        flags: [Option<bool>; 4],
        /// Bend line wire, when stored.
        bend_line: Option<FeatureWire>,
    },
    /// Unwrap.
    Unwrap(FeatureUnwrap),
    /// Base body of a derived, imported or repaired body.
    BaseBody {
        /// Body name.
        body: Option<String>,
        /// What created it.
        source: Option<String>,
    },
    /// Emboss or engrave.
    Emboss(FeatureEmboss),
    /// Rib.
    Rib(FeatureRib),
    /// Revolution.
    Revolve(FeatureRevolve),
    /// Coil.
    Coil(Box<FeatureCoil>),
    /// Extrusion.
    Extrusion(FeatureExtrusion),
}

impl FeatureKind {
    /// The reference reader's type label ("extrusion", "circular pattern", …).
    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::DerivedPart { .. } => "derived part",
            FeatureKind::Import { .. } => "import (reference)",
            FeatureKind::Decal { .. } => "decal",
            FeatureKind::Finish { .. } => "finish",
            FeatureKind::PunchTool { .. } => "punch tool",
            FeatureKind::SheetMetalGroup { group, .. } => group.type_name(),
            FeatureKind::RectangularPattern { .. } => "rectangular pattern",
            FeatureKind::CircularPattern(_) => "circular pattern",
            FeatureKind::Mirror { .. } => "mirror",
            FeatureKind::SketchDrivenPattern(_) => "sketch driven pattern",
            FeatureKind::MeshFeature { .. } => "mesh feature",
            FeatureKind::RepairBodies => "repair bodies",
            other => other.slotted_type_name(),
        }
    }

    fn slotted_type_name(&self) -> &'static str {
        match self {
            FeatureKind::Loft(_) => "loft",
            FeatureKind::Sweep(_) => "sweep",
            FeatureKind::Tagging { .. } => "tagging",
            FeatureKind::Mark { .. } => "mark",
            FeatureKind::Hole(h) if h.slot_length.is_some() => "slot",
            FeatureKind::Hole(_) => "hole",
            FeatureKind::Thread(_) => "thread",
            FeatureKind::MoveFace(_) => "direct edit (move)",
            FeatureKind::DeleteFace { .. } => "delete face",
            FeatureKind::Combine { .. } => "combine",
            FeatureKind::Split { .. } => "split",
            FeatureKind::Thicken { .. } => "thicken/offset",
            FeatureKind::Fillet(_) => "fillet",
            FeatureKind::Chamfer(_) => "chamfer",
            FeatureKind::Shell(_) => "shell",
            FeatureKind::FaceDraft(_) => "face draft",
            FeatureKind::PatternResultBody { .. } => "pattern result body",
            other => other.sheet_and_surface_type_name(),
        }
    }

    fn sheet_and_surface_type_name(&self) -> &'static str {
        match self {
            FeatureKind::SheetMetalPlate { .. } => "sheet metal plate",
            FeatureKind::CornerSeam { .. } => "corner seam",
            FeatureKind::SheetMetalBend { .. } => "sheet metal bend",
            FeatureKind::SheetMetalCorner { .. } => "sheet metal corner",
            FeatureKind::Hem { .. } => "hem",
            FeatureKind::FlatPattern { .. } => "flat pattern",
            FeatureKind::BoundaryPatch { .. } => "boundary patch",
            FeatureKind::Stitch { .. } => "stitch",
            FeatureKind::Sculpt { .. } => "sculpt",
            FeatureKind::RuledSurface(_) => "ruled surface",
            FeatureKind::Trim { .. } => "trim",
            FeatureKind::Extend { .. } => "extend",
            FeatureKind::ReplaceFace { .. } => "replace face",
            FeatureKind::MoveBodies { .. } => "move bodies",
            other => other.remaining_type_name(),
        }
    }

    fn remaining_type_name(&self) -> &'static str {
        match self {
            FeatureKind::CopyObject {
                fit_mesh_face: true,
                ..
            } => "fit mesh face",
            FeatureKind::CopyObject { .. } => "copy object",
            FeatureKind::BendPart { .. } => "bend part",
            FeatureKind::Unwrap(_) => "unwrap",
            FeatureKind::BaseBody { .. } => "base body",
            FeatureKind::Emboss(_) => "emboss",
            FeatureKind::Rib(_) => "rib",
            FeatureKind::Revolve(_) => "revolve",
            FeatureKind::Coil(_) => "coil",
            _ => "extrusion",
        }
    }
}

impl FeatureSheetMetalGroup {
    /// The reference reader's type label.
    pub fn type_name(self) -> &'static str {
        match self {
            FeatureSheetMetalGroup::Face => "sheet metal face",
            FeatureSheetMetalGroup::Flange => "sheet metal flange",
            FeatureSheetMetalGroup::ContourFlange => "sheet metal contour flange",
        }
    }
}
