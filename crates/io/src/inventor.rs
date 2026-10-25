//! Inventor part (`.ipt`) import as an editable timeline, through
//! [Corliss](https://github.com/Oblikovati/Corliss) (GPL-2.0-only, `vendor/corliss`, native
//! builds only). Corliss decodes the document into its parameters, sketches (entities,
//! constraints, dimensions), features and sheet-metal settings; SolidCraft restates the part
//! as its own features — a sketch and a feature for each of the file's — so the history is
//! editable like a part modelled here.
//!
//! The import is strict: every feature of the part must be one SolidCraft can build (sketches,
//! extrusions and revolves today; the rest follow). Anything else is an error naming the
//! feature, and `doc.open` then opens the part as geometry instead (the cadmpeg path of
//! [`crate::native_cad`]) and notes why.
//!
//! Inventor stores lengths in centimetres and angles in radians; the import converts to
//! millimetres and degrees, and keeps each driving dimension bound to the model parameter it
//! was driven by, so the rebuilt part stays parametric.
//!
//! Assemblies (`.iam`) open as geometry through [`crate::native_cad`]; drawings (`.idw`) are
//! not read yet.

mod build;
mod model;

#[cfg(test)]
mod tests;

use crate::{IoError, Result};

/// Is this an Inventor part file name (any case)? Assemblies open as geometry, not history.
pub fn is_inventor_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ipt"))
}

/// The part as a new design: its parameters, its sketches (with their constraints and
/// dimensions) and its features, in timeline order. `file` is the source file name (the design
/// is named after its stem).
#[cfg(not(target_arch = "wasm32"))]
pub fn import_inventor(bytes: &[u8], file: &str) -> Result<(solidcraft_doc::Document, Vec<String>)> {
    let stem = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Import".into());
    let (design, warnings) = solidcraft_kernel::guard("inventor import", || {
        let doc = corliss::Document::from_bytes(bytes).map_err(|e| solidcraft_kernel::KernelError::Invalid(format!("{file}: {e}")))?;
        match doc.kind() {
            Some(corliss::DocumentKind::Part) => Ok(()),
            Some(other) => Err(solidcraft_kernel::KernelError::Invalid(format!(
                "{file}: Inventor {other:?} documents open as geometry; part history is read from .ipt files"
            ))),
            None => Err(solidcraft_kernel::KernelError::Invalid(format!("{file}: not recognised as an Inventor document"))),
        }?;
        let model = corliss::model::Model::load(&doc).map_err(|e| solidcraft_kernel::KernelError::Invalid(format!("{file}: {e}")))?;
        let part = corliss::model::Part::new(&model).map_err(|e| solidcraft_kernel::KernelError::Invalid(format!("{file}: {e}")))?;
        let inv = model::read(&part, file).map_err(solidcraft_kernel::KernelError::Invalid)?;
        build::build(inv, &stem).map_err(solidcraft_kernel::KernelError::Invalid)
    })
    .map_err(|e| match e {
        solidcraft_kernel::KernelError::Invalid(m) => IoError::Invalid(m),
        e => IoError::Invalid(e.to_string()),
    })?;
    Ok((design, warnings))
}

/// The part's history, not available in the browser yet (desktop app and CLI).
#[cfg(target_arch = "wasm32")]
pub fn import_inventor(_bytes: &[u8], file: &str) -> Result<(solidcraft_doc::Document, Vec<String>)> {
    Err(IoError::Invalid(format!("{file}: not supported in the browser yet (Inventor history opens in the desktop app)")))
}
