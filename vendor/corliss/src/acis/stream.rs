//! A SAB stream split into header, records and markers (spec 13.6.1, 13.6.3).

use super::token::{Token, latin1};
use crate::bytes::ByteReader;
use crate::error::{CorlissError, FailureClass, Result};

const STRUCTURE: &str = "SAB stream";
const MAGICS: [&[u8]; 2] = [b"ASM BinaryFile4", b"ACIS BinaryFile"];

/// The stream header (spec 13.6.1, Table 13-5).
#[derive(Debug, Clone, PartialEq)]
pub struct SabHeader {
    /// "ASM BinaryFile4", or "ACIS BinaryFile" in 2010 files.
    pub magic: String,
    /// Save version: 23200 = ASM 232 (Inventor 2027).
    pub version: i32,
    /// Number of saved records, or 0.
    pub n_records: i32,
    /// Number of top-level entities (the first records).
    pub n_entities: i32,
    /// Bit 0: history saved.
    pub flags: i32,
    /// "Inventor".
    pub product_id: String,
    /// "ASM 232.0.1.65535 NT".
    pub product_version: String,
    /// Save time in C `ctime` format.
    pub date: String,
    /// Millimetres per model unit (10: the model is in centimetres).
    pub units_mm: f64,
    /// Linear resolution at save time.
    pub resabs: f64,
    /// Normal resolution at save time.
    pub resnor: f64,
}

impl SabHeader {
    /// The restore version V = 100 × `version` the version conditions test (spec 13.6.4).
    pub fn restore_version(&self) -> i64 {
        i64::from(self.version) * 100
    }
}

/// One record: its class (the identifier chain joined with "-") and its data tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct SabRecord {
    /// Record class, most derived first: `plane-surface`, `ATTRIB_CUSTOM-attrib`.
    pub class: String,
    /// Data tokens, without the identifier chain and the terminator.
    pub tokens: Vec<Token>,
    /// True for records of the live model (before the history section).
    pub live: bool,
}

/// A parsed SAB stream.
///
/// ```no_run
/// # let sab: Vec<u8> = Vec::new();
/// let stream = corliss::acis::SabStream::parse(&sab)?;
/// println!("{} records, saved by {}", stream.records.len(), stream.header.product_version);
/// # Ok::<(), corliss::CorlissError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SabStream {
    /// Header.
    pub header: SabHeader,
    /// Records in stream order; a pointer value is an index into this list.
    pub records: Vec<SabRecord>,
}

impl SabStream {
    /// Parses a stream up to its `End-of-ASM-data` marker [R-10].
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        let header = read_header(&mut r)?;
        let mut records = Vec::new();
        let mut live = true;
        loop {
            let class = read_class(&mut r)?;
            if is_marker(&class) {
                live &= !class.starts_with("Begin-of-");
                if class.starts_with("End-of-") && class.ends_with("-data") {
                    break;
                }
                continue;
            }
            records.push(SabRecord {
                class,
                tokens: read_data(&mut r)?,
                live,
            });
        }
        Ok(SabStream { header, records })
    }

    /// The record at a pointer value, or `None` for null or out-of-range pointers.
    pub fn record(&self, ptr: i32) -> Option<&SabRecord> {
        usize::try_from(ptr).ok().and_then(|k| self.records.get(k))
    }
}

fn read_header(r: &mut ByteReader<'_>) -> Result<SabHeader> {
    let Some(magic) = MAGICS.iter().find(|m| r.rest().starts_with(m)) else {
        return Err(CorlissError::invalid(
            STRUCTURE,
            "magic",
            0,
            "other bytes",
            "\"ASM BinaryFile4\" or \"ACIS BinaryFile\"",
        ));
    };
    r.take(magic.len(), "magic")?;
    let (version, n_records) = (r.i32("version")?, r.i32("n_records")?);
    let (n_entities, flags) = (r.i32("n_entities")?, r.i32("flags")?);
    Ok(SabHeader {
        magic: latin1(magic),
        version,
        n_records,
        n_entities,
        flags,
        product_id: header_text(r, "product_id")?,
        product_version: header_text(r, "product_version")?,
        date: header_text(r, "date")?,
        units_mm: header_double(r, "units_mm")?,
        resabs: header_double(r, "resabs")?,
        resnor: header_double(r, "resnor")?,
    })
}

fn header_text(r: &mut ByteReader<'_>, field: &'static str) -> Result<String> {
    let at = r.position();
    match Token::read(r)? {
        Token::Str(b) => Ok(latin1(&b)),
        other => Err(CorlissError::invalid(
            STRUCTURE,
            field,
            at,
            format!("{other:?}"),
            "a string token",
        )),
    }
}

fn header_double(r: &mut ByteReader<'_>, field: &'static str) -> Result<f64> {
    let at = r.position();
    match Token::read(r)? {
        Token::Double(v) => Ok(v),
        other => Err(CorlissError::invalid(
            STRUCTURE,
            field,
            at,
            format!("{other:?}"),
            "a double token",
        )),
    }
}

/// Reads an identifier chain: sub-identifiers, then one identifier.
fn read_class(r: &mut ByteReader<'_>) -> Result<String> {
    let mut parts = Vec::new();
    loop {
        let at = r.position();
        match Token::read(r)? {
            Token::SubIdent(s) => parts.push(s),
            Token::Ident(s) => {
                parts.push(s);
                return Ok(parts.join("-"));
            }
            other => {
                return Err(CorlissError::invalid(
                    STRUCTURE,
                    "class",
                    at,
                    format!("{other:?}"),
                    "a record identifier",
                ));
            }
        }
    }
}

fn read_data(r: &mut ByteReader<'_>) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    loop {
        match Token::read(r)? {
            Token::Terminator => return Ok(tokens),
            t => tokens.push(t),
        }
        if r.remaining() == 0 {
            let class = FailureClass::Truncated {
                needed: 1,
                available: 0,
            };
            return Err(CorlissError::decode(
                STRUCTURE,
                "terminator",
                r.position(),
                class,
            ));
        }
    }
}

/// The three section markers, identifier chains with no data (spec 13.6.3).
fn is_marker(class: &str) -> bool {
    matches!(
        class,
        "Begin-of-ASM-History-Data"
            | "End-of-ASM-History-Section"
            | "End-of-ASM-data"
            | "Begin-of-ACIS-History-Data"
            | "End-of-ACIS-History-Section"
            | "End-of-ACIS-data"
    )
}
