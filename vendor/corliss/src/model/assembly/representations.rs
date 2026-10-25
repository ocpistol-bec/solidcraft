//! Representations: model states (UFRxDoc lods), design views (DesignViewSegment) and
//! positional representations (UFRxDoc exports, AmRx override table).

use std::collections::HashSet;

use super::Assembly;
use crate::grammar::{DecodedNode, FieldValue};
use crate::model::SegmentView;
use crate::model::view::{floats, ref_list};
use crate::ufrx::{Record, Value};

/// The design-view segment.
const DV_SEGMENT: &str = "DesignViewSegment";
/// Design-view root; its `name` is the active view.
const T_VIEW_ROOT: u32 = 0x328F_C2EA;
/// One design view (name, `lst0` → camera, settings, hidden list).
const T_VIEW: u32 = 0x9DC2_A241;
/// A design view's list of hidden occurrence ids.
const T_HIDDEN_LIST: u32 = 0x71C2_2321;
/// A design view's camera.
const T_CAMERA: u32 = 0x0882_3621;
/// Positional-representation override table (AmRx).
const T_OVERRIDE_TABLE: u32 = 0x4E32_D51B;
/// UFRxDoc lod `suppressed` keys are 0x30 + occurrence id.
const SUPPRESSED_KEY_BASE: i64 = 0x30;
/// UFRxDoc export type of a positional representation.
const POSITIONAL_EXPORT: &[u8] = b"PosRep";

/// Model states, design views and positional representations of an assembly.
#[derive(Debug, Clone, PartialEq)]
pub struct Representations {
    /// Model-state names (`[Primary]` first).
    pub model_states: Vec<String>,
    /// Design views and the active one.
    pub design_views: DesignViews,
    /// Positional representation names; `[Primary]` (the master) first.
    pub positional: Vec<String>,
    /// Rows of the positional-representation override table.
    pub positional_overrides: Vec<PositionalOverride>,
}

/// The design views of an assembly.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DesignViews {
    /// Name of the active view.
    pub active: Option<String>,
    /// Every view, in segment order.
    pub views: Vec<DesignView>,
}

/// One design view.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignView {
    /// View name.
    pub name: Option<String>,
    /// Occurrence ids hidden in this view.
    pub hidden: Vec<i64>,
    /// The view's camera, when it stores one.
    pub camera: Option<Camera>,
}

/// A design-view camera (cm).
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// Target point.
    pub target: Vec<f64>,
    /// Eye point.
    pub eye: Vec<f64>,
}

/// One overridden item of the positional-representation table.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionalOverride {
    /// The item ("Mate:1 (Constraint Offset value)").
    pub item: Option<String>,
    /// Its value in the master representation, as text ("5 mm").
    pub master: Option<String>,
    /// (representation name, value as text) for every other representation with a cell.
    pub values: Vec<(Option<String>, Option<String>)>,
}

/// A column of the override table: its name and its cells' texts.
type OverrideColumn = (Option<String>, Vec<Option<String>>);

