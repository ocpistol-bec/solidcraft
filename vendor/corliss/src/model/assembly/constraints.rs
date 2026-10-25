//! Assembly constraints and joints (AmDc 4E86F047: style, selections, value parameters).

use super::{Assembly, Geometry, OccurrenceLabels, lookup_name};
use crate::grammar::DecodedNode;
use crate::model::view::{floats, node_ref, ref_list};

/// Assembly constraint node.
const T_CONSTRAINT: u32 = 0x4E86_F047;
/// Selection: occurrence (`ref_1`) + geometry proxy (`dir`).
const T_SELECTION: u32 = 0xB91F_CE52;
/// iMate-driven selection: occurrence, geometry, iMate uid.
const T_IMATE_SELECTION: u32 = 0x1CAE_FC09;
/// Tangent selection: occurrence (`ref_0`), surface (`ref_1`), pick point node (`ref_3`).
const T_TANGENT_SELECTION: u32 = 0xF2C7_61B8;
/// Constraint flag: created from iMates.
const FROM_IMATES: i64 = 0x0010_0000;
/// Browser node types of constraints and joints; their order pairs names with 4E86F047 nodes.
const BROWSER_CONSTRAINTS: [u32; 13] = [
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
    // AssmMateInterfaceResultEntry (iMate result)
    0xAB12_B392,
];

/// Constraint or joint type (4E86F047 `style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    /// 1.
    Mate,
    /// 2.
    Flush,
    /// 3.
    Angle,
    /// 4.
    Tangent,
    /// 5.
    Insert,
    /// 14.
    JointRotational,
    /// 15.
    JointSlider,
    /// 16.
    JointRigid,
    /// 17.
    JointBall,
    /// 18.
    JointCylindrical,
    /// 19.
    JointPlanar,
    /// Another style code (or none stored).
    Other(Option<i64>),
}

impl ConstraintKind {
    fn from_style(style: Option<i64>) -> Self {
        match style {
            Some(1) => ConstraintKind::Mate,
            Some(2) => ConstraintKind::Flush,
            Some(3) => ConstraintKind::Angle,
            Some(4) => ConstraintKind::Tangent,
            Some(5) => ConstraintKind::Insert,
            Some(14) => ConstraintKind::JointRotational,
            Some(15) => ConstraintKind::JointSlider,
            Some(16) => ConstraintKind::JointRigid,
            Some(17) => ConstraintKind::JointBall,
            Some(18) => ConstraintKind::JointCylindrical,
            Some(19) => ConstraintKind::JointPlanar,
            other => ConstraintKind::Other(other),
        }
    }
}

/// Role of a constraint value (258EC6E1 `n_0`, `n_1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// (3, 0).
    Offset,
    /// (4, 0).
    Angle,
    /// (4, 3).
    JointAngle,
    /// (3, 6).
    JointPosition,
    /// Another (n_0, n_1) pair.
    Other(Option<i64>, Option<i64>),
}

impl ValueKind {
    fn from_codes(n0: Option<i64>, n1: Option<i64>) -> Self {
        match (n0, n1) {
            (Some(3), Some(0)) => ValueKind::Offset,
            (Some(4), Some(0)) => ValueKind::Angle,
            (Some(4), Some(3)) => ValueKind::JointAngle,
            (Some(3), Some(6)) => ValueKind::JointPosition,
            _ => ValueKind::Other(n0, n1),
        }
    }
}

/// An assembly constraint or joint.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    /// AmDc node index.
    pub index: usize,
    /// Browser name ("Mate:1").
    pub name: Option<String>,
    /// Constraint type.
    pub kind: ConstraintKind,
    /// Raw flags word.
    pub flags: Option<i64>,
    /// Created from iMates (flags bit 0x00100000).
    pub from_imates: bool,
    /// The selected geometry, in stored order.
    pub selections: Vec<Selection>,
    /// The value parameters.
    pub values: Vec<ConstraintValue>,
}

/// One constraint selection.
#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    /// Geometry of an occurrence.
    Geometry(Box<GeometrySelection>),
    /// A selection node of a type the reader does not interpret.
    Unsupported {
        /// Its TypeID.
        type_id: u32,
    },
}

