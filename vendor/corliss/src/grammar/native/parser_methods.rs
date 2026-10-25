//! The `Parser` methods grammars call, dispatched by name.

use super::args::Args;
use super::parser::{scalar_value, strip_text, text8_value};
use super::token::Token;
use crate::grammar::interp::{Flow, HeapObj, Interp, Res};
use crate::grammar::value::{HeapId, Value};

impl Interp {
    /// Dispatches a `Parser` method.
    pub fn parser_method(&mut self, pid: HeapId, name: &str, a: Args) -> Res<Value> {
        if let Some(v) = self.parser_read(pid, name, &a)? {
            return Ok(v);
        }
        if let Some(v) = self.parser_compound(pid, name, &a)? {
            return Ok(v);
        }
        self.parser_misc(pid, name, &a)
    }

    pub(crate) fn parser_read(&mut self, pid: HeapId, name: &str, a: &Args) -> Res<Option<Value>> {
        let n0 = a.get(0, "name").unwrap_or(Value::None);
        let scalar = match name {
            "u8" => 'B',
            "s8" => 'b',
            "u16" | "enum16" => 'H',
            "s16" => 'h',
            "u32" | "enum32" => 'I',
            "s32" => 'i',
            "f32" => 'f',
            "f64" => 'd',
            _ => ' ',
        };
        if scalar != ' ' {
            return self.one_scalar(pid, scalar, &n0).map(Some);
        }
        let v = match name {
            "boolean" | "bool" => {
                let (data, i) = self.cursor(pid)?;
                let (t, next) = self.read_bool(&data, i, &n0)?;
                self.push(pid, t, i, next)?
            }
            "uuid" => {
                let (data, i) = self.cursor(pid)?;
                let (t, next) = self.read_uuid(&data, i, &n0)?;
                self.push(pid, t, i, next)?
            }
            "text8" => self.one_text(pid, false, &n0)?,
            "text16" => self.one_text(pid, true, &n0)?,
            "child_ref" => self.one_ref(pid, 2, &a.get(0, "name").unwrap_or(Value::str("ref")))?,
            "cross_ref" => self.one_ref(pid, 1, &a.get(0, "name").unwrap_or(Value::str("ref")))?,
            "parent_ref" => self.one_ref(pid, 1, &Value::str("parent"))?,
            "node_ref" => {
                let kind = a.get(0, "refkind").and_then(|v| v.as_int()).unwrap_or(1);
                self.one_ref(pid, kind, &a.get(1, "name").unwrap_or(Value::None))?
            }
            _ => return self.parser_arrays(pid, name, a),
        };
        Ok(Some(v))
    }

    pub(crate) fn parser_arrays(
        &mut self,
        pid: HeapId,
        name: &str,
        a: &Args,
    ) -> Res<Option<Value>> {
        let typ = match name {
            "u8a" => 'B',
            "s8a" => 'b',
            "u16a" => 'H',
            "s16a" => 'h',
            "u32a" => 'I',
            "s32a" => 'i',
            "f32a" => 'f',
            "f64a" => 'd',
            _ => return Ok(None),
        };
        let n = a.get(1, "n").and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
        self.array(pid, typ, n, &a.get(0, "name").unwrap_or(Value::None))
            .map(Some)
    }

