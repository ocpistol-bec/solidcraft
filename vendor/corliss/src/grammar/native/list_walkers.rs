//! Element walkers of the composite list element types (fonts, lights, App and result records).

use super::lists::{El, MapWalk, Part, Walk};
use crate::grammar::interp::{Interp, Res};
use crate::grammar::value::Value;

impl Interp {
    pub(crate) fn walk_app(
        &mut self,
        el: &El<'_>,
        walk: Walk,
        i: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let block = self.block(el);
        let file = self.ctx_int(el.ctx, "file", 2027);
        let major = self.ctx_int(el.ctx, "major", 27);
        let one = |r: (Value, usize)| (vec![r.0], r.1);
        Ok(match walk {
            Walk::Font => self.walk_font(el, i, 2 * block + usize::from(major >= 25))?,
            Walk::Lightning => self.walk_lightning(el, i, file)?,
            Walk::App1 => one(self.el_raw(
                el,
                i,
                if file > 2023 { 25 } else { 24 },
                if file > 2023 { 25 } else { 24 },
            )?),
            Walk::App2 => self.walk_app2(el, i, block)?,
            Walk::App3 => self.walk_app3(el, i, block)?,
            Walk::App4 => self.walk_app4(el, i, if major >= 25 { 12 } else { 11 } + 2 * block)?,
            Walk::App5 => one(self.el_raw(el, i, 24 + 2 * block, 24 + 2 * block)?),
            Walk::App6 => one(self.el_raw(el, i, 25 + 2 * block, 25 + 2 * block)?),
            Walk::NtEntry | Walk::TwoU32 => self.seq(el, i, &[Part::S('I'), Part::S('I')])?,
            Walk::MtmList => {
                let (mut toks, i) = self.seq(el, i, &[Part::S('I'), Part::S('B'), Part::S('I')])?;
                let (raw, i) = self.el_raw(
                    el,
                    i,
                    block + usize::from(file > 2018),
                    block + usize::from(file > 2018),
                )?;
                toks.push(raw);
                (toks, i)
            }
            Walk::List2Xref => {
                let (lst, i) = self.el_list(el, 2, 0x8008, i)?;
                let (r, i) = self.el_ref(el, 1, i)?;
                (vec![lst, r], i)
            }
            Walk::Transformation => self.walk_transformation(el, i)?,
            Walk::ListXref => {
                let (a, i) = self.el_raw(el, i, block, block)?;
                let (lst, i) = self.el_list(el, 2, 0x15, i)?;
                let (r, i) = self.el_ref(el, 1, i)?;
                let (b, i) = self.el_raw(el, i, block, block)?;
                (vec![a, lst, r, b], i)
            }
            Walk::U16Color => self.walk_u16_color(el, i, block)?,
            _ => self.walk_result(el, walk, i, block, file)?,
        })
    }

    /// Reads parts in order, each as its own token.
    pub(crate) fn seq(
        &mut self,
        el: &El<'_>,
        mut i: usize,
        parts: &[Part],
    ) -> Res<(Vec<Value>, usize)> {
        let mut toks = Vec::with_capacity(parts.len());
        for part in parts {
            let (t, next) = self.part(el, *part, i)?;
            toks.push(t);
            i = next;
        }
        Ok((toks, i))
    }

    pub(crate) fn part(&mut self, el: &El<'_>, part: Part, i: usize) -> Res<(Value, usize)> {
        match part {
            Part::S(c) => self.el_scalar(el, c, i, el.name),
            Part::Child => self.el_ref(el, 2, i),
            Part::Cross => self.el_ref(el, 1, i),
            Part::Uuid => {
                let (t, next) = self.read_uuid(el.data, i, el.name)?;
                Ok((self.tok(t), next))
            }
            Part::Text8 => self.el_text(el, false, i),
            Part::Text16 => self.el_text(el, true, i),
            Part::U32x2 => self.el_tuple(el, "u32", 'I', 2, i, el.name),
            Part::F32x3 => self.el_tuple(el, "f32", 'f', 3, i, el.name),
            Part::U32x7 => self.el_tuple(el, "u32", 'I', 7, i, el.name),
            Part::List2Cross => self.el_list(el, 2, 0x1A, i),
        }
    }

