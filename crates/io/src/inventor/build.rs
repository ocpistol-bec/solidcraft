//! The import's intermediate as a design: parameters on the table, sketches and features on the
//! timeline, in the part's order.

use solidcraft_doc::{Document, FeatureKind, PlaneRef, ProfileSel, RevolveConvention, SketchId};

use super::model::{BuiltSketch, InvFeature, InvModel};

pub(crate) fn build(mut m: InvModel, name: &str) -> Result<(Document, Vec<String>), String> {
    let mut doc = Document::new(name);
    let mut warnings = std::mem::take(&mut m.warnings);

    for p in &m.params {
        if doc.params.iter().any(|q| q.name == p.name) {
            warnings.push(format!("parameter `{}` appears more than once in the file; the first value is kept", p.name));
            continue;
        }
        doc.params.push(p.clone());
    }

    for s in &m.sketches {
        doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Custom { plane: s.plane }, sketch: s.sketch.clone() }, s.name.as_deref())
            .map_err(|e| format!("sketch {}: {e}", s.name.as_deref().unwrap_or("?")))?;
    }

    for f in &m.features {
        match f {
            InvFeature::Extrude(e) => {
                let sketch_id = sketch_of(&m.sketches, &e.sketch, "extrude", &e.name)?;
                doc.add_feature(
                    FeatureKind::Extrude {
                        sketch: SketchId(sketch_id),
                        profiles: ProfileSel::Curves { loops: e.loops.clone() },
                        extent: e.extent.clone(),
                        operation: e.operation,
                        targets: Vec::new(),
                        thin: None,
                    },
                    e.name.as_deref(),
                )
                .map_err(|err| format!("extrude {}: {err}", e.name.as_deref().unwrap_or("?")))?;
            }
            InvFeature::Revolve(r) => {
                let sketch_id = sketch_of(&m.sketches, &r.sketch, "revolve", &r.name)?;
                doc.add_feature(
                    FeatureKind::Revolve {
                        sketch: SketchId(sketch_id),
                        profiles: ProfileSel::Curves { loops: r.loops.clone() },
                        axis: r.axis.clone(),
                        angle: r.angle.clone(),
                        angle2: None,
                        convention: RevolveConvention::SketchNormal,
                        symmetric: r.symmetric,
                        operation: r.operation,
                        targets: Vec::new(),
                        to: None,
                    },
                    r.name.as_deref(),
                )
                .map_err(|err| format!("revolve {}: {err}", r.name.as_deref().unwrap_or("?")))?;
            }
            InvFeature::Unsupported { name, kind } => {
                return Err(format!(
                    "feature `{}` ({kind}) is not supported yet by the Inventor history import",
                    name.clone().unwrap_or_else(|| "unnamed".into())
                ));
            }
        }
    }

    Ok((doc, warnings))
}

/// The feature id of the sketch a feature's profile lives in: by name, else the file's only
/// sketch.
fn sketch_of(sketches: &[BuiltSketch], named: &Option<String>, what: &str, feature: &Option<String>) -> Result<u64, String> {
    if sketches.is_empty() {
        return Err(format!("{what} `{}` has no sketch in the file", feature.as_deref().unwrap_or("?")));
    }
    if sketches.len() == 1 {
        return Ok(1);
    }
    let Some(name) = named.as_deref() else {
        return Err(format!("{what} `{}` names no sketch and the file has more than one", feature.as_deref().unwrap_or("?")));
    };
    // Sketch features were added first, in order: the i-th sketch is the (i+1)-th feature.
    match sketches.iter().position(|s| s.name.as_deref() == Some(name)) {
        Some(i) => Ok(i as u64 + 1),
        None => Err(format!("{what} `{}` sketches `{name}`, which the file does not hold", feature.as_deref().unwrap_or("?"))),
    }
}