    pub(crate) fn parser_compound(
        &mut self,
        pid: HeapId,
        name: &str,
        a: &Args,
    ) -> Res<Option<Value>> {
        let n0 = a.get(0, "name").unwrap_or(Value::None);
        let v = match name {
            "f32_2d" | "f32_3d" | "f64_2d" | "f64_3d" => {
                let typ = if name.starts_with("f32") { 'f' } else { 'd' };
                let count = if name.ends_with("2d") { 2 } else { 3 };
                let values = (0..count)
                    .map(|_| self.one_scalar(pid, typ, &n0))
                    .collect::<Res<Vec<_>>>()?;
                Value::tuple(values)
            }
            "color" => self.color(pid, &n0)?,
            "raw" => {
                let n = a.get(1, "n").and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
                self.one_raw(pid, &n0, n)?
            }
            "skip" => {
                let n = a.get(1, "n").and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
                if n0.is_none() && n == 4 {
                    self.one_scalar(pid, 'i', &Value::str("s32_skip"))?
                } else {
                    self.one_raw(pid, &n0, n)?
                }
            }
            "skip_block_size" => {
                let ctx = self.parser(pid)?.ctx.clone();
                let block = self.ctx_block(&ctx);
                self.one_raw(pid, &n0, block)?
            }
            "header0" => self.header0(pid)?,
            "list2" | "list3" | "list4" | "list6" | "list7" | "list8" => {
                self.parser_list(pid, name, a)?
            }
            "text8n" => self.text8n(pid, a)?,
            "material" => self.material(pid, a)?,
            "transformation" => self.transformation(pid, a)?,
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    pub(crate) fn color(&mut self, pid: HeapId, name: &Value) -> Res<Value> {
        let (data, i) = self.cursor(pid)?;
        let ctx = self.parser(pid)?.ctx.clone();
        let block = self.ctx_block(&ctx);
        self.need(&data, i, 16)?;
        self.need(&data, i + 16, block)?;
        let values: Vec<Value> = data[i..i + 16]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| scalar_value('f', c))
            .collect();
        let v = Value::tuple(values);
        let t = Token::new(
            "color",
            name.clone(),
            v.clone(),
            &data[i..i + 16 + block],
            None,
        );
        self.push_plain(pid, t, i + 16 + block)?;
        Ok(v)
    }

    pub(crate) fn header0(&mut self, pid: HeapId) -> Res<Value> {
        let ctx = self.parser(pid)?.ctx.clone();
        if self.ctx_int(&ctx, "rse_ided", 1) == 0 {
            return Ok(Value::None);
        }
        self.one_scalar(pid, 'I', &Value::str("hdr.u32_0"))?;
        self.one_scalar(pid, 'H', &Value::str("hdr.u16_0"))?;
        let block = self.ctx_block(&ctx);
        self.one_raw(pid, &Value::str("hdr.block"), block)?;
        Ok(Value::None)
    }

    pub(crate) fn parser_list(&mut self, pid: HeapId, name: &str, a: &Args) -> Res<Value> {
        let code: i64 = name[4..].parse().unwrap_or(2);
        let typ = a.get(0, "typ").and_then(|v| v.as_int()).unwrap_or(0);
        let lname = a.get(1, "name").unwrap_or(Value::None);
        let size = if code == 2 {
            a.get(2, "arraySize").and_then(|v| v.as_int()).unwrap_or(1)
        } else {
            1
        };
        let (data, i) = self.cursor(pid)?;
        let ctx = self.parser(pid)?.ctx.clone();
        let (tok, next) = self.parse_list(&data, i, code, typ, &ctx, &lname, size)?;
        let value = tok.value.clone();
        self.push_plain(pid, tok, next)?;
        Ok(value)
    }

    pub(crate) fn text8n(&mut self, pid: HeapId, a: &Args) -> Res<Value> {
        let n = a.get(0, "n").and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
        let name = a.get(1, "name").unwrap_or(Value::None);
        let (data, i) = self.cursor(pid)?;
        self.need(&data, i, n)?;
        let raw = &data[i..i + n];
        let value = match text8_value(raw) {
            Value::Str(s) => Value::str(&strip_text(s.to_string())),
            other => other,
        };
        let t = Token::new("text8", name, value.clone(), raw, None);
        self.push_plain(pid, t, i + n)?;
        Ok(value)
    }

    pub(crate) fn sub_name(name: &Value, suffix: &str) -> Value {
        match name.as_str() {
            Some(n) => Value::str(&format!("{n}{suffix}")),
            None => Value::None,
        }
    }

    pub(crate) fn material(&mut self, pid: HeapId, a: &Args) -> Res<Value> {
        let name = a.get(0, "name").unwrap_or(Value::None);
        let count = a.get(1, "count").and_then(|v| v.as_int()).unwrap_or(0);
        self.color(pid, &Self::sub_name(&name, ".c0"))?;
        self.color(pid, &Self::sub_name(&name, ".diffuse"))?;
        for j in 0..count {
            self.color(pid, &Self::sub_name(&name, &format!(".c{}", j + 1)))?;
        }
        self.one_scalar(pid, 'f', &Self::sub_name(&name, ".f"))?;
        let ctx = self.parser(pid)?.ctx.clone();
        let block = self.ctx_block(&ctx);
        self.one_raw(pid, &Self::sub_name(&name, ".blk"), block)?;
        Ok(Value::None)
    }

    pub(crate) fn transformation(&mut self, pid: HeapId, a: &Args) -> Res<Value> {
        let name = a.get(0, "name").unwrap_or(Value::str("transformation"));
        let (data, i) = self.cursor(pid)?;
        self.need(&data, i, 4)?;
        if u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) == 0x203 {
            self.one_scalar(pid, 'I', &Self::sub_name(&name, ".marker"))?;
        }
        let d1 = self
            .one_scalar(pid, 'H', &Self::sub_name(&name, ".d1"))?
            .as_int()
            .unwrap_or(0);
        let d2 = self
            .one_scalar(pid, 'H', &Self::sub_name(&name, ".d2"))?
            .as_int()
            .unwrap_or(0);
        for j in 0..16 {
            if (d2 >> j) & 1 == 1 || (d1 >> j) & 1 == 1 {
                continue;
            }
            self.one_scalar(pid, 'd', &Self::sub_name(&name, &format!(".m{j}")))?;
        }
        Ok(Value::None)
    }

