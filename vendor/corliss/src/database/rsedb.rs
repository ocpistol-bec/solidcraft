//! `RSeStorage/V<n>/RSeDb`, the database header (spec 9.1).

use super::version::Version;
use crate::bytes::{ByteReader, ByteWriter};
use crate::error::{CorlissError, FailureClass, Result};
use crate::guid::Guid;

const STRUCTURE: &str = "RSeDb";
/// The oldest schema whose layout the specification covers (0x1D–0x1F share it).
const MIN_SCHEMA: u32 = 0x1D;

/// The database header.
///
/// ```no_run
/// let doc = corliss::Document::open("part.ipt")?;
/// let db = doc.rsedb()?;
/// println!("saved with Inventor {:?}", db.saved_with.release_year());
/// # Ok::<(), corliss::CorlissError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RseDb {
    /// Database id; equals Design Tracking Control pid 21.
    pub database_id: Guid,
    /// Schema, 0x1F in every sample.
    pub schema: u32,
    /// Version of the software that created the database.
    pub created_with: Version,
    /// Creation time (FILETIME).
    pub created: u64,
    /// Version of the last save; selects the block trailer (spec 12.2).
    pub saved_with: Version,
    /// Time of the last save (FILETIME).
    pub saved: u64,
    /// Empty, or the in-development lineage note. Writers keep it [W-06].
    pub lineage_note: String,
}

impl RseDb {
    /// Decodes the stream.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        let database_id = r.guid("database_id")?;
        let schema = r.u32("schema")?;
        if schema < MIN_SCHEMA {
            let class = FailureClass::Unsupported {
                reason: format!(
                    "schema {schema:#x}; the pre-2009 layouts start below {MIN_SCHEMA:#x}"
                ),
            };
            return Err(CorlissError::decode(STRUCTURE, "schema", 16, class));
        }
        let db = RseDb {
            database_id,
            schema,
            created_with: Version::read(&mut r, "created_with")?,
            created: r.u64("created")?,
            saved_with: Version::read(&mut r, "saved_with")?,
            saved: r.u64("saved")?,
            lineage_note: r.text16("lineage_note")?,
        };
        r.finish()?;
        Ok(db)
    }

    /// Encodes the stream.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::default();
        w.guid(&self.database_id).u32(self.schema);
        self.created_with.write(&mut w);
        w.u64(self.created);
        self.saved_with.write(&mut w);
        w.u64(self.saved).text16(&self.lineage_note);
        w.into_bytes()
    }

    /// True when blocks carry a trailer after the length echo: saved-with major > 18 (spec 12.1).
    pub fn has_block_trailers(&self) -> bool {
        self.saved_with.major() > 18
    }
}
