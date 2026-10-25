//! Part documents (spec clause 15), after the reference part reader.

mod extras;
mod features;
mod parameters;
mod sketches;
mod work_geometry;

pub use extras::*;
pub use features::*;
pub use parameters::*;
pub use sketches::*;
pub use work_geometry::*;

use super::{Model, SegmentView};
use crate::error::{CorlissError, Result};

/// Parameter node type (spec 14.1.1).
pub(crate) const T_PARAMETER: u32 = 0x9087_4D26;

/// A parameter reference as feature and geometry records show it: name and model value.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamValue {
    /// Parameter name ("d0").
    pub name: Option<String>,
    /// Value in database units (cm, rad), `valueModel`.
    pub value: Option<f64>,
}

/// The parametric model segment of a part.
const DC_SEGMENT: &str = "PmDCSegment";

/// A part's model: work geometry, sketches, features, parameters and more.
pub struct Part<'m> {
    model: &'m Model<'m>,
    view: SegmentView<'m>,
}

impl<'m> Part<'m> {
    /// Reads the part model of a part (or iFeature) document.
    pub fn new(model: &'m Model<'m>) -> Result<Self> {
        Self::on_segment(model, DC_SEGMENT)
    }

    /// Reads the part-model nodes of another DC segment: assembly features live in
    /// AmDcSegment with the same node types as part features (spec 16.6).
    pub fn on_segment(model: &'m Model<'m>, segment: &str) -> Result<Self> {
        let view = model
            .view(segment)
            .ok_or_else(|| CorlissError::missing("read", "part model", segment))?;
        Ok(Part { model, view })
    }

    /// The decoded document.
    pub fn model(&self) -> &'m Model<'m> {
        self.model
    }

    /// The labelled PmDCSegment.
    pub fn view(&self) -> &SegmentView<'m> {
        &self.view
    }

    /// The browser label of node `index`.
    pub fn name(&self, index: usize) -> Option<String> {
        self.view.name_of(Some(index))
    }

    /// Name and value of the parameter node at `index`.
    pub fn param(&self, index: usize) -> Option<ParamValue> {
        let n = self.view.node(index)?;
        Some(ParamValue {
            name: n.text("name").map(str::to_string),
            value: n.float("valueModel"),
        })
    }
}
