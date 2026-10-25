//! UFRxDoc end of stream: states, range box and tail (spec 10.10–10.11).

use super::parser::Parser;
use super::record::{Record, Value};
use crate::error::Result;

/// State field ids by value type (spec 10.10 field 8).
const STATE_U32: [u8; 2] = [0x27, 0x19];
const STATE_U8: [u8; 2] = [0x07, 0x10];
const STATE_PAIR: [u8; 9] = [0x11, 0x13, 0x16, 0x17, 0x18, 0x1F, 0x24, 0x25, 0x2A];
/// Below this many bytes left, the tail has the 2015 layout (appendix) instead of 2027's.
const TAIL_2027_MIN: usize = 10;

impl Parser<'_> {
    /// `exports`, the non-transient states (spec 10.10).
    pub(super) fn states(&mut self) -> Result<Record> {
        let mut r = Record::new("exports");
        let n = self.count(&mut r, "count", 30)?;
        self.list(&mut r, "entries", n, Self::state)?;
        Ok(r)
    }

    fn state(&mut self) -> Result<Record> {
        let mut e = Record::new("export");
        self.text8(&mut e, "type_name")?;
        self.text16(&mut e, "name")?;
        self.guid(&mut e, "id")?;
        if self.release_year >= 2012 {
            self.u8(&mut e, "system")?;
            self.u8(&mut e, "secondary")?;
        } else {
            self.u8(&mut e, "u8_flags")?;
        }
        self.u32(&mut e, "reserved_state_140")?;
        if self.release_year >= 2012 {
            self.u32(&mut e, "n_slots")?;
        }
        let n = self.count(&mut e, "n_present", 2)?;
        self.list(&mut e, "props", n, Self::state_field)?;
        self.state_end(&mut e)?;
        Ok(e)
    }

    fn state_field(&mut self) -> Result<Record> {
        let mut p = Record::new("xprop");
        let id = self.u8(&mut p, "type")?;
        let pair = if self.v(9) >= 4 {
            STATE_PAIR.contains(&id)
        } else {
            id == 0x2A
        };
        if STATE_U32.contains(&id) {
            self.u32(&mut p, "value")?;
        } else if STATE_U8.contains(&id) {
            self.u8(&mut p, "value")?;
        } else {
            self.guid(&mut p, "value")?;
            if pair {
                self.guid(&mut p, "value2")?;
            }
        }
        Ok(p)
    }

    fn state_end(&mut self, e: &mut Record) -> Result<()> {
        let at = self.r.position();
        let refs = self.u32(e, "n_refs")?;
        let has_cached = self.flag(e, "has_cached")?;
        if refs != 0 {
            return Err(self.unsupported(at, "n_refs", "a state reference (UFRxStateRef)"));
        }
        if has_cached {
            let cached = self.cached()?;
            e.push("cached", Value::Record(cached));
        }
        if self.v(0) >= 0x1F && self.v(1) >= 0x14 {
            self.persist(e)?;
        }
        Ok(())
    }

    /// `range_box`: the model box in cm, or the 2010 drawing/presentation form.
    pub(super) fn range_box(&mut self, is_3d: bool) -> Result<Record> {
        let mut r = Record::new("range_box");
        if is_3d || self.release_year >= 2012 {
            self.f32s(&mut r, "box", 6)?;
        } else {
            self.u8(&mut r, "u8_0")?;
            self.u32s(&mut r, "u32a_1", 4)?;
        }
        Ok(r)
    }

    /// `tail`: iPart factory members, mirror links, file ids and the persist flag.
    pub(super) fn tail(&mut self) -> Result<Record> {
        let mut r = Record::new("tail");
        if self.r.remaining() < TAIL_2027_MIN {
            self.u32s(&mut r, "appendix", 2)?;
            if self.v(20) >= 1 {
                self.u8(&mut r, "appendix_u8")?;
            }
            return Ok(r);
        }
        if self.has_factory {
            self.factory(&mut r)?;
        }
        let at = self.r.position();
        if self.u32(&mut r, "n_mirror_links")? != 0 {
            return Err(self.unsupported(at, "n_mirror_links", "a mirror link record"));
        }
        self.i32(&mut r, "non_rse_file_id")?;
        self.guid(&mut r, "assoc_mirror_id")?;
        self.u32(&mut r, "assoc_mirror_src_rep_id")?;
        self.persist(&mut r)?;
        Ok(r)
    }

    /// The iPart factory member table (UFRxFactory).
    fn factory(&mut self, r: &mut Record) -> Result<()> {
        let n = self.count(r, "n_members", 38)?;
        self.list(r, "ipart_members", n, |p| {
            let mut x = Record::new("ipart_member");
            p.u8(&mut x, "u8_0")?;
            p.text16(&mut x, "name")?;
            p.text16(&mut x, "file_name")?;
            p.u8(&mut x, "u8_1")?;
            p.guid(&mut x, "id")?;
            p.u32s(&mut x, "u32a_2", 3)?;
            Ok(x)
        })?;
        self.text16(r, "active_member")?;
        self.u32(r, "reserved_factory_a")?;
        self.u16(r, "reserved_factory_b")?;
        Ok(())
    }
}
