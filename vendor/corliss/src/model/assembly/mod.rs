//! Assembly documents (spec clause 16), after the reference assembly reader.
//!
//! The UFRxDoc lists the referenced files (ref_id → path) and one record per occurrence
//! (`occ_ids` = 0, file ref_id, occurrence id, occurrence id, instance number). AmDcSegment
//! holds the component occurrences (90874D61), the assembly constraints (4E86F047), the
//! component patterns and the assembly features; AmRxSegment the resolved placement of every
//! occurrence (232792BC) and the positional-representation override table.

mod constraints;
mod features;
mod geometry;
mod occurrences;
mod patterns;
mod representations;

pub use constraints::*;
pub use features::*;
pub use geometry::*;
pub use occurrences::*;
pub use patterns::*;
pub use representations::*;

use std::collections::{BTreeMap, HashMap};

use super::view::{node_ref, ref_list};
use super::{Model, SegmentView};
use crate::error::{CorlissError, Result};
use crate::grammar::DecodedNode;
use crate::ufrx::{Record, UfrxDoc};

/// The assembly model segment.
const DC_SEGMENT: &str = "AmDcSegment";
/// The resolved-placement segment.
const RX_SEGMENT: &str = "AmRxSegment";
/// The browser segment, the only place constraint and pattern names are stored.
const BROWSER_SEGMENT: &str = "AmBrowserSegment";

/// An assembly's model: referenced files, occurrences, representations, patterns, features and
/// constraints.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let model = corliss::model::Model::load(&doc)?;
/// let assembly = corliss::model::Assembly::new(&model)?;
/// for o in assembly.occurrences() {
///     println!("{:?} -> {:?}", o.name, o.file);
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub struct Assembly<'m> {
    model: &'m Model<'m>,
    dc: SegmentView<'m>,
    rx: SegmentView<'m>,
    ufrx: UfrxDoc,
}

impl<'m> Assembly<'m> {
    /// Reads the assembly model of an assembly document.
    pub fn new(model: &'m Model<'m>) -> Result<Self> {
        let view = |name: &'static str| {
            model
                .view(name)
                .ok_or_else(|| CorlissError::missing("read", "assembly model", name))
        };
        let (dc, rx) = (view(DC_SEGMENT)?, view(RX_SEGMENT)?);
        let ufrx = model.document().ufrx_doc()?;
        Ok(Assembly {
            model,
            dc,
            rx,
            ufrx,
        })
    }

    /// The decoded document.
    pub fn model(&self) -> &'m Model<'m> {
        self.model
    }

    /// The labelled AmDcSegment.
    pub fn view(&self) -> &SegmentView<'m> {
        &self.dc
    }

    /// The parsed UFRxDoc.
    pub fn ufrx(&self) -> &UfrxDoc {
        &self.ufrx
    }

    /// Referenced Inventor files: ref_id → path.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let files = corliss::model::Assembly::new(&model)?.files();
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn files(&self) -> BTreeMap<u32, String> {
        self.ufrx
            .references()
            .into_iter()
            .map(|f| (f.ref_id, f.path))
            .collect()
    }

    /// The AmDc node at `index`, `None` for 0 or a missing ref.
    fn node(&self, index: Option<usize>) -> Option<&'m DecodedNode> {
        index.and_then(|i| self.dc.node(i))
    }

    /// The node a NodeRef field `name` of `n` names.
    fn node_at(&self, n: &DecodedNode, name: &str) -> Option<&'m DecodedNode> {
        self.node(node_ref(n, name))
    }

    /// `valueModel` of the parameter-like node `n.name` names.
    fn value_model(&self, n: &DecodedNode, name: &str) -> Option<f64> {
        self.node_at(n, name).and_then(|m| m.float("valueModel"))
    }

    /// The entry list of UFRxDoc section `section`.
    fn ufrx_entries(&self, section: &str) -> &[Record] {
        self.ufrx
            .root
            .record(section)
            .map_or(&[], |r| r.list("entries"))
    }

    /// Occurrence label by AmDc node index, as patterns and constraints name occurrences.
    fn occurrence_names(&self) -> OccurrenceLabels {
        self.occurrences()
            .into_iter()
            .map(|o| (o.index, o.name))
            .collect()
    }

    /// Names of the AmBrowserSegment nodes whose TypeID is in `types`, in block order.
    fn browser_names(&self, types: &[u32]) -> Vec<Option<String>> {
        let Some(seg) = self.model.segment(BROWSER_SEGMENT) else {
            return Vec::new();
        };
        seg.nodes
            .iter()
            .filter(|n| types.contains(&n.type_id.0))
            .map(|n| n.text("name").map(str::to_string))
            .collect()
    }
}

/// Occurrence labels by AmDc node index.
type OccurrenceLabels = HashMap<usize, Option<String>>;

/// Labels of the occurrences list field `field` of `n` names.
fn occurrence_labels(n: &DecodedNode, field: &str, occ: &OccurrenceLabels) -> Vec<Option<String>> {
    let refs = ref_list(n, field).into_iter();
    refs.map(|i| lookup_name(occ, i)).collect()
}

/// `names[i]` when present, flattening a missing entry and a missing name into `None`.
fn lookup_name(names: &OccurrenceLabels, index: usize) -> Option<String> {
    names.get(&index).cloned().flatten()
}
