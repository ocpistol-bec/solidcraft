//! Lookups over a decoded segment that every model reader shares.

use std::collections::HashMap;

use crate::document::DecodedSegment;
use crate::grammar::{DecodedNode, Field, FieldValue};

/// FxAttribute (2B48A42B): the browser label of the node its `owner` names (spec 14.2).
pub(crate) const T_LABEL: u32 = 0x2B48_A42B;

/// A decoded segment with its browser labels indexed; node indexes are 1-based NodeRef indexes.
pub struct SegmentView<'a> {
    seg: &'a DecodedSegment,
    names: HashMap<usize, String>,
}

impl<'a> SegmentView<'a> {
    /// Indexes the labels of `seg`.
    pub fn new(seg: &'a DecodedSegment) -> Self {
        let mut names = HashMap::new();
        for (_, n) in by_type_in(seg, T_LABEL) {
            if let (Some(owner), Some(label)) = (node_ref(n, "owner"), n.text("label")) {
                names.insert(owner, label.to_string());
            }
        }
        SegmentView { seg, names }
    }

    /// The segment.
    pub fn segment(&self) -> &'a DecodedSegment {
        self.seg
    }

    /// The node at 1-based `index`.
    pub fn node(&self, index: usize) -> Option<&'a DecodedNode> {
        index.checked_sub(1).and_then(|k| self.seg.nodes.get(k))
    }

    /// The TypeID of the node at `index`, as u32.
    pub fn type_of(&self, index: usize) -> Option<u32> {
        self.node(index).map(|n| n.type_id.0)
    }

    /// (index, node) of every node of TypeID `tid`, in block order.
    pub fn by_type(&self, tid: u32) -> impl Iterator<Item = (usize, &'a DecodedNode)> {
        by_type_in(self.seg, tid)
    }

    /// Every (index, node).
    pub fn nodes(&self) -> impl Iterator<Item = (usize, &'a DecodedNode)> {
        self.seg.nodes.iter().enumerate().map(|(k, n)| (k + 1, n))
    }

    /// The browser label of the node at `index`.
    pub fn name(&self, index: usize) -> Option<&str> {
        self.names.get(&index).map(String::as_str)
    }

    /// The label as an owned string, for model records.
    pub fn name_of(&self, index: Option<usize>) -> Option<String> {
        index.and_then(|i| self.name(i)).map(str::to_string)
    }
}

fn by_type_in(seg: &DecodedSegment, tid: u32) -> impl Iterator<Item = (usize, &DecodedNode)> {
    seg.nodes
        .iter()
        .enumerate()
        .filter(move |(_, n)| n.type_id.0 == tid)
        .map(|(k, n)| (k + 1, n))
}

/// Top-level fields called `name`, else those that had the name before a rename.
pub fn named<'n>(n: &'n DecodedNode, name: &str) -> Vec<&'n Field> {
    let exact: Vec<&Field> = n
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some(name))
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    n.fields
        .iter()
        .filter(|f| f.aliases.iter().any(|a| a == name))
        .collect()
}

/// The f64 values of the top-level fields called `name` (one per scalar field, all of an array).
/// Points store `pos` as three consecutive f64 fields, so `floats(n, "pos")` gives the point.
pub fn floats(n: &DecodedNode, name: &str) -> Vec<f64> {
    named(n, name)
        .into_iter()
        .flat_map(|f| match &f.value {
            FieldValue::F64s(v) => v.clone(),
            FieldValue::F32s(v) => v.iter().map(|x| f64::from(*x)).collect(),
            other => other.as_f64().into_iter().collect(),
        })
        .collect()
}

/// The three floats of field `name` as a point or vector.
pub fn vec3(n: &DecodedNode, name: &str) -> [f64; 3] {
    let v = floats(n, name);
    [
        v.first().copied().unwrap_or(0.0),
        v.get(1).copied().unwrap_or(0.0),
        v.get(2).copied().unwrap_or(0.0),
    ]
}

/// The two floats of field `name`.
pub fn vec2(n: &DecodedNode, name: &str) -> [f64; 2] {
    let v = floats(n, name);
    [
        v.first().copied().unwrap_or(0.0),
        v.get(1).copied().unwrap_or(0.0),
    ]
}

/// The node index a NodeRef field `name` names, or `None` for a null ref or no such field.
pub fn node_ref(n: &DecodedNode, name: &str) -> Option<usize> {
    n.fields
        .iter()
        .find(|f| f.is(name) && matches!(f.value, FieldValue::Ref(..)))
        .and_then(|f| match f.value {
            FieldValue::Ref(r, _) => r.index(),
            _ => None,
        })
}

/// The NodeRef indexes of list field `name` (0 for null entries), as the reference readers list them.
pub fn ref_list(n: &DecodedNode, name: &str) -> Vec<usize> {
    let Some(f) = n
        .fields
        .iter()
        .find(|f| f.is(name) && matches!(f.value, FieldValue::List(_)))
    else {
        return Vec::new();
    };
    let FieldValue::List(l) = &f.value else {
        return Vec::new();
    };
    l.elements
        .iter()
        .filter_map(|el| match el.first().map(|f| &f.value) {
            Some(FieldValue::Ref(r, _)) => Some(r.index().unwrap_or(0)),
            _ => None,
        })
        .collect()
}

/// A row-major 4 × 4 matrix (translation in column 3, cm).
pub type Matrix4 = [[f64; 4]; 4];

/// The masked matrix stored as fields `prefix.d1`, `prefix.d2`, `prefix.m<j>` (spec 4.5.1).
pub fn transform(n: &DecodedNode, prefix: &str) -> Option<Matrix4> {
    let d1 = n.int(&format!("{prefix}.d1"))?;
    let d2 = n.int(&format!("{prefix}.d2")).unwrap_or(0);
    let cell_prefix = format!("{prefix}.m");
    let mut cells = n
        .fields
        .iter()
        .filter(|f| {
            f.name
                .as_deref()
                .is_some_and(|x| x.starts_with(&cell_prefix))
        })
        .filter_map(|f| f.value.as_f64());
    let mut m = [[0.0; 4]; 4];
    for j in 0..16 {
        let bit = 1 << j;
        m[j / 4][j % 4] = match (d2 & bit != 0, d1 & bit != 0) {
            (false, true) => 1.0,
            (true, false) => 0.0,
            (true, true) => -1.0,
            (false, false) => cells.next().unwrap_or(0.0),
        };
    }
    Some(m)
}

/// `m · (x, y, z, 1)`.
pub fn apply(m: &Matrix4, p: [f64; 3]) -> [f64; 3] {
    let row = |r: usize| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3];
    [row(0), row(1), row(2)]
}

/// The cross product.
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
