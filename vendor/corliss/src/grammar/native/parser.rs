//! The parser: the object grammars drive over a node payload.

use std::sync::Arc;

use super::token::Token;
use crate::grammar::interp::{Flow, HeapObj, Interp, Res};
use crate::grammar::value::{HeapId, Value};

/// Parser state.
#[derive(Debug)]
pub(crate) struct ParserState {
    pub data: Arc<[u8]>,
    pub i: i64,
    pub toks: HeapId,
    pub ctx: Value,
    pub partial: bool,
}

impl ParserState {
    pub fn field(&self, name: &str) -> Option<Value> {
        Some(match name {
            "data" => Value::Bytes(self.data.clone()),
            "i" => Value::Int(self.i),
            "toks" => Value::Heap(self.toks),
            "ctx" => self.ctx.clone(),
            "partial" => Value::Bool(self.partial),
            _ => return None,
        })
    }

    pub fn set_field(&mut self, name: &str, v: Value) -> Res<()> {
        match (name, v) {
            ("i", Value::Int(i)) => self.i = i,
            ("toks", Value::Heap(id)) => self.toks = id,
            ("partial", v) => self.partial = matches!(v, Value::Bool(true)),
            ("ctx", v) => self.ctx = v,
            (other, _) => {
                return Err(Flow::Fatal(format!(
                    "cannot set Parser.{other} (2010 layouts are not supported)"
                )));
            }
        }
        Ok(())
    }
}

/// Byte width and token kind of a struct scalar code.
pub(crate) fn scalar_kind(typ: char) -> Option<(usize, &'static str)> {
    Some(match typ {
        'B' => (1, "u8"),
        'b' => (1, "s8"),
        'H' => (2, "u16"),
        'h' => (2, "s16"),
        'I' => (4, "u32"),
        'i' => (4, "s32"),
        'f' => (4, "f32"),
        'd' => (8, "f64"),
        _ => return None,
    })
}

