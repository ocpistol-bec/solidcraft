//! Version stamps: the 8-byte Version and the UTxVersionInfo words (spec 4.5, 9.3.3).

use crate::bytes::{ByteReader, ByteWriter};
use crate::error::Result;

/// The 8-byte Version of RSeDb and the segment directory: revision, minor, major, then five
/// bytes of unknown use (OI-002) kept as read.
///
/// ```
/// let v = corliss::database::Version::from_bytes([0, 0, 31, 0x40, 6, 0, 1, 0x4b]);
/// assert_eq!(v.major(), 31);
/// assert_eq!(v.release_year(), Some(2027));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Version([u8; 8]);

impl Version {
    /// Wraps the stored bytes.
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Version(bytes)
    }

    /// The stored bytes.
    pub const fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Revision byte.
    pub fn revision(&self) -> u8 {
        self.0[0]
    }

    /// Minor release byte.
    pub fn minor(&self) -> u8 {
        self.0[1]
    }

    /// Major release byte (31 = Inventor 2027).
    pub fn major(&self) -> u8 {
        self.0[2]
    }

    /// Release year for majors above 11 (major + 1996); `None` for older majors.
    pub fn release_year(&self) -> Option<u16> {
        (self.major() > 11).then(|| u16::from(self.major()) + 1996)
    }

    pub(crate) fn read(r: &mut ByteReader<'_>, field: &'static str) -> Result<Self> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(r.take(8, field)?);
        Ok(Version(bytes))
    }

    pub(crate) fn write(&self, w: &mut ByteWriter) {
        w.bytes(&self.0);
    }
}

/// Two u32 UTxVersionInfo words read as four u16 `w0…w3` (spec 9.3.3).
///
/// ```
/// let v = corliss::database::VersionWords([0, 16415, 0, 15500]);
/// assert_eq!(v.major_release(), 31);
/// assert_eq!(v.build(), 155);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VersionWords(pub [u16; 4]);

impl VersionWords {
    /// `MajorRelease` = w1 & 0x3FFF.
    pub fn major_release(&self) -> u16 {
        self.0[1] & 0x3FFF
    }

    /// `MinorRelease` = w0 / 10.
    pub fn minor_release(&self) -> u16 {
        self.0[0] / 10
    }

    /// `ServicePack` = w0 % 10.
    pub fn service_pack(&self) -> u16 {
        self.0[0] % 10
    }

    /// `Intent` = w2 / 1000 (0 production, 6–9 internal builds).
    pub fn intent(&self) -> u16 {
        self.0[2] / 1000
    }

    /// `RebuildNumber` = w2 % 10.
    pub fn rebuild(&self) -> u16 {
        self.0[2] % 10
    }

    /// Build number = w3 / 100.
    pub fn build(&self) -> u16 {
        self.0[3] / 100
    }

    /// `ContinuousBuildNumber` = w3 % 100.
    pub fn continuous_build(&self) -> u16 {
        self.0[3] % 100
    }

    pub(crate) fn read(r: &mut ByteReader<'_>, field: &'static str) -> Result<Self> {
        Ok(VersionWords([
            r.u16(field)?,
            r.u16(field)?,
            r.u16(field)?,
            r.u16(field)?,
        ]))
    }

    pub(crate) fn write(&self, w: &mut ByteWriter) {
        for word in self.0 {
            w.u16(word);
        }
    }
}
