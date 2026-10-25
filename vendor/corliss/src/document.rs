//! A whole Inventor document: container plus typed access to every layer (spec 6.3).

use std::path::Path;

use crate::container::Container;
use crate::database::{RevisionInfo, RseDb, SegInfo};
use crate::error::{CorlissError, Result};
use crate::grammar::{self, DecodeContext, DecodedNode};
use crate::guid::Guid;
use crate::guid::TypeId;
use crate::naming;
use crate::propset::PropertySetStream;
use crate::segment::{MHeader, Segment};
use crate::ufrx::UfrxDoc;

const DATABASE: &str = "RSeStorage";
const TEMPLATES: &str = "RSeStorage/Templates";
const SEG_INFO: &str = "RSeStorage/RSeSegInfo";
const REVISION_INFO: &str = "RSeStorage/RSeDbRevisionInfo";
const UFRX_DOC: &str = "UFRxDoc";
/// Grammar table name of template segments, which have no RSeSegInfo entry (spec 11.6).
const TEMPLATE_SEGMENT_TYPE: &str = "TemplateSegment";
/// Length of `GetName(seg_id)` (spec 7.2).
const STREAM_ID_LEN: usize = 26;

/// The document type, from the root CLSID (spec Table 7-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentKind {
    /// .ipt
    Part,
    /// .iam
    Assembly,
    /// .idw
    Drawing,
    /// .ipn
    Presentation,
    /// .ide
    IFeature,
}

impl DocumentKind {
    /// Every kind with its root CLSID.
    pub const ALL: [(DocumentKind, &'static str); 5] = [
        (DocumentKind::Part, "4d29b490-49b2-11d0-93c3-7e0706000000"),
        (
            DocumentKind::Assembly,
            "e60f81e1-49b3-11d0-93c3-7e0706000000",
        ),
        (
            DocumentKind::Drawing,
            "bbf9fdf1-52dc-11d0-8c04-0800090be8ec",
        ),
        (
            DocumentKind::Presentation,
            "76283a80-50dd-11d3-a7e3-00c04f79d7bc",
        ),
        (
            DocumentKind::IFeature,
            "62fbb030-24c7-11d3-b78d-0060b0f159ef",
        ),
    ];

    /// The kind whose root CLSID is `clsid`.
    ///
    /// ```
    /// let clsid = corliss::Guid::parse("4d29b490-49b2-11d0-93c3-7e0706000000").unwrap();
    /// assert_eq!(corliss::DocumentKind::from_clsid(&clsid), Some(corliss::DocumentKind::Part));
    /// ```
    pub fn from_clsid(clsid: &Guid) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, text)| Guid::parse(text).as_ref() == Some(clsid))
            .map(|(kind, _)| *kind)
    }

    /// The usual file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            DocumentKind::Part => "ipt",
            DocumentKind::Assembly => "iam",
            DocumentKind::Drawing => "idw",
            DocumentKind::Presentation => "ipn",
            DocumentKind::IFeature => "ide",
        }
    }
}

/// Where a segment's M/B pair lives and what the M header says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentLocation {
    /// Segment name from the M stream [R-01]; empty for template segments.
    pub name: String,
    /// The 26-character stream id shared by `M<id>` and `B<id>`.
    pub stream_id: String,
    /// True for a pair in `RSeStorage/Templates` (spec 11.6).
    pub template: bool,
}

impl SegmentLocation {
    fn storage(&self) -> &'static str {
        if self.template { TEMPLATES } else { DATABASE }
    }

    /// Container path of the M stream.
    pub fn m_path(&self) -> String {
        format!("{}/M{}", self.storage(), self.stream_id)
    }

    /// Container path of the B stream.
    pub fn b_path(&self) -> String {
        format!("{}/B{}", self.storage(), self.stream_id)
    }
}

/// A segment's nodes decoded into typed fields.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSegment {
    /// Segment name ("" for a template segment).
    pub name: String,
    /// Segment type, which selects the grammar table.
    pub segment_type: String,
    /// Nodes in block order; NodeRef index k names `nodes[k - 1]`.
    pub nodes: Vec<DecodedNode>,
}

impl DecodedSegment {
    /// The node a NodeRef names.
    pub fn resolve(&self, r: crate::node::NodeRef) -> Option<&DecodedNode> {
        self.nodes.get(r.index()? - 1)
    }

