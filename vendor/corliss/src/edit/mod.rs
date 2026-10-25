//! Structural editing (writer class W3, spec clause 20): add, remove and reorder nodes, change
//! fields, and rebuild every derived field of the touched segments.
//!
//! An [`Editor`] opens segments as typed nodes ([`EditSegment`]), lets callers change them, and
//! [`Editor::finish`] rebuilds the M and B streams and the RSeSegInfo entry of each touched
//! segment the way the reference writer does. Untouched streams are kept byte for byte
//! [W-01].
//!
//! ```no_run
//! use corliss::grammar::FieldValue;
//! let doc = corliss::Document::open("part.ipt")?;
//! let mut ed = corliss::edit::Editor::new(&doc)?;
//! let seg = ed.segment("PmDCSegment")?;
//! let param = seg.of_type(0x9087_4D26)[0];
//! if let Some(n) = seg.node_mut(param) {
//!     n.node.set("valueModel", FieldValue::F64(2.5))?;
//! }
//! ed.finish()?.save("edited.ipt")?;
//! # Ok::<(), corliss::CorlissError>(())
//! ```

pub mod assembly;
mod commit;
pub mod drawing;
mod field;
pub mod idset;
mod part;
mod segment;
mod subgraph;

pub use field::node_ref;
pub use part::add_user_parameter;
pub use segment::{EditNode, EditSegment};

use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use crate::database::SegInfo;
use crate::document::Document;
use crate::error::{CorlissError, Result};
use crate::guid::Guid;

const SEG_INFO: &str = "RSeStorage/RSeSegInfo";

/// Where new GUIDs (revisions, state ids) come from.
pub enum GuidSource {
    /// Random version-4 GUIDs.
    Random,
    /// The given GUIDs in order, then random ones (reproducible edits and tests).
    Fixed(Vec<Guid>),
}

/// An editing session over one document.
pub struct Editor {
    doc: Document,
    seg_info: SegInfo,
    segments: BTreeMap<String, EditSegment>,
    trailers: bool,
    guids: GuidSource,
    revision: Option<Guid>,
}

impl Editor {
    /// Opens `doc` for editing; the document itself is not changed.
    pub fn new(doc: &Document) -> Result<Self> {
        let trailers = doc.rsedb()?.has_block_trailers();
        Ok(Editor {
            doc: doc.clone(),
            seg_info: doc.seg_info()?,
            segments: BTreeMap::new(),
            trailers,
            guids: GuidSource::Random,
            revision: None,
        })
    }

    /// Sets where new GUIDs come from.
    pub fn with_guids(mut self, guids: GuidSource) -> Self {
        self.guids = guids;
        self
    }

    /// A new GUID from the session's source.
    pub fn new_guid(&mut self) -> Guid {
        if let GuidSource::Fixed(list) = &mut self.guids
            && !list.is_empty()
        {
            return list.remove(0);
        }
        random_guid()
    }

    /// The edit session's revision GUID, drawn on first use: one per session, as Inventor
    /// writes one per save. The reference writer stores the drawn uuid's
    /// big-endian bytes (`uuid4().bytes`), so the first three groups are swapped back.
    pub fn session_revision(&mut self) -> Guid {
        if let Some(g) = self.revision {
            return g;
        }
        let mut bytes = *self.new_guid().as_bytes();
        bytes[0..4].reverse();
        bytes[4..6].reverse();
        bytes[6..8].reverse();
        let g = Guid::from_bytes(bytes);
        self.revision = Some(g);
        g
    }

    /// The segment named `name`, opened for editing on first use.
    pub fn segment(&mut self, name: &str) -> Result<&mut EditSegment> {
        if !self.segments.contains_key(name) {
            let opened = self.open(name)?;
            self.segments.insert(name.to_string(), opened);
        }
        self.segments
            .get_mut(name)
            .ok_or_else(|| CorlissError::missing("edit", "editor", name.to_string()))
    }

    fn open(&self, name: &str) -> Result<EditSegment> {
        let loc = self.doc.segment_location(name)?;
        let segment = self.doc.decode_segment(&loc)?;
        let decoded = self.doc.decode_nodes(&loc)?;
        let ctx = self.doc.decode_context(false)?;
        Ok(EditSegment::new(
            loc,
            decoded.segment_type,
            segment,
            decoded.nodes,
            ctx,
        ))
    }

    /// Names of the segments opened so far.
    pub fn opened(&self) -> impl Iterator<Item = &str> {
        self.segments.keys().map(String::as_str)
    }

    /// The segment directory, written back by [`Editor::finish`].
    pub fn seg_info_mut(&mut self) -> &mut SegInfo {
        &mut self.seg_info
    }

    /// The segment directory.
    pub fn seg_info(&self) -> &SegInfo {
        &self.seg_info
    }

    /// The document, for stream-level edits (property sets, UFRxDoc) during the session.
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// The document as it stands (segments not yet rebuilt).
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Rebuilds every opened segment and the segment directory; returns the edited document.
    pub fn finish(mut self) -> Result<Document> {
        let names: Vec<String> = self.segments.keys().cloned().collect();
        for name in &names {
            let Some(seg) = self.segments.get_mut(name) else {
                continue;
            };
            let seg_id = seg.segment.meta.header.identity.as_ref().map(|i| i.seg_id);
            let entry = self
                .seg_info
                .segments
                .iter_mut()
                .find(|e| Some(e.seg_id) == seg_id);
            let rebuilt = commit::commit(seg, entry, self.trailers)?;
            self.doc
                .replace_raw_stream(&seg.location.m_path(), rebuilt.m)?;
            self.doc
                .replace_raw_stream(&seg.location.b_path(), rebuilt.b)?;
        }
        if !names.is_empty() {
            self.doc
                .replace_raw_stream(SEG_INFO, self.seg_info.encode())?;
        }
        Ok(self.doc)
    }
}

/// A random version-4 GUID from the standard library's randomly keyed hasher.
fn random_guid() -> Guid {
    let state = RandomState::new();
    let mut bytes = [0u8; 16];
    for (k, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut h = state.build_hasher();
        h.write_usize(k);
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[7] = (bytes[7] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    Guid::from_bytes(bytes)
}
