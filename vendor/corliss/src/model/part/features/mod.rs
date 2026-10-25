//! Part features (spec 15.3–15.6), after the reference part reader's feature list.
//!
//! Imported and decorative features come first, then grouping sheet metal features and patterns,
//! then every 90874D91 feature node in block order, told apart by its slot signature.

mod bodies;
mod context;
mod dispatch;
mod kind;
mod patterns;
mod placed;
mod sheet_metal;
mod sketched;
mod sources;
mod values;

use std::collections::HashMap;

use crate::grammar::DecodedNode;
use crate::model::view::ref_list;

pub use bodies::{
    FeatureFaceOffset, FeaturePatchLoop, FeatureRuledSurface, FeatureSculptMode, FeatureSculptTool,
};
pub use context::{FeatureNaming, FeatureProfile, FeatureThreadInfo, FeatureWire};
pub use kind::FeatureKind;
pub use patterns::{
    FeatureCircularPattern, FeatureMirrorPlane, FeaturePatternBase, FeaturePatternDirection,
    FeaturePatternPath, FeatureSketchPattern,
};
pub use placed::{
    FeatureChamfer, FeatureFaceDraft, FeatureFillet, FeatureFilletPoint, FeatureFilletSet,
    FeatureFilletSetback, FeatureHole, FeatureMoveFace, FeatureShell, FeatureThread, FeatureUnwrap,
};
pub use sheet_metal::FeatureSheetMetalGroup;
pub use sketched::{
    FeatureAxis, FeatureCoil, FeatureCoilEnd, FeatureEmboss, FeatureExtrusion,
    FeatureExtrusionSide, FeatureLoft, FeatureLoftSection, FeatureRevolve, FeatureRib,
    FeatureSolidBase, FeatureSweep,
};
pub use sources::{FeatureDecalImage, FeatureFinishParameter};
pub use values::{
    FeatureChamferType, FeatureCoilMethod, FeatureCreationMethod, FeatureDraftMode,
    FeatureEmbossOperation, FeatureExtent, FeatureFilletType, FeatureHoleSeat, FeatureLoftType,
    FeatureOperation, FeatureRevolveDirection, FeatureRevolveExtent, FeatureRuledType,
    FeatureShellDirection, FeatureSplitMethod, FeatureSweepOrientation, FeatureValue,
};

use super::{Part, T_PARAMETER};
use context::{FeatureContext, Slots};

const T_EXTRUSION: u32 = 0x9087_4D91;
const T_DERIVED_PART: u32 = 0x255D_7ED7;
const T_IMPORT: u32 = 0x9C9E_3CB3;
const T_DECAL: u32 = 0x7F93_6BAA;
const T_FINISH: u32 = 0xCBA2_6FC8;
const T_DIRECT_EDIT: u32 = 0x1F6D_59F6;
const T_PUNCH_TOOL: u32 = 0xC098_D3CF;
const T_MESH: u32 = 0xEC2D_4C66;
/// Repaired Geometry: lst0 -> C2EF1CC7 -> base-body features.
const T_REPAIR: u32 = 0xFBC6_C635;

/// One feature of the part's history.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Node index of the feature.
    pub index: usize,
    /// Browser name (for a derived part or mesh feature, its stored name).
    pub name: Option<String>,
    /// What the feature is, with its typed definition.
    pub kind: FeatureKind,
    /// The direct edit feature that owns this one.
    pub direct_edit: Option<FeatureOwner>,
    /// The sheet metal face, flange or contour flange this feature belongs to.
    pub parent: Option<FeatureOwner>,
    /// The inserted iFeature this feature was replayed from.
    pub ifeature: Option<FeatureIFeature>,
}

/// A feature that owns others.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureOwner {
    /// Node index of the owner.
    pub index: usize,
    /// Browser name of the owner.
    pub name: Option<String>,
}