    pub(crate) fn walk_map(
        &mut self,
        el: &El<'_>,
        m: MapWalk,
        i: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let block = self.block(el);
        let file = self.ctx_int(el.ctx, "file", 2027);
        match m {
            MapWalk::Pair(a, b) => self.seq(el, i, &[a, b]),
            MapWalk::MdlTxnMgr => {
                let (mut toks, i) = self.seq(el, i, &[Part::S('I'), Part::S('I')])?;
                let (lst, i) = self.el_list(el, 2, 0x4A, i)?;
                let (raw, i) = self.el_raw(el, i, block, block)?;
                toks.extend([lst, raw]);
                Ok((toks, i))
            }
            MapWalk::KeyApp1 => {
                let (key, i) = self.el_scalar(el, 'I', i, el.name)?;
                let n = if file > 2017 { 50 } else { 49 };
                let (raw, i) = self.el_raw(el, i, n, n)?;
                Ok((vec![key, raw], i))
            }
            MapWalk::KeyMapApp1 => {
                let (mut toks, i) = self.seq(el, i, &[Part::S('I'), Part::S('B')])?;
                let (lst, i) = self.el_list(el, 6, 0x7019, i)?;
                toks.push(lst);
                Ok((toks, i))
            }
        }
    }

    pub(crate) fn walk_font(
        &mut self,
        el: &El<'_>,
        i: usize,
        skip: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let (a, i) = self.el_raw(el, i, 18, 18)?;
        let (t, i) = self.el_text(el, true, i)?;
        let (b, i) = self.el_raw(el, i, 11 + skip, 11 + skip)?;
        Ok((vec![a, t, b], i))
    }

    pub(crate) fn walk_lightning(
        &mut self,
        el: &El<'_>,
        i: usize,
        file: i64,
    ) -> Res<(Vec<Value>, usize)> {
        self.need(el.data, i, 126)?;
        if file <= 2010 {
            let (raw, i) = self.el_raw(el, i, 126, 126)?;
            return Ok((vec![raw], i));
        }
        let pre = format!("{}.", el.name.as_str().unwrap_or("light"));
        let named = |s: &str| Value::str(&format!("{pre}{s}"));
        let (kind, mut i) = self.el_scalar(el, 'H', i, &named("kind"))?;
        let mut toks = vec![kind];
        for k in 1..=3 {
            let (t, next) = self.el_tuple(el, "f32", 'f', 4, i, &named(&format!("color{k}")))?;
            toks.push(t);
            i = next;
        }
        for (field, typ, n, kind) in [
            ("position", 'd', 3, "f64"),
            ("direction", 'd', 3, "f64"),
            ("f32a", 'f', 7, "f32"),
        ] {
            let (t, next) = self.el_tuple(el, kind, typ, n, i, &named(field))?;
            toks.push(t);
            i = next;
        }
        Ok((toks, i))
    }

    pub(crate) fn walk_app2(
        &mut self,
        el: &El<'_>,
        i: usize,
        block: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let (mut toks, mut i) = self.seq(el, i, &[Part::S('I'), Part::Text16, Part::Text16])?;
        loop {
            let (t, next) = self.el_scalar(el, 'h', i, el.name)?;
            let end = self.token_value(&t).as_int() == Some(-1);
            toks.push(t);
            i = next;
            if end {
                break;
            }
        }
        let (raw, i) = self.el_raw(el, i, 3, 3)?;
        let (count_tok, i) = self.el_scalar(el, 'H', i, el.name)?;
        let count = self.token_value(&count_tok).as_int().unwrap_or(0).max(0) as usize;
        let (f64s, i) = self.el_tuple(el, "f64", 'd', count, i, el.name)?;
        let (f32s, i) = self.el_tuple(el, "f32", 'f', 2, i, el.name)?;
        let (u, i) = self.el_scalar(el, 'I', i, el.name)?;
        let (b1, i) = self.el_raw(el, i, block, block)?;
        let (b2, i) = self.el_raw(el, i, 16 + block, 16 + block)?;
        let (u2, i) = self.el_scalar(el, 'I', i, el.name)?;
        let (u8v, i) = self.el_scalar(el, 'B', i, el.name)?;
        let (b3, i) = self.el_raw(el, i, 2 * block, 2 * block)?;
        toks.extend([raw, count_tok, f64s, f32s, u, b1, b2, u2, u8v, b3]);
        Ok((toks, i))
    }

    pub(crate) fn token_value(&self, t: &Value) -> Value {
        match t {
            Value::Heap(id) => self
                .token(*id)
                .map(|t| t.value.clone())
                .unwrap_or(Value::None),
            _ => Value::None,
        }
    }

    pub(crate) fn walk_app3(
        &mut self,
        el: &El<'_>,
        i: usize,
        block: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let (mut toks, i) = self.seq(el, i, &[Part::S('I'), Part::Text16, Part::Text16])?;
        let (lst, i) = self.el_list(el, 2, 0x18, i)?;
        let (u, i) = self.el_scalar(el, 'B', i, el.name)?;
        let (raw, i) = self.el_raw(el, i, block, block)?;
        toks.extend([lst, u, raw]);
        Ok((toks, i))
    }

