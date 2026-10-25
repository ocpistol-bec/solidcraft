//! UFRxDoc component body: occurrences, notification fields, client interest, BOM settings
//! (spec 10.7–10.9).

use super::parser::Parser;
use super::record::Record;
use crate::error::Result;

/// Field ids of cached notification fields by value type (spec 10.8).
const FIELD_TEXT: [u8; 3] = [0x04, 0x05, 0x1E];
const FIELD_U8: [u8; 9] = [0x07, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0F, 0x10, 0x1D];
const FIELD_U32: [u8; 3] = [0x06, 0x19, 0x27];
/// Document fields (spec 10.8.1) that are GUID pairs; 0x11 is single before section [9] = 5.
const DOC_PAIR: [u8; 9] = [0x11, 0x13, 0x16, 0x17, 0x18, 0x1F, 0x24, 0x25, 0x2A];
/// Instance status bit LODInstance (Table 10-3).
const STATUS_LOD_INSTANCE: u64 = 0x2_0000_0000;
/// BOM setting kinds seen; neither carries kind-specific data.
const BOM_KINDS: [u8; 2] = [0, 1];

impl Parser<'_> {
    /// Reads a typed value whose type follows from the field id (spec 10.8).
    fn field_value(&mut self, rec: &mut Record, id: u8, name: &'static str) -> Result<()> {
        if FIELD_TEXT.contains(&id) {
            self.text16(rec, name)
        } else if FIELD_U8.contains(&id) {
            self.u8(rec, name).map(drop)
        } else if FIELD_U32.contains(&id) {
            self.u32(rec, name).map(drop)
        } else {
            self.guid(rec, name)
        }
    }

    /// A FieldSection: slot count, present count, then the present fields.
    pub(super) fn section(&mut self, kind: &'static str) -> Result<Record> {
        let mut r = Record::new(kind);
        self.u32(&mut r, "n_slots")?;
        let n = self.count(&mut r, "n_present", 10)?;
        self.list(&mut r, "fields", n, Self::field)?;
        Ok(r)
    }

    fn field(&mut self) -> Result<Record> {
        let mut e = Record::new("field");
        self.flag(&mut e, "cumulative")?;
        let id = self.u8(&mut e, "type")?;
        let n = self.count(&mut e, "n_values", 2)?;
        for k in 0..n {
            let at = self.r.position();
            let again = self.u8(&mut e, if k == 0 { "type2" } else { "type2_more" })?;
            if again != id {
                return Err(crate::CorlissError::invalid(
                    super::parser::STRUCTURE,
                    "type2",
                    at,
                    again,
                    id,
                ));
            }
            self.field_value(&mut e, id, if k == 0 { "value" } else { "value_more" })?;
        }
        self.u32(&mut e, "use_count")?;
        Ok(e)
    }

    /// A CachedSet: per set a key, the reactor and the propagator sections.
    pub(super) fn cached(&mut self) -> Result<Record> {
        let mut r = Record::new("cached");
        let n = self.count(&mut r, "n_sets", 20)?;
        self.list(&mut r, "sets", n, |p| {
            let mut s = Record::new("set");
            p.u32(&mut s, "key")?;
            let reactor = p.section("reactor")?;
            s.push("reactor", super::record::Value::Record(reactor));
            let propagator = p.section("propagator")?;
            s.push("propagator", super::record::Value::Record(propagator));
            Ok(s)
        })?;
        Ok(r)
    }

    fn nested(
        &mut self,
        rec: &mut Record,
        name: &'static str,
        read: fn(&mut Self) -> Result<Record>,
    ) -> Result<()> {
        let value = read(self)?;
        rec.push(name, super::record::Value::Record(value));
        Ok(())
    }

    /// `occurrences` with their occurrence records (spec 10.7).
    pub(super) fn occurrences(&mut self) -> Result<Record> {
        let mut r = Record::new("occurrences");
        let n = self.count(&mut r, "count", 24)?;
        self.list(&mut r, "entries", n, Self::occurrence)?;
        let groups = self.count(&mut r, "n_groups", 6)?;
        self.list(&mut r, "groups", groups, Self::group)?;
        Ok(r)
    }

    fn occurrence(&mut self) -> Result<Record> {
        let mut e = Record::new("occurrence");
        let ids = self.u32s(&mut e, "occ_ids", 5)?;
        self.text16(&mut e, "title")?;
        if self.v(3) < 0x14 {
            return self.occurrence_2010(e);
        }
        self.text8(&mut e, "segment_load_state")?;
        let status = self.u64(&mut e, "status")?;
        if status & STATUS_LOD_INSTANCE != 0 || ids[1] != 0 {
            self.nested(&mut e, "cached", Self::cached)?;
        }
        self.settings(&mut e)?;
        self.u32(&mut e, "n_refing_states")?;
        self.u32(&mut e, "interchange_set_id")?;
        self.u8(&mut e, "deferred")?;
        self.u8(&mut e, "disabled_type")?;
        self.i32(&mut e, "reserved_instance_9c")?;
        self.text16(&mut e, "prop_manager_id")?;
        self.persist(&mut e)?;
        Ok(e)
    }

    /// The 2010 occurrence tail (section version [3] < 0x14; spec 10.12, status Typed).
    fn occurrence_2010(&mut self, mut e: Record) -> Result<Record> {
        self.u32(&mut e, "u32_1")?;
        self.u8(&mut e, "u8_1")?;
        self.nested(&mut e, "sec1", |p| p.section("sec1"))?;
        self.nested(&mut e, "sec2", |p| p.section("sec2"))?;
        self.settings(&mut e)?;
        self.u16s(&mut e, "u16a_export", 5)?;
        Ok(e)
    }

    /// RefdStates: states of the referenced document that an occurrence uses.
    fn settings(&mut self, e: &mut Record) -> Result<()> {
        let n = self.count(e, "n_settings", 24)?;
        self.list(e, "settings", n, |p| {
            let mut v = Record::new("setting");
            p.text16(&mut v, "name")?;
            p.guid(&mut v, "id")?;
            p.text8(&mut v, "value")?;
            Ok(v)
        })
    }

    /// An occurrence record keyed by occurrence path (spec 10.7.1).
    fn group(&mut self) -> Result<Record> {
        let mut e = Record::new("group");
        let len = self.u32(&mut e, "key_len")? as usize;
        self.u16(&mut e, "id")?;
        if len > 1 {
            self.u16s(&mut e, "key_more", len - 1)?;
        }
        if self.v(3) < 0x14 {
            self.nested(&mut e, "sec", |p| p.section("sec"))?;
            self.u32s(&mut e, "u32a_end", 3)?;
            return Ok(e);
        }
        self.nested(&mut e, "cached", Self::cached)?;
        let n = self.count(&mut e, "n_path_keys", 4)?;
        self.list(&mut e, "path_keys", n, |p| {
            let mut x = Record::new("path_key");
            p.text16(&mut x, "key")?;
            Ok(x)
        })?;
        self.persist(&mut e)?;
        Ok(e)
    }

    /// `doc_props`, the document's own fields (spec 10.8.1).
    pub(super) fn doc_props(&mut self) -> Result<Record> {
        let mut r = Record::new("doc_props");
        self.u32(&mut r, "n_slots")?;
        let n = self.count(&mut r, "n_present", 2)?;
        self.list(&mut r, "props", n, Self::doc_prop)?;
        Ok(r)
    }

    fn doc_prop(&mut self) -> Result<Record> {
        let mut e = Record::new("dprop");
        let id = self.u8(&mut e, "type")?;
        let pair = DOC_PAIR.contains(&id) && (self.v(9) >= 0x05 || id != 0x11);
        if id == 0x04 {
            self.text16(&mut e, "value")?;
        } else if [0x0F, 0x10, 0x1D].contains(&id) {
            self.u8(&mut e, "value")?;
        } else {
            self.guid(&mut e, "value")?;
            if pair {
                self.guid(&mut e, "value2")?;
            }
        }
        Ok(e)
    }

    /// `id_list`: component flag bits and the uninterested clients (spec 10.9).
    pub(super) fn id_list(&mut self) -> Result<Record> {
        let mut r = Record::new("id_list");
        self.u8(&mut r, "reserved_component_bit8")?;
        self.u8(&mut r, "reserved_component_bit9")?;
        let n = self.count(&mut r, "count", 16)?;
        self.list(&mut r, "ids", n, |p| {
            let mut e = Record::new("id");
            p.guid(&mut e, "id")?;
            Ok(e)
        })?;
        Ok(r)
    }

    /// `id_records`: the interested clients (UFRxInterest).
    pub(super) fn id_records(&mut self) -> Result<Record> {
        let mut r = Record::new("id_records");
        let n = self.count(&mut r, "count", 31)?;
        self.list(&mut r, "records", n, |p| {
            let mut e = Record::new("id_record");
            p.guid(&mut e, "id")?;
            p.text16(&mut e, "interest_text")?;
            p.u32(&mut e, "client_version")?;
            p.u16(&mut e, "reserved_interest_54")?;
            p.u8(&mut e, "reserved_interest_58")?;
            p.text16(&mut e, "text")?;
            Ok(e)
        })?;
        Ok(r)
    }

    /// `post_office` of assemblies.
    pub(super) fn post_office(&mut self) -> Result<Record> {
        let mut r = Record::new("post_office");
        let n = self.count(&mut r, "n_forwarding", 8)?;
        self.list(&mut r, "forwarding", n, |p| {
            let mut e = Record::new("forwarding");
            p.text16(&mut e, "from_path")?;
            p.text16(&mut e, "to_path")?;
            Ok(e)
        })?;
        let promoted = self.u32(&mut r, "n_promoted")? as usize;
        if promoted > 0 {
            self.u32s(&mut r, "promoted", promoted)?;
        }
        self.persist(&mut r)?;
        Ok(r)
    }

    /// `component_tail`: BOM settings and component flags.
    pub(super) fn component_tail(&mut self) -> Result<Record> {
        let mut r = Record::new("component_tail");
        if self.release_year <= 2015 {
            self.u32(&mut r, "reserved_2015")?;
        }
        let n = self.count(&mut r, "n_bom_settings", 7)?;
        self.list(&mut r, "bom_settings", n, Self::bom_setting)?;
        self.u32(&mut r, "component_flags")?;
        self.persist(&mut r)?;
        Ok(r)
    }

    fn bom_setting(&mut self) -> Result<Record> {
        let mut e = Record::new("bom_setting");
        self.u32(&mut e, "setting_id")?;
        self.u8(&mut e, "reserved_bom_24")?;
        let at = self.r.position();
        let kind = self.u8(&mut e, "setting_kind")?;
        if !BOM_KINDS.contains(&kind) {
            return Err(self.unsupported(at, "setting_kind", &format!("BOM setting kind {kind}")));
        }
        self.persist(&mut e)?;
        Ok(e)
    }

    /// The 2010 `bom` block (status L; no sample since the re-split).
    pub(super) fn bom_2010(&mut self) -> Result<Record> {
        let mut r = Record::new("bom");
        if self.u8(&mut r, "has_bom")? != 0 {
            self.text16(&mut r, "text_0")?;
            self.u16(&mut r, "u16_1")?;
            self.u8(&mut r, "u8_2")?;
            self.u8(&mut r, "u8_3")?;
            self.u8(&mut r, "u8_4")?;
        }
        Ok(r)
    }

    /// The 2010 `misc3` block (status L).
    pub(super) fn misc3_2010(&mut self) -> Result<Record> {
        let mut r = Record::new("misc3");
        self.u8(&mut r, "u8_a")?;
        self.u8(&mut r, "u8_b")?;
        self.u8(&mut r, "u8_c")?;
        if self.is_assembly {
            self.u32s(&mut r, "u32a_assy", 2)?;
        }
        Ok(r)
    }
}
