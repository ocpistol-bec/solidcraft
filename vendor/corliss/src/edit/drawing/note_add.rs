//! Adding a general note: the note group of a template drawing's first
//! note is copied into the sheet DC, the sheet SM and the document DC, and wired into the
//! cross-segment tables.

use super::fields::{append_ref, floats_named, int_of, put, set_float, set_int, u32_at};
use super::fields::{refs_named, update_first_word};
use super::locate::sheet_prefix;
use super::note_doc::{Placement, add_note_doc};
use super::note_link::link_note;
use super::note_parts::{IndexMap, append_copies, clone_into, note_group, redecode, remap_flagged};
use super::text::replace_keeping_codes;
use super::xref::{TemplateSegment, bump_revision};
use super::{DOC_SEGMENT, EditSegment, Editor, T_COMPONENT, T_NOTE, T_TEXT};
use super::{first_of_type, missing, node_in, node_in_mut};
use crate::database::SegInfo;
use crate::document::Document;
use crate::error::Result;
use crate::model::drawing::plain;

/// The one t8 record (id, kind, GUID) a first note brings (dw_proj → dw_note0, made by
/// Inventor: e557bd9c-11d2-6f8e-…).
const NOTE_T8_GUID: [u8; 16] = [
    0x9c, 0xbd, 0x57, 0xe5, 0xd2, 0x11, 0x8e, 0x6f, 0x60, 0x00, 0xf0, 0x91, 0x79, 0x03, 0x57, 0xb0,
];
/// Sheet DC and SM ids just above the saved id space can be in use by Inventor in ways the
/// file does not record (dw_proj SM ids 56-69 crash it while 113+ load; dw_dim SM 227-231
/// are even marked allocated), so new ids start this far past the allocation set's top.
const ID_GAP: u64 = 64;
/// Component flag set on sheets holding a note (dw_note, dw_sheet2, dw_asm sheet 1; clear in
/// dw_proj and dw_dim).
pub(super) const HAS_NOTES: i64 = 0x100;
/// Spare width: character counts underestimate wide glyphs ('WRITER NOTE' at 1.1x the
/// template's box still wrapped in Inventor).
const SPARE_WIDTH: f64 = 1.25;

/// The template drawing's first sheet DC and SM and its document DC, opened for reading.
pub(super) struct NoteTemplate {
    editor: Editor,
    prefix: String,
}

impl NoteTemplate {
    fn open(doc: &Document) -> Result<Self> {
        let prefix = sheet_prefix(doc, 1)?;
        let mut editor = Editor::new(doc)?;
        for suffix in ["DCSegment", "SMSegment"] {
            editor.segment(&format!("{prefix}{suffix}"))?;
        }
        editor.segment(DOC_SEGMENT)?;
        Ok(NoteTemplate { editor, prefix })
    }

    fn seg(&self, name: &str) -> Result<&EditSegment> {
        let found = self.editor.segments.get(name);
        found.ok_or_else(|| missing(format!("template segment {name}")))
    }

    /// The template sheet segment `suffix` ("DCSegment", "SMSegment") with the directory.
    pub(super) fn sheet(&self, suffix: &str) -> Result<TemplateSegment<'_>> {
        let seg = self.seg(&format!("{}{suffix}", self.prefix))?;
        Ok(TemplateSegment {
            seg,
            info: self.info(),
        })
    }

    fn info(&self) -> &SegInfo {
        self.editor.seg_info()
    }
}

/// A copied note group: template indexes and their copies, in the same order (note first).
pub(super) struct CopiedNote {
    pub(super) sheet: usize,
    pub(super) prefix: String,
    pub(super) group: Vec<usize>,
    pub(super) copies: Vec<usize>,
}

impl CopiedNote {
    pub(super) fn dc_name(&self) -> String {
        format!("{}DCSegment", self.prefix)
    }

    /// The copied note object.
    pub(super) fn note(&self) -> Result<usize> {
        self.copies
            .first()
            .copied()
            .ok_or_else(|| missing("copied note object"))
    }

    /// The first copy of type `tid` in `dc`, and the template node it was copied from.
    pub(super) fn first_of(&self, dc: &EditSegment, tid: u32) -> Result<(usize, usize)> {
        let is = |k: &(&usize, &usize)| dc.node(*k.0).is_some_and(|n| n.node.type_id.0 == tid);
        let found = self
            .copies
            .iter()
            .zip(&self.group)
            .find(is)
            .map(|(c, t)| (*c, *t));
        found.ok_or_else(|| missing(format!("copied note part {tid:08X}")))
    }
}

