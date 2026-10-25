//! Component occurrences: AmDc 90874D61 joined with the UFRxDoc occurrence records, the
//! AmRx placements and the representation states.

use std::collections::{HashMap, HashSet};

use super::Assembly;
use crate::grammar::DecodedNode;
use crate::model::view::{Matrix4, transform};

/// Component occurrence node (label "ex_rect20:1" via 2B48A42B).
const T_OCCURRENCE: u32 = 0x9087_4D61;
/// Resolved placement per occurrence in AmRxSegment (`u32_1` = occurrence id).
const T_PLACEMENT: u32 = 0x2327_92BC;
/// Per-occurrence record, in occurrence order; `u32_0` low byte = BOM structure override.
const T_BOM_RECORD: u32 = 0xE273_976D;
/// Occurrence flag: grounded.
const GROUNDED: i64 = 0x0200_0000;
/// Occurrence flag: set on the source and every element of a component pattern.
const PATTERN_MEMBER: i64 = 0x0400_0000;

/// BOM structure override of an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BomStructure {
    /// 0: the referenced document's own BOM structure.
    Default,
    /// 1: Reference (as_bomref).
    Reference,
    /// A code no fixture shows.
    Other(u8),
}

impl BomStructure {
    fn from_code(code: u8) -> Self {
        match code {
            0 => BomStructure::Default,
            1 => BomStructure::Reference,
            other => BomStructure::Other(other),
        }
    }
}

/// One component occurrence.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// AmDc node index of the 90874D61 node.
    pub index: usize,
    /// Browser label ("ex_rect20:1").
    pub name: Option<String>,
    /// Occurrence id (`u32_1` of the node's `value` holder, 90874D60).
    pub id: Option<i64>,
    /// Grounded (flags bit 0x02000000).
    pub grounded: bool,
    /// Member of a component pattern (flags bit 0x04000000).
    pub pattern_member: bool,
    /// Hidden in the active design view.
    pub hidden: bool,
    /// Suppressed in a model state.
    pub suppressed: bool,
    /// BOM structure override, when the document stores per-occurrence records.
    pub bom_structure: Option<BomStructure>,
    /// Resolved placement (row-major, translation in cm).
    pub transform: Option<Matrix4>,
    /// Referenced file path; `None` when no UFRxDoc record or file matches.
    pub file: Option<String>,
    /// Instance number (the `:n` label suffix); `None` when the UFRxDoc has no record.
    pub instance: Option<u32>,
}

/// File path and instance number of one UFRxDoc occurrence record.
type UfrxOccurrence = (Option<String>, u32);

/// What an occurrence record joins: UFRxDoc records and AmRx placements by occurrence id,
/// the hidden and suppressed ids, and the BOM codes in occurrence order.
struct OccurrenceSources {
    records: HashMap<i64, UfrxOccurrence>,
    placements: HashMap<i64, Option<Matrix4>>,
    hidden: HashSet<i64>,
    suppressed: HashSet<i64>,
    bom: Vec<u8>,
}

impl Assembly<'_> {
    /// Every component occurrence, in AmDc order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let grounded = corliss::model::Assembly::new(&model)?
    ///     .occurrences()
    ///     .into_iter()
    ///     .filter(|o| o.grounded)
    ///     .count();
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn occurrences(&self) -> Vec<Occurrence> {
        let sources = OccurrenceSources {
            records: self.ufrx_occurrences(),
            placements: self.placements(),
            hidden: self.hidden_ids(),
            suppressed: self.suppressed_ids(),
            bom: self.bom_codes(),
        };
        let nodes = self.dc.by_type(T_OCCURRENCE).enumerate();
        nodes
            .map(|(position, (index, n))| self.occurrence(&sources, position, index, n))
            .collect()
    }

    /// The occurrence `n` at AmDc `index`, the `position`-th 90874D61 node.
    fn occurrence(
        &self,
        src: &OccurrenceSources,
        position: usize,
        index: usize,
        n: &DecodedNode,
    ) -> Occurrence {
        let id = self.node_at(n, "value").and_then(|h| h.int("u32_1"));
        let flags = n.int("flags").unwrap_or(0);
        let record = id.and_then(|i| src.records.get(&i));
        Occurrence {
            index,
            name: self.dc.name_of(Some(index)),
            id,
            grounded: flags & GROUNDED != 0,
            pattern_member: flags & PATTERN_MEMBER != 0,
            hidden: id.is_some_and(|i| src.hidden.contains(&i)),
            suppressed: id.is_some_and(|i| src.suppressed.contains(&i)),
            bom_structure: src.bom.get(position).map(|c| BomStructure::from_code(*c)),
            transform: id.and_then(|i| src.placements.get(&i).copied().flatten()),
            file: record.and_then(|r| r.0.clone()),
            instance: record.map(|r| r.1),
        }
    }

    /// UFRxDoc occurrence records by occurrence id.
    fn ufrx_occurrences(&self) -> HashMap<i64, UfrxOccurrence> {
        let files = self.files();
        let file_of = |ref_id: u32| files.get(&ref_id).cloned();
        let occurrences = self.ufrx.occurrences().into_iter();
        occurrences
            .map(|o| (i64::from(o.id), (file_of(o.file_ref_id), o.name_suffix)))
            .collect()
    }

    /// AmRx placements by occurrence id.
    fn placements(&self) -> HashMap<i64, Option<Matrix4>> {
        let rx = self.rx.by_type(T_PLACEMENT);
        rx.filter_map(|(_, n)| Some((n.int("u32_1")?, transform(n, "transformation"))))
            .collect()
    }

    /// BOM structure codes of the E273976D records, in occurrence order.
    fn bom_codes(&self) -> Vec<u8> {
        let records = self.dc.by_type(T_BOM_RECORD);
        records
            .map(|(_, n)| (n.int("u32_0").unwrap_or(0) & 0xFF) as u8)
            .collect()
    }

    /// Occurrence ids of `self.occurrences()` paired with their labels.
    pub(super) fn names_by_id(&self) -> HashMap<i64, Option<String>> {
        let occurrences = self.occurrences().into_iter();
        occurrences.filter_map(|o| Some((o.id?, o.name))).collect()
    }
}
