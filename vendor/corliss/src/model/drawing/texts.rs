//! Sheet texts (1FBB3C01, spec 17.4): title-block fields are listed by the title-block
//! instance with `loc` relative to its anchor; notes are held by the sheet component and carry
//! an absolute `loc`; dimension values are free texts.

use std::collections::BTreeSet;

use super::{Drawing, T_TEXT, decoded, decoded_of_type, point2, refs_named, text_of};
use crate::error::{CorlissError, Result};
use crate::grammar::{DecodedNode, Field, FieldValue};
use crate::model::SegmentView;

/// DLxTitleBlockInstance (E679C2B1).
const T_TITLE_BLOCK: u32 = 0xE679_C2B1;
/// Sheet component (90874D63), whose `objects` hold the notes.
const T_SHEET_COMPONENT: u32 = 0x9087_4D63;

/// A title-block field.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleBlockField {
    /// The field text.
    pub text: Option<String>,
    /// Position on the sheet (anchor + `loc`), cm.
    pub pos: [f64; 2],
    /// Text node index.
    pub node: usize,
}

/// A note on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Text without format codes.
    pub text: String,
    /// Text with Inventor's format codes ('\F29;HELLO NOTE').
    pub raw: Option<String>,
    /// Position on the sheet, cm.
    pub pos: Option<[f64; 2]>,
    /// Text node index.
    pub node: usize,
}

/// A dimension value as shown: a free text that is no note, title-block field or view label.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionText {
    /// The value text ('40.00').
    pub text: String,
    /// Position on the sheet, cm.
    pub pos: Option<[f64; 2]>,
    /// Text node index.
    pub node: usize,
}

/// How one sheet's texts are owned.
pub(super) struct SheetTexts<'m> {
    /// The sheet DC segment.
    pub(super) view: SegmentView<'m>,
    /// Text nodes the title block lists.
    pub(super) title_block: BTreeSet<usize>,
    /// The title block's anchor on the sheet.
    pub(super) anchor: [f64; 2],
    /// Nodes the sheet component holds (notes among them).
    pub(super) held: BTreeSet<usize>,
}

impl<'m> Drawing<'m> {
    /// The title-block fields of sheet `sheet` (1-based; the reference reader defaults to 1).
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("drawing.idw")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let drawing = corliss::model::Drawing::new(&model)?;
    /// for f in drawing.title_block(1)? {
    ///     println!("{:?} at {:?}", f.text, f.pos);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn title_block(&self, sheet: usize) -> Result<Vec<TitleBlockField>> {
        let texts = self.sheet_texts(sheet)?;
        let fields = texts.title_block.iter().filter_map(|&i| {
            let n = texts
                .view
                .node(i)
                .filter(|n| decoded(n) && n.type_id.0 == T_TEXT)?;
            Some(title_block_field(i, n, texts.anchor))
        });
        fields.collect()
    }

    /// The notes of sheet `sheet` (1-based; the reference reader defaults to 1).
    pub fn notes(&self, sheet: usize) -> Result<Vec<Note>> {
        let texts = self.sheet_texts(sheet)?;
        let notes = texts
            .free_texts()
            .filter(|(i, _)| texts.held.contains(i))
            .map(|(node, n)| {
                let raw = non_empty_text(n, "resolved_text").or_else(|| text_of(n, "name"));
                Note {
                    text: plain(raw.as_deref().unwrap_or_default()),
                    raw,
                    pos: point2(n, "loc"),
                    node,
                }
            });
        Ok(notes.collect())
    }

    /// The dimension values shown on sheet `sheet` (1-based; the reference reader defaults to 1).
    pub fn dimension_texts(&self, sheet: usize) -> Result<Vec<DimensionText>> {
        let texts = self.sheet_texts(sheet)?;
        let values = texts
            .free_texts()
            .filter(|(i, _)| !texts.held.contains(i))
            .map(|(node, n)| (node, n, n.text("name").unwrap_or_default()))
            .filter(|(_, _, text)| !text.contains("\\Y"))
            .map(|(node, n, text)| DimensionText {
                text: text.to_string(),
                pos: point2(n, "loc"),
                node,
            });
        Ok(values.collect())
    }

    /// Title-block membership, anchor and sheet-component holdings of sheet `sheet`.
    pub(super) fn sheet_texts(&self, sheet: usize) -> Result<SheetTexts<'m>> {
        let view = self.sheet_dc(sheet)?;
        let mut title_block = BTreeSet::new();
        let mut anchor = [0.0, 0.0];
        for (_, n) in decoded_of_type(&view, T_TITLE_BLOCK) {
            anchor = title_block_anchor(n).unwrap_or(anchor);
            title_block.extend(title_block_texts(n));
        }
        let held = decoded_of_type(&view, T_SHEET_COMPONENT)
            .into_iter()
            .flat_map(|(_, c)| refs_named(c, "objects"))
            .collect();
        Ok(SheetTexts {
            view,
            title_block,
            anchor,
            held,
        })
    }
}