/// An inserted iFeature instance: its features' parameters carry the instance-name prefix.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureIFeature {
    /// Instance name ("End_mill_straight:1").
    pub instance: String,
    /// Parameter names with the prefix removed.
    pub parameters: Vec<String>,
}

impl Part<'_> {
    /// Every feature of the part: typed definitions with their parameters, profiles and
    /// face references.
    ///
    /// ```no_run
    /// let doc = corliss::Document::open("part.ipt")?;
    /// let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// for f in part.features() {
    ///     println!("{:?} {}", f.name, f.kind.type_name());
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn features(&self) -> Vec<Feature> {
        let mut ctx = FeatureContext::new(self);
        ctx.has_meshes = self.view().by_type(T_MESH).next().is_some();
        ctx.base_sources = base_sources(&ctx);
        let mut out = leading_features(&ctx);
        let slotted = self.view().by_type(T_EXTRUSION);
        out.extend(slotted.map(|(i, n)| slotted_feature(&ctx, i, n)));
        annotate(&ctx, &mut out);
        out
    }
}

/// Adds the owning direct edit, sheet metal group and iFeature instance to each feature.
fn annotate(ctx: &FeatureContext, out: &mut [Feature]) {
    let direct = owners(ctx, &[T_DIRECT_EDIT]);
    let groups: Vec<u32> = FeatureSheetMetalGroup::ALL
        .iter()
        .map(|(t, _)| *t)
        .collect();
    let parents = owners(ctx, &groups);
    let pnames = parameter_names(ctx.part);
    for f in out {
        f.direct_edit = direct.get(&f.index).cloned();
        f.parent = parents.get(&f.index).cloned();
        f.ifeature = ifeature(f.name.as_deref(), &pnames);
    }
}

fn feature(ctx: &FeatureContext, index: usize, kind: FeatureKind) -> Feature {
    Feature {
        index,
        name: ctx.name(index),
        kind,
        direct_edit: None,
        parent: None,
        ifeature: None,
    }
}

fn slotted_feature(ctx: &FeatureContext, index: usize, n: &DecodedNode) -> Feature {
    let slots = Slots::new(ctx, index, ref_list(n, "properties"));
    feature(ctx, index, dispatch::read_slotted(&slots))
}

type NodeReader = fn(&FeatureContext, usize, &DecodedNode) -> Feature;

/// Features listed before the 90874D91 nodes, in the reference reader's order.
const LEADING: [(u32, NodeReader); 14] = [
    (T_DERIVED_PART, derived),
    (T_IMPORT, |c, i, n| {
        feature(c, i, FeatureKind::Import { file: container(n) })
    }),
    (T_DECAL, |c, i, n| feature(c, i, sources::decal(c, n))),
    (T_FINISH, |c, i, n| feature(c, i, sources::finish(c, n))),
    (T_PUNCH_TOOL, |c, i, n| {
        feature(c, i, sources::punch_tool(c, n))
    }),
    (FeatureSheetMetalGroup::ALL[0].0, sheet_metal_group),
    (FeatureSheetMetalGroup::ALL[1].0, sheet_metal_group),
    (FeatureSheetMetalGroup::ALL[2].0, sheet_metal_group),
    (patterns::T_PATTERN_RECT, pattern),
    (patterns::T_PATTERN_POLAR, pattern),
    (patterns::T_MIRROR, pattern),
    (patterns::T_PATTERN_SKETCH, pattern),
    (T_MESH, mesh),
    (T_REPAIR, |c, i, _| feature(c, i, FeatureKind::RepairBodies)),
];

fn leading_features(ctx: &FeatureContext) -> Vec<Feature> {
    let view = ctx.part.view();
    let mut out = Vec::new();
    for (tid, read) in LEADING {
        out.extend(view.by_type(tid).map(|(i, n)| read(ctx, i, n)));
    }
    out
}

