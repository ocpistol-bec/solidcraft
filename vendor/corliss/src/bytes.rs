//! Little-endian byte cursor and builder shared by every decoder (spec 4.3–4.5).

use crate::error::{CorlissError, FailureClass, Result};
use crate::guid::Guid;

/// A bounds-checked little-endian reader over a byte slice.
///
/// Every read names the structure and field so a failure reports where it happened.
///
/// ```
/// let mut r = corliss::bytes::ByteReader::new(&[1, 0, 0, 0], "example");
/// assert_eq!(r.u32("count").unwrap(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
    structure: &'static str,
}

impl<'a> ByteReader<'a> {
    /// Starts reading `data` as the structure `structure`.
    pub fn new(data: &'a [u8], structure: &'static str) -> Self {
        ByteReader {
            data,
            pos: 0,
            structure,
        }
    }

    /// Current offset from the start of the input.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// The unread bytes, without consuming them.
    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// The bytes read since offset `start` (clamped to the current position).
    pub fn since(&self, start: usize) -> &'a [u8] {
        &self.data[start.min(self.pos)..self.pos]
    }

    /// The structure name this reader reports in errors.
    pub fn structure(&self) -> &'static str {
        self.structure
    }

    /// Takes `n` bytes as a slice.
    pub fn take(&mut self, n: usize, field: &'static str) -> Result<&'a [u8]> {
        if n > self.remaining() {
            let class = FailureClass::Truncated {
                needed: n,
                available: self.remaining(),
            };
            return Err(CorlissError::decode(self.structure, field, self.pos, class));
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn array<const N: usize>(&mut self, field: &'static str) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N, field)?);
        Ok(out)
    }

    /// Reads a u8.
    pub fn u8(&mut self, field: &'static str) -> Result<u8> {
        Ok(self.array::<1>(field)?[0])
    }

    /// Reads a little-endian u16.
    pub fn u16(&mut self, field: &'static str) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array(field)?))
    }

    /// Reads a little-endian i16.
    pub fn i16(&mut self, field: &'static str) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array(field)?))
    }

    /// Reads a little-endian u32.
    pub fn u32(&mut self, field: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array(field)?))
    }

    /// Reads a little-endian i32.
    pub fn i32(&mut self, field: &'static str) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array(field)?))
    }

    /// Reads a little-endian u64.
    pub fn u64(&mut self, field: &'static str) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array(field)?))
    }

    /// Reads a little-endian i64.
    pub fn i64(&mut self, field: &'static str) -> Result<i64> {
        Ok(i64::from_le_bytes(self.array(field)?))
    }

    /// Reads an IEEE 754 double.
    pub fn f64(&mut self, field: &'static str) -> Result<f64> {
        Ok(f64::from_le_bytes(self.array(field)?))
    }

    /// Reads a GUID in Windows byte layout.
    pub fn guid(&mut self, field: &'static str) -> Result<Guid> {
        Ok(Guid::from_bytes(self.array(field)?))
    }

    /// Reads a u32 count that must fit in the remaining input at `unit` bytes per element.
    pub fn count(&mut self, field: &'static str, unit: usize) -> Result<usize> {
        let at = self.pos;
        let n = self.u32(field)? as usize;
        if n.saturating_mul(unit) > self.remaining() {
            let expected = format!(
                "at most {} elements of {unit} bytes",
                self.remaining() / unit.max(1)
            );
            return Err(CorlissError::invalid(
                self.structure,
                field,
                at,
                n,
                expected,
            ));
        }
        Ok(n)
    }

    /// Reads a Text8: u32 byte count, then the bytes (spec Table 4-1).
    pub fn text8(&mut self, field: &'static str) -> Result<Vec<u8>> {
        let n = self.count(field, 1)?;
        Ok(self.take(n, field)?.to_vec())
    }

    /// Reads a Text16: u32 character count, then UTF-16LE code units (spec Table 4-1).
    pub fn text16(&mut self, field: &'static str) -> Result<String> {
        let n = self.count(field, 2)?;
        let raw = self.take(2 * n, field)?;
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// Fails unless the whole input has been read.
    pub fn finish(&self) -> Result<()> {
        if self.remaining() == 0 {
            return Ok(());
        }
        let class = FailureClass::TrailingBytes {
            count: self.remaining(),
        };
        Err(CorlissError::decode(self.structure, "", self.pos, class))
    }
}

/// A little-endian byte builder, the inverse of [`ByteReader`].
///
/// ```
/// let mut w = corliss::bytes::ByteWriter::default();
/// w.u16(0x1F).text16("PmDCSegment");
/// assert_eq!(w.len(), 2 + 4 + 22);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ByteWriter {
    out: Vec<u8>,
}

impl ByteWriter {
    /// Number of bytes written so far.
    pub fn len(&self) -> usize {
        self.out.len()
    }

    /// True when nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// Appends raw bytes.
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.out.extend_from_slice(b);
        self
    }

    /// Appends a u8.
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes(&[v])
    }

    /// Appends a little-endian u16.
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian i16.
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian u32.
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian i32.
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian u64.
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a little-endian i64.
    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// Appends a GUID in Windows byte layout.
    pub fn guid(&mut self, g: &Guid) -> &mut Self {
        self.bytes(g.as_bytes())
    }

    /// Appends a length as u32; lengths beyond u32 cannot occur in a CFB stream.
    pub fn len_u32(&mut self, n: usize) -> &mut Self {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// Appends a Text8 (u32 byte count, then the bytes).
    pub fn text8(&mut self, b: &[u8]) -> &mut Self {
        self.len_u32(b.len()).bytes(b)
    }

    /// Appends a Text16 (u32 character count, then UTF-16LE).
    pub fn text16(&mut self, s: &str) -> &mut Self {
        let units: Vec<u16> = s.encode_utf16().collect();
        self.len_u32(units.len());
        for u in units {
            self.u16(u);
        }
        self
    }

    /// Returns the built bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }
}
