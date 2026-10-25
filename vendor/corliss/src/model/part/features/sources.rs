//! Features that bring geometry or decoration in: derived parts, imports, decals, finishes,
//! punch tools, meshes and repaired bodies.

use crate::grammar::{DecodeStatus, DecodedNode};
use crate::model::view::{floats, node_ref, ref_list, transform};

use super::FeatureKind;
use super::context::{FeatureContext, FeatureNaming};
use super::values::FeatureValue;

/// Parameter node type.
const T_PARAMETER: u32 = 0x9087_4D26;
/// Face set node type.
const T_FACE_SET: u32 = 0x24BC_B2F1;
/// Finish definition: faces, area parameter and text parameters.
const T_FINISH_DEFINITION: u32 = 0x4119_1B90;
/// Text parameter (finish appearance and comment).
const T_TEXT_PARAMETER: u32 = 0x8367_B125;

/// One decal image.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDecalImage {
    /// Width.
    pub width: Option<f64>,
    /// Height.
    pub height: Option<f64>,
    /// Position in the sketch.
    pub pos: Vec<f64>,
    /// Scale.
    pub scale: Option<f64>,
}

/// A named finish parameter value.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureFinishParameter {
    /// Parameter name.
    pub name: Option<String>,
    /// Model value (numeric parameters) or text.
    pub value: Option<FeatureValue>,
}

pub(super) fn derived_part(ctx: &FeatureContext, n: &DecodedNode) -> FeatureKind {
    let linked =
        |n: Option<&DecodedNode>, name: &str| n.and_then(|n| node_ref(n, name)).unwrap_or(0);
    let r = ctx.node(linked(Some(n), "ref_1"));
    let sc = ctx.node(linked(r, "ref_1"));
    FeatureKind::DerivedPart {
        file: n.text("name").map(str::to_string),
        scale: ctx.param(linked(sc, "parameter")),
        transform: ctx
            .node(linked(sc, "transformation"))
            .and_then(|t| transform(t, "transformation")),
    }
}

fn decal_image(ctx: &FeatureContext, i: usize) -> FeatureDecalImage {
    let im = ctx.node(i);
    let float = |k: &str| im.and_then(|m| m.float(k));
    FeatureDecalImage {
        width: float("width"),
        height: float("height"),
        pos: im.map(|m| floats(m, "pos")).unwrap_or_default(),
        scale: float("scale"),
    }
}

pub(super) fn decal(ctx: &FeatureContext, n: &DecodedNode) -> FeatureKind {
    let images = node_ref(n, "images")
        .and_then(|r| ctx.node(r))
        .map(|m| ref_list(m, "images"));
    let props = ref_list(n, "properties");
    let wrap = props.get(2).copied().filter(|&p| p != 0);
    FeatureKind::Decal {
        sketch: ctx.name(node_ref(n, "sketch").unwrap_or(0)),
        images: images
            .unwrap_or_default()
            .into_iter()
            .map(|i| decal_image(ctx, i))
            .collect(),
        face: props.get(1).and_then(|&p| ctx.facerefs(p)),
        wrap_to_face: wrap.map(|p| ctx.flag(p).unwrap_or(false)),
    }
}

pub(super) fn punch_tool(ctx: &FeatureContext, n: &DecodedNode) -> FeatureKind {
    let pr = ref_list(n, "properties");
    let params = pr
        .iter()
        .filter(|&&k| k != 0 && ctx.tid(k) == Some(T_PARAMETER));
    let placement = pr.get(3).and_then(|&k| ctx.node(k));
    FeatureKind::PunchTool {
        parameters: params.map(|&k| ctx.param(k)).collect(),
        transform: placement.and_then(|t| transform(t, "transformation")),
    }
}

/// NodeRef indexes found by scanning `payload` for flagged words naming an existing node.
fn payload_refs(ctx: &FeatureContext, payload: &[u8]) -> Vec<usize> {
    let words = payload
        .windows(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    let flagged = words
        .filter(|v| v & 0x8000_0000 != 0)
        .map(|v| (v & 0x7FFF_FFFF) as usize);
    // The reference reader stops one short of the node count.
    flagged.filter(|&i| 0 < i && i < ctx.node_count()).collect()
}

#[derive(Default)]
struct FinishParts {
    faces: Option<Vec<Option<FeatureNaming>>>,
    parameters: Vec<FeatureFinishParameter>,
}

fn add_finish_parameter(parts: &mut FinishParts, m: &DecodedNode) {
    let field = if m.type_id.0 == T_PARAMETER {
        "valueModel"
    } else {
        "value"
    };
    let value = m.get(field).map(FeatureValue::from_field);
    let name = m.text("name").map(str::to_string);
    match parts.parameters.iter_mut().find(|p| p.name == name) {
        Some(p) => p.value = value,
        None => parts
            .parameters
            .push(FeatureFinishParameter { name, value }),
    }
}

fn scan_finish_definition(ctx: &FeatureContext, def: &DecodedNode, parts: &mut FinishParts) {
    for j in payload_refs(ctx, &def.payload()) {
        let Some(m) = ctx.node(j) else { continue };
        if m.type_id.0 == T_FACE_SET {
            parts.faces = Some(ctx.face_list(Some(m)));
        } else if matches!(m.type_id.0, T_PARAMETER | T_TEXT_PARAMETER)
            && m.status != DecodeStatus::NoGrammar
        {
            add_finish_parameter(parts, m);
        }
    }
}

pub(super) fn finish(ctx: &FeatureContext, n: &DecodedNode) -> FeatureKind {
    let mut parts = FinishParts::default();
    for i in payload_refs(ctx, &n.payload()) {
        if let Some(def) = ctx.node(i).filter(|d| d.type_id.0 == T_FINISH_DEFINITION) {
            scan_finish_definition(ctx, def, &mut parts);
        }
    }
    let parameters = (!parts.parameters.is_empty()).then_some(parts.parameters);
    FeatureKind::Finish {
        faces: parts.faces,
        parameters,
    }
}

pub(super) fn mesh(ctx: &FeatureContext, n: &DecodedNode) -> FeatureKind {
    FeatureKind::MeshFeature {
        meshes: ctx.names(Some(n), "meshes"),
    }
}
