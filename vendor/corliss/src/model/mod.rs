//! Semantic models built on the decoded nodes: parts, assemblies and drawings as a CAD kernel
//! reads them (spec clauses 14–17).
//!
//! The readers follow the reference implementation's part, assembly and drawing readers field
//! for field; `tests/fixtures/models.json` holds those readers' output for every fixture
//! and the model tests compare against it. Lengths are in centimetres and angles in radians.

pub mod assembly;
pub mod drawing;
pub mod part;
pub mod view;

use std::collections::BTreeMap;

pub use assembly::Assembly;
pub use drawing::Drawing;
pub use part::Part;
pub use view::{Matrix4, SegmentView};

use crate::document::{DecodedSegment, Document};
use crate::error::Result;

/// A document with every segment's nodes decoded.
///
/// ```no_run
/// let doc = corliss::Document::open("part.ipt")?;
/// let model = corliss::model::Model::load(&doc)?;
/// let part = corliss::model::Part::new(&model)?;
/// for p in part.parameters() {
///     println!("{} = {:?}", p.name, p.value);
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub struct Model<'a> {
    doc: &'a Document,
    segments: BTreeMap<String, DecodedSegment>,
}

impl<'a> Model<'a> {
    /// Decodes every (non-template) segment of `doc`.
    pub fn load(doc: &'a Document) -> Result<Self> {
        let mut segments = BTreeMap::new();
        for loc in doc.segments()?.into_iter().filter(|l| !l.template) {
            // SolidCraft: a segment that will not decode (sheet-metal B-rep streams in newer
            // layouts, for example) is kept empty rather than failing the whole document; the
            // model readers then simply find no nodes in it.
            let decoded = match doc.decode_nodes(&loc) {
                Ok(d) => d,
                Err(_) => DecodedSegment {
                    name: loc.name.clone(),
                    segment_type: doc.segment_type(&loc).unwrap_or_default(),
                    nodes: Vec::new(),
                },
            };
            segments.insert(loc.name.clone(), decoded);
        }
        Ok(Model { doc, segments })
    }

    /// The document.
    pub fn document(&self) -> &'a Document {
        self.doc
    }

    /// The decoded segment named `name`.
    pub fn segment(&self, name: &str) -> Option<&DecodedSegment> {
        self.segments.get(name)
    }

    /// Names of the decoded segments.
    pub fn segment_names(&self) -> impl Iterator<Item = &str> {
        self.segments.keys().map(String::as_str)
    }

    /// The ACIS B-reps of segment `name` (e.g. "PmBRepSegment"), one per body node with a SAB stream.
    pub fn breps(&self, name: &str) -> Result<Vec<crate::acis::Brep>> {
        match self.segment(name) {
            Some(seg) => crate::acis::breps_in(seg),
            None => Ok(Vec::new()),
        }
    }

    /// A labelled view of segment `name`.
    pub fn view(&self, name: &str) -> Option<SegmentView<'_>> {
        self.segment(name).map(SegmentView::new)
    }
}
