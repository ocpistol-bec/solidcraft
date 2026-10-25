//! Drawing documents (spec clause 17), after the reference drawing reader.
//!
//! Sheet order and display names come from the browser segment; each sheet is a segment
//! triple `DLSheet<k>DCSegment` / `DLSheet<k>DLSegment` / `DLSheet<k>SMSegment` (k = 21, 22 …
//! in creation order) of which the DC segment holds the data; sheet sizes are parameter groups
//! in `DlDocDCSegment`, in sheet order.

mod dimensions;
mod references;
mod sheet_formats;
mod sheets;
mod texts;
mod views;

pub use dimensions::*;
pub use references::*;
pub use sheet_formats::*;
pub use sheets::*;
pub use texts::*;
pub use views::*;

use super::view::floats;
use super::{Model, SegmentView};
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodeStatus, DecodedNode, Field, FieldValue};

/// The document-level DC segment: sheet size groups.
const DOC_SEGMENT: &str = "DlDocDCSegment";
/// The browser segment: sheet and view entries.
const BROWSER_SEGMENT: &str = "DlBrowserSegment";
/// Parameter node type (spec 14.1.1).
const T_PARAMETER: u32 = 0x9087_4D26;
/// Text node type (1FBB3C01): title-block fields, notes, view labels, dimension values.
const T_TEXT: u32 = 0x1FBB_3C01;

/// A drawing's sheets, views, texts and dimensions.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let model = corliss::model::Model::load(&doc)?;
/// let drawing = corliss::model::Drawing::new(&model)?;
/// for sheet in drawing.sheets() {
///     println!("{:?} {:?} x {:?}", sheet.name, sheet.width, sheet.height);
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub struct Drawing<'m> {
    model: &'m Model<'m>,
    doc_view: SegmentView<'m>,
    browser: SegmentView<'m>,
}

impl<'m> Drawing<'m> {
    /// Reads the drawing model of a drawing document.
    pub fn new(model: &'m Model<'m>) -> Result<Self> {
        let segment = |name: &'static str| {
            model
                .view(name)
                .ok_or_else(|| CorlissError::missing("read", "drawing model", name))
        };
        Ok(Drawing {
            model,
            doc_view: segment(DOC_SEGMENT)?,
            browser: segment(BROWSER_SEGMENT)?,
        })
    }

    /// The decoded document.
    pub fn model(&self) -> &'m Model<'m> {
        self.model
    }

    /// Sheet segment prefixes (`DLSheet21`, `DLSheet22` …) in creation order.
    pub fn sheet_prefixes(&self) -> Vec<String> {
        let mut found: Vec<(u64, String)> = self
            .model
            .segment_names()
            .filter_map(sheet_prefix)
            .collect();
        found.sort();
        found.into_iter().map(|(_, prefix)| prefix).collect()
    }

    /// The DC segment of the sheet with segment prefix `prefix`.
    fn sheet_view(&self, prefix: &str) -> Option<SegmentView<'m>> {
        self.model.view(&format!("{prefix}DCSegment"))
    }

    /// The DC segment of 1-based sheet `sheet`.
    fn sheet_dc(&self, sheet: usize) -> Result<SegmentView<'m>> {
        let prefixes = self.sheet_prefixes();
        let prefix = sheet.checked_sub(1).and_then(|k| prefixes.get(k));
        prefix.and_then(|p| self.sheet_view(p)).ok_or_else(|| {
            let what = format!("sheet {sheet} (expected 1..={})", prefixes.len());
            CorlissError::missing("read", "drawing sheet", what)
        })
    }
}

/// (number, prefix) of a segment named `DLSheet<number>DCSegment`.
fn sheet_prefix(name: &str) -> Option<(u64, String)> {
    let prefix = name.strip_suffix("DCSegment")?;
    let digits = prefix.strip_prefix("DLSheet")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, prefix.to_string()))
}

/// True when a grammar decoded the node (the reference reader's test for a decoded node).
fn decoded(n: &DecodedNode) -> bool {
    n.status != DecodeStatus::NoGrammar
}

/// The (index, node) of every decoded node, in block order.
fn decoded_nodes<'a>(view: &SegmentView<'a>) -> Vec<(usize, &'a DecodedNode)> {
    view.nodes().filter(|(_, n)| decoded(n)).collect()
}

/// The decoded nodes of type `tid`.
fn decoded_of_type<'a>(view: &SegmentView<'a>, tid: u32) -> Vec<(usize, &'a DecodedNode)> {
    view.by_type(tid).filter(|(_, n)| decoded(n)).collect()
}

/// A text field as an owned string.
fn text_of(n: &DecodedNode, name: &str) -> Option<String> {
    n.text(name).map(str::to_string)
}

/// The first two floats of field `name`, when there are two.
fn point2(n: &DecodedNode, name: &str) -> Option<[f64; 2]> {
    match floats(n, name)[..] {
        [x, y, ..] => Some([x, y]),
        _ => None,
    }
}

/// Every non-null NodeRef in `fields` (nested lists included) as (field, node index).
fn field_refs(fields: &[Field]) -> Vec<(&Field, usize)> {
    let mut out = Vec::new();
    for f in fields {
        match &f.value {
            FieldValue::Ref(r, _) => out.extend(r.index().map(|i| (f, i))),
            FieldValue::List(l) => l.elements.iter().for_each(|el| out.extend(field_refs(el))),
            _ => {}
        }
    }
    out
}

/// The targets of the NodeRefs called `name` anywhere in `n` (`n.refs()` filtered by name).
fn refs_named(n: &DecodedNode, name: &str) -> Vec<usize> {
    field_refs(&n.fields)
        .into_iter()
        .filter(|(f, _)| f.is(name))
        .map(|(_, i)| i)
        .collect()
}
