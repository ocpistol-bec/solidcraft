//! UFRxDoc sections before the component body: headers, representations and referenced files
//! (spec 10.2–10.6).

use super::parser::Parser;
use super::record::Record;
use crate::error::Result;

/// Representation type 1: substitute (it carries a substitute instance id).
const REP_SUBSTITUTE: u8 = 1;
/// Representation type 4: LOD factory representation (no build infos).
const REP_LOD_FACTORY: u8 = 4;

impl Parser<'_> {
    /// `header1` (spec 10.2).
    pub(super) fn header1(&mut self) -> Result<Record> {
        let mut r = Record::new("header1");
        self.u16(&mut r, "schema")?;
        let n = self.u16(&mut r, "n_versions")? as usize;
        self.versions = self.u16s(&mut r, "section_versions", n)?;
        let saved = self.version(&mut r, "saved_version")?;
        self.release_year = if saved[2] > 11 {
            u16::from(saved[2]) + 1996
        } else {
            u16::from(saved[2])
        };
        self.u64(&mut r, "saved_time")?;
        self.version(&mut r, "saved_version2")?;
        self.u64(&mut r, "saved_time2")?;
        self.text16(&mut r, "comment")?;
        self.version(&mut r, "created_version")?;
        self.u64(&mut r, "created_time")?;
        self.version(&mut r, "created_version2")?;
        self.u64(&mut r, "created_time2")?;
        self.guid(&mut r, "db_revision")?;
        self.u32(&mut r, "doc_flags")?;
        self.guid(&mut r, "internal_name")?;
        self.text16(&mut r, "original_file_name")?;
        self.u8(&mut r, "reserved_doc_2ec")?;
        self.has_factory = self.u8(&mut r, "has_factory")? != 0;
        Ok(r)
    }

    /// `lod_toc` (spec 10.3).
    pub(super) fn lod_toc(&mut self) -> Result<Record> {
        let mut r = Record::new("lod_toc");
        let n = self.count(&mut r, "count", 16)?;
        self.list(&mut r, "entries", n, |p| {
            let mut e = Record::new("lod_toc_entry");
            p.u32(&mut e, "rep_id")?;
            p.text16(&mut e, "name")?;
            p.u8(&mut e, "rep_type")?;
            p.u8(&mut e, "reserved_rep_bits")?;
            p.u16(&mut e, "is_substitute_rep")?;
            p.text16(&mut e, "rep_text")?;
            Ok(e)
        })?;
        Ok(r)
    }

    /// `header2` (spec 10.4).
    pub(super) fn header2(&mut self) -> Result<Record> {
        let mut r = Record::new("header2");
        let n = self.count(&mut r, "n_pairs", 8)?;
        self.list(&mut r, "pairs", n, |p| {
            let mut e = Record::new("pair");
            p.text16(&mut e, "name")?;
            p.text16(&mut e, "kind")?;
            Ok(e)
        })?;
        self.u32(&mut r, "last_active_rep_id")?;
        if self.v(2) >= 0x0C {
            self.text16(&mut r, "active_design_view")?;
        }
        if self.v(2) >= 0x07 {
            self.u32(&mut r, "file_version_number")?;
        }
        if self.v(2) > 0x0D {
            self.u32(&mut r, "saved_rep_id")?;
        } else {
            self.u16(&mut r, "saved_rep_id")?;
        }
        self.header2_counters(&mut r)?;
        Ok(r)
    }

    fn header2_counters(&mut self, r: &mut Record) -> Result<()> {
        self.u32(r, "max_occurrence_id")?;
        self.u16(r, "next_lod_id")?;
        self.u16(r, "always_1")?;
        self.u32(r, "max_inv_file_ref_id")?;
        self.u32(r, "max_ole_file_ref_id")?;
        if self.v(2) >= 0x0D {
            self.guid(r, "doc_sub_type_1")?;
            self.guid(r, "doc_sub_type_2")?;
        }
        Ok(())
    }

    /// `lods`, the representation records (spec 10.5).
    pub(super) fn lods(&mut self) -> Result<Record> {
        let mut r = Record::new("lods");
        let n = self.count(&mut r, "count", 13)?;
        self.list(&mut r, "entries", n, Self::lod)?;
        Ok(r)
    }

    fn lod(&mut self) -> Result<Record> {
        let mut e = Record::new("lod");
        let kind = self.u8(&mut e, "rep_type")?;
        self.text16(&mut e, "name")?;
        self.u32(&mut e, "rep_id")?;
        self.text16(&mut e, "rep_text")?;
        if kind == REP_SUBSTITUTE {
            self.u32(&mut e, "substitute_instance_id")?;
        }
        let n = self.count(&mut e, "n_suppressed", 13)?;
        self.list(&mut e, "suppressed", n, Self::suppressed)?;
        self.u16(&mut e, "rep_flags")?;
        self.u32(&mut e, "reserved_rep_2e4")?;
        self.u32(&mut e, "component_flags")?;
        if kind == REP_LOD_FACTORY {
            self.u32(&mut e, "reserved_rep_2ec")?;
        }
        if self.is_assembly {
            let n = self.count(&mut e, "n_patterns", 20)?;
            self.list(&mut e, "patterns", n, Self::pattern)?;
        }
        self.persist(&mut e)?;
        let infos = if kind == REP_LOD_FACTORY { 0 } else { 4 };
        self.list(&mut e, "build_infos", infos, |p| {
            let mut h = Record::new("build_info");
            p.version(&mut h, "version")?;
            p.u64(&mut h, "time")?;
            Ok(h)
        })?;
        Ok(e)
    }

    /// A suppressed occurrence: its UTF-16 decimal key, stored as a length and code units.
    fn suppressed(&mut self) -> Result<Record> {
        let mut x = Record::new("suppressed");
        let len = self.u32(&mut x, "key_len")? as usize;
        self.u16(&mut x, "key")?;
        if len > 1 {
            self.u16s(&mut x, "key_more", len - 1)?;
        }
        self.u32(&mut x, "u32_1")?;
        self.u8(&mut x, "u8_2")?;
        self.u16(&mut x, "u16_3")?;
        Ok(x)
    }

    /// A component pattern: its dimensions and (1, index, index, occurrence id) cells.
    fn pattern(&mut self) -> Result<Record> {
        let mut x = Record::new("pattern");
        self.u32(&mut x, "id")?;
        self.u32s(&mut x, "dims", 2)?;
        self.u32s(&mut x, "u32a_1", 2)?;
        let n = self.count(&mut x, "count", 16)?;
        self.list(&mut x, "elements", n, |p| {
            let mut y = Record::new("element");
            p.u32s(&mut y, "cell", 4)?;
            Ok(y)
        })?;
        Ok(x)
    }

    /// The literal 0x27 before the component body (section version [1] > 0x14).
    pub(super) fn marker(&mut self) -> Result<Record> {
        let mut x = Record::new("marker");
        self.u16(&mut x, "body_marker")?;
        Ok(x)
    }

    /// `inv_files`, the referenced Inventor files (spec 10.6).
    pub(super) fn inv_files(&mut self) -> Result<Record> {
        let mut r = Record::new("inv_files");
        let n = self.count(&mut r, "count", 40)?;
        self.text16(&mut r, "caption")?;
        self.u32(&mut r, "u32_pad")?;
        self.list(&mut r, "files", n, Self::inv_file)?;
        if self.v(4) >= 0x02 {
            self.u8(&mut r, "u8_end")?;
        }
        Ok(r)
    }

    fn inv_file(&mut self) -> Result<Record> {
        let mut e = Record::new("inv_file");
        self.text16(&mut e, "path")?;
        self.i32(&mut e, "library_id")?;
        self.text16(&mut e, "library_name")?;
        self.u16(&mut e, "u16_0")?;
        self.text16(&mut e, "relative_path")?;
        let n = self.count(&mut e, "n_triples", 6)?;
        self.list(&mut e, "triples", n, |p| {
            let mut t = Record::new("triple");
            p.u16s(&mut t, "v", 3)?;
            Ok(t)
        })?;
        self.u16s(&mut e, "u16a_1", 2)?;
        self.guid(&mut e, "internal_name")?;
        self.guid(&mut e, "db_revision")?;
        self.u32(&mut e, "ref_id")?;
        self.u32(&mut e, "n_occurrences")?;
        self.u32(&mut e, "u32_version")?;
        self.u32(&mut e, "u32_flags")?;
        Ok(e)
    }

    /// `ole_files`, the referenced non-Inventor files (spec 10.6).
    pub(super) fn ole_files(&mut self) -> Result<Record> {
        let mut r = Record::new("ole_files");
        let n = self.count(&mut r, "count", 40)?;
        self.list(&mut r, "files", n, Self::ole_file)?;
        if self.v(15) >= 0x06 {
            self.u8(&mut r, "u8_end")?;
        }
        Ok(r)
    }

    fn ole_file(&mut self) -> Result<Record> {
        let mut e = Record::new("ole_file");
        self.u32(&mut e, "u32_0")?;
        self.u64(&mut e, "time")?;
        self.u32(&mut e, "u32_1")?;
        if self.v(15) >= 0x07 {
            self.u32(&mut e, "u32_2")?;
        }
        self.u32(&mut e, "ref_id")?;
        self.text16(&mut e, "path")?;
        self.i32(&mut e, "library_id")?;
        self.text16(&mut e, "library_name")?;
        self.u16(&mut e, "u16_3")?;
        self.text16(&mut e, "relative_path")?;
        self.u16s(&mut e, "u16a_4", 4)?;
        Ok(e)
    }
}
