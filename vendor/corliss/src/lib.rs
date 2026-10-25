//! Corliss reads and writes Autodesk Inventor documents (.ipt, .iam, .idw, .ipn, .ide) and
//! project files (.ipj).
//!
//! It implements the *Inventor File Format Specification*. Each module follows one layer of
//! that specification:
//!
//! | Module | Specification |
//! | --- | --- |
//! | [`container`] | clause 7, compound-file container |
//! | [`propset`] | clause 8, property sets (iProperties) |
//! | [`database`] | clause 9, RSeDb, RSeSegInfo, RSeDbRevisionInfo |
//! | [`ufrx`] | clause 10, UFRxDoc: referenced files and occurrences |
//! | [`segment`] | clauses 11–12, M and B streams, blocks |
//! | [`node`] | clause 13, node framing, NodeRefs, masked matrices |
//! | [`grammar`] | clauses 13–18, typed fields of every node type |
//! | [`ipj`] | clause 19, project files (.ipj) |
//! | [`edit`] | clause 20, structural edits (W3) |
//!
//! Reading conforms to reader classes R1 and R2 for the database and segment layers; writing
//! conforms to writer classes W1 (property sets) and W2 (in-place payload edits). Every byte the
//! caller does not change is written back unchanged [W-01].
//!
//! ```no_run
//! let doc = corliss::Document::open("part.ipt")?;
//! assert_eq!(doc.kind(), Some(corliss::DocumentKind::Part));
//! for seg in doc.segments()? {
//!     let decoded = doc.decode_segment(&seg)?;
//!     println!("{} has {} blocks", seg.name, decoded.blocks().len());
//! }
//! # Ok::<(), corliss::CorlissError>(())
//! ```

pub mod acis;
pub mod bytes;
pub mod compression;
pub mod container;
pub mod database;
pub mod document;
pub mod edit;
pub mod error;
pub mod grammar;
pub mod guid;
pub mod ipj;
pub mod model;
pub mod naming;
pub mod node;
pub mod propset;
pub mod segment;
pub mod ufrx;

pub use container::Container;
pub use document::{DecodedSegment, Document, DocumentKind, SegmentLocation};
pub use error::{CorlissError, FailureClass, Result};
pub use guid::{Guid, TypeId};
