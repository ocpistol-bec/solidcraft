//! `RSeStorage/RSeDbRevisionInfo`, the revision history (spec 9.3).

use super::version::VersionWords;
use crate::bytes::{ByteReader, ByteWriter};
use crate::error::{CorlissError, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "RSeDbRevisionInfo";
/// `type` value of a version record.
const VERSION_RECORD: u16 = 0xFFFF;

/// What follows the id and flags of a revision record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevisionKind {
    /// A save record; the index of the version record of the software that wrote it.
    Save {
        /// Index of an earlier version record in the same stream.
        version_record: u16,
    },
    /// A version record (`type` 0xFFFF): one version, or the software then the schema version.
    Versions(Vec<VersionWords>),
    /// The version-2 layout: eight u16 of unknown meaning (InventorLoader, status L).
    Legacy([u16; 8]),
}

/// One revision record (spec 9.3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionRecord {
    /// Revision GUID.
    pub id: Guid,
    /// Dirty-reason mask of the save (spec 9.4).
    pub flags: u32,
    /// Save or version record.
    pub kind: RevisionKind,
}

/// The whole revision history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionInfo {
    /// Stream layout version: 3, or 2 in older files.
    pub version: i32,
    /// Records in stream order; each save appends one.
    pub records: Vec<RevisionRecord>,
}

impl RevisionInfo {
    /// Decodes the stream.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        let version = r.i32("version")?;
        if version != 2 && version != 3 {
            return Err(CorlissError::invalid(
                STRUCTURE, "version", 0, version, "2 or 3",
            ));
        }
        let count = r.count("count", 22)?;
        let records = (0..count)
            .map(|_| read_record(&mut r, version))
            .collect::<Result<Vec<_>>>()?;
        r.finish()?;
        Ok(RevisionInfo { version, records })
    }

    /// Encodes the stream.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        w.i32(self.version).len_u32(self.records.len());
        for record in &self.records {
            write_record(&mut w, record);
        }
        w.into_bytes()
    }

    /// The newest revision record, which names the revision of the last save.
    pub fn latest(&self) -> Option<&RevisionRecord> {
        self.records.last()
    }
}

fn read_record(r: &mut ByteReader<'_>, version: i32) -> Result<RevisionRecord> {
    let id = r.guid("id")?;
    let flags = r.u32("flags")?;
    let kind = if version == 2 {
        read_legacy(r)?
    } else {
        read_kind(r)?
    };
    Ok(RevisionRecord { id, flags, kind })
}

fn read_legacy(r: &mut ByteReader<'_>) -> Result<RevisionKind> {
    let mut words = [0u16; 8];
    for word in &mut words {
        *word = r.u16("legacy")?;
    }
    Ok(RevisionKind::Legacy(words))
}

fn read_kind(r: &mut ByteReader<'_>) -> Result<RevisionKind> {
    let kind = r.u16("type")?;
    if kind != VERSION_RECORD {
        return Ok(RevisionKind::Save {
            version_record: kind,
        });
    }
    let at = r.position();
    let count = match r.u8("b")? {
        1 => 1,
        0 => 2,
        other => return Err(CorlissError::invalid(STRUCTURE, "b", at, other, "0 or 1")),
    };
    let versions = (0..count)
        .map(|_| VersionWords::read(r, "versions"))
        .collect::<Result<Vec<_>>>()?;
    Ok(RevisionKind::Versions(versions))
}

fn write_record(w: &mut ByteWriter, record: &RevisionRecord) {
    w.guid(&record.id).u32(record.flags);
    match &record.kind {
        RevisionKind::Save { version_record } => {
            w.u16(*version_record);
        }
        RevisionKind::Versions(versions) => {
            w.u16(VERSION_RECORD).u8(u8::from(versions.len() == 1));
            versions.iter().for_each(|v| v.write(w));
        }
        RevisionKind::Legacy(words) => {
            words.iter().for_each(|word| {
                w.u16(*word);
            });
        }
    }
}
