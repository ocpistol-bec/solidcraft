//! Sheet metal features: plates, bends, corners, hems, flat patterns, bend part (spec 15.6).

use crate::model::view::{node_ref, ref_list};

use super::FeatureKind;
use super::context::Slots;
use super::sketched::slot_vec;
use super::values::FeatureOperation;

/// Which sheet metal command groups member features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSheetMetalGroup {
    /// Face (C4C14B90).
    Face,
    /// Flange (C3DDDC08).
    Flange,
    /// Contour flange (A03874B0).
    ContourFlange,
}

impl FeatureSheetMetalGroup {
    /// Node types of the grouping features, in the order the reference reader lists them.
    pub(super) const ALL: [(u32, FeatureSheetMetalGroup); 3] = [
        (0xC4C1_4B90, FeatureSheetMetalGroup::Face),
        (0xC3DD_DC08, FeatureSheetMetalGroup::Flange),
        (0xA038_74B0, FeatureSheetMetalGroup::ContourFlange),
    ];
}

pub(super) fn plate(s: &Slots) -> FeatureKind {
    FeatureKind::SheetMetalPlate {
        thickness: s.param(3),
        direction: slot_vec(s, 1, "dir"),
        dimensions: s.ctx.dims(s.at(8)),
        operation: FeatureOperation::from_value(s.value(16)),
    }
}

pub(super) fn corner_seam(s: &Slots) -> FeatureKind {
    FeatureKind::CornerSeam {
        gap_type: s.value(0),
        thickness: s.param(2),
        gap: s.param(3),
        overlap: s.param(4),
        edges: s.ctx.edges(s.at(5)),
        relief_size: s.param(12),
        relief_type: s.value(13),
        percent: s.param(18),
    }
}

pub(super) fn bend(s: &Slots) -> FeatureKind {
    FeatureKind::SheetMetalBend {
        radius: s.param(1),
        thickness: s.param(2),
        edges: s.ctx.edges(s.at(0)),
    }
}

pub(super) fn corner(s: &Slots) -> FeatureKind {
    let r = |name: &str| s.node(0).and_then(|c| node_ref(c, name)).unwrap_or(0);
    FeatureKind::SheetMetalCorner {
        gap: s.ctx.param(r("gapDim")),
        relief_size: s.ctx.param(r("reliefSize")),
        relief_type: s.ctx.value(r("reliefType")),
        gap_type: s.ctx.value(r("gapType")),
    }
}

pub(super) fn hem(s: &Slots) -> FeatureKind {
    FeatureKind::Hem {
        thickness: s.param(2),
        edges: s.ctx.edges(s.at(4)),
        dimensions: s.ctx.dims(s.at(7)),
    }
}

pub(super) fn flat_pattern(s: &Slots) -> FeatureKind {
    FeatureKind::FlatPattern {
        thickness: s.param(0),
        flat_body: s.name(3),
        folded_body: s.name(11),
    }
}

pub(super) fn bend_part(s: &Slots) -> FeatureKind {
    let lines = s.node(0).map(|n| ref_list(n, "lst0")).unwrap_or_default();
    FeatureKind::BendPart {
        radius: s.param(1),
        angle: s.param(2),
        flags: [s.flag(4), s.flag(5), s.flag(6), s.flag(7)],
        bend_line: lines.first().map(|&l| s.ctx.sections(s.ctx.node(l))),
    }
}
