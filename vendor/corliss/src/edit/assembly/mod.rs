//! Assembly document edits: constraint values, grounding, placement, adding
//! and removing occurrences, removing and copying constraints. Every operation keeps the
//! derived data Inventor checks on load in step: AmRx / AmGraphics / browser records, the
//! cross-segment M `t6` / `t7` / `t9` tables and their RSeSegInfo mirrors, the UFRxDoc
//! occurrence records and the document revision.

mod add_constraint;
mod add_constraint_dc;
mod add_constraint_links;
mod add_constraint_names;
mod add_occurrence;
mod add_occurrence_graphics;
mod add_occurrence_rx;
mod constraint_value;
mod dependents;
mod fields;
mod lookup;
mod occurrence;
mod order;
mod remove_constraint;
mod remove_occurrence;
mod remove_occurrence_parts;
mod root_classes;
mod tables;
mod transform;
mod ufrx_records;

pub use add_constraint::add_constraint_from;
pub use add_occurrence::add_occurrence;
pub use constraint_value::set_constraint_value;
pub use occurrence::{set_grounded, set_occurrence_transform};
pub use remove_constraint::remove_constraint;
pub use remove_occurrence::remove_occurrence;
pub use transform::{IDENTITY, Placement};