/// Decodes one little-endian scalar.
pub(crate) fn scalar_value(typ: char, b: &[u8]) -> Value {
    let a = |n: usize| -> [u8; 8] {
        let mut out = [0u8; 8];
        out[..n].copy_from_slice(&b[..n]);
        out
    };
    match typ {
        'B' => Value::Int(i64::from(b[0])),
        'b' => Value::Int(i64::from(b[0] as i8)),
        'H' => Value::Int(i64::from(u16::from_le_bytes([b[0], b[1]]))),
        'h' => Value::Int(i64::from(i16::from_le_bytes([b[0], b[1]]))),
        'I' => Value::Int(i64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        'i' => Value::Int(i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        'f' => Value::Float(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        'd' => Value::Float(f64::from_le_bytes(a(8))),
        _ => Value::None,
    }
}

/// Strips one trailing NUL, then one trailing newline, as the reference grammar engine's text
/// readers do.
pub(crate) fn strip_text(mut s: String) -> String {
    if s.ends_with('\0') {
        s.pop();
    }
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

impl Interp {
    pub fn parser(&self, id: HeapId) -> Res<&ParserState> {
        match self.obj(id) {
            HeapObj::Parser(p) => Ok(p),
            _ => Err(Flow::Fatal("heap object is not a Parser".into())),
        }
    }

    pub(crate) fn parser_mut(&mut self, id: HeapId) -> Res<&mut ParserState> {
        match self.obj_mut(id) {
            HeapObj::Parser(p) => Ok(p),
            _ => Err(Flow::Fatal("heap object is not a Parser".into())),
        }
    }

    /// A new parser over `data` with version context `ctx`.
    pub fn new_parser(&mut self, data: Arc<[u8]>, ctx: Value) -> Value {
        let toks = match self.new_list(Vec::new()) {
            Value::Heap(id) => id,
            _ => 0,
        };
        self.alloc(HeapObj::Parser(Box::new(ParserState {
            data,
            i: 0,
            toks,
            ctx,
            partial: false,
        })))
    }

    /// Fails with GrammarError unless `n` bytes are left at `i`.
    pub fn need(&mut self, data: &[u8], i: usize, n: usize) -> Res<()> {
        if i + n > data.len() {
            let have = data.len() as i64 - i as i64;
            return self.grammar_error(format!("short read at 0x{i:X} (need {n}, have {have})"));
        }
        Ok(())
    }

    /// `read_scalar`.
    pub fn read_scalar(
        &mut self,
        data: &[u8],
        i: usize,
        typ: char,
        name: &Value,
    ) -> Res<(Token, usize)> {
        let Some((n, kind)) = scalar_kind(typ) else {
            return Err(Flow::Fatal(format!("unknown scalar type {typ}")));
        };
        self.need(data, i, n)?;
        let raw = &data[i..i + n];
        Ok((
            Token::new(kind, name.clone(), scalar_value(typ, raw), raw, Some(typ)),
            i + n,
        ))
    }

    pub fn read_text8(&mut self, data: &[u8], i: usize, name: &Value) -> Res<(Token, usize)> {
        self.need(data, i, 4)?;
        let l = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        self.need(data, i + 4, l)?;
        let raw = &data[i..i + 4 + l];
        let value = text8_value(&data[i + 4..i + 4 + l]);
        Ok((
            Token::new("text8", name.clone(), value, raw, None),
            i + 4 + l,
        ))
    }

    pub fn read_text16(&mut self, data: &[u8], i: usize, name: &Value) -> Res<(Token, usize)> {
        self.need(data, i, 4)?;
        let l = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        self.need(data, i + 4, 2 * l)?;
        let units: Vec<u16> = data[i + 4..i + 4 + 2 * l]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let Ok(text) = String::from_utf16(&units) else {
            return self.raise(
                "builtins.UnicodeDecodeError",
                format!("invalid UTF-16 at 0x{i:X}"),
            );
        };
        let raw = &data[i..i + 4 + 2 * l];
        Ok((
            Token::new(
                "text16",
                name.clone(),
                Value::str(&strip_text(text)),
                raw,
                None,
            ),
            i + 4 + 2 * l,
        ))
    }

    pub fn read_uuid(&mut self, data: &[u8], i: usize, name: &Value) -> Res<(Token, usize)> {
        self.need(data, i, 16)?;
        let raw = &data[i..i + 16];
        Ok((
            Token::new("uuid", name.clone(), Value::bytes(raw), raw, None),
            i + 16,
        ))
    }

    pub fn read_ref(
        &mut self,
        data: &[u8],
        i: usize,
        kind: i64,
        name: &Value,
    ) -> Res<(Token, usize)> {
        let (mut t, i) = self.read_scalar(data, i, 'I', name)?;
        t.kind = Arc::from("ref");
        t.extra = Value::Int(kind);
        Ok((t, i))
    }

    pub fn read_bool(&mut self, data: &[u8], i: usize, name: &Value) -> Res<(Token, usize)> {
        self.need(data, i, 1)?;
        let v = data[i];
        if v > 1 {
            return self.grammar_error(format!("bad boolean 0x{v:02X} at 0x{i:X}"));
        }
        Ok((
            Token::new(
                "bool",
                name.clone(),
                Value::Bool(v == 1),
                &data[i..i + 1],
                Some('B'),
            ),
            i + 1,
        ))
    }

    /// Appends a token to the parser and returns its value.
    pub(crate) fn push(
        &mut self,
        pid: HeapId,
        mut t: Token,
        off: usize,
        next: usize,
    ) -> Res<Value> {
        t.off = Value::Int(off as i64);
        let v = t.value.clone();
        self.parser_mut(pid)?.i = next as i64;
        let tok = self.new_token(t);
        let list = self.parser(pid)?.toks;
        if let HeapObj::List(items) = self.obj_mut(list) {
            items.push(tok);
        }
        Ok(v)
    }

    /// Appends a token without setting its offset (the reference grammar engine's color, ref,
    /// raw and list paths).
    pub fn push_plain(&mut self, pid: HeapId, t: Token, next: usize) -> Res<Value> {
        let v = t.value.clone();
        self.parser_mut(pid)?.i = next as i64;
        let tok = self.new_token(t);
        let list = self.parser(pid)?.toks;
        if let HeapObj::List(items) = self.obj_mut(list) {
            items.push(tok);
        }
        Ok(v)
    }

    pub(crate) fn cursor(&self, pid: HeapId) -> Res<(Arc<[u8]>, usize)> {
        let p = self.parser(pid)?;
        Ok((p.data.clone(), p.i.max(0) as usize))
    }

    pub fn ctx_int(&mut self, ctx: &Value, name: &str, default: i64) -> i64 {
        match ctx {
            Value::Heap(id) => match self.obj(*id) {
                HeapObj::Attrs(m) => m.get(name).and_then(Value::as_int).unwrap_or(default),
                _ => default,
            },
            _ => default,
        }
    }

    /// `ctx.block`: 4 bytes of padding in files from 2010 and earlier (spec 4.7).
    pub fn ctx_block(&mut self, ctx: &Value) -> usize {
        if self.ctx_int(ctx, "file", 2027) > 2010 {
            0
        } else {
            4
        }
    }

    pub(crate) fn one_scalar(&mut self, pid: HeapId, typ: char, name: &Value) -> Res<Value> {
        let (data, i) = self.cursor(pid)?;
        let (t, next) = self.read_scalar(&data, i, typ, name)?;
        Ok(self.push(pid, t, i, next))?
    }

    pub(crate) fn one_text(&mut self, pid: HeapId, wide: bool, name: &Value) -> Res<Value> {
        let (data, i) = self.cursor(pid)?;
        let (t, next) = if wide {
            self.read_text16(&data, i, name)?
        } else {
            self.read_text8(&data, i, name)?
        };
        Ok(self.push(pid, t, i, next))?
    }

    pub(crate) fn one_raw(&mut self, pid: HeapId, name: &Value, n: usize) -> Res<Value> {
        let (data, i) = self.cursor(pid)?;
        self.need(&data, i, n)?;
        let raw = &data[i..i + n];
        let t = Token::new("raw", name.clone(), Value::None, raw, None);
        self.push_plain(pid, t, i + n)?;
        Ok(Value::bytes(raw))
    }

    pub(crate) fn one_ref(&mut self, pid: HeapId, kind: i64, name: &Value) -> Res<Value> {
        let (data, i) = self.cursor(pid)?;
        let (t, next) = self.read_ref(&data, i, kind, name)?;
        let v = self.push_plain(pid, t, next)?;
        Ok(if v.as_int() == Some(0) {
            Value::None
        } else {
            v
        })
    }

    pub(crate) fn array(&mut self, pid: HeapId, typ: char, n: usize, name: &Value) -> Res<Value> {
        let Some((w, kind)) = scalar_kind(typ) else {
            return Err(Flow::Fatal(format!("unknown array type {typ}")));
        };
        let (data, i) = self.cursor(pid)?;
        self.need(&data, i, w * n)?;
        let raw = &data[i..i + w * n];
        let values: Vec<Value> = raw.chunks_exact(w).map(|c| scalar_value(typ, c)).collect();
        let v = Value::tuple(values);
        let t = Token::new(kind, name.clone(), v.clone(), raw, Some(typ));
        self.push_plain(pid, t, i + w * n)?;
        Ok(v)
    }
}

/// A text8 value: UTF-8 text with the trailing NUL / newline stripped, else the bytes.
pub(crate) fn text8_value(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => Value::str(&strip_text(s.to_string())),
        Err(_) => Value::bytes(b),
    }
}