impl<'m> SheetTexts<'m> {
    /// Decoded text nodes that are not title-block fields.
    fn free_texts(&self) -> impl Iterator<Item = (usize, &'m DecodedNode)> + '_ {
        decoded_of_type(&self.view, T_TEXT)
            .into_iter()
            .filter(|(i, _)| !self.title_block.contains(i))
    }
}

/// The title-block field at node `i`: its `loc` offset by the title block's anchor.
fn title_block_field(i: usize, n: &DecodedNode, anchor: [f64; 2]) -> Result<TitleBlockField> {
    let loc = point2(n, "loc").ok_or_else(|| {
        CorlissError::invalid(
            "title block field",
            "loc",
            0,
            format!("node {i}"),
            "two floats",
        )
    })?;
    let pos = [anchor[0] + loc[0], anchor[1] + loc[1]];
    Ok(TitleBlockField {
        text: text_of(n, "name"),
        pos,
        node: i,
    })
}

/// The title-block anchor: `local_origin` (2027 grammar), else `pos` (older files), taken
/// only when the name has at least two fields (the reference reader counts fields, not values).
///
/// The 2027 grammar decodes `local_origin` as one two-float array field, so the reference
/// reader keeps the (0, 0) anchor there and reports each field's stored `loc`; Corliss keeps
/// that result so it matches the reference reader's title block (models.json).
fn title_block_anchor(n: &DecodedNode) -> Option<[f64; 2]> {
    let fields = |name: &str| -> Vec<&Field> {
        n.fields
            .iter()
            .filter(|f| f.name.as_deref() == Some(name))
            .collect()
    };
    let mut pos = fields("local_origin");
    if pos.is_empty() {
        pos = fields("pos");
    }
    match pos[..] {
        [x, y, ..] => Some([x.value.as_f64()?, y.value.as_f64()?]),
        _ => None,
    }
}

/// Text nodes the title-block instance lists in `strings` (`lst1` in older grammars).
fn title_block_texts(n: &DecodedNode) -> Vec<usize> {
    let lists = n.fields.iter().filter(|f| f.is("strings") || f.is("lst1"));
    let elements = lists.filter_map(|f| match &f.value {
        FieldValue::List(l) => Some(l.elements.iter().flatten()),
        _ => None,
    });
    elements
        .flatten()
        .filter_map(|x| match x.value {
            FieldValue::Ref(r, _) if r.0 != 0 => Some((r.0 & 0x7FFF_FFFF) as usize),
            _ => None,
        })
        .collect()
}

/// A text field that is present and not empty (as the reference reader tests it).
fn non_empty_text(n: &DecodedNode, name: &str) -> Option<String> {
    n.text(name).filter(|t| !t.is_empty()).map(str::to_string)
}

/// Strips Inventor's inline format codes (`\F29;`, `\Y0;` …): a backslash, letters, an
/// optional number (`[-0-9.]*`) and a semicolon.
///
/// ```
/// assert_eq!(corliss::model::drawing::plain("\\F29;HELLO \\H0.35;NOTE"), "HELLO NOTE");
/// assert_eq!(corliss::model::drawing::plain("a\\b"), "a\\b");
/// ```
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        let code = format_code_len(&rest[at..]);
        // A backslash that starts no code is kept, and scanning resumes after it.
        let skip = if code > 0 {
            code
        } else {
            out.push('\\');
            1
        };
        rest = &rest[at + skip..];
    }
    out.push_str(rest);
    out
}

/// The byte length of the format code at the start of `s`, or 0.
pub(crate) fn format_code_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'\\') {
        return 0;
    }
    let letters = bytes[1..]
        .iter()
        .take_while(|b| b.is_ascii_alphabetic())
        .count();
    if letters == 0 {
        return 0;
    }
    let number = bytes[1 + letters..]
        .iter()
        .take_while(|b| b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        .count();
    let end = 1 + letters + number;
    if bytes.get(end) == Some(&b';') {
        end + 1
    } else {
        0
    }
}
