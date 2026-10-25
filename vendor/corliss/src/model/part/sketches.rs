//! 2D sketches (spec clause 15.3), after the reference part reader: placement, entities in sketch
//! coordinates (cm), geometric constraints, driving dimensions, texts and sketch patterns.

mod annotations;
mod entities;
mod types;

pub use types::*;

use std::collections::HashMap;

use super::Part;
use crate::grammar::DecodedNode;
use crate::model::view::{Matrix4, node_ref, ref_list, vec3};

/// RDxPlanarSketch.
const T_SKETCH2D: u32 = 0x9087_4D11;
/// Sketch placement: the sketch's Transformation (`transformation2`) → its plane.
const T_SKETCH_PLACEMENT: u32 = 0x3D64_CCF0;

impl Part<'_> {
    /// Every 2D sketch, in block order.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// for sketch in part.sketches() {
    ///     println!("{:?} on {:?}: {} lines", sketch.name, sketch.plane, sketch.lines.len());
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn sketches(&self) -> Vec<Sketch> {
        let placement = self.sketch_placements();
        self.view()
            .by_type(T_SKETCH2D)
            .map(|(index, sk)| self.sketch(index, sk, &placement))
            .collect()
    }

    /// Transformation node index → plane node index.
    fn sketch_placements(&self) -> HashMap<usize, Option<usize>> {
        self.view()
            .by_type(T_SKETCH_PLACEMENT)
            .filter_map(|(_, n)| Some((node_ref(n, "transformation2")?, node_ref(n, "plane"))))
            .collect()
    }

    fn sketch(
        &self,
        index: usize,
        sk: &DecodedNode,
        placement: &HashMap<usize, Option<usize>>,
    ) -> Sketch {
        let mut sketch = self.sketch_frame(index, sk, placement);
        for entity in ref_list(sk, "entities") {
            self.add_sketch_entity(&mut sketch, entity);
        }
        sketch.texts = self.sketch_texts(index);
        sketch.patterns = self.sketch_patterns(index);
        sketch
    }

    /// The sketch's name, plane, matrix and normal, with no entities yet.
    fn sketch_frame(
        &self,
        index: usize,
        sk: &DecodedNode,
        placement: &HashMap<usize, Option<usize>>,
    ) -> Sketch {
        let xf = node_ref(sk, "transformation");
        let plane = xf.and_then(|x| placement.get(&x).copied().flatten());
        let direction = node_ref(sk, "direction").and_then(|d| self.view().node(d));
        Sketch {
            index,
            name: self.name(index),
            plane: plane.and_then(|p| self.name(p)),
            matrix: xf.and_then(|x| self.view().node(x)).map(sketch_matrix),
            normal: direction.map(|d| vec3(d, "dir")),
            ..Sketch::default()
        }
    }
}

/// The sketch → model matrix of a Transformation (90874D18) node. Unlike the general masked
/// matrix (`view::transform`), entry j is 0 whenever bit j of d2 is set, whatever d1 says; the
/// reference reader decodes sketch placements this way.
fn sketch_matrix(n: &DecodedNode) -> Matrix4 {
    let d1 = n.int("transformation.d1").unwrap_or(0);
    let d2 = n.int("transformation.d2").unwrap_or(0);
    let mut cells = n
        .fields
        .iter()
        .filter(|f| {
            f.name
                .as_deref()
                .is_some_and(|x| x.starts_with("transformation.m"))
        })
        .filter_map(|f| f.value.as_f64());
    let mut m = [[0.0; 4]; 4];
    for j in 0..16 {
        let bit = 1 << j;
        m[j / 4][j % 4] = if d2 & bit != 0 {
            0.0
        } else if d1 & bit != 0 {
            1.0
        } else {
            cells.next().unwrap_or(0.0)
        };
    }
    m
}
