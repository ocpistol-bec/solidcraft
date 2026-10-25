//! Node framing: common header, NodeRefs and masked matrices (spec clause 13, 4.5.1).

mod matrix;

pub use matrix::MaskedMatrix4;

use crate::bytes::ByteReader;
use crate::error::Result;

/// A segment-local reference to a block (spec 13.3).
///
/// Bits 0–30 are the 1-based index of a present block in walk order; 0 is null. Bit 31 is
/// the flag of persistence-graph references.
///
/// ```
/// let r = corliss::node::NodeRef(0x8000_0003);
/// assert!(r.is_flagged());
/// assert_eq!(r.index(), Some(3));
/// assert_eq!(corliss::node::NodeRef(0).index(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeRef(pub u32);

impl NodeRef {
    /// The 1-based block index, or `None` for the null reference.
    pub fn index(self) -> Option<usize> {
        let index = (self.0 & 0x7FFF_FFFF) as usize;
        (index != 0).then_some(index)
    }

    /// True when bit 31 is set.
    pub fn is_flagged(self) -> bool {
        self.0 & 0x8000_0000 != 0
    }
}

/// The 6-byte common header of nodes outside template segments (spec 13.1).
///
/// ```
/// let payload = [0, 0, 0, 0, 0x13, 0, 0xAA];
/// let header = corliss::node::CommonHeader::read(&payload).unwrap();
/// assert_eq!(header.object_id(), 0x13);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommonHeader {
    /// High part of the object id in drawing DC segments.
    pub high: u32,
    /// Segment-unique object id (low 16 bits).
    pub low: u16,
}

impl CommonHeader {
    /// Byte length of the header.
    pub const LEN: usize = 6;

    /// Reads the header at the start of a node payload; never call it for template nodes [R-09].
    pub fn read(payload: &[u8]) -> Result<Self> {
        let mut r = ByteReader::new(payload, "node common header");
        Ok(CommonHeader {
            high: r.u32("hdr.u32_0")?,
            low: r.u16("hdr.u16_0")?,
        })
    }

    /// The object id `(high << 16) | low` (spec 11.4).
    pub fn object_id(&self) -> u64 {
        (u64::from(self.high) << 16) | u64::from(self.low)
    }
}
