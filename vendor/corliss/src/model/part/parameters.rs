//! Model and user parameters (spec clause 14.1) and their equations, after the reference part
//! reader.

mod float_repr;

use std::collections::HashMap;

use super::{Part, T_PARAMETER};
use crate::grammar::{DecodedNode, RefKind};
use crate::model::view::{node_ref, ref_list};

/// Equation stored as a linear relation: target, (parameter, coefficient) terms, constant.
const T_PARAM_MAPPING: u32 = 0x9087_4D40;
/// Child node that marks a parameter as a user parameter.
const T_USERPARAM_MARK: u32 = 0x2F39_A056;
/// Expression constant (value in database units).
const T_EXPR_VALUE: u32 = 0xF8A7_7A04;
/// Expression reference to another parameter (`operand1`).
const T_EXPR_REF: u32 = 0xF8A7_7A05;
/// Unary minus expression node.
const T_EXPR_NEG: u32 = 0xF8A7_7A0C;
/// Binary expression nodes and their operators.
const EXPR_OPERATORS: [(u32, &str); 6] = [
    (0xF8A7_7A06, "+"),
    (0xF8A7_7A07, "-"),
    (0xF8A7_7A08, "*"),
    (0xF8A7_7A09, "/"),
    (0xF8A7_7A0A, "%"),
    (0xF8A7_7A0B, "^"),
];

/// `flags` bit of a user parameter.
const FLAG_USER: i64 = 0x0400_0000;
/// `flags` bit of a driven (reference) parameter.
const FLAG_DRIVEN: i64 = 0x0100_0000;

/// A model or user parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// Node index of the parameter.
    pub index: usize,
    /// Parameter name ("d0", "Thickness").
    pub name: String,
    /// Value in database units (cm, rad).
    pub value: Option<f64>,
    /// True for a user parameter.
    pub user: bool,
    /// True for a driven (reference) parameter.
    pub driven: bool,
    /// The equation rebuilt from the expression tree, in database units ("(d0 * 2.0)").
    pub equation: Option<String>,
    /// The linear relation Inventor stores for an equation that references parameters.
    pub relation: Option<ParameterRelation>,
}

/// `Σ coefficient · parameter + constant = 0` over the parameters an equation uses.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterRelation {
    /// (parameter name, coefficient) terms; the target appears with coefficient -1.
    pub terms: Vec<(Option<String>, Option<f64>)>,
    /// Constant term.
    pub constant: Option<f64>,
}

impl Part<'_> {
    /// Every parameter, in block order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// for p in part.parameters().iter().filter(|p| p.user) {
    ///     println!("{} = {:?} ({:?})", p.name, p.value, p.equation);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn parameters(&self) -> Vec<Parameter> {
        let mut relations = self.parameter_relations();
        self.view()
            .by_type(T_PARAMETER)
            .map(|(index, n)| {
                let flags = n.int("flags").unwrap_or(0);
                Parameter {
                    index,
                    name: n.text("name").unwrap_or_default().to_string(),
                    value: n.float("valueModel"),
                    user: flags & FLAG_USER != 0 || self.has_user_mark(index),
                    driven: flags & FLAG_DRIVEN != 0,
                    equation: node_ref(n, "formula").and_then(|f| self.expression(f)),
                    relation: relations.remove(&index),
                }
            })
            .collect()
    }

    /// The equation of expression node `index` as text (database units): constants as Python
    /// prints them, parameter names, `(a op b)` and `-(a)`, `<TYPEID>` for unknown nodes.
    /// `None` when the root is a reference to an unnamed parameter or a missing node.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// let equation = part.expression(84);
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn expression(&self, index: usize) -> Option<String> {
        let n = self.view().node(index)?;
        match n.type_id.0 {
            T_EXPR_VALUE => Some(
                n.float("value")
                    .map_or_else(|| "None".to_string(), float_repr::rounded_repr),
            ),
            T_EXPR_REF => {
                let target = node_ref(n, "operand1").and_then(|i| self.view().node(i))?;
                target.text("name").map(str::to_string)
            }
            T_EXPR_NEG => Some(format!("-({})", self.operand(n, "operand1"))),
            tid => Some(self.binary_expression(n, tid)),
        }
    }

    /// `(a op b)` for an operator node, `<TYPEID>` for a node that is not one.
    fn binary_expression(&self, n: &DecodedNode, tid: u32) -> String {
        match EXPR_OPERATORS.iter().find(|(t, _)| *t == tid) {
            Some((_, op)) => format!(
                "({} {op} {})",
                self.operand(n, "operand1"),
                self.operand(n, "operand2")
            ),
            None => format!("<{tid:08X}>"),
        }
    }

    /// An operand's text; a reference to an unnamed parameter reads "None", as the reference
    /// reader's string formatting prints it.
    fn operand(&self, n: &DecodedNode, field: &str) -> String {
        let text = node_ref(n, field).and_then(|i| self.expression(i));
        text.unwrap_or_else(|| "None".to_string())
    }

    /// True when the parameter owns a user-parameter marker child.
    fn has_user_mark(&self, index: usize) -> bool {
        let Some(n) = self.view().node(index) else {
            return false;
        };
        n.refs()
            .into_iter()
            .filter(|(_, kind)| *kind == RefKind::Child)
            .filter_map(|(r, _)| r.index())
            .any(|i| self.view().type_of(i) == Some(T_USERPARAM_MARK))
    }

    /// Parameter index → the linear relation stored for its equation.
    fn parameter_relations(&self) -> HashMap<usize, ParameterRelation> {
        let mut out = HashMap::new();
        for (_, n) in self.view().by_type(T_PARAM_MAPPING) {
            let terms = ref_list(n, "mapping")
                .into_iter()
                .map(|m| self.relation_term(m))
                .collect();
            if let Some(target) = node_ref(n, "target") {
                out.insert(
                    target,
                    ParameterRelation {
                        terms,
                        constant: n.float("f"),
                    },
                );
            }
        }
        out
    }

    /// (parameter name, coefficient) of mapping node `index`.
    fn relation_term(&self, index: usize) -> (Option<String>, Option<f64>) {
        let Some(m) = self.view().node(index) else {
            return (None, None);
        };
        let param = node_ref(m, "parameter").and_then(|p| self.view().node(p));
        (
            param.and_then(|p| p.text("name")).map(str::to_string),
            m.float("x"),
        )
    }
}
