//! Framed lists and their element walkers, as the reference grammar engine parses them.

use super::parser::scalar_value;
use super::token::{TYP_CHAR, Token};
use crate::grammar::interp::{Interp, Res};
use crate::grammar::value::Value;

/// Cap on element counts of lists whose elements may take no bytes, so garbage cannot hang a walk.
const MAX_ELEMENTS: usize = 1 << 24;

/// Element walker selected by list code and element type.
#[derive(Clone, Copy)]
pub(crate) enum Walk {
    Scalar(char),
    ScalarArray(char),
    Child,
    Cross,
    Text8,
    Text16,
    Font,
    Lightning,
    App1,
    App2,
    App3,
    App4,
    App5,
    App6,
    NtEntry,
    MtmList,
    List2Xref,
    Transformation,
    U16Color,
    TwoU32,
    Result1,
    Result2,
    Result3,
    Result4,
    Result5,
    ResultItem4,
    Nested(i64),
    ListXref,
    Map(MapWalk),
}

#[derive(Clone, Copy)]
pub(crate) enum MapWalk {
    Pair(Part, Part),
    MdlTxnMgr,
    KeyApp1,
    KeyMapApp1,
}

/// One part of a map element.
#[derive(Clone, Copy)]
pub(crate) enum Part {
    S(char),
    Child,
    Cross,
    Uuid,
    Text8,
    Text16,
    U32x2,
    F32x3,
    U32x7,
    List2Cross,
}

pub(crate) fn list2_walk(typ: i64) -> Option<Walk> {
    let w = match typ {
        0x19 => Walk::Child,
        0x1A => Walk::Cross,
        0x1B => Walk::Text8,
        0x1C => Walk::Text16,
        0x11..=0x18 => Walk::Scalar(scalar_code(typ - 0x11)),
        0x20..=0x27 => Walk::ScalarArray(scalar_code(typ - 0x20)),
        0x40 => Walk::Font,
        0x41 => Walk::App1,
        0x42 => Walk::Lightning,
        0x44 => Walk::App2,
        0x45 => Walk::App3,
        0x46 => Walk::App4,
        0x47 => Walk::App5,
        0x4E => Walk::App6,
        0x48 => Walk::NtEntry,
        0x4A => Walk::MtmList,
        0x4B => Walk::List2Xref,
        0x4C => Walk::Transformation,
        0x4D => Walk::U16Color,
        0x8001..=0x8006 => {
            Walk::Nested([0x22, 0x23, 0x24, 0x25, 0x26, 0x27][(typ - 0x8001) as usize])
        }
        0x8007 => Walk::Nested(0x40),
        0x8008 => Walk::ListXref,
        0x8009 => Walk::Nested(TYP_CHAR),
        _ => return None,
    };
    Some(w)
}

/// u8, s8, u16, s16, u32, s32, f32, f64 in the reference grammar engine's type-code order.
pub(crate) fn scalar_code(k: i64) -> char {
    ['B', 'b', 'H', 'h', 'I', 'i', 'f', 'd'][k.clamp(0, 7) as usize]
}

pub(crate) fn list4_walk(typ: i64) -> Option<Walk> {
    Some(match typ {
        0x19 => Walk::Child,
        0x1A => Walk::Cross,
        0x43 => Walk::ResultItem4,
        0x25 => Walk::ScalarArray('i'),
        0x1C => Walk::Text16,
        0x1B => Walk::Text8,
        0x15 => Walk::Scalar('I'),
        0x24 => Walk::ScalarArray('I'),
        0x49 => Walk::TwoU32,
        0x5001 => Walk::Result1,
        0x5002 => Walk::Result2,
        0x5003 => Walk::Result3,
        0x5004 => Walk::Result4,
        0x5005 => Walk::Result5,
        _ => return None,
    })
}

pub(crate) fn list3_walk(typ: i64) -> Option<Walk> {
    Some(match typ {
        0x15 => Walk::Scalar('I'),
        0x19 => Walk::Child,
        0x1A => Walk::Cross,
        _ => return None,
    })
}