/// Where the copied text starts and how much wider the new text is than the template's.
struct TextFit {
    origin: (f64, f64),
    shift: (f64, f64),
    ratio: f64,
}

impl TextFit {
    /// The text box scales with the text length (one line, same font): x of a box point
    /// measured from the template's left edge times the length ratio.
    fn wx(&self, v: f64) -> f64 {
        self.origin.0 + (v - self.origin.0) * self.ratio + self.shift.0
    }
}

/// Adds a general note reading `text` to sheet `sheet` (1-based) with its text box's top
/// left at (`x`, `y`) cm, copied from the first note of `template` (a drawing such as
/// `dw_note.idw`): parts, text, links and style in the sheet DC, its graphics in the sheet
/// SM, the text copy and box in the document DC, the cross-segment references and usage
/// counts. The document moves to the session revision. Returns the new note's node index.
///
/// ```no_run
/// let doc = corliss::Document::open("drawing.idw")?;
/// let template = corliss::Document::open("dw_note.idw")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::drawing::add_note(&mut ed, 1, "WRITER NOTE", 5.0, 20.0, &template)?;
/// ed.finish()?.save("noted.idw")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn add_note(
    ed: &mut Editor,
    sheet: usize,
    text: &str,
    x: f64,
    y: f64,
    template: &Document,
) -> Result<usize> {
    let tpl = NoteTemplate::open(template)?;
    let tdc = tpl.sheet("DCSegment")?;
    let tnote = first_of_type(tdc.seg, T_NOTE)?;
    let note = copy_note(ed, sheet, tdc.seg, tnote)?;
    let fit = fit_text(ed.segment(&note.dc_name())?, &note, text, (x, y))?;
    let wx = |v: f64| fit.wx(v);
    let at = Placement {
        dx: fit.shift.0,
        dy: fit.shift.1,
        wx: &wx,
    };
    let template_text = template_note_text(tdc.seg, tnote)?;
    add_note_doc(ed, &note, tpl.seg(DOC_SEGMENT)?, &template_text, text, &at)?;
    link_note(ed, &tpl, &note, text, &at)?;
    bump_revision(ed)?;
    widen_box(ed.segment(&note.dc_name())?, &note.copies, &at)?;
    note.note()
}

/// Prepares the sheet and appends copies of the template note group to its DC.
fn copy_note(ed: &mut Editor, sheet: usize, tdc: &EditSegment, tnote: usize) -> Result<CopiedNote> {
    let prefix = sheet_prefix(ed.document(), sheet)?;
    let group = note_group(tdc, tnote)?;
    prepare(ed, &prefix, tdc)?;
    let copies = copy_group(
        ed.segment(&format!("{prefix}DCSegment"))?,
        tdc,
        tnote,
        &group,
    )?;
    Ok(CopiedNote {
        sheet,
        prefix,
        group,
        copies,
    })
}

/// The template's class list (a superset of a note-less sheet's), its note t8 record, and
/// the id gap on the sheet DC and SM.
fn prepare(ed: &mut Editor, prefix: &str, tdc: &EditSegment) -> Result<()> {
    let dc = ed.segment(&format!("{prefix}DCSegment"))?;
    let body = &tdc.segment.meta.body;
    dc.adopt_type_table(&body.types, &body.tail.t4)?;
    // The template's other t8 records belong to its dimension and must not be copied
    // (dw_proj with them as well crashes Inventor).
    let t8 = &mut dc.segment.meta.body.tail.t8;
    let have: Vec<Vec<u8>> = t8.iter().map(|e| e.payload.clone()).collect();
    let brought = body
        .tail
        .t8
        .iter()
        .filter(|e| e.payload == NOTE_T8_GUID && !have.contains(&e.payload));
    t8.extend(brought.cloned());
    dc.id_gap = ID_GAP;
    ed.segment(&format!("{prefix}SMSegment"))?.id_gap = ID_GAP;
    Ok(())
}

