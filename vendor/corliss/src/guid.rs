//! GUIDs and TypeIDs (spec 4.3, Table 4-1).

use std::fmt;

/// A GUID stored in Windows byte layout ([MS-DTYP]).
///
/// It prints in lower case the way Python's `uuid.UUID(bytes_le=…)` does, which is how the
/// specification writes GUIDs.
///
/// ```
/// let g = corliss::Guid::parse("4d29b490-49b2-11d0-93c3-7e0706000000").unwrap();
/// assert_eq!(g.to_string(), "4d29b490-49b2-11d0-93c3-7e0706000000");
/// assert_eq!(g.type_id().to_string(), "4D29B490");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Guid([u8; 16]);

impl Guid {
    /// The all-zero GUID.
    pub const NIL: Guid = Guid([0; 16]);

    /// Wraps 16 bytes in Windows GUID layout.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Guid(bytes)
    }

    /// The 16 bytes in Windows GUID layout.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The first u32 of the GUID: the TypeID of a node type (spec 3.1).
    pub fn type_id(&self) -> TypeId {
        TypeId(u32::from_le_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3],
        ]))
    }

    /// The four u32 words of the GUID in stored order, as the stream-name encoding reads them.
    pub fn words(&self) -> [u32; 4] {
        let w =
            |k: usize| u32::from_le_bytes([self.0[k], self.0[k + 1], self.0[k + 2], self.0[k + 3]]);
        [w(0), w(4), w(8), w(12)]
    }

    /// Parses the text form `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`; returns `None` on any other shape.
    pub fn parse(text: &str) -> Option<Self> {
        let hex: String = text.chars().filter(|c| *c != '-').collect();
        if hex.len() != 32 || text.len() != 36 {
            return None;
        }
        let mut be = [0u8; 16];
        for (k, byte) in be.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(2 * k..2 * k + 2)?, 16).ok()?;
        }
        Some(Guid::from_text_order(be))
    }

    /// Converts the big-endian text order to the Windows layout (first three groups swapped).
    fn from_text_order(be: [u8; 16]) -> Self {
        let mut le = be;
        le[0..4].reverse();
        le[4..6].reverse();
        le[6..8].reverse();
        Guid(le)
    }

    fn text_order(&self) -> [u8; 16] {
        Guid::from_text_order(self.0).0
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.text_order();
        for (k, byte) in b.iter().enumerate() {
            if matches!(k, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Guid({self})")
    }
}

/// The first u32 of a node type GUID, printed as 8 upper-case hex digits (`90874D91`).
///
/// ```
/// assert_eq!(corliss::TypeId(0x90874D91).to_string(), "90874D91");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeId(pub u32);

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}", self.0)
    }
}

impl fmt::Debug for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({self})")
    }
}