pub(crate) fn map_walk(typ: i64) -> Option<Walk> {
    use Part::*;
    let pair = |a, b| Walk::Map(MapWalk::Pair(a, b));
    Some(match typ {
        0x7017 => pair(S('H'), S('H')),
        0x7018 => pair(S('H'), Cross),
        0x7001 => pair(S('I'), S('B')),
        0x7002 => pair(S('I'), S('I')),
        0x7003 => pair(S('I'), S('d')),
        0x7004 => pair(S('I'), Child),
        0x7005 => pair(S('I'), Cross),
        0x7006 => pair(Child, Child),
        0x700D => pair(Cross, Child),
        0x700E => pair(Cross, S('d')),
        0x7011 => pair(Cross, Cross),
        0x700F => pair(Cross, S('I')),
        0x7010 => pair(Cross, U32x2),
        0x7012 => pair(S('I'), List2Cross),
        0x7013 => pair(Cross, List2Cross),
        0x7014 => pair(Uuid, S('I')),
        0x7016 => pair(Uuid, Child),
        0x7015 => pair(Uuid, Cross),
        0x7007 => pair(Text8, Child),
        0x7008 => pair(Text8, Cross),
        0x7009 => pair(Text8, F32x3),
        0x700A => pair(Text16, Child),
        0x700B => pair(Text16, Cross),
        0x700C => pair(Text16, S('I')),
        0x7021 => pair(Text16, U32x7),
        0x6001 => Walk::Map(MapWalk::MdlTxnMgr),
        0x7019 => Walk::Map(MapWalk::KeyApp1),
        0x7020 => Walk::Map(MapWalk::KeyMapApp1),
        _ => return None,
    })
}

/// Walker context: the payload, the parse context and the element name.
pub(crate) struct El<'a> {
    pub data: &'a [u8],
    pub ctx: &'a Value,
    pub name: &'a Value,
}

