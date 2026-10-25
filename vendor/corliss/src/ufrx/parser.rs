//! Field readers of the UFRxDoc parser: every read appends a typed field to a record.

use super::record::{Record, Value};
use crate::bytes::ByteReader;
use crate::error::{CorlissError, FailureClass, Result};

pub(super) const STRUCTURE: &str = "UFRxDoc";

/// Parser state: the cursor plus the layout switches read from the stream itself.
pub(super) struct Parser<'a> {
    pub r: ByteReader<'a>,
    /// Per-section format versions (spec Table 10-2).
    pub versions: Vec<u16>,
    /// Release year of the save (major + 1996 for majors above 11).
    pub release_year: u16,
    /// 1 when an iPart factory member table follows the range box.
    pub has_factory: bool,
    /// True for assemblies (post office and component patterns).
    pub is_assembly: bool,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Parser {
            r: ByteReader::new(data, STRUCTURE),
            versions: Vec::new(),
            release_year: 2027,
            has_factory: false,
            is_assembly: false,
        }
    }

    /// Section version `k`, or −1 when the vector is shorter.
    pub fn v(&self, k: usize) -> i32 {
        self.versions.get(k).map_or(-1, |v| i32::from(*v))
    }

    pub fn u8(&mut self, rec: &mut Record, name: &'static str) -> Result<u8> {
        let v = self.r.u8(name)?;
        rec.push(name, Value::U8(v));
        Ok(v)
    }

    pub fn u16(&mut self, rec: &mut Record, name: &'static str) -> Result<u16> {
        let v = self.r.u16(name)?;
        rec.push(name, Value::U16(v));
        Ok(v)
    }

    pub fn u32(&mut self, rec: &mut Record, name: &'static str) -> Result<u32> {
        let v = self.r.u32(name)?;
        rec.push(name, Value::U32(v));
        Ok(v)
    }

    pub fn i32(&mut self, rec: &mut Record, name: &'static str) -> Result<i32> {
        let v = self.r.i32(name)?;
        rec.push(name, Value::I32(v));
        Ok(v)
    }

    pub fn u64(&mut self, rec: &mut Record, name: &'static str) -> Result<u64> {
        let v = self.r.u64(name)?;
        rec.push(name, Value::U64(v));
        Ok(v)
    }

    /// A u32 element count that must fit in the rest of the stream at `unit` bytes each.
    pub fn count(&mut self, rec: &mut Record, name: &'static str, unit: usize) -> Result<usize> {
        let n = self.r.count(name, unit)?;
        rec.push(name, Value::U32(n as u32));
        Ok(n)
    }

    pub fn guid(&mut self, rec: &mut Record, name: &'static str) -> Result<()> {
        let v = self.r.guid(name)?;
        rec.push(name, Value::Guid(v));
        Ok(())
    }

    pub fn version(&mut self, rec: &mut Record, name: &'static str) -> Result<[u8; 8]> {
        let mut v = [0u8; 8];
        v.copy_from_slice(self.r.take(8, name)?);
        rec.push(name, Value::Version(v));
        Ok(v)
    }

    pub fn text16(&mut self, rec: &mut Record, name: &'static str) -> Result<()> {
        let v = self.r.text16(name)?;
        rec.push(name, Value::Text16(v));
        Ok(())
    }

    pub fn text8(&mut self, rec: &mut Record, name: &'static str) -> Result<()> {
        let v = self.r.text8(name)?;
        rec.push(name, Value::Text8(v));
        Ok(())
    }

    pub fn u16s(&mut self, rec: &mut Record, name: &'static str, n: usize) -> Result<Vec<u16>> {
        let v = (0..n)
            .map(|_| self.r.u16(name))
            .collect::<Result<Vec<_>>>()?;
        rec.push(name, Value::U16s(v.clone()));
        Ok(v)
    }

    pub fn u32s(&mut self, rec: &mut Record, name: &'static str, n: usize) -> Result<Vec<u32>> {
        let v = (0..n)
            .map(|_| self.r.u32(name))
            .collect::<Result<Vec<_>>>()?;
        rec.push(name, Value::U32s(v.clone()));
        Ok(v)
    }

    pub fn f32s(&mut self, rec: &mut Record, name: &'static str, n: usize) -> Result<()> {
        let v = (0..n)
            .map(|_| Ok(f32::from_bits(self.r.u32(name)?)))
            .collect::<Result<Vec<_>>>()?;
        rec.push(name, Value::F32s(v));
        Ok(())
    }

    /// A Bool that MUST be 0 or 1.
    pub fn flag(&mut self, rec: &mut Record, name: &'static str) -> Result<bool> {
        let at = self.r.position();
        match self.u8(rec, name)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CorlissError::invalid(STRUCTURE, name, at, other, "0 or 1")),
        }
    }

    /// The UFRxPersist flag that closes every UFRx record; auxiliary data has no sample (OI-093).
    pub fn persist(&mut self, rec: &mut Record) -> Result<()> {
        let at = self.r.position();
        if self.u8(rec, "persist_flag")? != 0 {
            return Err(self.unsupported(at, "persist_flag", "UFRxPersist auxiliary data"));
        }
        Ok(())
    }

    /// An error for a branch Inventor's writer has but no sample shows (spec 10.1 conformance limits).
    pub fn unsupported(&self, at: usize, field: &'static str, what: &str) -> CorlissError {
        let class = FailureClass::Unsupported {
            reason: format!("{what} has no sample, so its layout is not specified (OI-093)"),
        };
        CorlissError::decode(STRUCTURE, field, at, class)
    }

    /// Reads `n` records with `read` into a list field named `name`.
    pub fn list(
        &mut self,
        rec: &mut Record,
        name: &'static str,
        n: usize,
        read: fn(&mut Self) -> Result<Record>,
    ) -> Result<()> {
        let items = (0..n).map(|_| read(self)).collect::<Result<Vec<_>>>()?;
        rec.push(name, Value::List(items));
        Ok(())
    }
}