fn sheet_metal_group(ctx: &FeatureContext, index: usize, n: &DecodedNode) -> Feature {
    let mut groups = FeatureSheetMetalGroup::ALL.iter();
    let group = groups
        .find(|(t, _)| *t == n.type_id.0)
        .map_or(FeatureSheetMetalGroup::Face, |(_, g)| *g);
    let members = ctx.names(Some(n), "properties");
    feature(ctx, index, FeatureKind::SheetMetalGroup { group, members })
}

fn pattern(ctx: &FeatureContext, index: usize, n: &DecodedNode) -> Feature {
    feature(ctx, index, patterns::read_pattern(ctx, index, n))
}

fn derived(ctx: &FeatureContext, index: usize, n: &DecodedNode) -> Feature {
    Feature {
        name: n.text("name").map(str::to_string),
        ..feature(ctx, index, sources::derived_part(ctx, n))
    }
}

fn mesh(ctx: &FeatureContext, index: usize, n: &DecodedNode) -> Feature {
    let stored = n.text("name").filter(|s| !s.is_empty()).map(str::to_string);
    Feature {
        name: stored.or_else(|| ctx.name(index)),
        ..feature(ctx, index, sources::mesh(ctx, n))
    }
}

fn container(n: &DecodedNode) -> Option<String> {
    n.text("container").map(str::to_string)
}

/// Base-body feature index -> the derived part, import or repair that created it.
fn base_sources(ctx: &FeatureContext) -> HashMap<usize, Option<String>> {
    let view = ctx.part.view();
    let mut out = HashMap::new();
    for (_, n) in view.by_type(T_DERIVED_PART) {
        let name = n.text("name").map(str::to_string);
        out.extend(ref_list(n, "lst0").into_iter().map(|f| (f, name.clone())));
    }
    for (_, n) in view.by_type(T_IMPORT) {
        out.extend(ref_list(n, "lst0").into_iter().map(|f| (f, container(n))));
    }
    for (i, n) in view.by_type(T_REPAIR) {
        out.extend(repaired_bodies(ctx, i, n));
    }
    out
}

/// The base-body features a repair lists through its C2EF1CC7 wrappers.
fn repaired_bodies(
    ctx: &FeatureContext,
    index: usize,
    n: &DecodedNode,
) -> Vec<(usize, Option<String>)> {
    // Python's str(None) when the repair has no label.
    let name = ctx.name(index).unwrap_or_else(|| "None".to_string());
    let label = format!("repair: {name}");
    let wrappers = ref_list(n, "lst0").into_iter().filter_map(|c| ctx.node(c));
    let members = wrappers.flat_map(|c| ref_list(c, "properties"));
    members.map(|f| (f, Some(label.clone()))).collect()
}

/// Member feature index -> the feature (of one of `tids`) listing it in `properties`.
fn owners(ctx: &FeatureContext, tids: &[u32]) -> HashMap<usize, FeatureOwner> {
    let mut out = HashMap::new();
    for &tid in tids {
        for (i, n) in ctx.part.view().by_type(tid) {
            let owner = FeatureOwner {
                index: i,
                name: ctx.name(i),
            };
            out.extend(
                ref_list(n, "properties")
                    .into_iter()
                    .map(|f| (f, owner.clone())),
            );
        }
    }
    out
}

fn parameter_names(part: &Part) -> Vec<String> {
    let params = part.view().by_type(T_PARAMETER);
    params
        .map(|(_, n)| n.text("name").unwrap_or_default().to_string())
        .collect()
}

/// The iFeature instance a feature named `name` belongs to: parameters are named
/// "<instance>:<parameter>" ("End_mill_straight:1:Diameter").
fn ifeature(name: Option<&str>, pnames: &[String]) -> Option<FeatureIFeature> {
    let nm = name.filter(|n| n.contains(':'))?;
    let prefix = format!("{nm}:");
    let parameters: Vec<String> = pnames
        .iter()
        .filter_map(|x| x.strip_prefix(&prefix))
        .map(str::to_string)
        .collect();
    (!parameters.is_empty()).then(|| FeatureIFeature {
        instance: nm.to_string(),
        parameters,
    })
}
