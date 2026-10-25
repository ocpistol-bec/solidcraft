//! ACIS / ShapeManager bodies (SAB streams) of F645595C nodes: the solid geometry
//! (spec 13.6).
//!
//! [`SabStream::parse`] splits a stream into records. [`Brep::read`] walks the live topology
//! of each body: faces with their surfaces, loops of coedges, edges with their curves, and
//! vertices. Analytic geometry (planes, cones and cylinders, spheres, tori, lines, circles) is
//! decoded exactly. Procedural geometry comes with its approximating B-splines. The history
//! section is skipped [R-07].
//!
//! ```no_run
//! let doc = corliss::Document::open("part.ipt")?;
//! let seg = doc.decode_nodes(&doc.segment_location("PmBRepSegment")?)?;
//! for brep in corliss::acis::breps_in(&seg)? {
//!     println!("{} bodies, {} edges", brep.bodies.len(), brep.edges.len());
//! }
//! # Ok::<(), corliss::CorlissError>(())
//! ```

mod brep;
mod cursor;
mod decode;
pub mod eval;
mod geometry;
mod stream;
mod subtypes;
mod token;
mod topology;

pub use brep::*;
pub use cursor::Interval;
pub use geometry::*;
pub use stream::{SabHeader, SabRecord, SabStream};
pub use token::Token;

use crate::document::DecodedSegment;
use crate::error::Result;

/// TypeID of the node that holds a SAB stream.
pub const T_ACIS_BODY: u32 = 0xF645_595C;

/// The B-reps of every F645595C node of a segment that carries a SAB stream, in node order.
pub fn breps_in(seg: &DecodedSegment) -> Result<Vec<Brep>> {
    seg.nodes
        .iter()
        .filter(|n| n.type_id.0 == T_ACIS_BODY)
        .filter_map(|n| n.field("sab").map(|f| f.raw.as_slice()))
        .map(|sab| Brep::read(&SabStream::parse(sab)?))
        .collect()
}
