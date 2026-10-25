//! Referenced model files, from the drawing's UFRxDoc (spec clause 10).

use super::Drawing;
use crate::error::Result;
use crate::ufrx::{FileReference, Record, Value};

/// The UFRxDoc stream; a drawing without one references no files.
const UFRX_STREAM: &str = "UFRxDoc";

/// A model file the drawing references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingReference {
    /// Path, identity and occurrence count (one occurrence per view plus one per sheet).
    pub file: FileReference,
    /// Library id (-1 outside a library).
    pub library_id: i32,
    /// The record's `u32_flags`.
    pub flags: u32,
}

impl Drawing<'_> {
    /// The referenced Inventor files, as last resolved.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("drawing.idw")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let drawing = corliss::model::Drawing::new(&model)?;
    /// for r in drawing.references()? {
    ///     println!("{} ({} occurrences)", r.file.path, r.file.n_occurrences);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn references(&self) -> Result<Vec<DrawingReference>> {
        let doc = self.model.document();
        if doc.container().stream(UFRX_STREAM).is_none() {
            return Ok(Vec::new());
        }
        let ufrx = doc.ufrx_doc()?;
        let records = ufrx
            .root
            .record("inv_files")
            .map_or(&[][..], |r| r.list("files"));
        Ok(ufrx
            .references()
            .into_iter()
            .zip(records)
            .map(|(file, r)| with_record_fields(file, r))
            .collect())
    }
}

/// Adds the record fields the typed [`FileReference`] leaves out.
fn with_record_fields(file: FileReference, record: &Record) -> DrawingReference {
    let library_id = match record.get("library_id") {
        Some(Value::I32(v)) => *v,
        _ => -1,
    };
    let flags = record.uint("u32_flags").unwrap_or_default() as u32;
    DrawingReference {
        file,
        library_id,
        flags,
    }
}
