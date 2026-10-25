//! MaskedMatrix4, the compressed 4 × 4 matrix of placements and sketch transforms (spec 4.5.1).

use crate::bytes::{ByteReader, ByteWriter};
use crate::error::Result;

/// Cells this close to 0, ±1 are rounded on read [R-08].
const ROUND_TOLERANCE: f64 = 1e-12;

/// A row-major 4 × 4 matrix; translation is cells 3, 7, 11 in centimetres.
///
/// Stored as `u16 d1, u16 d2` then one f64 per cell that neither mask encodes. For cell
/// j = col + 4·row, bits j of (d2, d1) give (0, 0) explicit f64, (0, 1) = 1, (1, 0) = 0, (1, 1) = −1.
///
/// ```
/// // XY Plane sketch transform: identity with no stored doubles.
/// let bytes = [0x21, 0x84, 0xDE, 0x7B];
/// let (m, used) = corliss::node::MaskedMatrix4::decode(&bytes).unwrap();
/// assert_eq!(used, 4);
/// assert_eq!(m, corliss::node::MaskedMatrix4::IDENTITY);
/// assert_eq!(m.encode(), bytes);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskedMatrix4(pub [f64; 16]);

impl MaskedMatrix4 {
    /// The identity matrix.
    pub const IDENTITY: MaskedMatrix4 = MaskedMatrix4([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);

    /// Decodes a matrix at the start of `data`; returns it with the bytes consumed.
    pub fn decode(data: &[u8]) -> Result<(Self, usize)> {
        let mut r = ByteReader::new(data, "MaskedMatrix4");
        let d1 = r.u16("d1")?;
        let d2 = r.u16("d2")?;
        let mut cells = [0.0; 16];
        for (j, cell) in cells.iter_mut().enumerate() {
            *cell = match (d2 >> j & 1, d1 >> j & 1) {
                (0, 1) => 1.0,
                (1, 0) => 0.0,
                (1, 1) => -1.0,
                _ => r.f64("cells")?,
            };
        }
        Ok((MaskedMatrix4(cells), r.position()))
    }

    /// Encodes the matrix; cells equal to 0, 1 or −1 go into the masks.
    pub fn encode(&self) -> Vec<u8> {
        let (mut d1, mut d2) = (0u16, 0u16);
        let mut explicit = Vec::new();
        for (j, cell) in self.0.iter().enumerate() {
            // Compare bits so that −0.0 stays an explicit cell and decodes back unchanged.
            match cell.to_bits() {
                bits if bits == 1.0f64.to_bits() => d1 |= 1 << j,
                bits if bits == 0.0f64.to_bits() => d2 |= 1 << j,
                bits if bits == (-1.0f64).to_bits() => {
                    d1 |= 1 << j;
                    d2 |= 1 << j;
                }
                _ => explicit.push(*cell),
            }
        }
        let mut w = ByteWriter::default();
        w.u16(d1).u16(d2);
        explicit.iter().for_each(|v| {
            w.bytes(&v.to_le_bytes());
        });
        w.into_bytes()
    }

    /// A copy with near-unit cells (±1 ± 1e-12, ±1e-12) snapped to −1, 0 and 1 [R-08].
    pub fn rounded(&self) -> Self {
        let mut cells = self.0;
        for cell in &mut cells {
            for target in [-1.0, 0.0, 1.0] {
                if (*cell - target).abs() < ROUND_TOLERANCE {
                    *cell = target;
                }
            }
        }
        MaskedMatrix4(cells)
    }

    /// The translation (cells 3, 7, 11) in centimetres.
    pub fn translation(&self) -> [f64; 3] {
        [self.0[3], self.0[7], self.0[11]]
    }
}
