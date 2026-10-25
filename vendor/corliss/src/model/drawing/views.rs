//! Drawing views (spec 17.3): FAF5EB4D, CE52DF35 (centre `pos`), 6BD04D47 (scale), F7309622,
//! 22CB425C, then two RDxVar parameters (frame width and height) in the sheet DC segment.

use super::{Drawing, T_PARAMETER, T_TEXT, decoded_nodes, decoded_of_type, point2, text_of};
use crate::grammar::DecodedNode;
use crate::model::SegmentView;

/// View scale node (6BD04D47): `value`, `text` ('2:1').
const T_VIEW_SCALE: u32 = 0x6BD0_4D47;
/// View centre node (CE52DF35): `pos` on the sheet.
const T_VIEW_CENTRE: u32 = 0xCE52_DF35;
/// Browser view entry (6B6FA569): 'VIEW1:ex_rect20.ipt'.
const T_BROWSER_VIEW: u32 = 0x6B6F_A569;
/// The format code that marks a view label's scale line.
const LABEL_MARK: &str = "\\Y1;";

/// Node indexes a view was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewNodes {
    /// The scale node.
    pub scale: usize,
    /// The centre node.
    pub centre: Option<usize>,
    /// The frame parameters (up to two).
    pub frame: Vec<usize>,
}

/// A drawing view on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingView {
    /// 1-based sheet.
    pub sheet: usize,
    /// View name ('VIEW1').
    pub name: Option<String>,
    /// File name of the viewed model ('ex_rect20.ipt').
    pub model: Option<String>,
    /// View centre on the sheet, cm.
    pub centre: Option<[f64; 2]>,
    /// Scale (2.0 for 2:1).
    pub scale: Option<f64>,
    /// Scale as shown ('2:1').
    pub scale_text: Option<String>,
    /// Frame width and height on the sheet (model extent x scale), cm.
    pub frame: Option<[f64; 2]>,
    /// Label text as shown ('VIEW1\nSCALE 1:1').
    pub label: Option<String>,
    /// Source nodes.
    pub nodes: ViewNodes,
}

/// What every view of a sheet shares while its scale nodes are read.
struct SheetViewScan<'a> {
    sheet: usize,
    nodes: Vec<(usize, &'a DecodedNode)>,
    labels: Vec<&'a DecodedNode>,
}

impl Drawing<'_> {
    /// Browser view entries in browser order: (view name, model file name).
    pub fn view_entries(&self) -> Vec<(String, String)> {
        self.browser
            .by_type(T_BROWSER_VIEW)
            .map(|(_, n)| {
                let name = n.text("name").unwrap_or_default();
                let (view, model) = name.split_once(':').unwrap_or((name, ""));
                (view.to_string(), model.to_string())
            })
            .collect()
    }

    /// The views of sheet `sheet` (1-based), or of every sheet for `None` (the reference
    /// reader's default).
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("drawing.idw")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let drawing = corliss::model::Drawing::new(&model)?;
    /// for v in drawing.views(None) {
    ///     println!("{:?} of {:?} at {:?}", v.name, v.model, v.centre);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn views(&self, sheet: Option<usize>) -> Vec<DrawingView> {
        let entries = self.view_entries();
        let mut out = Vec::new();
        for (si, prefix) in self.sheet_prefixes().iter().enumerate() {
            if let Some(view) = self.sheet_view(prefix) {
                scan_sheet(&SheetViewScan::of(&view, si + 1), &entries, &mut out);
            }
        }
        out.retain(|v| sheet.is_none_or(|s| v.sheet == s));
        out
    }
}

impl<'a> SheetViewScan<'a> {
    /// The decoded nodes and view labels (texts with a scale line) of one sheet.
    fn of(view: &SegmentView<'a>, sheet: usize) -> Self {
        let labels = decoded_of_type(view, T_TEXT)
            .into_iter()
            .map(|(_, n)| n)
            .filter(|n| n.text("name").is_some_and(|t| t.contains(LABEL_MARK)))
            .collect();
        SheetViewScan {
            sheet,
            nodes: decoded_nodes(view),
            labels,
        }
    }
}

/// Appends the views of one sheet, one per scale node; `out.len()` counts the views read so
/// far, which picks the browser entry of a view without a label.
fn scan_sheet(scan: &SheetViewScan<'_>, entries: &[(String, String)], out: &mut Vec<DrawingView>) {
    let scales = scan
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, (_, n))| n.type_id.0 == T_VIEW_SCALE);
    for (view_index, (i, _)) in scales.enumerate() {
        let label = scan.labels.get(view_index).copied();
        let identity = view_identity(label, entries, out.len());
        out.push(read_view(scan, i, label, identity));
    }
}

/// The view whose scale node is `scan.nodes[i]`.
fn read_view(
    scan: &SheetViewScan<'_>,
    i: usize,
    label: Option<&DecodedNode>,
    (name, model): (Option<String>, Option<String>),
) -> DrawingView {
    let (index, n) = scan.nodes[i];
    let centre = centre_node(&scan.nodes, i);
    let frame = frame_nodes(&scan.nodes, i);
    DrawingView {
        sheet: scan.sheet,
        name,
        model,
        centre: centre.and_then(|(_, c)| point2(c, "pos")),
        scale: n.float("value"),
        scale_text: text_of(n, "text"),
        frame: frame_size(&frame),
        label: label.and_then(|l| text_of(l, "resolved_text")),
        nodes: ViewNodes {
            scale: index,
            centre: centre.map(|(k, _)| k),
            frame: frame.iter().map(|(k, _)| *k).collect(),
        },
    }
}

/// View name and model: the label's first line names the view and the browser entry with that
/// name gives the model (browser order is not sheet order); without a label, the `k`-th entry.
fn view_identity(
    label: Option<&DecodedNode>,
    entries: &[(String, String)],
    k: usize,
) -> (Option<String>, Option<String>) {
    let Some(label) = label else {
        return entries
            .get(k)
            .map_or((None, None), |(v, m)| (Some(v.clone()), Some(m.clone())));
    };
    let resolved = label.text("resolved_text").unwrap_or_default();
    let name = resolved.split('\n').next().unwrap_or_default().to_string();
    // dict(entries): the last entry with a name wins.
    let model = entries
        .iter()
        .rev()
        .find(|(v, _)| *v == name)
        .map(|(_, m)| m.clone());
    (Some(name), model)
}

/// The centre node among the three nodes before the scale node at `i`, nearest first.
fn centre_node<'a>(
    nodes: &[(usize, &'a DecodedNode)],
    i: usize,
) -> Option<(usize, &'a DecodedNode)> {
    nodes[i.saturating_sub(3)..i]
        .iter()
        .rev()
        .find(|(_, m)| m.type_id.0 == T_VIEW_CENTRE)
        .copied()
}

/// The first two RDxVar parameters among the eight nodes after the scale node at `i`.
fn frame_nodes<'a>(nodes: &[(usize, &'a DecodedNode)], i: usize) -> Vec<(usize, &'a DecodedNode)> {
    let end = (i + 9).min(nodes.len());
    nodes[i + 1..end]
        .iter()
        .filter(|(_, m)| {
            m.type_id.0 == T_PARAMETER && m.text("name").is_some_and(|t| t.starts_with("RDxVar"))
        })
        .take(2)
        .copied()
        .collect()
}

/// Frame width and height from the two frame parameters.
fn frame_size(frame: &[(usize, &DecodedNode)]) -> Option<[f64; 2]> {
    match frame {
        [(_, w), (_, h)] => Some([w.float("valueModel")?, h.float("valueModel")?]),
        _ => None,
    }
}
