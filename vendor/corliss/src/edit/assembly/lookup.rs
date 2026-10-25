//! Finding occurrences and constraints by name, through the browser entries and the assembly
//! reader.

use std::collections::HashMap;

use super::fields::{int, node, ref_of, required_ref};
use crate::edit::EditSegment;
use crate::error::{CorlissError, Result};

pub(super) const DC: &str = "AmDcSegment";
pub(super) const RX: &str = "AmRxSegment";
pub(super) const BROWSER: &str = "AmBrowserSegment";
pub(super) const GRAPHICS: &str = "AmGraphicsSegment";
pub(super) const DESIGN_VIEW: &str = "DesignViewSegment";

pub(super) const T_OCCURRENCE: u32 = 0x9087_4D61;
pub(super) const T_LABEL: u32 = 0x2B48_A42B;
pub(super) const T_CONSTRAINT: u32 = 0x4E86_F047;
pub(super) const T_CACHE: u32 = 0xCB07_2B3B;
pub(super) const T_RX_PLACEMENT: u32 = 0x2327_92BC;
pub(super) const T_BROWSER_OCCURRENCE: u32 = 0x767A_2031;
pub(super) const T_BROWSER_DOCUMENT: u32 = 0x2524_F6EB;
pub(super) const T_BROWSER_ARRANGEMENT: u32 = 0x189B_3560;
/// DC record pair holding each occurrence's placement, in occurrence order.
pub(super) const PLACEMENT_RECORDS: [u32; 2] = [0x216B_3A55, 0xE273_976D];

/// Object flag of a grounded occurrence.
pub(super) const GROUNDED: i64 = 0x0200_0000;

/// Browser entry classes of constraints and joints, as the reference reader lists them.
pub(super) const BROWSER_CONSTRAINTS: [u32; 13] = [
    0x98D7_9BB0,
    0x98D7_9BB1,
    0x98D7_9BB2,
    0xB606_17B3,
    0xD375_99BD,
    0x4834_4B91,
    0x474E_0249,
    0xAA7B_1426,
    0xF9ED_BED1,
    0xCA52_E762,
    0x28BC_24F0,
    0x3D5C_ABE0,
    0xAB12_B392,
];

/// Browser entries sharing the one `index` counter.
pub(super) fn is_browser_indexed(tid: u32) -> bool {
    [
        0x2524_F6EB,
        0xD81C_DE47,
        0xA742_A7CD,
        0x767A_2031,
        0x9180_FF9F,
        0xE9AC_0931,
    ]
    .contains(&tid)
        || BROWSER_CONSTRAINTS.contains(&tid)
}

/// Occurrence node → label text, from the label nodes' owner refs.
pub(super) fn labels(dc: &EditSegment) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    for i in dc.of_type(T_LABEL) {
        let Some(n) = dc.node(i).map(|n| &n.node) else {
            continue;
        };
        if let Some(owner) = ref_of(n, "owner") {
            out.insert(owner, n.text("label").unwrap_or_default().to_string());
        }
    }
    out
}

/// The occurrence node labelled `name` (e.g. "ex_rect20:2") and its occurrence id.
pub(super) fn occurrence(dc: &EditSegment, name: &str) -> Result<(usize, i64)> {
    let labels = labels(dc);
    let Some(occ) = dc
        .of_type(T_OCCURRENCE)
        .into_iter()
        .find(|i| labels.get(i).is_some_and(|l| l == name))
    else {
        let mut have: Vec<&String> = labels.values().collect();
        have.sort();
        return Err(CorlissError::invalid(
            "assembly",
            "occurrence",
            0,
            name,
            format!("an occurrence label, one of {have:?}"),
        ));
    };
    Ok((occ, occurrence_id(dc, occ)?))
}

/// The occurrence id an occurrence node's holder stores.
pub(super) fn occurrence_id(dc: &EditSegment, occ: usize) -> Result<i64> {
    let holder = required_ref(node(dc, occ)?, "value")?;
    int(node(dc, holder)?, "u32_1")
}

/// Browser constraint / joint entries in node order.
pub(super) fn browser_constraints(browser: &EditSegment) -> Vec<usize> {
    let is_entry = |k: &usize| {
        browser
            .node(*k)
            .is_some_and(|n| BROWSER_CONSTRAINTS.contains(&n.node.type_id.0))
    };
    (1..=browser.nodes.len()).filter(is_entry).collect()
}

/// Constraint names from the browser, paired with the DC constraint nodes in order.
pub(super) fn constraint_names(browser: &EditSegment) -> Vec<String> {
    browser_constraints(browser)
        .iter()
        .map(|k| {
            browser
                .node(*k)
                .and_then(|n| n.node.text("name"))
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// Position of constraint `name` among the constraints.
pub(super) fn constraint_position(names: &[String], name: &str) -> Result<usize> {
    names.iter().position(|n| n == name).ok_or_else(|| {
        CorlissError::invalid(
            "assembly",
            "constraint",
            0,
            name,
            format!("a constraint name, one of {names:?}"),
        )
    })
}

/// The AmRx placement record of occurrence id `oid`.
pub(super) fn rx_placement(rx: &EditSegment, oid: i64) -> Result<usize> {
    rx.of_type(T_RX_PLACEMENT)
        .into_iter()
        .find(|i| rx.node(*i).and_then(|n| n.node.int("u32_1")) == Some(oid))
        .ok_or_else(|| {
            CorlissError::missing(
                "edit",
                "AmRxSegment",
                format!("placement of occurrence id {oid}"),
            )
        })
}

/// A DC constraint node with its browser name and the occurrence node of each selection
/// (as the reference reader pairs them).
pub(super) struct ConstraintInfo {
    pub index: usize,
    pub name: String,
    pub occurrences: Vec<usize>,
}

/// Every constraint of the assembly, in DC node order.
pub(super) fn constraints(dc: &EditSegment, names: &[String]) -> Result<Vec<ConstraintInfo>> {
    let mut out = Vec::new();
    for (k, c) in dc.of_type(T_CONSTRAINT).into_iter().enumerate() {
        let mut occurrences = Vec::new();
        for sel in super::fields::ref_list(node(dc, c)?, "selection") {
            occurrences.extend(selection_occurrence(node(dc, sel)?));
        }
        out.push(ConstraintInfo {
            index: c,
            name: names.get(k).cloned().unwrap_or_default(),
            occurrences,
        });
    }
    Ok(out)
}

/// The occurrence a selection node names: occurrence + proxy (B91FCE52), iMate-driven
/// (1CAEFC09) or tangent (F2C761B8) selections.
fn selection_occurrence(m: &crate::grammar::DecodedNode) -> Option<usize> {
    match m.type_id.0 {
        0xB91F_CE52 => ref_of(m, "ref_1"),
        0x1CAE_FC09 => ref_of(m, "occurrence"),
        0xF2C7_61B8 => ref_of(m, "ref_0"),
        _ => None,
    }
}