impl Interp {
    /// `parse_list(buf, offset, code, typ, ctx, name, arraysize)` → (list token, next offset).
    #[allow(clippy::too_many_arguments)]
    pub fn parse_list(
        &mut self,
        data: &[u8],
        offset: usize,
        code: i64,
        typ: i64,
        ctx: &Value,
        name: &Value,
        arraysize: i64,
    ) -> Res<(Token, usize)> {
        self.need(data, offset, 8)?;
        let framing = data[offset..offset + 4].to_vec();
        let seen = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let z = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        if z != 0x3000 || i64::from(seen) != code {
            return self.grammar_error(format!(
                "bad list framing at 0x{offset:X}: code={seen:04X} z={z:04X} (want {code:04X}/3000)"
            ));
        }
        let mut i = offset + 4;
        let cnt = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        i += 4;
        let mut meta = None;
        if cnt > 0 {
            let w = if matches!(code, 2 | 6 | 7) { 8 } else { 4 };
            self.need(data, i, w)?;
            meta = Some(data[i..i + w].to_vec());
            i += w;
        }
        let (elements, next) = self.list_elements(data, i, code, typ, ctx, name, arraysize, cnt)?;
        let extra = self.new_dict(vec![
            (Value::str("framing_raw"), Value::bytes(&framing)),
            (
                Value::str("meta_raw"),
                meta.map_or(Value::None, |m| Value::bytes(&m)),
            ),
            (Value::str("code"), Value::Int(code)),
            (Value::str("typ"), Value::Int(typ)),
        ])?;
        let mut tok = Token::new("list", name.clone(), elements, &[], None);
        tok.extra = extra;
        Ok((tok, next))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn list_elements(
        &mut self,
        data: &[u8],
        mut i: usize,
        code: i64,
        typ: i64,
        ctx: &Value,
        name: &Value,
        arraysize: i64,
        cnt: usize,
    ) -> Res<(Value, usize)> {
        let (walk, size) = match code {
            6 | 7 => (map_walk(typ), 1),
            4 => (list4_walk(typ), 1),
            3 | 8 => (list3_walk(typ), 1),
            _ if typ == TYP_CHAR => return self.char_list(data, i, name, cnt),
            _ => (list2_walk(typ), arraysize),
        };
        let Some(walk) = walk else {
            return self.grammar_error(format!(
                "no element walker for typ 0x{typ:04X} (list code {code})"
            ));
        };
        if cnt > MAX_ELEMENTS && cnt > data.len() {
            return self.grammar_error(format!("list count {cnt} exceeds the payload"));
        }
        let el = El { data, ctx, name };
        let mut elements = Vec::with_capacity(cnt.min(4096));
        for _ in 0..cnt {
            let (toks, next) = self.walk(&el, walk, i, size)?;
            i = next;
            elements.push(self.new_list(toks));
        }
        Ok((self.new_list(elements), i))
    }

    pub(crate) fn char_list(
        &mut self,
        data: &[u8],
        i: usize,
        name: &Value,
        cnt: usize,
    ) -> Res<(Value, usize)> {
        self.need(data, i, cnt)?;
        let raw = &data[i..i + cnt];
        let value = match std::str::from_utf8(raw) {
            Ok(s) => Value::str(s),
            Err(_) => Value::bytes(raw),
        };
        let tok = self.new_token(Token::new("text8", name.clone(), value, raw, None));
        let element = self.new_list(vec![tok]);
        Ok((self.new_list(vec![element]), i + cnt))
    }

    pub(crate) fn tok(&mut self, t: Token) -> Value {
        self.new_token(t)
    }

    pub(crate) fn el_scalar(
        &mut self,
        el: &El<'_>,
        typ: char,
        i: usize,
        name: &Value,
    ) -> Res<(Value, usize)> {
        let (t, next) = self.read_scalar(el.data, i, typ, name)?;
        Ok((self.tok(t), next))
    }

    pub(crate) fn el_raw(
        &mut self,
        el: &El<'_>,
        i: usize,
        need: usize,
        take: usize,
    ) -> Res<(Value, usize)> {
        self.need(el.data, i, need)?;
        let end = (i + take).min(el.data.len());
        let t = Token::new("raw", el.name.clone(), Value::None, &el.data[i..end], None);
        Ok((self.tok(t), i + take))
    }

    pub(crate) fn el_ref(&mut self, el: &El<'_>, kind: i64, i: usize) -> Res<(Value, usize)> {
        let (t, next) = self.read_ref(el.data, i, kind, el.name)?;
        Ok((self.tok(t), next))
    }

    pub(crate) fn el_text(&mut self, el: &El<'_>, wide: bool, i: usize) -> Res<(Value, usize)> {
        let (t, next) = if wide {
            self.read_text16(el.data, i, el.name)?
        } else {
            self.read_text8(el.data, i, el.name)?
        };
        Ok((self.tok(t), next))
    }

    /// A token holding a tuple of `n` scalars of `typ`.
    pub(crate) fn el_tuple(
        &mut self,
        el: &El<'_>,
        kind: &str,
        typ: char,
        n: usize,
        i: usize,
        name: &Value,
    ) -> Res<(Value, usize)> {
        let w = super::parser::scalar_kind(typ).map_or(4, |(w, _)| w);
        self.need(el.data, i, w * n)?;
        let raw = &el.data[i..i + w * n];
        let v = Value::tuple(raw.chunks_exact(w).map(|c| scalar_value(typ, c)).collect());
        let t = Token::new(kind, name.clone(), v, raw, Some(typ));
        Ok((self.tok(t), i + w * n))
    }

    pub(crate) fn el_list(
        &mut self,
        el: &El<'_>,
        code: i64,
        typ: i64,
        i: usize,
    ) -> Res<(Value, usize)> {
        let (t, next) = self.parse_list(el.data, i, code, typ, el.ctx, el.name, 1)?;
        Ok((self.tok(t), next))
    }

    pub(crate) fn block(&mut self, el: &El<'_>) -> usize {
        self.ctx_block(el.ctx)
    }

    pub(crate) fn walk(
        &mut self,
        el: &El<'_>,
        walk: Walk,
        i: usize,
        size: i64,
    ) -> Res<(Vec<Value>, usize)> {
        let one = |r: (Value, usize)| (vec![r.0], r.1);
        Ok(match walk {
            Walk::Scalar(c) => one(self.el_scalar(el, c, i, el.name)?),
            Walk::ScalarArray(c) => {
                let kind = super::parser::scalar_kind(c).map_or("u32", |(_, k)| k);
                one(self.el_tuple(el, kind, c, size.max(0) as usize, i, el.name)?)
            }
            Walk::Child => one(self.el_ref(el, 2, i)?),
            Walk::Cross => one(self.el_ref(el, 1, i)?),
            Walk::Text8 => one(self.el_text(el, false, i)?),
            Walk::Text16 => one(self.el_text(el, true, i)?),
            Walk::Nested(typ) => one(self.el_list(el, 2, typ, i)?),
            Walk::Map(m) => self.walk_map(el, m, i)?,
            _ => self.walk_app(el, walk, i)?,
        })
    }
}
