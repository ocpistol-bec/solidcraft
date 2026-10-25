//! zlib and Zstandard codecs of M and B streams (spec 11.3).

use std::io::{Read, Write};

use crate::error::{CorlissError, FailureClass, Result};

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// The compression method of a segment's streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compression {
    /// zlib (RFC 1950); Inventor 2025 and earlier.
    Zlib,
    /// Zstandard (RFC 8878); Inventor 2027.
    Zstd,
}

impl Compression {
    /// The M stream compression byte and the high byte of the B stream method word.
    pub fn code(self) -> u8 {
        match self {
            Compression::Zlib => 1,
            Compression::Zstd => 2,
        }
    }

    /// Maps the M stream compression byte; `None` for codes the specification does not define.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Compression::Zlib),
            2 => Some(Compression::Zstd),
            _ => None,
        }
    }

    /// Detects the method from the first bytes of compressed data.
    pub fn sniff(data: &[u8]) -> Option<Self> {
        if data.starts_with(&ZSTD_MAGIC) {
            return Some(Compression::Zstd);
        }
        (data.first() == Some(&0x78)).then_some(Compression::Zlib)
    }
}

/// Inflates `data`, which must use `method`.
///
/// Zstd input may hold several frames without a declared content size, so frames are
/// decoded one after the other by streaming decompression [R-02].
pub fn decompress(method: Compression, data: &[u8], structure: &'static str) -> Result<Vec<u8>> {
    let inflated = match method {
        Compression::Zlib => inflate_zlib(data),
        Compression::Zstd => inflate_zstd(data),
    };
    inflated.map_err(|detail| compression_error("decode", structure, detail))
}

/// Deflates `data` with `method`.
///
/// The output is valid for Inventor but not byte-identical to Inventor's own encoder; callers
/// that need identical bytes keep the original compressed data of unchanged streams.
pub fn compress(method: Compression, data: &[u8], structure: &'static str) -> Result<Vec<u8>> {
    let deflated = match method {
        Compression::Zlib => deflate_zlib(data),
        Compression::Zstd => Ok(deflate_zstd(data)),
    };
    deflated.map_err(|detail| compression_error("encode", structure, detail))
}

/// Compressed bytes as read, with what they inflate to.
///
/// Re-encoders compare against it so that data the caller left alone is written back as the
/// exact bytes Inventor produced [W-01], not as this crate's (different) encoder output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Packed {
    method: Compression,
    compressed: Vec<u8>,
    inflated: Vec<u8>,
}

impl Packed {
    /// Inflates `compressed` and remembers both forms.
    pub(crate) fn unpack(
        method: Compression,
        compressed: &[u8],
        structure: &'static str,
    ) -> Result<Self> {
        let inflated = decompress(method, compressed, structure)?;
        Ok(Packed {
            method,
            compressed: compressed.to_vec(),
            inflated,
        })
    }

    /// The inflated bytes.
    pub(crate) fn inflated(&self) -> &[u8] {
        &self.inflated
    }

    /// Compressed bytes for `inflated`: the original ones when nothing changed, else fresh output.
    pub(crate) fn repack(
        &self,
        method: Compression,
        inflated: &[u8],
        structure: &'static str,
    ) -> Result<Vec<u8>> {
        if method == self.method && inflated == self.inflated {
            return Ok(self.compressed.clone());
        }
        compress(method, inflated, structure)
    }
}

fn compression_error(
    operation: &'static str,
    structure: &'static str,
    detail: String,
) -> CorlissError {
    CorlissError {
        operation,
        structure,
        field: "data",
        offset: None,
        class: FailureClass::Compression { detail },
    }
}

fn inflate_zlib(data: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn inflate_zstd(data: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut input = data;
    let mut out = Vec::new();
    while input.starts_with(&ZSTD_MAGIC) {
        let mut frame =
            ruzstd::decoding::StreamingDecoder::new(&mut input).map_err(|e| e.to_string())?;
        frame.read_to_end(&mut out).map_err(|e| e.to_string())?;
    }
    if !input.is_empty() {
        return Err(format!("{} bytes after the last zstd frame", input.len()));
    }
    Ok(out)
}

fn deflate_zlib(data: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

fn deflate_zstd(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}
