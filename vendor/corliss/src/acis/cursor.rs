//! A typed cursor over the data tokens of one record.

use super::token::{Token, latin1};
use crate::error::{CorlissError, Result};

const STRUCTURE: &str = "SAB record";

/// An interval bound pair; `None` is an unbounded end (spec 13.6.2 `interval`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Interval {
    /// Low bound.
    pub low: Option<f64>,
    /// High bound.
    pub high: Option<f64>,
}

/// Reads typed fields from a record's tokens, failing with the field name on a mismatch.
#[derive(Debug, Clone)]
pub(crate) struct Cursor<'a> {
    pub toks: &'a [Token],
    pub i: usize,
    /// Restore version V of the stream (spec 13.6.4).
    pub version: i64,
}

impl<'a> Cursor<'a> {
    pub fn new(toks: &'a [Token], version: i64) -> Self {
        Cursor {
            toks,
            i: 0,
            version,
        }
    }

    pub fn at(toks: &'a [Token], i: usize, version: i64) -> Self {
        Cursor { toks, i, version }
    }

    pub fn peek(&self) -> Option<&'a Token> {
        self.toks.get(self.i)
    }

    fn next(&mut self, field: &'static str, want: &str) -> Result<&'a Token> {
        let t = self
            .peek()
            .ok_or_else(|| self.mismatch(field, "end of record", want))?;
        self.i += 1;
        Ok(t)
    }

    pub fn mismatch(&self, field: &'static str, found: &str, want: &str) -> CorlissError {
        CorlissError::invalid(STRUCTURE, field, self.i, found, want)
    }

    fn wrong(&self, field: &'static str, t: &Token, want: &str) -> CorlissError {
        CorlissError::invalid(
            STRUCTURE,
            field,
            self.i.saturating_sub(1),
            format!("{t:?}"),
            want,
        )
    }

    pub fn ptr(&mut self, field: &'static str) -> Result<i32> {
        match self.next(field, "pointer")? {
            Token::Ptr(v) => Ok(*v),
            t => Err(self.wrong(field, t, "a pointer")),
        }
    }

    pub fn long(&mut self, field: &'static str) -> Result<i32> {
        match self.next(field, "long")? {
            Token::Long(v) => Ok(*v),
            t => Err(self.wrong(field, t, "a long")),
        }
    }

    pub fn double(&mut self, field: &'static str) -> Result<f64> {
        match self.next(field, "double")? {
            Token::Double(v) => Ok(*v),
            t => Err(self.wrong(field, t, "a double")),
        }
    }

    pub fn position(&mut self, field: &'static str) -> Result<[f64; 3]> {
        match self.next(field, "position")? {
            Token::Position(v) => Ok(*v),
            t => Err(self.wrong(field, t, "a position")),
        }
    }

    pub fn vector(&mut self, field: &'static str) -> Result<[f64; 3]> {
        match self.next(field, "vector")? {
            Token::Vector(v) => Ok(*v),
            t => Err(self.wrong(field, t, "a vector")),
        }
    }

    pub fn logical(&mut self, field: &'static str) -> Result<bool> {
        match self.next(field, "logical")? {
            Token::True => Ok(true),
            Token::False => Ok(false),
            t => Err(self.wrong(field, t, "true or false")),
        }
    }

    pub fn enumeration(&mut self, field: &'static str) -> Result<i32> {
        match self.next(field, "enum")? {
            Token::Enum(v) => Ok(*v),
            t => Err(self.wrong(field, t, "an enum")),
        }
    }

    pub fn string(&mut self, field: &'static str) -> Result<String> {
        match self.next(field, "string")? {
            Token::Str(b) => Ok(latin1(b)),
            t => Err(self.wrong(field, t, "a string")),
        }
    }

    pub fn ident(&mut self, field: &'static str) -> Result<&'a str> {
        match self.next(field, "identifier")? {
            Token::Ident(s) => Ok(s),
            t => Err(self.wrong(field, t, "an identifier")),
        }
    }

    pub fn expect(&mut self, field: &'static str, want: &Token) -> Result<()> {
        let t = self.next(field, "token")?;
        if t != want {
            return Err(self.wrong(field, t, &format!("{want:?}")));
        }
        Ok(())
    }

    /// An interval: each end is `false`, or `true` then a double.
    pub fn interval(&mut self, field: &'static str) -> Result<Interval> {
        let low = self.bound(field)?;
        let high = self.bound(field)?;
        Ok(Interval { low, high })
    }

    fn bound(&mut self, field: &'static str) -> Result<Option<f64>> {
        if self.logical(field)? {
            Ok(Some(self.double(field)?))
        } else {
            Ok(None)
        }
    }

    /// The ENTITY part (spec 13.6.5): attribute pointer, entity id, and the pattern pointer of
    /// patternable classes. Returns the entity id (`None` for −1).
    pub fn entity(&mut self, pattern: bool) -> Result<Option<i32>> {
        self.ptr("attrib")?;
        let id = if self.version >= 70_000 {
            self.long("entity_id")?
        } else {
            -1
        };
        if pattern && self.version >= 70_000 {
            self.ptr("pattern")?;
        }
        Ok((id >= 0).then_some(id))
    }

    /// Index of the `SubtypeEnd` that closes the subtype the cursor is inside.
    pub fn close_of_subtype(&self) -> usize {
        let mut depth = 0usize;
        for (k, t) in self.toks.iter().enumerate().skip(self.i) {
            match t {
                Token::SubtypeStart => depth += 1,
                Token::SubtypeEnd if depth == 0 => return k,
                Token::SubtypeEnd => depth -= 1,
                _ => {}
            }
        }
        self.toks.len()
    }
}
