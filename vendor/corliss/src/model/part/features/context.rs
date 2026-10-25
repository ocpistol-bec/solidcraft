//! Lookups every feature reader shares: names, parameters, face references, profiles.

use std::collections::HashMap;

use crate::grammar::DecodedNode;
use crate::model::part::{ParamValue, Part};
use crate::model::view::{node_ref, ref_list};

use super::values::FeatureValue;

/// Sketch profile (F9884C43) node type.
const T_PROFILE: u32 = 0xF988_4C43;
/// Thread table row attribute owned by a hole or thread feature.
const T_THREAD_INFO: u32 = 0xA408_7E1F;

/// B-rep naming indexes of one face or edge reference (509FB5CC `indexRefs`).
pub type FeatureNaming = Vec<i64>;

/// The sketch profile a feature consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureProfile {
    /// Name of the profile's sketch.
    pub sketch: Option<String>,
    /// Loops of sketch-entity associative ids (`entityAI`).
    pub loops: Vec<Vec<Option<i64>>>,
}

/// A loft section or sweep path: a wire body in the segment's ACIS stream.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureWire {
    /// Index of the ACIS stream node holding the wire.
    pub wire_asm: Option<usize>,
    /// Index of the wire body in that stream.
    pub wire_index: Option<i64>,
}

/// The thread table row of a hole or thread feature (A4087E1F).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureThreadInfo {
    /// Nominal size ("6").
    pub size: Option<String>,
    /// Designation ("M6x1").
    pub designation: Option<String>,
    /// Thread standard.
    pub standard: Option<String>,
    /// Tolerance class ("6H").
    pub class: Option<String>,
    /// Raw flag word.
    pub flags: i64,
    /// Right-hand thread (flag 0x02).
    pub right_hand: bool,
    /// Full-depth thread (flag 0x20).
    pub full_depth: bool,
}

/// Shared state for one `Part::features` run.
pub(super) struct FeatureContext<'p, 'm> {
    pub part: &'p Part<'m>,
    /// Profile proxy index -> profile node index.
    pub profiles: HashMap<usize, usize>,
    /// The part holds a mesh feature (decides copy object versus fit mesh face).
    pub has_meshes: bool,
    /// Base-body feature index -> what created it.
    pub base_sources: HashMap<usize, Option<String>>,
}

impl<'p, 'm> FeatureContext<'p, 'm> {
    pub fn new(part: &'p Part<'m>) -> Self {
        let mut profiles = HashMap::new();
        for (i, n) in part.view().by_type(T_PROFILE) {
            if let Some(proxy) = node_ref(n, "proxy") {
                profiles.insert(proxy, i);
            }
        }
        FeatureContext {
            part,
            profiles,
            has_meshes: false,
            base_sources: HashMap::new(),
        }
    }