/// Appends copies of the template note group (Inventor renumbers on save); object indexes
/// 0x40000000|n come from the sheet component's counter (90874D63 `index`, +9 per note as in
/// dw_dim → dw_note), and the note objects join its `objects`. Returns the copies in order.
fn copy_group(
    dc: &mut EditSegment,
    tdc: &EditSegment,
    tnote: usize,
    group: &[usize],
) -> Result<Vec<usize>> {
    let (comp, tcomp) = (
        first_of_type(dc, T_COMPONENT)?,
        first_of_type(tdc, T_COMPONENT)?,
    );
    let (copies, mut map) = append_copies(dc, tdc, group, clone_into)?;
    // Template refs to its component point at the sheet's own (unless the group holds it).
    map.entry(tcomp).or_insert(comp);
    let base = int_of(&node_in(dc, comp)?.node, "index");
    let tbase = int_of(&node_in(tdc, tnote)?.node, "index");
    let (span, objects) = renumber_objects(dc, &copies, &map, (base, tbase))?;
    let c = &mut node_in_mut(dc, comp)?.node;
    set_int(c, "index", base + span)?;
    update_first_word(c, "a0", |a| a | HAS_NOTES)?;
    objects
        .iter()
        .try_for_each(|k| append_ref(c, "objects", *k))?;
    Ok(copies)
}

/// Remaps each copy's flagged words and moves the object index at byte 22 from the
/// template's counter range (`bases.1`) to the sheet's (`bases.0`). Returns the span used
/// and the object nodes.
fn renumber_objects(
    dc: &mut EditSegment,
    copies: &[usize],
    map: &IndexMap,
    bases: (i64, i64),
) -> Result<(i64, Vec<usize>)> {
    let (base, tbase) = bases;
    let (mut span, mut objects) = (0, Vec::new());
    for k in copies {
        remap_flagged(&mut node_in_mut(dc, *k)?.node.fields, map)?;
        let mut payload = node_in(dc, *k)?.payload();
        let v = if payload.len() >= 26 {
            u32_at(&payload, 22).map_or(0, i64::from)
        } else {
            0
        };
        if v >> 30 == 1 && (tbase..tbase + 16).contains(&v) {
            put(&mut payload, 22, &((base + v - tbase) as u32).to_le_bytes());
            span = span.max(v - tbase + 1);
            objects.push(*k);
        }
        redecode(dc, *k, &payload)?;
    }
    Ok((span, objects))
}

/// Measures the copied text against `text` and writes the text and its new start (`to`).
fn fit_text(
    dc: &mut EditSegment,
    note: &CopiedNote,
    text: &str,
    to: (f64, f64),
) -> Result<TextFit> {
    let (node, _) = note.first_of(dc, T_TEXT)?;
    let n = &mut node_in_mut(dc, node)?.node;
    let origin = match floats_named(n, "loc")[..] {
        [x, y, ..] => (x, y),
        _ => return Err(missing("loc of the copied note text")),
    };
    let shift = (to.0 - origin.0, to.1 - origin.1);
    let told = Some(plain(n.text("name").unwrap_or_default())).filter(|t| !t.is_empty());
    let told = told.unwrap_or_else(|| " ".to_string());
    let ratio = SPARE_WIDTH * text.chars().count() as f64 / told.chars().count() as f64;
    replace_keeping_codes(n, &["name", "resolved_text"], text)?;
    set_float(n, "loc", 0, origin.0 + shift.0)?;
    set_float(n, "loc", 1, origin.1 + shift.1)?;
    Ok(TextFit {
        origin,
        shift,
        ratio,
    })
}

/// The template note's text (its 1FBB3C01 child's `name`).
fn template_note_text(tdc: &EditSegment, tnote: usize) -> Result<String> {
    let children = refs_named(&node_in(tdc, tnote)?.node, "children");
    let text = children
        .into_iter()
        .filter_map(|k| tdc.node(k))
        .find(|n| n.node.type_id.0 == T_TEXT);
    let text = text.ok_or_else(|| missing("text of the template note"))?;
    Ok(text.node.text("name").unwrap_or_default().to_string())
}

/// Text box corner point (CE52DF3E).
const T_BOX_CORNER: u32 = 0xCE52_DF3E;

/// The text box corners (CE52DF3E: corner x, y, then 24 bytes of f64 0 and two empty lists)
/// follow the text and its width.
fn widen_box(dc: &mut EditSegment, copies: &[usize], at: &Placement<'_>) -> Result<()> {
    for k in copies {
        if dc.node(*k).is_none_or(|n| n.node.type_id.0 != T_BOX_CORNER) {
            continue;
        }
        let mut payload = node_in(dc, *k)?.payload();
        let o = payload.len().saturating_sub(40);
        let corner = |at: usize| super::fields::f64_at(&payload, at).unwrap_or_default();
        let (cx, cy) = (corner(o), corner(o + 8));
        put(&mut payload, o, &(at.wx)(cx).to_le_bytes());
        put(&mut payload, o + 8, &(cy + at.dy).to_le_bytes());
        redecode(dc, *k, &payload)?;
    }
    Ok(())
}
