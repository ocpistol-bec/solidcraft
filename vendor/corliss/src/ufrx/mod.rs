//! `UFRxDoc`, the document record: identity, representations, referenced files, occurrences
//! and states (spec clause 10).

mod body;
mod head;
mod parser;
mod record;
mod tail;

pub use record::{Field, Record, Value};

use crate::bytes::ByteWriter;
use crate::document::DocumentKind;
use crate::error::Result;
use crate::guid::Guid;
use parser::Parser;

/// Document classes as UFRxDoc stores them in `doc_sub_type_1`: the CLSID written as four u32
/// words, so the text form differs from the root CLSID (spec 8.4 note, 10.4).
const PART_CLASS: &str = "4d29b490-11d0-49b2-077e-c39300000006";
const ASSEMBLY_CLASS: &str = "e60f81e1-11d0-49b3-077e-c39300000006";

/// A section reader of the component body.
type SectionReader<'a> = fn(&mut Parser<'a>) -> Result<Record>;

/// A referenced Inventor file (spec 10.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReference {
    /// Full path as last resolved.
    pub path: String,
    /// Path relative to the project, when Inventor stored one.
    pub relative_path: String,
    /// Library name ("" when the file is not in a library).
    pub library_name: String,
    /// The referenced document's `internal_name` (its UFRxDoc header).
    pub internal_name: Guid,
    /// The referenced document's `db_revision` at its last save.
    pub db_revision: Guid,
    /// File reference id; occurrences point at it.
    pub ref_id: u32,
    /// Occurrence records that use this file.
    pub n_occurrences: u32,
}

/// One occurrence (UFRxInstance, spec 10.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// 0 Standard, 1 InstanceToFactory, 2 SubstituteInstance.
    pub instance_type: u32,
    /// `ref_id` of the referenced file.
    pub file_ref_id: u32,
    /// Occurrence id (the AmDc occurrence id).
    pub id: u32,
    /// 1-based ordinal.
    pub order: u32,
    /// Instance number, the `:n` suffix of the browser label.
    pub name_suffix: u32,
    /// Occurrence name.
    pub title: String,
}

/// The parsed UFRxDoc stream; encoding gives the stream back byte for byte.
///
/// ```no_run
/// let doc = corliss::Document::open("assembly.iam")?;
/// let ufrx = doc.ufrx_doc()?;
/// for file in ufrx.references() {
///     println!("{} ({} occurrences)", file.path, file.n_occurrences);
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct UfrxDoc {
    /// The whole stream as one record of nested records.
    pub root: Record,
}

impl UfrxDoc {
    /// Decodes the stream; `kind` stands in for the document class of pre-2012 saves, whose
    /// `header2` does not store it.
    pub fn decode(data: &[u8], kind: DocumentKind) -> Result<Self> {
        let mut p = Parser::new(data);
        let mut root = Record::new("UFRxDoc");
        let header1 = p.header1()?;
        root.push("header1", Value::Record(header1));
        let lod_toc = p.lod_toc()?;
        root.push("lod_toc", Value::Record(lod_toc));
        let header2 = p.header2()?;
        let class = header2.guid("doc_sub_type_1");
        root.push("header2", Value::Record(header2));
        let (is_assembly, is_3d) = classify(class, kind);
        p.is_assembly = is_assembly;
        parse_body(&mut p, &mut root, is_3d)?;
        p.r.finish()?;
        Ok(UfrxDoc { root })
    }

    /// Encodes the stream.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        self.root.encode(&mut w);
        w.into_bytes()
    }

    fn section(&self, name: &str) -> &[Record] {
        self.root
            .record(name)
            .map_or(&[], |r| r.list(list_of(name)))
    }

    /// This document's `internal_name`: what referencing documents store to identify it.
    pub fn internal_name(&self) -> Option<Guid> {
        self.root.record("header1")?.guid("internal_name")
    }

    /// This document's `db_revision` at its last save.
    pub fn db_revision(&self) -> Option<Guid> {
        self.root.record("header1")?.guid("db_revision")
    }

    /// The referenced Inventor files.
    pub fn references(&self) -> Vec<FileReference> {
        self.section("inv_files")
            .iter()
            .map(file_reference)
            .collect()
    }

    /// The occurrences of the component.
    pub fn occurrences(&self) -> Vec<Occurrence> {
        self.section("occurrences")
            .iter()
            .filter_map(occurrence)
            .collect()
    }

    /// Points every reference whose path equals `old` (ignoring case) at `new`; returns how many
    /// records changed. `relative`, when given, replaces the relative path too.
    pub fn set_reference_path(&mut self, old: &str, new: &str, relative: Option<&str>) -> usize {
        let mut changed = 0;
        for section in ["inv_files", "ole_files"] {
            for file in files_mut(&mut self.root, section) {
                if file
                    .text("path")
                    .is_some_and(|p| p.eq_ignore_ascii_case(old))
                {
                    set_text(file, "path", new);
                    if let Some(rel) = relative {
                        set_text(file, "relative_path", rel);
                    }
                    changed += 1;
                }
            }
        }
        changed
    }
}

