//! SAB tokens: one tag byte, then a payload fixed by the tag (spec 13.6.2, Table 13-6).

use crate::bytes::ByteReader;
use crate::error::{CorlissError, Result};

const STRUCTURE: &str = "SAB token";

/// One token of a SAB stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// 0x02 char.
    Char(u8),
    /// 0x03 short.
    Short(i16),
    /// 0x04 long.
    Long(i32),
    /// 0x05 float.
    Float(f32),
    /// 0x06 double.
    Double(f64),
    /// 0x07, 0x08, 0x09 strings and 0x12 literal strings (cp1252 bytes).
    Str(Vec<u8>),
    /// 0x0A true.
    True,
    /// 0x0B false.
    False,
    /// 0x0C pointer: a record index, −1 = null.
    Ptr(i32),
    /// 0x0D identifier (the last of a chain).
    Ident(String),
    /// 0x0E identifier followed by more.
    SubIdent(String),
    /// 0x0F subtype start.
    SubtypeStart,
    /// 0x10 subtype end.
    SubtypeEnd,
    /// 0x11 end of record.
    Terminator,
    /// 0x13 position.
    Position([f64; 3]),
    /// 0x14 vector.
    Vector([f64; 3]),
    /// 0x15 enum.
    Enum(i32),
    /// 0x16 parameter-space position.
    ParPos([f64; 2]),
    /// 0x17 64-bit integer.
    Int64(i64),
}

impl Token {
    /// Reads one token.
    pub(crate) fn read(r: &mut ByteReader<'_>) -> Result<Token> {
        let at = r.position();
        let tag = r.u8("tag")?;
        Ok(match tag {
            0x02 => Token::Char(r.u8("char")?),
            0x03 => Token::Short(r.i16("short")?),
            0x04 => Token::Long(r.i32("long")?),
            0x05 => Token::Float(f32::from_bits(r.u32("float")?)),
            0x06 => Token::Double(r.f64("double")?),
            0x07 => Token::Str(read_counted(r, 1)?),
            0x08 => Token::Str(read_counted(r, 2)?),
            0x09 | 0x12 => Token::Str(read_counted(r, 4)?),
            0x0A => Token::True,
            0x0B => Token::False,
            0x0C => Token::Ptr(r.i32("pointer")?),
            0x0D => Token::Ident(latin1(&read_counted(r, 1)?)),
            0x0E => Token::SubIdent(latin1(&read_counted(r, 1)?)),
            0x0F => Token::SubtypeStart,
            0x10 => Token::SubtypeEnd,
            0x11 => Token::Terminator,
            0x13 => Token::Position(read_triple(r)?),
            0x14 => Token::Vector(read_triple(r)?),
            0x15 => Token::Enum(r.i32("enum")?),
            0x16 => Token::ParPos([r.f64("par_pos")?, r.f64("par_pos")?]),
            0x17 => Token::Int64(r.i64("int64")?),
            other => {
                return Err(CorlissError::invalid(
                    STRUCTURE,
                    "tag",
                    at,
                    format!("{other:#04x}"),
                    "a SAB tag 0x02-0x17",
                ));
            }
        })
    }

    /// The identifier text of an `Ident` or `SubIdent`.
    pub fn ident(&self) -> Option<&str> {
        match self {
            Token::Ident(s) | Token::SubIdent(s) => Some(s),
            _ => None,
        }
    }
}

/// Reads a length of `width` bytes (1, 2 or 4), then that many bytes.
fn read_counted(r: &mut ByteReader<'_>, width: usize) -> Result<Vec<u8>> {
    let n = match width {
        1 => usize::from(r.u8("length")?),
        2 => usize::from(r.u16("length")?),
        _ => r.u32("length")? as usize,
    };
    Ok(r.take(n, "string")?.to_vec())
}

fn read_triple(r: &mut ByteReader<'_>) -> Result<[f64; 3]> {
    Ok([r.f64("x")?, r.f64("y")?, r.f64("z")?])
}

/// Bytes as cp1252/latin-1 text (identifiers are ASCII).
pub(crate) fn latin1(b: &[u8]) -> String {
    b.iter().map(|c| char::from(*c)).collect()
}
