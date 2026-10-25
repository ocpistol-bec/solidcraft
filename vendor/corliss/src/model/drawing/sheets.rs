//! Sheets: browser names, base names and sizes (spec 17.2).

use super::{Drawing, T_PARAMETER, decoded, decoded_nodes, decoded_of_type, text_of};
use crate::grammar::{DecodedNode, FieldValue, OtherValue};

/// Sheet size group head in `DlDocDCSegment` (866CBE44).
const T_SHEET_SIZE: u32 = 0x866C_BE44;
/// Browser sheet entry (79EBBBEA): 'Sheet:1'.
const T_BROWSER_SHEET: u32 = 0x79EB_BBEA;

/// A sheet size: a standard size letter, or the stored code when it names no standard size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetSize {
    /// A standard size ("A" … "E", "A0" … "A4").
    Named(&'static str),
    /// A code the reader has no name for (custom sizes).
    Code(i64),
}

impl SheetSize {
    /// The size that `code` stores (the reference reader's size-code table).
    ///
    /// ```
    /// use corliss::model::drawing::SheetSize;
    /// assert_eq!(SheetSize::from_code(3), SheetSize::Named("D"));
    /// assert_eq!(SheetSize::from_code(5), SheetSize::Code(5));
    /// ```
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => SheetSize::Named("A"),
            1 => SheetSize::Named("B"),
            2 => SheetSize::Named("C"),
            3 => SheetSize::Named("D"),
            4 => SheetSize::Named("E"),
            7 => SheetSize::Named("A0"),
            8 => SheetSize::Named("A1"),
            9 => SheetSize::Named("A2"),
            10 => SheetSize::Named("A3"),
            11 => SheetSize::Named("A4"),
            other => SheetSize::Code(other),
        }
    }
}

/// One sheet of the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// 1-based position in creation order.
    pub index: usize,
    /// Segment prefix (`DLSheet21`).
    pub segment: String,
    /// Display name from the browser ('Sheet:1').
    pub name: Option<String>,
    /// Base name from the sheet's head node ('Sheet').
    pub base_name: Option<String>,
    /// Sheet size.
    pub size: Option<SheetSize>,
    /// Width, cm.
    pub width: Option<f64>,
    /// Height, cm.
    pub height: Option<f64>,
}

/// (size code, width, height) of one sheet from its `DlDocDCSegment` group.
type SizeGroup = (Option<i64>, Option<f64>, Option<f64>);

impl Drawing<'_> {
    /// The sheets in creation order with their browser names and sizes.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("drawing.idw")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let drawing = corliss::model::Drawing::new(&model)?;
    /// let first = drawing.sheets().into_iter().next();
    /// println!("{:?}", first.and_then(|s| s.size));
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn sheets(&self) -> Vec<Sheet> {
        let names: Vec<Option<String>> = decoded_of_type(&self.browser, T_BROWSER_SHEET)
            .into_iter()
            .map(|(_, n)| text_of(n, "name"))
            .collect();
        let sizes = self.sheet_size_groups();
        let prefixes = self.sheet_prefixes();
        prefixes
            .into_iter()
            .enumerate()
            .map(|(i, prefix)| {
                let size = sizes.get(i).copied().unwrap_or((None, None, None));
                self.sheet_at(i, prefix, names.get(i).cloned().flatten(), size)
            })
            .collect()
    }

    /// The sheet at 0-based position `i`.
    fn sheet_at(&self, i: usize, prefix: String, name: Option<String>, size: SizeGroup) -> Sheet {
        let (code, width, height) = size;
        Sheet {
            index: i + 1,
            base_name: self.sheet_base_name(&prefix),
            segment: prefix,
            name,
            size: code.map(SheetSize::from_code),
            width,
            height,
        }
    }

    /// The `name` of the sheet segment's first node.
    fn sheet_base_name(&self, prefix: &str) -> Option<String> {
        let view = self.sheet_view(prefix)?;
        view.node(1)
            .filter(|n| decoded(n))
            .and_then(|n| text_of(n, "name"))
    }

    /// One size group per sheet: an 866CBE44 head followed, within four nodes, by the width
    /// and height parameters.
    fn sheet_size_groups(&self) -> Vec<SizeGroup> {
        let nodes = decoded_nodes(&self.doc_view);
        let mut groups = Vec::new();
        for (i, (_, n)) in nodes.iter().enumerate() {
            if n.type_id.0 != T_SHEET_SIZE {
                continue;
            }
            let end = (i + 5).min(nodes.len());
            let params: Vec<&DecodedNode> = nodes[i + 1..end]
                .iter()
                .map(|(_, m)| *m)
                .filter(|m| m.type_id.0 == T_PARAMETER)
                .collect();
            if let [w, h, ..] = params[..] {
                groups.push((size_code(n), w.float("valueModel"), h.float("valueModel")));
            }
        }
        groups
    }
}

/// The size code of a group head: `sheet_size`, else the second word of `a0` (older grammars).
fn size_code(n: &DecodedNode) -> Option<i64> {
    if let Some(value) = n.get("sheet_size") {
        return value.as_int();
    }
    match n.get("a0") {
        None => Some(0),
        Some(FieldValue::Other(_, OtherValue::Tuple(items))) => match items.get(1) {
            Some(OtherValue::Int(code)) => Some(*code),
            _ => None,
        },
        Some(value) => value.as_ints().and_then(|v| v.get(1).copied()),
    }
}
