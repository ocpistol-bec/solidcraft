//! Moving and scaling drawing views: the sheet DC view
//! records (CE52DF35 centre, 6BD04D47 scale, RDxVar frame size, label) and the SM placement.

use super::fields::{index_of, set_float, set_text};
use super::locate::{find_view, sheet_prefix, with_drawing};
use super::placement::{placement_of, set_transform, transform};
use super::text::{dl_replace, is_dl_text};
use super::{EditSegment, Editor, T_DL_TEXT, T_TEXT, missing, node_in_mut};
use crate::error::Result;
use crate::grammar::DecodeStatus;
use crate::model::Drawing;
use crate::model::drawing::{DrawingView, TitleBlockField};

/// The label's `<Scale>` property value, one to three nodes after its 614A01F1 link.
const T_PROPERTY_VALUE: u32 = 0x7B7C_E84E;

/// Puts the centre of view `view` on `sheet` (1-based) at (`x`, `y`) cm on the sheet: the
/// sheet DC centre record and the SM placement's translation.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::drawing::move_view(&mut ed, 1, "VIEW1", 21.59, 30.0)?;
/// ed.finish()?.save("moved.idw")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn move_view(ed: &mut Editor, sheet: usize, view: &str, x: f64, y: f64) -> Result<()> {
    let (found, _) = with_drawing(ed.document(), |d| find_view(d, sheet, view))?;
    let prefix = sheet_prefix(ed.document(), sheet)?;
    let centre = found
        .nodes
        .centre
        .ok_or_else(|| missing(format!("centre record of view {view}")))?;
    let node = &mut node_in_mut(ed.segment(&format!("{prefix}DCSegment"))?, centre)?.node;
    set_float(node, "pos", 0, x)?;
    set_float(node, "pos", 1, y)?;
    update_placement(ed, &prefix, view, |m| {
        m[3] = x;
        m[7] = y;
    })
}

/// Rewrites the view's SM placement transform through `change`.
fn update_placement(
    ed: &mut Editor,
    prefix: &str,
    view: &str,
    change: impl FnOnce(&mut [f64; 16]),
) -> Result<()> {
    let sm = ed.segment(&format!("{prefix}SMSegment"))?;
    let at = placement_of(sm, view)?;
    let node = &mut node_in_mut(sm, at)?.node;
    let mut m = transform(node)?;
    change(&mut m);
    set_transform(node, &m)
}

/// A view's scale before and after, with its `n:d` labels.
struct ScaleChange {
    old: f64,
    new: f64,
    old_text: String,
    text: String,
}

impl ScaleChange {
    fn of(view: &DrawingView, scale: f64) -> Result<Self> {
        let name = view.name.as_deref().unwrap_or_default();
        let old = view
            .scale
            .ok_or_else(|| missing(format!("scale of view {name}")))?;
        let old_text = view.scale_text.clone().unwrap_or_default();
        Ok(ScaleChange {
            old,
            new: scale,
            old_text,
            text: scale_text(scale),
        })
    }
}

/// Sets the scale of view `view` on `sheet` (1-based) to `scale`, labelled `n:d` (the
/// closest fraction with a denominator up to 1000): the scale record, the frame size
/// parameters (model extent × scale), the label and its cached text, the placement
/// diagonal, and for the sheet's first view the title block's SCALE field.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::drawing::set_view_scale(&mut ed, 1, "VIEW1", 1.0)?;
/// ed.finish()?.save("scaled.idw")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn set_view_scale(ed: &mut Editor, sheet: usize, view: &str, scale: f64) -> Result<()> {
    let read = |d: &Drawing<'_>| Ok((find_view(d, sheet, view)?, d.title_block(sheet)?));
    let ((found, first), fields) = with_drawing(ed.document(), read)?;
    let prefix = sheet_prefix(ed.document(), sheet)?;
    let change = ScaleChange::of(&found, scale)?;
    // The title block's SCALE field shows the sheet's first view scale (dw_scale: '1:1').
    if first {
        retitle(ed, &prefix, &fields, &change)?;
    }
    let dc = ed.segment(&format!("{prefix}DCSegment"))?;
    scale_records(dc, &found, &change)?;
    relabel(dc, view, &change)?;
    recache_label(ed, &prefix, view, &change)?;
    update_placement(ed, &prefix, view, |m| {
        [0, 5, 10].iter().for_each(|k| m[*k] = scale)
    })
}