/// Geometry of one occurrence, as a constraint selects it.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometrySelection {
    /// Label of the occurrence.
    pub occurrence: Option<String>,
    /// The geometry proxy, in part coordinates.
    pub geometry: Option<Geometry>,
    /// The iMate uid (lower-case hex of the stored bytes) for iMate-driven selections.
    pub imate_uid: Option<String>,
    /// The pick point of a tangent selection.
    pub pick_point: Option<Vec<f64>>,
}

/// A constraint value and the parameter that drives it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintValue {
    /// Its role.
    pub kind: ValueKind,
    /// Parameter name ("d0").
    pub parameter: Option<String>,
    /// Parameter value (cm or rad).
    pub value: Option<f64>,
}

impl Assembly<'_> {
    /// Constraints and joints in AmDc order. Their names live
    /// only in the browser, so they are paired with the 4E86F047 nodes in order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// for c in corliss::model::Assembly::new(&model)?.constraints() {
    ///     println!("{:?} {:?}", c.name, c.kind);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn constraints(&self) -> Vec<Constraint> {
        let occ = self.occurrence_names();
        let names = self.browser_names(&BROWSER_CONSTRAINTS);
        let mut out = Vec::new();
        for (index, n) in self.dc.by_type(T_CONSTRAINT) {
            let flags = n.int("flags");
            out.push(Constraint {
                index,
                name: names.get(out.len()).cloned().flatten(),
                kind: ConstraintKind::from_style(n.int("style")),
                flags,
                from_imates: flags.unwrap_or(0) & FROM_IMATES != 0,
                selections: self.constraint_selections(n, &occ),
                values: self.constraint_values(n),
            });
        }
        out
    }

    fn constraint_selections(&self, n: &DecodedNode, occ: &OccurrenceLabels) -> Vec<Selection> {
        let selections = ref_list(n, "selection").into_iter();
        selections.filter_map(|s| self.selection(s, occ)).collect()
    }

    fn constraint_values(&self, n: &DecodedNode) -> Vec<ConstraintValue> {
        let values = ref_list(n, "values").into_iter();
        values.filter_map(|v| self.constraint_value(v)).collect()
    }

    fn selection(&self, index: usize, occ: &OccurrenceLabels) -> Option<Selection> {
        let m = self.node(Some(index))?;
        let (occurrence, geometry) = match m.type_id.0 {
            T_SELECTION => (node_ref(m, "ref_1"), node_ref(m, "dir")),
            T_IMATE_SELECTION => (node_ref(m, "occurrence"), node_ref(m, "geometry")),
            T_TANGENT_SELECTION => (node_ref(m, "ref_0"), node_ref(m, "ref_1")),
            type_id => return Some(Selection::Unsupported { type_id }),
        };
        Some(Selection::Geometry(Box::new(GeometrySelection {
            occurrence: occurrence.and_then(|o| lookup_name(occ, o)),
            geometry: self.geometry(geometry),
            imate_uid: (m.type_id.0 == T_IMATE_SELECTION)
                .then(|| imate_uid(m))
                .flatten(),
            pick_point: self.pick_point(m),
        })))
    }

    /// The `pos` of a tangent selection's pick-point node; `None` when empty or absent.
    fn pick_point(&self, m: &DecodedNode) -> Option<Vec<f64>> {
        if m.type_id.0 != T_TANGENT_SELECTION {
            return None;
        }
        let point = floats(self.node_at(m, "ref_3")?, "pos");
        (!point.is_empty()).then_some(point)
    }

    fn constraint_value(&self, index: usize) -> Option<ConstraintValue> {
        let m = self.node(Some(index))?;
        let parameter = self.node_at(m, "parameter");
        Some(ConstraintValue {
            kind: ValueKind::from_codes(m.int("n_0"), m.int("n_1")),
            parameter: parameter.and_then(|p| p.text("name")).map(str::to_string),
            value: parameter.and_then(|p| p.float("valueModel")),
        })
    }
}

/// Lower-case hex of the stored `imate_uid` bytes.
fn imate_uid(m: &DecodedNode) -> Option<String> {
    let field = m.field("imate_uid")?;
    Some(field.raw.iter().map(|b| format!("{b:02x}")).collect())
}
