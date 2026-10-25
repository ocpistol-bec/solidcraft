//! Drawing dimensions (spec 17.5): a dimension object 5E4E86C6 lists its parts as `children`;
//! its E1D3D023 geometry holds, in `dimension_line_points`, the dimension line ends, the two
//! attachment points and the text position (x, y, z each).

use super::{Drawing, DrawingView, T_TEXT, decoded, decoded_of_type, point2, refs_named, text_of};
use crate::error::{CorlissError, Result};
use crate::grammar::DecodedNode;
use crate::model::SegmentView;
use crate::model::view::floats;

/// Dimension object (5E4E86C6).
const T_DIMENSION: u32 = 0x5E4E_86C6;
/// Dimension geometry (E1D3D023).
const T_DIMENSION_GEOMETRY: u32 = 0xE1D3_D023;
/// Slack when testing whether an attachment point lies in a view's frame, cm.
const FRAME_SLACK: f64 = 1e-6;

/// The kind of a drawing dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionKind {
    /// A linear dimension between two attachment points.
    Linear,
}

/// A drawing dimension.
#[derive(Debug, Clone, PartialEq)]
pub struct Dimension {
    /// Dimension node index.
    pub node: usize,
    /// Dimension kind.
    pub kind: DimensionKind,
    /// The two attachment points on the sheet, cm.
    pub points: [[f64; 2]; 2],
    /// Text position on the sheet, cm.
    pub text_pos: [f64; 2],
    /// The value as shown ('40.00'): the text at `text_pos`.
    pub text: Option<String>,
    /// Name of the view both attachment points fall in.
    pub view: Option<String>,
    /// Attachment distance divided by that view's scale (model units, cm).
    pub measured: f64,
}

impl Drawing<'_> {
    /// The dimensions of sheet `sheet` (1-based; the reference reader defaults to 1).
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("drawing.idw")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let drawing = corliss::model::Drawing::new(&model)?;
    /// for d in drawing.dimensions(1)? {
    ///     println!("{:?} in {:?}: {} cm", d.text, d.view, d.measured);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn dimensions(&self, sheet: usize) -> Result<Vec<Dimension>> {
        let view = self.sheet_dc(sheet)?;
        let texts: Vec<&DecodedNode> = decoded_of_type(&view, T_TEXT)
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let views = self.views(Some(sheet));
        let mut out = Vec::new();
        for (index, d) in decoded_of_type(&view, T_DIMENSION) {
            let Some(geometry) = dimension_geometry(&view, d) else {
                continue;
            };
            let points = dimension_points(geometry, index)?;
            out.push(linear_dimension(index, points, &texts, &views));
        }
        Ok(out)
    }
}

/// The dimension at node `index` from its attachment points and text position: the shown
/// value is the text at that position, the view the last one whose frame holds both points.
fn linear_dimension(
    index: usize,
    [p1, p2, text_pos]: [[f64; 2]; 3],
    texts: &[&DecodedNode],
    views: &[DrawingView],
) -> Dimension {
    let text = texts
        .iter()
        .find(|t| point2(t, "loc") == Some(text_pos))
        .and_then(|t| text_of(t, "name"));
    let owner = views.iter().rev().find(|v| frame_holds(v, &[p1, p2]));
    Dimension {
        node: index,
        kind: DimensionKind::Linear,
        points: [p1, p2],
        text_pos,
        text,
        view: owner.and_then(|v| v.name.clone()),
        measured: measured(p1, p2, owner),
    }
}

/// The first decoded geometry node among the dimension's children.
fn dimension_geometry<'a>(view: &SegmentView<'a>, d: &DecodedNode) -> Option<&'a DecodedNode> {
    refs_named(d, "children")
        .into_iter()
        .filter_map(|i| view.node(i))
        .find(|k| k.type_id.0 == T_DIMENSION_GEOMETRY && decoded(k))
}

/// Attachment points 1 and 2 and the text position (x, y of points 3, 4 and 5).
fn dimension_points(geometry: &DecodedNode, node: usize) -> Result<[[f64; 2]; 3]> {
    let a = floats(geometry, "dimension_line_points");
    let xy = |k: usize| a.get(k..k + 2).map(|p| [p[0], p[1]]);
    match (xy(6), xy(9), xy(12)) {
        (Some(p1), Some(p2), Some(tp)) => Ok([p1, p2, tp]),
        _ => Err(CorlissError::invalid(
            "dimension geometry",
            "dimension_line_points",
            0,
            format!("{} floats in node {node}", a.len()),
            "at least 14 floats",
        )),
    }
}

/// True when every point lies within half the view's frame width of its centre (in x).
fn frame_holds(v: &DrawingView, points: &[[f64; 2]]) -> bool {
    let (Some(centre), Some(frame)) = (v.centre, v.frame) else {
        return false;
    };
    points
        .iter()
        .all(|p| (p[0] - centre[0]).abs() <= frame[0] / 2.0 + FRAME_SLACK)
}

/// The sheet distance between the points, divided by the owning view's scale when it has one.
fn measured(p1: [f64; 2], p2: [f64; 2], owner: Option<&DrawingView>) -> f64 {
    let dist = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
    match owner.and_then(|v| v.scale) {
        Some(scale) if scale != 0.0 => dist / scale,
        _ => dist,
    }
}
