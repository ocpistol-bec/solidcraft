//! Sketch constraints, dimensions, texts and patterns.

use std::collections::BTreeSet;

use super::types::{Sketch, SketchConstraint, SketchDimension, SketchPattern, SketchText};
use crate::grammar::{DecodeStatus, DecodedNode, FieldValue};
use crate::model::part::{Part, T_PARAMETER};
use crate::model::view::{node_ref, ref_list, vec3};

/// Constraint and dimension TypeIDs. A dimension is a constraint whose `parameter` names a
/// parameter node.
const SKETCH_CONSTRAINTS: [(u32, &str); 27] = [
    (0x9087_4D94, "coincident"),
    (0x9087_4D95, "parallel"),
    (0x9087_4D96, "perpendicular"),
    (0x9087_4D97, "tangent"),
    (0x9087_4D98, "horizontal"),
    (0x9087_4D99, "vertical"),
    (0x5A9A_7BE0, "collinear"),
    (0xC681_C2E0, "equal length"),
    (0x442C_7DD0, "equal radius"),
    // Also binds an arc or circle to its own center point.
    (0xE110_8C00, "concentric"),
    (0x0F17_7BB0, "fix"),
    (0x0DDD_7C10, "symmetric"),
    (0x8F55_A3C0, "horizontal alignment"),
    (0x64DA_5250, "vertical alignment"),
    (0x21E8_70BF, "midpoint"),
    (0x8EC6_B314, "smooth (G2)"),
    (0x7C6D_149E, "spline fit point"),
    (0x7A98_AD0E, "text box"),
    (0x4E4B_14BC, "offset"),
    (0xFBDB_891F, "polygon / pattern"),
    (0xBF3B_5C84, "three-point angle dimension"),
    (0xA789_EEB0, "ellipse major radius dimension"),
    // From InventorLoader; not yet seen in a fixture.
    (0xB496_4E90, "ellipse minor radius dimension"),
    (0x1105_8558, "distance dimension"),
    (0x590D_0A10, "angle dimension"),
    (0x74DF_96E0, "diameter dimension"),
    (0x671B_B700, "radius dimension"),
];

/// NodeRef fields of a constraint that are bookkeeping, not constrained entities.
const NON_ENTITY_REFS: [&str; 5] = ["next", "ref", "group", "parameter", "ref_1"];

/// Sketch text: anchor, formatted string, box corners; `ref_1` names the sketch.
const T_TEXT2D: u32 = 0x1FBB_3C01;
/// One per pattern occurrence (`pattern` = its number, `lst0` = its entities).
const T_PATTERN_OCC: u32 = 0x75A6_689B;
/// Sketch pattern features.
const SKETCH_PATTERNS: [(u32, &str); 2] = [(0xB5D4_DEE6, "circular"), (0x72C9_7D63, "rectangular")];

/// `flags` bit of a driven (reference) parameter.
const FLAG_DRIVEN: i64 = 0x0100_0000;

impl Part<'_> {
    /// Files a constraint node under the sketch's constraints or dimensions; other node types
    /// are ignored.
    pub(super) fn add_sketch_constraint(&self, sketch: &mut Sketch, index: usize, n: &DecodedNode) {
        let Some(kind) = lookup(&SKETCH_CONSTRAINTS, n.type_id.0) else {
            return;
        };
        let entities = constrained_entities(n);
        match node_ref(n, "parameter").filter(|&p| self.view().type_of(p) == Some(T_PARAMETER)) {
            Some(param) => sketch
                .dimensions
                .push(self.sketch_dimension(index, kind, entities, param)),
            None => sketch.constraints.push(SketchConstraint {
                index,
                constraint: kind,
                entities,
            }),
        }
    }

    fn sketch_dimension(
        &self,
        index: usize,
        kind: &'static str,
        entities: Vec<usize>,
        param: usize,
    ) -> SketchDimension {
        let flags = self
            .view()
            .node(param)
            .and_then(|p| p.int("flags"))
            .unwrap_or(0);
        SketchDimension {
            index,
            dimension: kind,
            entities,
            parameter: self.param(param),
            driven: flags & FLAG_DRIVEN != 0,
        }
    }

    /// The texts placed on sketch `sketch`.
    pub(super) fn sketch_texts(&self, sketch: usize) -> Vec<SketchText> {
        self.view()
            .by_type(T_TEXT2D)
            .filter(|(_, n)| node_ref(n, "ref_1") == Some(sketch))
            .map(|(index, n)| SketchText {
                index,
                text: n.text("resolved_text").map(str::to_string),
                anchor: vec3(n, "loc"),
                box_corners: [vec3(n, "box_corner1"), vec3(n, "box_corner2")],
            })
            .collect()
    }

    /// The sketch patterns with an occurrence entity on sketch `sketch`.
    pub(super) fn sketch_patterns(&self, sketch: usize) -> Vec<SketchPattern> {
        self.view()
            .nodes()
            .filter(|(_, n)| n.status != DecodeStatus::NoGrammar)
            .filter_map(|(index, n)| self.sketch_pattern(index, n, sketch))
            .collect()
    }

    /// Node `n` as a pattern of sketch `sketch`, when it is a sketch pattern on that sketch.
    fn sketch_pattern(
        &self,
        index: usize,
        n: &DecodedNode,
        sketch: usize,
    ) -> Option<SketchPattern> {
        let pattern = lookup(&SKETCH_PATTERNS, n.type_id.0)?;
        let occurrences = self.pattern_occurrences(n);
        let entities = || occurrences.iter().flat_map(|o| ref_list(o, "lst0"));
        let on_sketch = |e: usize| {
            self.view()
                .node(e)
                .is_some_and(|en| node_ref(en, "sketch") == Some(sketch))
        };
        if !entities().any(on_sketch) {
            return None;
        }
        let counts = occurrences
            .iter()
            .map(|o| ref_list(o, "lst0").len())
            .collect();
        Some(SketchPattern {
            index,
            name: self.name(index),
            pattern,
            occurrences: counts,
        })
    }

    /// The occurrence nodes a pattern references, ordered by occurrence number.
    fn pattern_occurrences(&self, n: &DecodedNode) -> Vec<&DecodedNode> {
        let indexes: BTreeSet<usize> = n
            .fields
            .iter()
            .filter_map(|f| match f.value {
                FieldValue::Ref(r, _) => r.index(),
                _ => None,
            })
            .filter(|&i| self.view().type_of(i) == Some(T_PATTERN_OCC))
            .collect();
        let mut occurrences: Vec<&DecodedNode> = indexes
            .into_iter()
            .filter_map(|i| self.view().node(i))
            .collect();
        occurrences.sort_by_key(|o| o.int("pattern").unwrap_or(0));
        occurrences
    }
}

/// The constrained node indexes: every NodeRef that is not bookkeeping (a flagged null
/// reference counts, as index 0).
fn constrained_entities(n: &DecodedNode) -> Vec<usize> {
    n.fields
        .iter()
        .filter(|f| {
            !f.name
                .as_deref()
                .is_some_and(|name| NON_ENTITY_REFS.contains(&name))
        })
        .filter_map(|f| match f.value {
            FieldValue::Ref(r, _) if r.0 != 0 => Some((r.0 & 0x7FFF_FFFF) as usize),
            _ => None,
        })
        .collect()
}

fn lookup(table: &[(u32, &'static str)], tid: u32) -> Option<&'static str> {
    table.iter().find(|(t, _)| *t == tid).map(|(_, s)| *s)
}