impl Assembly<'_> {
    /// Model states, design views and positional representations.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let reps = corliss::model::Assembly::new(&model)?.representations();
    /// println!("active view: {:?}", reps.design_views.active);
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn representations(&self) -> Representations {
        let lods = self.ufrx_entries("lods").iter();
        let exports = self.ufrx_entries("exports").iter();
        let positional = exports
            .filter(|x| is_positional(x))
            .map(|x| text(x, "name"));
        Representations {
            model_states: lods.map(|l| text(l, "name")).collect(),
            design_views: self.design_views(),
            positional: std::iter::once("[Primary]".to_string())
                .chain(positional)
                .collect(),
            positional_overrides: self.positional_overrides(),
        }
    }

    /// The design views (DesignViewSegment); empty when the document has none.
    pub fn design_views(&self) -> DesignViews {
        let Some(dv) = self.model.view(DV_SEGMENT) else {
            return DesignViews::default();
        };
        let root = dv.by_type(T_VIEW_ROOT).next();
        DesignViews {
            active: root.and_then(|(_, n)| n.text("name")).map(str::to_string),
            views: dv
                .by_type(T_VIEW)
                .map(|(_, n)| design_view(&dv, n))
                .collect(),
        }
    }

    /// Occurrence ids hidden in the active design view (as_vis, as_viewrep).
    pub fn hidden_ids(&self) -> HashSet<i64> {
        let DesignViews { active, views } = self.design_views();
        let active_view = views.into_iter().find(|v| v.name == active);
        active_view
            .map(|v| v.hidden.into_iter().collect())
            .unwrap_or_default()
    }

    /// Suppressed occurrence ids: UFRxDoc model-state records, key = 0x30 + id.
    pub fn suppressed_ids(&self) -> HashSet<i64> {
        let lods = self.ufrx_entries("lods").iter();
        let keys = lods.flat_map(|l| l.list("suppressed").iter().filter_map(|x| x.uint("key")));
        keys.map(|k| k as i64 - SUPPRESSED_KEY_BASE).collect()
    }

    /// The positional-representation override table (AmRxSegment 4E32D51B, as_posover): one
    /// 82B9E07A column per representation, first the row-label column, then the master.
    pub fn positional_overrides(&self) -> Vec<PositionalOverride> {
        let tables = self.rx.by_type(T_OVERRIDE_TABLE);
        let columns = tables.map(|(_, t)| override_columns(&self.rx, t));
        columns
            .filter(|c| c.len() >= 2)
            .flat_map(|c| override_rows(&c))
            .collect()
    }
}

fn design_view(dv: &SegmentView<'_>, n: &DecodedNode) -> DesignView {
    let mut view = DesignView {
        name: n.text("name").map(str::to_string),
        hidden: Vec::new(),
        camera: None,
    };
    for m in ref_list(n, "lst0").into_iter().filter_map(|c| dv.node(c)) {
        match m.type_id.0 {
            T_HIDDEN_LIST => view.hidden.extend(hidden_occurrences(m)),
            T_CAMERA => {
                view.camera = Some(Camera {
                    target: floats(m, "target"),
                    eye: floats(m, "eye"),
                })
            }
            _ => {}
        }
    }
    view
}

/// The first-field integers of every top-level `occurrences` list of a 71C22321 node.
fn hidden_occurrences(m: &DecodedNode) -> Vec<i64> {
    let lists = m
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some("occurrences"));
    let elements = lists.filter_map(|f| match &f.value {
        FieldValue::List(l) => Some(&l.elements),
        _ => None,
    });
    elements
        .flatten()
        .filter_map(|e| e.first().and_then(|f| f.value.as_int()))
        .collect()
}

fn override_columns(rx: &SegmentView<'_>, table: &DecodedNode) -> Vec<OverrideColumn> {
    let name_of = |i: usize| rx.node(i).and_then(|n| n.text("name")).map(str::to_string);
    let columns = ref_list(table, "lst0")
        .into_iter()
        .filter_map(|c| rx.node(c));
    columns
        .map(|col| {
            (
                col.text("name").map(str::to_string),
                ref_list(col, "lst0").into_iter().map(name_of).collect(),
            )
        })
        .collect()
}

/// One row per cell of the label column; a column shorter than it has no value for the row.
fn override_rows(columns: &[OverrideColumn]) -> Vec<PositionalOverride> {
    let cell = |column: &OverrideColumn, i: usize| column.1.get(i).cloned();
    let rows = columns[0].1.iter().enumerate();
    rows.map(|(i, item)| PositionalOverride {
        item: item.clone(),
        master: cell(&columns[1], i).flatten(),
        values: columns[2..]
            .iter()
            .filter_map(|c| Some((c.0.clone(), cell(c, i)?)))
            .collect(),
    })
    .collect()
}

fn is_positional(export: &Record) -> bool {
    match export.get("type_name") {
        Some(Value::Text8(b)) => b.strip_suffix(b"\0").unwrap_or(b) == POSITIONAL_EXPORT,
        _ => false,
    }
}

fn text(r: &Record, name: &str) -> String {
    r.text(name).unwrap_or_default().to_string()
}