    /// The node at `index` (0 = none).
    pub fn node(&self, index: usize) -> Option<&'m DecodedNode> {
        self.part.view().node(index)
    }

    /// The TypeID of the node at `index`.
    pub fn tid(&self, index: usize) -> Option<u32> {
        self.part.view().type_of(index)
    }

    /// Number of nodes in the segment.
    pub fn node_count(&self) -> usize {
        self.part.view().segment().nodes.len()
    }

    /// The browser label of `index` (0 = none).
    pub fn name(&self, index: usize) -> Option<String> {
        self.part.name(index)
    }

    /// Labels of the nodes a list field names.
    pub fn names(&self, n: Option<&DecodedNode>, list: &str) -> Vec<Option<String>> {
        n.map(|n| ref_list(n, list))
            .unwrap_or_default()
            .into_iter()
            .map(|i| self.name(i))
            .collect()
    }

    /// The parameter at `index` (0 = none).
    pub fn param(&self, index: usize) -> Option<ParamValue> {
        self.part.param(index)
    }

    /// The `value` field of the node at `index`.
    pub fn value(&self, index: usize) -> Option<FeatureValue> {
        value_of(self.node(index)?, "value")
    }

    /// Truthiness of the `value` of the node at `index`; `None` when there is no node.
    pub fn flag(&self, index: usize) -> Option<bool> {
        let n = self.node(index)?;
        Some(value_of(n, "value").is_some_and(|v| v.truthy()))
    }

    /// The `indexRefs` of a face or edge reference node.
    pub fn facerefs(&self, index: usize) -> Option<FeatureNaming> {
        let n = self.node(index)?;
        let f = n
            .fields
            .iter()
            .find(|f| f.name.as_deref() == Some("indexRefs") && n_is_list(f))?;
        let elements = match &f.value {
            crate::grammar::FieldValue::List(l) => &l.elements,
            _ => return None,
        };
        Some(
            elements
                .iter()
                .map(|e| e.first().and_then(|x| x.value.as_int()).unwrap_or(0))
                .collect(),
        )
    }

    /// Face references of the `items` of a face set node.
    pub fn face_list(&self, n: Option<&DecodedNode>) -> Vec<Option<FeatureNaming>> {
        n.map(|n| ref_list(n, "items"))
            .unwrap_or_default()
            .into_iter()
            .map(|f| self.facerefs(f))
            .collect()
    }

    /// Edge references of a 90874D51 edge set (`None` when `index` is 0).
    pub fn edges(&self, index: usize) -> Option<Vec<Option<FeatureNaming>>> {
        (index != 0).then(|| self.face_list(self.node(index)))
    }

    /// Driving parameters of a D83EF271 feature-dimension node.
    pub fn dims(&self, index: usize) -> Vec<ParamValue> {
        let Some(n) = self.node(index) else {
            return Vec::new();
        };
        let dims = ref_list(n, "fxDimensions")
            .into_iter()
            .filter_map(|k| self.node(k));
        dims.filter_map(|d| node_ref(d, "parameter"))
            .filter_map(|r| self.param(r))
            .collect()
    }

    /// The profile of a profile (or open profile) node.
    pub fn profile(&self, index: usize) -> Option<FeatureProfile> {
        let prof = self.node(index)?;
        let loop_ids = |lp: usize| -> Vec<Option<i64>> {
            let edges = self
                .node(lp)
                .map(|l| ref_list(l, "edges"))
                .unwrap_or_default();
            edges
                .into_iter()
                .map(|e| self.node(e).and_then(|e| e.int("entityAI")))
                .collect()
        };
        let loops = ref_list(prof, "parts").into_iter().map(loop_ids).collect();
        Some(FeatureProfile {
            sketch: self.name(ref_or_zero(prof, "sketch")),
            loops,
        })
    }

    /// The profile whose proxy is `proxy`.
    pub fn profile_of_proxy(&self, proxy: usize) -> Option<FeatureProfile> {
        self.profiles.get(&proxy).and_then(|&i| self.profile(i))
    }

    /// The wire a CC0F7521 wrapper names.
    pub fn sections(&self, wrapped: Option<&DecodedNode>) -> FeatureWire {
        let w = wrapped.and_then(|n| self.node(ref_or_zero(n, "refWrapper")));
        FeatureWire {
            wire_asm: w.and_then(|w| node_ref(w, "asm")),
            wire_index: w.and_then(|w| w.int("u32_1")),
        }
    }

    /// The thread table row owned by feature `index`.
    pub fn thread_info(&self, index: usize) -> Option<FeatureThreadInfo> {
        let (_, a) = self
            .part
            .view()
            .by_type(T_THREAD_INFO)
            .find(|(_, a)| node_ref(a, "owner") == Some(index))?;
        let text = |k: &str| a.text(k).map(str::to_string);
        let flags = a.int("u32_0").unwrap_or(0);
        Some(FeatureThreadInfo {
            size: text("name"),
            designation: text("txt0"),
            standard: text("standard"),
            class: text("txt5"),
            flags,
            right_hand: flags & 0x02 != 0,
            full_depth: flags & 0x20 != 0,
        })
    }
}

fn n_is_list(f: &crate::grammar::Field) -> bool {
    matches!(f.value, crate::grammar::FieldValue::List(_))
}

/// A NodeRef field's index, 0 when null or absent.
pub(super) fn ref_or_zero(n: &DecodedNode, name: &str) -> usize {
    node_ref(n, name).unwrap_or(0)
}

/// Field `name` of `n` as a raw value.
pub(super) fn value_of(n: &DecodedNode, name: &str) -> Option<FeatureValue> {
    n.get(name).map(FeatureValue::from_field)
}

/// The feature's slot references (`properties`), padded to 32 with zeros.
pub(super) struct Slots<'c, 'p, 'm> {
    pub ctx: &'c FeatureContext<'p, 'm>,
    pub index: usize,
    pub props: Vec<usize>,
}

impl<'m> Slots<'_, '_, 'm> {
    pub fn new<'c, 'p>(
        ctx: &'c FeatureContext<'p, 'm>,
        index: usize,
        mut props: Vec<usize>,
    ) -> Slots<'c, 'p, 'm> {
        if props.len() < 32 {
            props.resize(32, 0);
        }
        Slots { ctx, index, props }
    }

    /// Slot `k`'s node index (0 = empty).
    pub fn at(&self, k: usize) -> usize {
        self.props.get(k).copied().unwrap_or(0)
    }

    /// Slot `k`'s node.
    pub fn node(&self, k: usize) -> Option<&'m DecodedNode> {
        self.ctx.node(self.at(k))
    }

    /// Slot `k`'s TypeID.
    pub fn tid(&self, k: usize) -> Option<u32> {
        self.ctx.tid(self.at(k))
    }

    /// True when slot `k` holds a node of type `tid`.
    pub fn is(&self, k: usize, tid: u32) -> bool {
        self.tid(k) == Some(tid)
    }

    /// Slot `k`'s `value`.
    pub fn value(&self, k: usize) -> Option<FeatureValue> {
        self.ctx.value(self.at(k))
    }

    /// Truthiness of slot `k`'s `value`, `None` for an empty slot.
    pub fn flag(&self, k: usize) -> Option<bool> {
        self.ctx.flag(self.at(k))
    }

    /// Slot `k`'s parameter.
    pub fn param(&self, k: usize) -> Option<ParamValue> {
        self.ctx.param(self.at(k))
    }

    /// Label of slot `k`'s node.
    pub fn name(&self, k: usize) -> Option<String> {
        self.ctx.name(self.at(k))
    }

    /// Number of non-empty slots.
    pub fn filled(&self) -> usize {
        self.props.iter().filter(|&&p| p != 0).count()
    }
}
