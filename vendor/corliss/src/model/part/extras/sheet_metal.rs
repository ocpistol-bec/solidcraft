//! Sheet metal rules and settings.

use super::text_of;
use crate::grammar::{DecodedNode, FieldValue};
use crate::guid::TypeId;
use crate::model::SegmentView;
use crate::model::part::{Part, T_PARAMETER};
use crate::model::view::node_ref;

/// PmAppSegment sheet metal rule style.
const T_RULE: u32 = 0x10D6_C06B;
/// The part's sheet metal settings: refs to its style parameters and enum values.
const T_SETTINGS: u32 = 0x6045_2313;

/// A sheet metal part's rules and settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetMetal {
    /// The rule styles cached in the document.
    pub rules: Vec<SheetMetalRule>,
    /// The part's settings node, when it has one.
    pub settings: Option<SheetMetalSettings>,
}

/// A sheet metal rule style, its values as the expressions Inventor shows.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetMetalRule {
    /// Style name ("Default_mm").
    pub name: Option<String>,
    /// Thickness ("0.500 mm").
    pub thickness: Option<String>,
    /// Bend radius, relief width, relief depth, minimum remnant, transition radius,
    /// two-bend and three-bend intersection sizes ("#Thickness# * 0.5 ul").
    pub expressions: Vec<Option<String>>,
    /// Its unfold rule.
    pub unfold: Option<UnfoldRule>,
}

/// An unfold rule style.
#[derive(Debug, Clone, PartialEq)]
pub struct UnfoldRule {
    /// Style name ("Default_KFactor").
    pub name: Option<String>,
    /// K-factor ("0.440 ul").
    pub k_factor: Option<String>,
    /// Spline factor ("0.500 ul").
    pub spline_factor: Option<String>,
}

/// The settings node's style parameters and enum values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SheetMetalSettings {
    /// (parameter name, value in database units), first-seen order, later duplicates updating.
    pub parameters: Vec<(Option<String>, Option<f64>)>,
    /// Enumerated settings, in stored order.
    pub enums: Vec<SheetMetalEnum>,
}

/// An enumerated sheet metal setting node.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetMetalEnum {
    /// TypeID of the node.
    pub type_id: TypeId,
    /// Its `type` code.
    pub kind: i64,
    /// Its `value`.
    pub value: i64,
}

impl Part<'_> {
    /// Sheet metal rules and settings; `None` for a part with neither.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// if let Some(sm) = part.sheet_metal() {
    ///     println!("{} rules", sm.rules.len());
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn sheet_metal(&self) -> Option<SheetMetal> {
        let app = self.model().view("PmAppSegment");
        let rules: Vec<SheetMetalRule> = app
            .as_ref()
            .map(|a| {
                a.by_type(T_RULE)
                    .map(|(_, r)| sheet_metal_rule(a, r))
                    .collect()
            })
            .unwrap_or_default();
        let settings = self
            .view()
            .by_type(T_SETTINGS)
            .last()
            .map(|(_, n)| self.sheet_metal_settings(n));
        if rules.is_empty() && settings.is_none() {
            return None;
        }
        Some(SheetMetal { rules, settings })
    }

    fn sheet_metal_settings(&self, n: &DecodedNode) -> SheetMetalSettings {
        let mut out = SheetMetalSettings::default();
        for m in setting_refs(n).filter_map(|i| self.view().node(i)) {
            if m.type_id.0 == T_PARAMETER {
                upsert(
                    &mut out.parameters,
                    text_of(m, "name"),
                    m.float("valueModel"),
                );
            } else if let (Some(kind), Some(value)) = (m.int("type"), m.int("value")) {
                out.enums.push(SheetMetalEnum {
                    type_id: m.type_id,
                    kind,
                    value,
                });
            }
        }
        out
    }
}

/// The node indexes the settings node's `properties` and `t<k>` NodeRefs name.
fn setting_refs(n: &DecodedNode) -> impl Iterator<Item = usize> + '_ {
    n.fields
        .iter()
        .filter(|f| is_setting_ref(f.name.as_deref()))
        .filter_map(|f| match f.value {
            FieldValue::Ref(r, _) => r.index(),
            _ => None,
        })
}

/// The settings refs that name style parameters and enum nodes: `properties` and `t<k>`.
fn is_setting_ref(name: Option<&str>) -> bool {
    let name = name.unwrap_or("");
    name == "properties" || name.starts_with('t')
}

/// Sets `key` to `value`, keeping the position of an existing entry.
fn upsert(list: &mut Vec<(Option<String>, Option<f64>)>, key: Option<String>, value: Option<f64>) {
    match list.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => list.push((key, value)),
    }
}

fn sheet_metal_rule(app: &SegmentView<'_>, r: &DecodedNode) -> SheetMetalRule {
    let unfold = node_ref(r, "unfoldRule")
        .and_then(|u| app.node(u))
        .map(|u| UnfoldRule {
            name: text_of(u, "name"),
            k_factor: text_of(u, "facK"),
            spline_factor: text_of(u, "facSpline"),
        });
    SheetMetalRule {
        name: text_of(r, "name"),
        thickness: text_of(r, "thickness"),
        expressions: (1..=7).map(|k| text_of(r, &format!("txt_{k}"))).collect(),
        unfold,
    }
}
