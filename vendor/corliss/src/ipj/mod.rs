//! Inventor project files (.ipj, spec clause 19): the UTF-16 XML files that name a design's
//! search paths, folders and options. They are not compound files; clauses 7–18 do not apply.
//!
//! [`ProjectFile`] keeps every byte of the file, so an unchanged file writes back as it was
//! read, in whichever layout a writer produced (19.1, 19.2); edits change only the character
//! data or attribute they touch. [`ProjectFile::project`] reads the typed [`Project`],
//! [`Project::to_ipj`] writes a new file in Inventor's layout or the Default.ipj one, and
//! [`resolve_path`] turns a relative path into the folder Inventor uses [R-15].
//!
//! ```no_run
//! let file = corliss::ipj::ProjectFile::open("Design.ipj")?;
//! let project = file.project()?;
//! if let Some(ws) = project.workspace() {
//!     println!("workspace: {}", corliss::ipj::resolve_path(r"C:\Work\Design", ws));
//! }
//! # Ok::<(), corliss::CorlissError>(())
//! ```

mod check;
mod parse;
mod path;
mod project;
mod render;
mod schema;
mod xml;

pub use path::resolve_path;
pub use project::{
    AssetLibrary, FolderOverride, PathKind, Project, ProjectOptions, ProjectPath, ProjectType,
    VaultOptions,
};
pub use render::Layout;
pub use xml::{Attribute, Content, Element};

use std::path::Path;

use crate::error::{CorlissError, Result};
use parse::parse_xml;
use render::BOM;

const STRUCTURE: &str = "project file";
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// The text encoding of a project file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// UTF-16LE, the encoding Inventor writes [F-10].
    Utf16Le,
    /// UTF-8, accepted for files written by other tools.
    Utf8,
}

/// A project file as read: byte order mark, text encoding, the text before and after the
/// root element, and the root element as a lossless tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    /// The byte order mark as read (empty when there was none).
    pub bom: Vec<u8>,
    /// Text encoding.
    pub encoding: TextEncoding,
    /// Everything before the root start tag: the declaration and whitespace.
    pub prolog: String,
    /// The `InventorProject` element.
    pub root: Element,
    /// Everything after the root end tag.
    pub epilog: String,
}

impl ProjectFile {
    /// Reads the project file at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<ProjectFile> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| {
            CorlissError::missing("open", STRUCTURE, format!("{}: {e}", path.display()))
        })?;
        ProjectFile::parse(&bytes)
    }

    /// Reads a project file from its bytes. UTF-16LE (with or without a BOM) and UTF-8 are
    /// accepted; UTF-16BE is not, as Inventor never writes it.
    ///
    /// ```
    /// let text = "<?xml version=\"1.0\" encoding=\"utf-16\"?>\n<InventorProject schemarevid=\"14\"/>";
    /// let bytes: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    /// let file = corliss::ipj::ProjectFile::parse(&bytes).unwrap();
    /// assert_eq!(file.to_bytes(), bytes);
    /// assert_eq!(file.project().unwrap().schema_revision, Some(14));
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<ProjectFile> {
        let (bom, encoding, text) = decode_text(bytes)?;
        let parsed = parse_xml(&text)?;
        Ok(ProjectFile {
            bom,
            encoding,
            prolog: parsed.prolog,
            root: parsed.root,
            epilog: parsed.epilog,
        })
    }

    /// The file's bytes: identical to the bytes read when nothing was changed.
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = format!("{}{}{}", self.prolog, self.root.to_xml(), self.epilog);
        let body: Vec<u8> = match self.encoding {
            TextEncoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            TextEncoding::Utf8 => text.into_bytes(),
        };
        [self.bom.clone(), body].concat()
    }

    /// Writes the file to `path`.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        std::fs::write(path, self.to_bytes()).map_err(|e| {
            CorlissError::missing("save", STRUCTURE, format!("{}: {e}", path.display()))
        })
    }

    /// The typed view. Fails only on a malformed `contentcenterguid`.
    pub fn project(&self) -> Result<Project> {
        Project::from_root(&self.root)
    }

    /// The layout the file was written in: Inventor's writes `standalone` in the declaration.
    pub fn layout(&self) -> Layout {
        if self.prolog.contains("standalone") {
            Layout::Inventor
        } else {
            Layout::Default
        }
    }

    /// Departures from clause 19, worded as the reference checker words them;
    /// empty for a conforming file.
    pub fn check(&self) -> Vec<String> {
        check::check(self)
    }
}

impl Project {
    /// The project as a complete .ipj file in `layout` [W-16]; [`Layout::Inventor`] is the
    /// layout writers should use [W-19].
    ///
    /// ```
    /// use corliss::ipj::{Layout, Project, ProjectFile};
    /// let bytes = Project::default().to_ipj(Layout::Inventor);
    /// let file = ProjectFile::parse(&bytes).unwrap();
    /// assert!(file.check().is_empty());
    /// assert_eq!(file.project().unwrap().to_ipj(Layout::Inventor), bytes);
    /// ```
    pub fn to_ipj(&self, layout: Layout) -> Vec<u8> {
        render::render(self, layout)
    }
}

/// (BOM, encoding, text) of a project file's bytes.
fn decode_text(bytes: &[u8]) -> Result<(Vec<u8>, TextEncoding, String)> {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(CorlissError::invalid(
            STRUCTURE,
            "bom",
            0,
            "FE FF (UTF-16BE)",
            "FF FE (UTF-16LE) [F-10]",
        ));
    }
    if let Some(body) = bytes.strip_prefix(&BOM) {
        return Ok((BOM.to_vec(), TextEncoding::Utf16Le, utf16le(body)?));
    }
    if bytes.starts_with(b"<\0") {
        return Ok((Vec::new(), TextEncoding::Utf16Le, utf16le(bytes)?));
    }
    let (bom, body) = match bytes.strip_prefix(&UTF8_BOM) {
        Some(body) => (UTF8_BOM.to_vec(), body),
        None => (Vec::new(), bytes),
    };
    let text = String::from_utf8(body.to_vec()).map_err(|e| {
        CorlissError::invalid(
            STRUCTURE,
            "text",
            e.utf8_error().valid_up_to(),
            "invalid UTF-8",
            "UTF-16LE or UTF-8 text",
        )
    })?;
    Ok((bom, TextEncoding::Utf8, text))
}

fn utf16le(body: &[u8]) -> Result<String> {
    if !body.len().is_multiple_of(2) {
        return Err(CorlissError::invalid(
            STRUCTURE,
            "text",
            body.len(),
            format!("{} bytes", body.len()),
            "an even number of UTF-16LE bytes",
        ));
    }
    let units: Vec<u16> = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16(&units).map_err(|_| {
        CorlissError::invalid(
            STRUCTURE,
            "text",
            0,
            "an unpaired surrogate",
            "well-formed UTF-16LE",
        )
    })
}
