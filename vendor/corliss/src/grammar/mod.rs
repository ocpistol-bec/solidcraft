//! Node payload decoding (spec clauses 13–18): every node type's grammar, run by a small
//! interpreter, gives typed fields.
//!
//! The grammars are those of the reference implementation. They are Python functions, so the
//! reference implementation's export tooling snapshots them (functions as syntax trees, their
//! closures and constants, the table of each reader) into `grammars.bin.zst`, and this module
//! runs that snapshot. The engine objects the grammars call (the reference grammar engine's
//! parser, its list walkers, `struct`) are implemented natively here, to the byte. Re-export the
//! snapshot when the grammars change; `tests/fixtures/grammar-digests.tsv` holds the reference
//! engine's fingerprint of every fixture segment, and the tests require Corliss to reproduce
//! each one.

mod ast;
mod astconv;
mod astconvert;
mod canonical;
mod decode;
mod interp;
mod native;
mod snapshot;
mod tree;
mod value;

use std::sync::OnceLock;

pub use canonical::{fingerprint, fnv1a, node_text};
pub use decode::DecodeContext;
pub use tree::{DecodeStatus, DecodedNode, Field, FieldValue, ListField, OtherValue, RefKind};

use crate::compression::{self, Compression};
use crate::error::{CorlissError, Result};
use crate::guid::TypeId;
use snapshot::Program;

/// The compressed grammar snapshot.
const SNAPSHOT: &[u8] = include_bytes!("grammars.bin.zst");

static PROGRAM: OnceLock<std::result::Result<Program, CorlissError>> = OnceLock::new();

fn program() -> Result<&'static Program> {
    let loaded = PROGRAM.get_or_init(|| {
        let data = compression::decompress(Compression::Zstd, SNAPSHOT, "grammar snapshot")?;
        Program::parse(&data)
    });
    loaded.as_ref().map_err(Clone::clone)
}

/// Decodes the payloads of one segment's nodes, given as (TypeID, payload) in block order.
///
/// `segment_type` is the RSeSegInfo type name ("PmDcSegmentType"), or "TemplateSegment" for a
/// template segment. Unknown node types come back as [`DecodeStatus::NoGrammar`].
///
/// ```no_run
/// use corliss::grammar::{decode_segment, DecodeContext};
/// let ctx = DecodeContext { file_version: 2027, seg_major: 31, doctype: 3, rse_ided: true };
/// let nodes = decode_segment("PmDcSegmentType", ctx, &[])?;
/// assert!(nodes.is_empty());
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn decode_segment(
    segment_type: &str,
    ctx: DecodeContext,
    nodes: &[(TypeId, &[u8])],
) -> Result<Vec<DecodedNode>> {
    Ok(decode::decode_nodes(program()?, segment_type, ctx, nodes))
}

/// The reader (grammar table) of a segment type, e.g. "DCReader" for "PmDcSegmentType".
pub fn reader_of(segment_type: &str) -> Result<Option<String>> {
    Ok(program()?.segtypes.get(segment_type).cloned())
}

/// The document type code the grammar tables are keyed by, for a document kind (1 assembly,
/// 2 drawing, 3 part and iFeature, 4 presentation).
pub fn doctype_code(kind: crate::DocumentKind) -> i64 {
    match kind {
        crate::DocumentKind::Assembly => 1,
        crate::DocumentKind::Drawing => 2,
        crate::DocumentKind::Part | crate::DocumentKind::IFeature => 3,
        crate::DocumentKind::Presentation => 4,
    }
}