/// (is assembly, is part or assembly) from the stored class, or from the container's kind.
fn classify(class: Option<Guid>, kind: DocumentKind) -> (bool, bool) {
    let matches = |text: &str| class.is_some_and(|c| Guid::parse(text) == Some(c));
    match class {
        Some(_) => (
            matches(ASSEMBLY_CLASS),
            matches(PART_CLASS) || matches(ASSEMBLY_CLASS),
        ),
        None => {
            let assembly = kind == DocumentKind::Assembly;
            (
                assembly,
                assembly || matches!(kind, DocumentKind::Part | DocumentKind::IFeature),
            )
        }
    }
}

fn parse_body<'a>(p: &mut Parser<'a>, root: &mut Record, is_3d: bool) -> Result<()> {
    let lods = p.lods()?;
    root.push("lods", Value::Record(lods));
    if p.v(1) > 0x14 {
        let marker = p.marker()?;
        root.push("marker", Value::Record(marker));
    }
    let sections: [(&'static str, SectionReader<'a>); 6] = [
        ("inv_files", Parser::inv_files),
        ("ole_files", Parser::ole_files),
        ("occurrences", Parser::occurrences),
        ("doc_props", Parser::doc_props),
        ("id_list", Parser::id_list),
        ("id_records", Parser::id_records),
    ];
    for (name, read) in sections {
        let rec = read(p)?;
        root.push(name, Value::Record(rec));
    }
    parse_component_tail(p, root)?;
    let states = p.states()?;
    root.push("exports", Value::Record(states));
    let range_box = p.range_box(is_3d)?;
    root.push("range_box", Value::Record(range_box));
    let tail = p.tail()?;
    root.push("tail", Value::Record(tail));
    Ok(())
}

fn parse_component_tail(p: &mut Parser<'_>, root: &mut Record) -> Result<()> {
    if p.release_year < 2012 {
        let bom = p.bom_2010()?;
        root.push("bom", Value::Record(bom));
        let misc = p.misc3_2010()?;
        root.push("misc3", Value::Record(misc));
        return Ok(());
    }
    if p.is_assembly {
        let post = p.post_office()?;
        root.push("post_office", Value::Record(post));
    }
    let tail = p.component_tail()?;
    root.push("component_tail", Value::Record(tail));
    Ok(())
}

/// The list field of each section that the typed views read.
fn list_of(section: &str) -> &'static str {
    match section {
        "occurrences" => "entries",
        _ => "files",
    }
}

fn file_reference(f: &Record) -> FileReference {
    let text = |name| f.text(name).unwrap_or_default().to_string();
    FileReference {
        path: text("path"),
        relative_path: text("relative_path"),
        library_name: text("library_name"),
        internal_name: f.guid("internal_name").unwrap_or_default(),
        db_revision: f.guid("db_revision").unwrap_or_default(),
        ref_id: f.uint("ref_id").unwrap_or_default() as u32,
        n_occurrences: f.uint("n_occurrences").unwrap_or_default() as u32,
    }
}

fn occurrence(e: &Record) -> Option<Occurrence> {
    let Some(Value::U32s(ids)) = e.get("occ_ids") else {
        return None;
    };
    let [instance_type, file_ref_id, id, order, name_suffix] = ids[..] else {
        return None;
    };
    Some(Occurrence {
        instance_type,
        file_ref_id,
        id,
        order,
        name_suffix,
        title: e.text("title").unwrap_or_default().to_string(),
    })
}

fn files_mut<'a>(root: &'a mut Record, section: &str) -> impl Iterator<Item = &'a mut Record> {
    let files = match root.get_mut(section) {
        Some(Value::Record(r)) => r.get_mut("files"),
        _ => None,
    };
    let list = match files {
        Some(Value::List(items)) => items.as_mut_slice(),
        _ => &mut [],
    };
    list.iter_mut()
}

fn set_text(rec: &mut Record, name: &str, text: &str) {
    if let Some(value) = rec.get_mut(name) {
        *value = Value::Text16(text.to_string());
    }
}