    pub(crate) fn walk_app4(
        &mut self,
        el: &El<'_>,
        i: usize,
        b_len: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let (a, i) = self.el_raw(el, i, 18, 18)?;
        let (t, i) = self.el_text(el, true, i)?;
        let (b, i) = self.el_raw(el, i, b_len, b_len)?;
        Ok((vec![a, t, b], i))
    }

    pub(crate) fn walk_transformation(
        &mut self,
        el: &El<'_>,
        mut i: usize,
    ) -> Res<(Vec<Value>, usize)> {
        self.need(el.data, i, 4)?;
        let mut toks = Vec::new();
        if u32::from_le_bytes([el.data[i], el.data[i + 1], el.data[i + 2], el.data[i + 3]]) == 0x203
        {
            let (t, next) = self.el_scalar(el, 'I', i, el.name)?;
            toks.push(t);
            i = next;
        }
        self.need(el.data, i, 4)?;
        let (d1, next) = self.el_scalar(el, 'H', i, el.name)?;
        let (d2, next) = self.el_scalar(el, 'H', next, el.name)?;
        i = next;
        let (m1, m2) = (
            self.token_value(&d1).as_int().unwrap_or(0),
            self.token_value(&d2).as_int().unwrap_or(0),
        );
        toks.extend([d1, d2]);
        for j in 0..16 {
            if (m2 >> j) & 1 == 1 || (m1 >> j) & 1 == 1 {
                continue;
            }
            let (t, next) = self.el_scalar(el, 'd', i, el.name)?;
            toks.push(t);
            i = next;
        }
        Ok((toks, i))
    }

    pub(crate) fn walk_u16_color(
        &mut self,
        el: &El<'_>,
        i: usize,
        block: usize,
    ) -> Res<(Vec<Value>, usize)> {
        let (head, mut i) = self.el_scalar(el, 'H', i, el.name)?;
        let mut toks = vec![head];
        for _ in 0..3 {
            let (t, next) = self.el_raw(el, i, 16 + block, 16 + block)?;
            toks.push(t);
            i = next;
        }
        let (mid, i) = self.el_raw(el, i, 60, 60)?;
        let (tail, i) = self.el_raw(el, i, 16 + block, 16 + block)?;
        toks.extend([mid, tail]);
        Ok((toks, i))
    }

    pub(crate) fn walk_result(
        &mut self,
        el: &El<'_>,
        walk: Walk,
        i: usize,
        block: usize,
        file: i64,
    ) -> Res<(Vec<Value>, usize)> {
        let one = |r: (Value, usize)| (vec![r.0], r.1);
        Ok(match walk {
            Walk::Result1 => {
                let (u, i) = self.el_scalar(el, 'I', i, el.name)?;
                let (l1, i) = self.el_list(el, 4, 0x15, i)?;
                let (mut toks, i) = self.seq(el, i, &[Part::S('I'), Part::S('I')])?;
                let (l2, i) = self.el_list(el, 4, 0x15, i)?;
                let (raw, i) = self.el_raw(el, i, 48 + block, 48 + block)?;
                toks.splice(0..0, [u, l1]);
                toks.extend([l2, raw]);
                (toks, i)
            }
            Walk::Result2 => one(self.el_raw(el, i, 8 + block, 8 + block)?),
            Walk::Result3 => one(self.el_raw(el, i, 56 + block, 56 + block)?),
            Walk::Result4 => {
                let (u, i) = self.el_scalar(el, 'I', i, el.name)?;
                let (l, i) = self.el_list(el, 4, 0x15, i)?;
                let (raw, i) = self.el_raw(el, i, block, block)?;
                (vec![u, l, raw], i)
            }
            Walk::Result5 => self.walk_result5(el, i, block, file)?,
            Walk::ResultItem4 => one(self.el_raw(el, i, 56 + block, 60 + block)?),
            _ => return self.grammar_error("unknown list walker"),
        })
    }

    pub(crate) fn walk_result5(
        &mut self,
        el: &El<'_>,
        i: usize,
        block: usize,
        file: i64,
    ) -> Res<(Vec<Value>, usize)> {
        let (u, mut i) = self.el_scalar(el, 'I', i, el.name)?;
        let mut toks = vec![u];
        if file > 2017 {
            let (raw, next) = self.el_raw(el, i, 1, 1)?;
            toks.push(raw);
            i = next;
        }
        let (h, i) = self.el_scalar(el, 'H', i, el.name)?;
        let (l, i) = self.el_list(el, 4, 0x15, i)?;
        let (u2, i) = self.el_scalar(el, 'I', i, el.name)?;
        let (raw, i) = self.el_raw(el, i, block, block)?;
        toks.extend([h, l, u2, raw]);
        Ok((toks, i))
    }
}
