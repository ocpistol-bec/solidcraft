//! Sheet formats saved in the drawing (Drawing Resources > Sheet Formats, spec 8.8): one
//! VT_BLOB per format in the '_Inventor Drawing Sheet Formats Properties' set.

use super::{Drawing, SheetSize};
use crate::bytes::ByteReader;
use crate::error::{CorlissError, Result};
use crate::guid::Guid;
use crate::propset::{PropertySet, PropertyValue};

/// FMTID of '_Inventor Drawing Sheet Formats Properties'.
const SHEET_FORMATS_FMTID: &str = "c5955304-0648-4f61-8722-364f594147ac";
/// Structure name for errors.
const STRUCTURE: &str = "sheet format";

/// Sheet orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Stored as 0.
    Landscape,
    /// Stored as 1.
    Portrait,
    /// Any other stored value.
    Other(u32),
}

/// The preview header of a sheet format (a CF_METAFILEPICT-style clipboard header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetFormatPreview {
    /// Clipboard format (3, CF_METAFILEPICT).
    pub format: u32,
    /// Mapping mode (8).
    pub mapping_mode: u16,
    /// Preview width, pixels.
    pub width: u16,
    /// Preview height, pixels.
    pub height: u16,
}

/// One saved sheet format.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetFormat {
    /// Sheet width, cm.
    pub width: f64,
    /// Sheet height, cm.
    pub height: f64,
    /// Sheet size.
    pub size: SheetSize,
    /// Orientation.
    pub orientation: Orientation,
    /// Layout version (9).
    pub version: u32,
    /// Name of the sheet the format was made from.
    pub sheet_name: String,
    /// The byte after the sheet name (0).
    pub flag: u8,
    /// Number of base views.
    pub base_views: u32,
    /// Number of projected views.
    pub projected_views: u32,
    /// A second string, empty in every observed save.
    pub extra: String,
    /// Preview header, when the format has a preview.
    pub preview: Option<SheetFormatPreview>,
    /// The preview PNG (180 x 180), when the format has a preview.
    pub png: Option<Vec<u8>>,
    /// Property id of the blob.
    pub pid: u32,
    /// Format name, from the set's dictionary.
    pub name: Option<String>,
}

impl Drawing<'_> {
    /// The sheet formats saved in the drawing, in property-id order within each set.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("drawing.idw")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let drawing = corliss::model::Drawing::new(&model)?;
    /// for f in drawing.sheet_formats()? {
    ///     println!("{:?}: {:?} views", f.name, f.base_views);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn sheet_formats(&self) -> Result<Vec<SheetFormat>> {
        let Some(fmtid) = Guid::parse(SHEET_FORMATS_FMTID) else {
            return Ok(Vec::new());
        };
        let Some(stream) = self.model.document().property_set(&fmtid)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for set in &stream.sets {
            out.extend(formats_of_set(set)?);
        }
        Ok(out)
    }
}

/// The formats of one property set, by ascending property id.
fn formats_of_set(set: &PropertySet) -> Result<Vec<SheetFormat>> {
    let mut blobs: Vec<(u32, &[u8])> = set
        .properties
        .iter()
        .filter_map(|p| match &p.value {
            PropertyValue::Blob(b) => Some((p.id, b.as_slice())),
            _ => None,
        })
        .collect();
    blobs.sort_by_key(|(id, _)| *id);
    blobs
        .into_iter()
        .map(|(pid, blob)| {
            let mut format = parse_sheet_format(blob)?;
            format.pid = pid;
            format.name = set.name_of(pid).map(str::to_string);
            Ok(format)
        })
        .collect()
}

