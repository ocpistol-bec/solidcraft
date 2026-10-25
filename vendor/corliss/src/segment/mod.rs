//! Segments: the M/B stream pairs of `RSeStorage` (spec clauses 11–12).

mod bstream;
mod mstream;
mod mtail;

pub use bstream::{B_STREAM_GUID, BStream, Block, EndTable, SegmentData};
pub use mstream::{BlockSlot, MBody, MHeader, MStream, SegmentIdentity, TypeEntry};
pub use mtail::{CLOSING_GUID, MTail, Ownership, SegmentLink, TemplateRef};

use crate::error::{CorlissError, Result};
use crate::guid::TypeId;
use crate::node::NodeRef;

/// A decoded segment: its metadata and its blocks.
///
/// ```no_run
/// let doc = corliss::Document::open("part.ipt")?;
/// let loc = doc.segment_location("PmDCSegment")?;
/// let seg = doc.decode_segment(&loc)?;
/// for block in seg.blocks() {
///     println!("{:?} {} bytes", seg.type_id(block), block.payload.len());
/// }
/// # Ok::<(), corliss::CorlissError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The M stream.
    pub meta: MStream,
    /// The B stream.
    pub data: BStream,
}

impl Segment {
    /// Decodes an M/B pair; `template` for `RSeStorage/Templates` pairs, `trailers` from RSeDb.
    pub fn decode(m: &[u8], b: &[u8], template: bool, trailers: bool) -> Result<Self> {
        let meta = MStream::decode(m, template)?;
        let data = BStream::decode(b, &meta.body, trailers)?;
        if data.compression != meta.header.compression {
            let found = format!("B method {:?}", data.compression);
            let expected = format!("M compression {:?} [F-06]", meta.header.compression);
            return Err(CorlissError::invalid(
                "B stream", "method", 16, found, expected,
            ));
        }
        Ok(Segment { meta, data })
    }

    /// Encodes the pair as (M bytes, B bytes), first rewriting the block-table lengths.
    pub fn encode(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut meta = self.meta.clone();
        self.data.data.sync_block_table(&mut meta.body.blocks);
        Ok((meta.encode()?, self.data.encode()?))
    }

    /// The segment name, or `None` for a template segment.
    pub fn name(&self) -> Option<&str> {
        self.meta
            .header
            .identity
            .as_ref()
            .map(|id| id.name.as_str())
    }

    /// Present blocks in walk order; NodeRef index k names `blocks()[k - 1]` [R-05].
    pub fn blocks(&self) -> &[Block] {
        &self.data.data.blocks
    }

    /// Mutable blocks, for in-place payload edits.
    pub fn blocks_mut(&mut self) -> &mut [Block] {
        &mut self.data.data.blocks
    }

    /// TypeID of a block of this segment.
    pub fn type_id(&self, block: &Block) -> Option<TypeId> {
        block.type_id(&self.meta.body)
    }

    /// The block a NodeRef names, or `None` for the null ref or an index past the blocks.
    pub fn resolve(&self, node: NodeRef) -> Option<&Block> {
        let index = node.index()?;
        self.blocks().get(index - 1)
    }
}