    pub(crate) fn parser_misc(&mut self, pid: HeapId, name: &str, a: &Args) -> Res<Value> {
        match name {
            "get" => self.parser_get(pid, &a.get(0, "name").unwrap_or(Value::None)),
            "set" => self.parser_set(
                pid,
                &a.get(0, "name").unwrap_or(Value::None),
                a.get(1, "value").unwrap_or(Value::None),
            ),
            "peek_u32" | "peek_s32" => {
                let (data, i) = self.cursor(pid)?;
                self.need(&data, i, 4)?;
                Ok(scalar_value(
                    if name == "peek_u32" { 'I' } else { 'i' },
                    &data[i..i + 4],
                ))
            }
            "raw_rest" => {
                let (data, i) = self.cursor(pid)?;
                let rest = data[i.min(data.len())..].to_vec();
                let t = Token::new(
                    "raw",
                    a.get(0, "name").unwrap_or(Value::None),
                    Value::None,
                    &rest,
                    None,
                );
                self.push_plain(pid, t, data.len())?;
                Ok(Value::bytes(&rest))
            }
            "reset" => {
                let list = self.parser(pid)?.toks;
                if let HeapObj::List(items) = self.obj_mut(list) {
                    items.clear();
                }
                self.parser_mut(pid)?.i = 0;
                Ok(Value::None)
            }
            "mark" => {
                let list = self.parser(pid)?.toks;
                let n = self.len_of(&Value::Heap(list)).unwrap_or(0) as i64;
                Ok(Value::tuple(vec![
                    Value::Int(n),
                    Value::Int(self.parser(pid)?.i),
                ]))
            }
            "rewind" => self.rewind(pid, a.get(0, "m").unwrap_or(Value::None)),
            "tokens" => self.parser_tokens(pid, a.get(0, "name").unwrap_or(Value::None)),
            "serialize" => {
                let list = self.parser(pid)?.toks;
                let mut out = Vec::new();
                for t in self.iterate(&Value::Heap(list))? {
                    if let Value::Heap(tid) = t {
                        out.extend(self.serialize_token(tid)?);
                    }
                }
                Ok(Value::bytes(&out))
            }
            other => Err(Flow::Fatal(format!("Parser.{other} is not implemented"))),
        }
    }

    pub(crate) fn top_tokens(&mut self, pid: HeapId) -> Res<Vec<HeapId>> {
        let list = self.parser(pid)?.toks;
        Ok(self
            .iterate(&Value::Heap(list))?
            .into_iter()
            .filter_map(|t| match t {
                Value::Heap(id) => Some(id),
                _ => None,
            })
            .collect())
    }

    /// `Parser.get`: the last top-level token with that name, else the last that had it.
    pub(crate) fn parser_get(&mut self, pid: HeapId, name: &Value) -> Res<Value> {
        let Some(name) = name.as_str() else {
            return Ok(Value::None);
        };
        let toks = self.top_tokens(pid)?;
        for id in toks.iter().rev() {
            if let Some(t) = self.token(*id)
                && t.name.as_str() == Some(name)
            {
                return Ok(t.value.clone());
            }
        }
        for id in toks.iter().rev() {
            if let Some(t) = self.token(*id)
                && t.had(name)
            {
                return Ok(t.value.clone());
            }
        }
        Ok(Value::None)
    }

    pub(crate) fn parser_set(&mut self, pid: HeapId, name: &Value, value: Value) -> Res<Value> {
        let Some(name) = name.as_str() else {
            return self.raise("builtins.KeyError", "None");
        };
        let toks = self.top_tokens(pid)?;
        let by_name = toks.iter().rev().find(|id| {
            self.token(**id)
                .is_some_and(|t| t.name.as_str() == Some(name))
        });
        let found = by_name.or_else(|| {
            toks.iter()
                .rev()
                .find(|id| self.token(**id).is_some_and(|t| t.had(name)))
        });
        let Some(id) = found.copied() else {
            return self.raise("builtins.KeyError", name.to_string());
        };
        let HeapObj::Token(t) = self.obj_mut(id) else {
            return Ok(Value::None);
        };
        if t.kind.as_ref() == "raw" {
            return self.grammar_error(format!("cannot set() on raw token '{name}'"));
        }
        t.value = value;
        t.dirty = true;
        Ok(Value::None)
    }

    pub(crate) fn rewind(&mut self, pid: HeapId, mark: Value) -> Res<Value> {
        let Value::Tuple(m) = mark else {
            return Err(Flow::Fatal("rewind() needs a mark".into()));
        };
        let n = m.first().and_then(Value::as_int).unwrap_or(0).max(0) as usize;
        let off = m.get(1).and_then(Value::as_int).unwrap_or(0);
        let list = self.parser(pid)?.toks;
        if let HeapObj::List(items) = self.obj_mut(list) {
            items.truncate(n);
        }
        self.parser_mut(pid)?.i = off;
        Ok(Value::None)
    }

    pub(crate) fn parser_tokens(&mut self, pid: HeapId, name: Value) -> Res<Value> {
        let toks = self.top_tokens(pid)?;
        let picked: Vec<Value> = toks
            .into_iter()
            .filter(|id| {
                name.is_none() || self.token(*id).is_some_and(|t| self.py_eq(&t.name, &name))
            })
            .map(Value::Heap)
            .collect();
        Ok(self.new_list(picked))
    }
}