/// Parses one sheet-format blob (little-endian): f64 width, f64 height, u32 size code,
/// u32 orientation, u32 version, CString sheet name, u8 flag, u32 base views, u32 projected
/// views, CString extra, u32 preview size, then the preview (u32 0xFFFFFFFF, u32 format,
/// u16 mapping mode, u16 width, u16 height, u16 0, PNG) or, without a preview, one zero u32.
///
/// ```
/// let mut blob = Vec::new();
/// blob.extend(27.94f64.to_le_bytes());
/// blob.extend(21.59f64.to_le_bytes());
/// for word in [0u32, 1, 9] { blob.extend(word.to_le_bytes()); }
/// blob.extend([0xFF, 0xFE, 0xFF, 1, b'S', 0]);
/// blob.extend([0, 0, 0, 0, 0, 0, 0, 0, 0]);
/// blob.extend([0xFF, 0xFE, 0xFF, 0]);
/// blob.extend([0u8; 8]);
/// let f = corliss::model::drawing::parse_sheet_format(&blob)?;
/// assert_eq!(f.sheet_name, "S");
/// assert!(f.png.is_none());
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn parse_sheet_format(blob: &[u8]) -> Result<SheetFormat> {
    let mut r = ByteReader::new(blob, STRUCTURE);
    let (width, height) = (r.f64("width")?, r.f64("height")?);
    let size = SheetSize::from_code(i64::from(r.u32("size")?));
    let orientation = orientation_of(r.u32("orientation")?);
    let version = r.u32("version")?;
    let sheet_name = cstring(&mut r, "sheet_name")?;
    let (flag, base_views, projected_views) = (
        r.u8("flag")?,
        r.u32("base_views")?,
        r.u32("projected_views")?,
    );
    let extra = cstring(&mut r, "extra")?;
    let (preview, png) = read_preview(&mut r)?;
    r.finish()?;
    Ok(SheetFormat {
        width,
        height,
        size,
        orientation,
        version,
        sheet_name,
        flag,
        base_views,
        projected_views,
        extra,
        preview,
        png,
        pid: 0,
        name: None,
    })
}

fn orientation_of(code: u32) -> Orientation {
    match code {
        0 => Orientation::Landscape,
        1 => Orientation::Portrait,
        other => Orientation::Other(other),
    }
}

/// The preview size word and the preview it announces (or the zero word that stands for none).
fn read_preview(r: &mut ByteReader<'_>) -> Result<(Option<SheetFormatPreview>, Option<Vec<u8>>)> {
    let size = r.u32("preview_size")? as usize;
    if size == 0 {
        r.u32("no_preview")?;
        return Ok((None, None));
    }
    let start = r.position();
    r.u32("preview_marker")?;
    let preview = SheetFormatPreview {
        format: r.u32("preview_format")?,
        mapping_mode: r.u16("mapping_mode")?,
        width: r.u16("preview_width")?,
        height: r.u16("preview_height")?,
    };
    r.u16("preview_hmf")?;
    let png_len = size.checked_sub(r.position() - start).ok_or_else(|| {
        CorlissError::invalid(
            STRUCTURE,
            "preview_size",
            start,
            size,
            "at least 16 header bytes",
        )
    })?;
    let png = r.take(png_len, "png")?.to_vec();
    Ok((Some(preview), Some(png)))
}

/// An MFC CArchive CString in its Unicode form: FF FE FF, u8 length (FF: a u16 length
/// follows), UTF-16LE characters.
fn cstring(r: &mut ByteReader<'_>, field: &'static str) -> Result<String> {
    let at = r.position();
    let marker = r.take(3, field)?;
    if marker != [0xFF, 0xFE, 0xFF] {
        return Err(CorlissError::invalid(
            STRUCTURE,
            field,
            at,
            format!("{marker:02X?}"),
            "FF FE FF",
        ));
    }
    let mut len = usize::from(r.u8(field)?);
    if len == 0xFF {
        len = usize::from(r.u16(field)?);
    }
    let units: Vec<u16> = r
        .take(2 * len, field)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16(&units).map_err(|_| {
        CorlissError::invalid(STRUCTURE, field, at, "unpaired surrogate", "UTF-16LE text")
    })
}
