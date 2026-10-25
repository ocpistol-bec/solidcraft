//! Component patterns: rectangular (34AACB53 + ABC2E4ED parameters) and circular
//! (D739EDBB + 6C5CD68F parameters).

use super::{Assembly, OccurrenceLabels, occurrence_labels};
use crate::grammar::DecodedNode;
use crate::model::view::{floats, node_ref, ref_list};

/// Rectangular component pattern (lst0 = sources, lst1 = elements, ref_2 → definition).
const T_RECTANGULAR: u32 = 0x34AA_CB53;
/// Circular component pattern (sources, elements, definition).
const T_CIRCULAR: u32 = 0xD739_EDBB;
/// Rectangular pattern parameters (r0 = pattern definition, r1/r2 counts, r3/r4 spacings,
/// r5/r6 direction vectors).
const T_RECTANGULAR_PARAMS: u32 = 0xABC2_E4ED;
/// Circular pattern parameters (r0 = definition, r1 count, r2 angle, r3 axis).
const T_CIRCULAR_PARAMS: u32 = 0x6C5C_D68F;
/// Browser entry of a component pattern.
const T_BROWSER_PATTERN: u32 = 0x83FB_036D;

/// A component pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// AmDc node index.
    pub index: usize,
    /// Browser name; Inventor's default `Component Pattern <n>:1` when none is stored.
    pub name: String,
    /// Labels of the source occurrences.
    pub sources: Vec<Option<String>>,
    /// The pattern elements.
    pub elements: Vec<PatternElement>,
    /// Layout and parameters, when the definition node is found.
    pub layout: Option<PatternLayout>,
}

/// One element of a component pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternElement {
    /// 1-based element number.
    pub element: Option<i64>,
    /// Labels of the element's occurrences.
    pub occurrences: Vec<Option<String>>,
}

/// The layout of a component pattern (counts unitless, lengths cm, angles rad).
#[derive(Debug, Clone, PartialEq)]
pub enum PatternLayout {
    /// Two-direction rectangular pattern.
    Rectangular {
        /// Element counts per direction.
        counts: [Option<f64>; 2],
        /// Spacings per direction.
        spacings: [Option<f64>; 2],
        /// Direction vectors (`None` for an unused direction).
        directions: [Option<Vec<f64>>; 2],
    },
    /// Circular pattern.
    Circular {
        /// Element count.
        count: Option<f64>,
        /// Angle between elements.
        angle: Option<f64>,
        /// Rotation axis.
        axis: Option<PatternAxis>,
    },
}

/// The rotation axis of a circular pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternAxis {
    /// A point on the axis.
    pub point: Vec<f64>,
    /// Axis direction.
    pub direction: Vec<f64>,
}

/// A pattern node with the names of its sources, elements and definition fields.
type PatternNode<'n> = (usize, &'n DecodedNode, [&'static str; 3]);

impl Assembly<'_> {
    /// Component patterns in AmDc order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// for p in corliss::model::Assembly::new(&model)?.patterns() {
    ///     println!("{} has {} elements", p.name, p.elements.len());
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn patterns(&self) -> Vec<Pattern> {
        let occ = self.occurrence_names();
        let names = self.browser_names(&[T_BROWSER_PATTERN]);
        let mut out = Vec::new();
        for (index, n, [src, els, dfn]) in self.pattern_nodes() {
            out.push(Pattern {
                index,
                name: pattern_name(&names, out.len()),
                sources: occurrence_labels(n, src, &occ),
                elements: self.pattern_elements(n, els, &occ),
                layout: self.pattern_layout(n, dfn),
            });
        }
        out
    }

    /// Rectangular then circular pattern nodes, merged into block order.
    fn pattern_nodes(&self) -> Vec<PatternNode<'_>> {
        let rect = self
            .dc
            .by_type(T_RECTANGULAR)
            .map(|(i, n)| (i, n, ["lst0", "lst1", "ref_2"]));
        let circ = self
            .dc
            .by_type(T_CIRCULAR)
            .map(|(i, n)| (i, n, ["sources", "elements", "definition"]));
        let mut nodes: Vec<PatternNode<'_>> = rect.chain(circ).collect();
        nodes.sort_by_key(|p| p.0);
        nodes
    }

    fn pattern_elements(
        &self,
        n: &DecodedNode,
        els: &str,
        occ: &OccurrenceLabels,
    ) -> Vec<PatternElement> {
        let elements = ref_list(n, els)
            .into_iter()
            .filter_map(|e| self.node(Some(e)));
        elements
            .map(|el| PatternElement {
                element: el.int("element"),
                occurrences: occurrence_labels(el, "occurrences", occ),
            })
            .collect()
    }

    fn pattern_layout(&self, n: &DecodedNode, dfn: &str) -> Option<PatternLayout> {
        let definition = node_ref(n, dfn)?;
        let params_of = |tid: u32| {
            self.dc
                .by_type(tid)
                .filter(|(_, p)| node_ref(p, "r0") == Some(definition))
                .last()
        };
        match n.type_id.0 {
            T_CIRCULAR => params_of(T_CIRCULAR_PARAMS).map(|(_, c)| self.circular_layout(c)),
            _ => params_of(T_RECTANGULAR_PARAMS).map(|(_, p)| self.rectangular_layout(p)),
        }
    }

    fn circular_layout(&self, c: &DecodedNode) -> PatternLayout {
        let axis = self.node_at(c, "r3").map(|ax| PatternAxis {
            point: floats(ax, "pos"),
            direction: floats(ax, "dir"),
        });
        PatternLayout::Circular {
            count: self.value_model(c, "r1"),
            angle: self.value_model(c, "r2"),
            axis,
        }
    }

    fn rectangular_layout(&self, p: &DecodedNode) -> PatternLayout {
        let direction = |k: &str| self.node_at(p, k).map(|m| floats(m, "dir"));
        PatternLayout::Rectangular {
            counts: [self.value_model(p, "r1"), self.value_model(p, "r2")],
            spacings: [self.value_model(p, "r3"), self.value_model(p, "r4")],
            directions: [direction("r5"), direction("r6")],
        }
    }
}

/// The browser name of the `k`-th pattern; the name is often not stored (empty browser entry)
/// and Inventor shows the default "Component Pattern <k + 1>:1".
fn pattern_name(names: &[Option<String>], k: usize) -> String {
    let stored = names.get(k).cloned().flatten().filter(|s| !s.is_empty());
    stored.unwrap_or_else(|| format!("Component Pattern {}:1", k + 1))
}