/// The title-block fields showing the old scale, and their display-list cache.
fn retitle(
    ed: &mut Editor,
    prefix: &str,
    fields: &[TitleBlockField],
    change: &ScaleChange,
) -> Result<()> {
    for f in fields
        .iter()
        .filter(|f| f.text.as_deref() == Some(change.old_text.as_str()))
    {
        let dc = ed.segment(&format!("{prefix}DCSegment"))?;
        set_text(&mut node_in_mut(dc, f.node)?.node, "name", &change.text)?;
        dl_replace(ed, prefix, &change.old_text, &change.text, None)?;
    }
    Ok(())
}

/// The scale record and the frame size parameters, which scale with the view.
fn scale_records(dc: &mut EditSegment, view: &DrawingView, change: &ScaleChange) -> Result<()> {
    let n = &mut node_in_mut(dc, view.nodes.scale)?.node;
    set_float(n, "value", 0, change.new)?;
    set_text(n, "text", &change.text)?;
    for k in &view.nodes.frame {
        let p = &mut node_in_mut(dc, *k)?.node;
        for name in ["valueNominal", "valueModel"] {
            let v = p
                .float(name)
                .ok_or_else(|| missing(format!("{name} of frame parameter #{k}")))?;
            set_float(p, name, 0, v * change.new / change.old)?;
        }
    }
    Ok(())
}

/// The view label 'VIEW1\nSCALE 2:1' and its `<Scale>` property value.
fn relabel(dc: &mut EditSegment, view: &str, change: &ScaleChange) -> Result<()> {
    for lab in dc.of_type(T_TEXT) {
        let n = &mut node_in_mut(dc, lab)?.node;
        let resolved = n.text("resolved_text").unwrap_or_default();
        let labels_view = resolved.split('\n').next() == Some(view) && resolved.contains("SCALE");
        if n.status == DecodeStatus::NoGrammar || resolved.is_empty() || !labels_view {
            continue;
        }
        set_text(
            n,
            "resolved_text",
            &format!("{view}\nSCALE {}", change.text),
        )?;
        let link = index_of(n, "next");
        relabel_property(dc, link, change)?;
    }
    Ok(())
}

/// The 7B7CE84E value among the three nodes after the label's link that still holds the
/// old scale text.
fn relabel_property(dc: &mut EditSegment, link: usize, change: &ScaleChange) -> Result<()> {
    if dc.node(link).is_none() {
        return Ok(());
    }
    for k in link + 1..=link + 3 {
        let Some(pv) = dc.node_mut(k).map(|n| &mut n.node) else {
            continue;
        };
        let decoded = pv.type_id.0 == T_PROPERTY_VALUE && pv.status != DecodeStatus::NoGrammar;
        if decoded && pv.text("value") == Some(change.old_text.as_str()) {
            set_text(pv, "value", &change.text)?;
        }
    }
    Ok(())
}

/// The cached label text: the DL text named after the view shares its group (`parent`)
/// with the 'SCALE ..' line.
fn recache_label(ed: &mut Editor, prefix: &str, view: &str, change: &ScaleChange) -> Result<()> {
    let dl = ed.segment(&format!("{prefix}DLSegment"))?;
    let texts = dl
        .of_type(T_DL_TEXT)
        .into_iter()
        .filter_map(|k| dl.node(k))
        .map(|n| &n.node);
    let named = texts
        .filter(|n| is_dl_text(n))
        .find(|n| n.text("string") == Some(view));
    let Some(parent) = named.map(|n| index_of(n, "parent")) else {
        return Ok(());
    };
    let (old, new) = (
        format!("SCALE {}", change.old_text),
        format!("SCALE {}", change.text),
    );
    dl_replace(ed, prefix, &old, &new, Some(parent))
}

/// `n:d` for the fraction closest to `scale` with a denominator up to 1000
/// (`Fraction(scale).limit_denominator(1000)`); the smallest denominator wins a tie, so the
/// fraction is reduced.
pub(super) fn scale_text(scale: f64) -> String {
    let (mut best_n, mut best_d) = (scale.round(), 1.0_f64);
    let mut best_err = (scale - best_n).abs();
    for d in 2..=1000 {
        let d = f64::from(d);
        let n = (scale * d).round();
        let err = (scale - n / d).abs();
        if err < best_err {
            (best_n, best_d, best_err) = (n, d, err);
        }
    }
    format!("{}:{}", best_n as i64, best_d as i64)
}

#[cfg(test)]
mod tests {
    use super::scale_text;

    #[test]
    fn scale_labels_are_reduced_fractions() {
        assert_eq!(scale_text(1.0), "1:1");
        assert_eq!(scale_text(2.0), "2:1");
        assert_eq!(scale_text(0.5), "1:2");
        assert_eq!(scale_text(0.75), "3:4");
        assert_eq!(scale_text(1.0 / 3.0), "1:3");
    }
}