    /// Nodes of one TypeID.
    pub fn of_type(&self, type_id: TypeId) -> impl Iterator<Item = &DecodedNode> {
        self.nodes.iter().filter(move |n| n.type_id == type_id)
    }
}

/// An Inventor document held in memory.
///
/// Decoding is on demand: each accessor parses its layer from the container, so a document
/// that is only inspected is never rewritten. Editing methods replace streams in the
/// container; [`Document::to_bytes`] writes every other stream back unchanged [W-01].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    container: Container,
}

impl Document {
    /// Reads a document file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(path.as_ref())
            .map_err(|e| CorlissError::io("read", "document file", &e))?;
        Self::from_bytes(&bytes)
    }

    /// Reads a document from memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(Document {
            container: Container::from_bytes(bytes)?,
        })
    }

    /// Writes the document as a compound file.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.container.to_bytes()
    }

    /// Writes the document to `path`.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = self.to_bytes()?;
        std::fs::write(path.as_ref(), bytes)
            .map_err(|e| CorlissError::io("write", "document file", &e))
    }

    /// The container with every stream.
    pub fn container(&self) -> &Container {
        &self.container
    }

    /// Replaces the bytes of an existing stream as they are (no re-encoding).
    pub fn replace_raw_stream(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.container.replace_stream(path, bytes)
    }

    /// The document type, or `None` for an unknown root CLSID.
    pub fn kind(&self) -> Option<DocumentKind> {
        DocumentKind::from_clsid(&self.container.root_clsid())
    }

    /// The bytes of a stream, or a `Missing` error naming it.
    pub fn stream(&self, path: &str) -> Result<&[u8]> {
        self.container
            .stream(path)
            .ok_or_else(|| CorlissError::missing("read", "document", format!("stream {path}")))
    }

    /// Path of `RSeStorage/V<n>/RSeDb` (the `<n>` varies, spec 7.2).
    pub fn rsedb_path(&self) -> Result<String> {
        let found = self.container.entries().keys().find(|k| {
            let mut parts = k.split('/');
            parts.next() == Some(DATABASE)
                && parts.next().is_some_and(|v| v.starts_with('V'))
                && parts.next() == Some("RSeDb")
                && parts.next().is_none()
        });
        found
            .cloned()
            .ok_or_else(|| CorlissError::missing("read", "document", "RSeStorage/V<n>/RSeDb"))
    }

    /// The database header (spec 9.1).
    pub fn rsedb(&self) -> Result<RseDb> {
        RseDb::decode(self.stream(&self.rsedb_path()?)?)
    }

    /// The segment directory (spec 9.2).
    pub fn seg_info(&self) -> Result<SegInfo> {
        SegInfo::decode(self.stream(SEG_INFO)?, self.rsedb()?.schema)
    }

    /// The revision history (spec 9.3).
    pub fn revision_info(&self) -> Result<RevisionInfo> {
        RevisionInfo::decode(self.stream(REVISION_INFO)?)
    }

    /// The document record (spec clause 10).
    pub fn ufrx_doc(&self) -> Result<UfrxDoc> {
        let kind = self.kind().unwrap_or(DocumentKind::Part);
        UfrxDoc::decode(self.stream(UFRX_DOC)?, kind)
    }

    /// Replaces the document record, e.g. after relinking a referenced file.
    pub fn write_ufrx_doc(&mut self, record: &UfrxDoc) -> Result<()> {
        self.container.replace_stream(UFRX_DOC, record.encode())
    }

    /// Paths and FMTIDs of the property-set streams at the root (spec 8.2).
    pub fn property_set_paths(&self) -> Vec<(String, Guid)> {
        self.container
            .streams_in("")
            .filter_map(|path| naming::property_set_fmtid(path).map(|g| (path.to_string(), g)))
            .collect()
    }

    /// The property-set stream with `fmtid`, if the document has it.
    pub fn property_set(&self, fmtid: &Guid) -> Result<Option<PropertySetStream>> {
        let Some((path, _)) = self
            .property_set_paths()
            .into_iter()
            .find(|(_, g)| g == fmtid)
        else {
            return Ok(None);
        };
        Ok(Some(PropertySetStream::decode(self.stream(&path)?)?))
    }

    /// Replaces the property-set stream whose first set has the same FMTID (writer class W1).
    pub fn write_property_set(&mut self, stream: &PropertySetStream) -> Result<()> {
        let fmtid = stream.sets.first().map(|s| s.fmtid).unwrap_or_default();
        let path = self
            .property_set_paths()
            .into_iter()
            .find(|(_, g)| *g == fmtid)
            .map(|(p, _)| p)
            .ok_or_else(|| {
                CorlissError::missing("write", "document", format!("property set {fmtid}"))
            })?;
        self.container.replace_stream(&path, stream.encode())
    }

    /// Every segment and template segment, identified by its M stream header [R-01].
    pub fn segments(&self) -> Result<Vec<SegmentLocation>> {
        let mut out = self.locations_in(DATABASE, false)?;
        out.extend(self.locations_in(TEMPLATES, true)?);
        Ok(out)
    }

    fn locations_in(&self, storage: &str, template: bool) -> Result<Vec<SegmentLocation>> {
        let ids: Vec<String> = self
            .container
            .streams_in(storage)
            .filter_map(|path| {
                path.rsplit('/')
                    .next()?
                    .strip_prefix('M')
                    .map(str::to_string)
            })
            .filter(|id| id.len() == STREAM_ID_LEN)
            .collect();
        ids.into_iter()
            .map(|stream_id| {
                let mut loc = SegmentLocation {
                    name: String::new(),
                    stream_id,
                    template,
                };
                let header = MHeader::decode(self.stream(&loc.m_path())?, template)?;
                loc.name = header.identity.map(|id| id.name).unwrap_or_default();
                Ok(loc)
            })
            .collect()
    }

    /// The location of the segment named `name` (e.g. "PmDCSegment").
    pub fn segment_location(&self, name: &str) -> Result<SegmentLocation> {
        self.segments()?
            .into_iter()
            .find(|loc| !loc.template && loc.name == name)
            .ok_or_else(|| CorlissError::missing("read", "document", format!("segment {name}")))
    }

    /// Decodes a segment's M and B streams and walks its blocks.
    pub fn decode_segment(&self, loc: &SegmentLocation) -> Result<Segment> {
        let trailers = self.rsedb()?.has_block_trailers();
        Segment::decode(
            self.stream(&loc.m_path())?,
            self.stream(&loc.b_path())?,
            loc.template,
            trailers,
        )
    }

    /// The version context the node grammars need (spec 4.7).
    ///
    /// The release year follows the RSeDb created-with major, as the reference engine does.
    pub fn decode_context(&self, template: bool) -> Result<DecodeContext> {
        let major = i64::from(self.rsedb()?.created_with.major());
        let doctype = grammar::doctype_code(self.kind().unwrap_or(DocumentKind::Part));
        Ok(DecodeContext {
            file_version: 1996 + major,
            seg_major: major,
            doctype,
            rse_ided: !template,
        })
    }

    /// The segment type name ("PmDcSegmentType"), or "TemplateSegment" for a template segment.
    pub fn segment_type(&self, loc: &SegmentLocation) -> Result<String> {
        if loc.template {
            return Ok(TEMPLATE_SEGMENT_TYPE.to_string());
        }
        let info = self.seg_info()?;
        let entry = info.entry(&loc.name).ok_or_else(|| {
            CorlissError::missing("read", "RSeSegInfo", format!("entry {}", loc.name))
        })?;
        Ok(entry.segment_type.clone())
    }

    /// Decodes every node of a segment into typed fields (spec clauses 13–18).
    pub fn decode_nodes(&self, loc: &SegmentLocation) -> Result<DecodedSegment> {
        let segment = self.decode_segment(loc)?;
        let segment_type = self.segment_type(loc)?;
        let ctx = self.decode_context(loc.template)?;
        let nodes: Vec<(TypeId, &[u8])> = segment
            .blocks()
            .iter()
            .map(|b| {
                (
                    segment.type_id(b).unwrap_or(TypeId(0)),
                    b.payload.as_slice(),
                )
            })
            .collect();
        let nodes = grammar::decode_segment(&segment_type, ctx, &nodes)?;
        Ok(DecodedSegment {
            name: loc.name.clone(),
            segment_type,
            nodes,
        })
    }

    /// Writes an edited segment back to its M/B streams (writer class W2).
    pub fn write_segment(&mut self, loc: &SegmentLocation, segment: &Segment) -> Result<()> {
        let (m, b) = segment.encode()?;
        self.container.replace_stream(&loc.m_path(), m)?;
        self.container.replace_stream(&loc.b_path(), b)
    }
}
