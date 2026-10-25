//! The token: one decoded field with its source bytes.

use std::sync::Arc;

use crate::grammar::interp::{Flow, HeapObj, Interp, Res};
use crate::grammar::value::{HeapId, Value};

/// List element type code of a CHAR list, whose count is its byte length.
pub(crate) const TYP_CHAR: i64 = 0x10;

/// A token as the reference grammar engine holds it.
#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub kind: Arc<str>,
    pub name: Value,
    pub value: Value,
    pub raw: Arc<[u8]>,
    pub typ: Value,
    pub extra: Value,
    pub off: Value,
    pub alias: Value,
    pub dirty: bool,
}

impl Token {
    pub fn new(kind: &str, name: Value, value: Value, raw: &[u8], typ: Option<char>) -> Self {
        Token {
            kind: Arc::from(kind),
            name,
            value,
            raw: Arc::from(raw),
            typ: typ.map_or(Value::None, |c| Value::str(&c.to_string())),
            extra: Value::None,
            off: Value::None,
            alias: Value::None,
            dirty: false,
        }
    }

    pub fn field(&self, name: &str) -> Option<Value> {
        Some(match name {
            "kind" => Value::Str(self.kind.clone()),
            "name" => self.name.clone(),
            "value" => self.value.clone(),
            "raw" => Value::Bytes(self.raw.clone()),
            "typ" => self.typ.clone(),
            "extra" => self.extra.clone(),
            "off" => self.off.clone(),
            "alias" => self.alias.clone(),
            "dirty" => Value::Bool(self.dirty),
            _ => return None,
        })
    }

    pub fn set_field(&mut self, name: &str, v: Value) -> Res<()> {
        match name {
            "kind" => self.kind = Arc::from(v.as_str().unwrap_or("raw")),
            "name" => self.name = v,
            "value" => self.value = v,
            "raw" => {
                if let Value::Bytes(b) = v {
                    self.raw = b;
                }
            }
            "typ" => self.typ = v,
            "extra" => self.extra = v,
            "off" => self.off = v,
            "alias" => self.alias = v,
            "dirty" => self.dirty = matches!(v, Value::Bool(true)),
            other => return Err(Flow::Fatal(format!("Tok has no attribute {other}"))),
        }
        Ok(())
    }

    /// True when the token carried `name` before a docnames rename.
    pub fn had(&self, name: &str) -> bool {
        match &self.alias {
            Value::Str(a) => a.as_ref() == name,
            Value::Tuple(items) => items.iter().any(|x| x.as_str() == Some(name)),
            _ => false,
        }
    }
}

impl Interp {
    pub fn token(&self, id: HeapId) -> Option<&Token> {
        match self.obj(id) {
            HeapObj::Token(t) => Some(t),
            _ => None,
        }
    }

    pub fn new_token(&mut self, t: Token) -> Value {
        self.alloc(HeapObj::Token(Box::new(t)))
    }

    /// The bytes the token stands for.
    pub fn serialize_token(&mut self, id: HeapId) -> Res<Vec<u8>> {
        let Some(t) = self.token(id) else {
            return Err(Flow::Fatal("serialize() on a non-token".into()));
        };
        match t.kind.as_ref() {
            "list" => {
                let (value, extra) = (t.value.clone(), t.extra.clone());
                self.serialize_list(&value, &extra)
            }
            "ref" => {
                let v = t.value.as_int().unwrap_or(0);
                Ok(((v & 0x7FFF_FFFF) as u32 | (v as u32 & 0x8000_0000))
                    .to_le_bytes()
                    .to_vec())
            }
            _ if !t.dirty => Ok(t.raw.to_vec()),
            _ => {
                let t = t.clone();
                self.encode_dirty(&t)
            }
        }
    }

    fn serialize_list(&mut self, value: &Value, extra: &Value) -> Res<Vec<u8>> {
        let framing = self.extra_bytes(extra, "framing_raw")?.unwrap_or_default();
        let meta = self.extra_bytes(extra, "meta_raw")?;
        let typ = self.extra_value(extra, "typ")?.as_int();
        let elements = self.iterate(value)?;
        let mut body = Vec::new();
        for el in &elements {
            for t in self.iterate(el)? {
                let Value::Heap(tid) = t else { continue };
                body.extend(self.serialize_token(tid)?);
            }
        }
        let n = if typ == Some(TYP_CHAR) {
            body.len()
        } else {
            elements.len()
        };
        let mut out = framing;
        out.extend((n as u32).to_le_bytes());
        if n == 0 {
            return Ok(out);
        }
        if let Some(meta) = meta {
            out.extend(meta);
        }
        out.extend(body);
        Ok(out)
    }

    fn extra_value(&mut self, extra: &Value, key: &str) -> Res<Value> {
        match extra {
            Value::Heap(_) | Value::Obj(_) => {
                let k = Value::str(key);
                let has = self.contains(extra, &k)?;
                if has {
                    self.getitem(extra, &k)
                } else {
                    Ok(Value::None)
                }
            }
            _ => Ok(Value::None),
        }
    }

    fn extra_bytes(&mut self, extra: &Value, key: &str) -> Res<Option<Vec<u8>>> {
        Ok(match self.extra_value(extra, key)? {
            Value::Bytes(b) => Some(b.to_vec()),
            _ => None,
        })
    }

    /// Re-encodes a token whose value was changed with `Parser.set`.
    fn encode_dirty(&mut self, t: &Token) -> Res<Vec<u8>> {
        let v = &t.value;
        Ok(match t.kind.as_ref() {
            "text8" => {
                let b = v.as_str().unwrap_or("").as_bytes().to_vec();
                [(b.len() as u32).to_le_bytes().to_vec(), b].concat()
            }
            "text16" => {
                let s = v.as_str().unwrap_or("");
                let units: Vec<u16> = s.encode_utf16().collect();
                let mut out = (s.chars().count() as u32).to_le_bytes().to_vec();
                units.iter().for_each(|u| out.extend(u.to_le_bytes()));
                out
            }
            "bool" => vec![u8::from(self.truthy(v)?)],
            "uuid" => match v {
                Value::Bytes(b) => b.to_vec(),
                _ => t.raw.to_vec(),
            },
            _ => {
                let typ = t.typ.as_str().and_then(|s| s.chars().next());
                let Some(typ) = typ else {
                    return Err(Flow::Fatal(format!(
                        "cannot re-encode token kind {}",
                        t.kind
                    )));
                };
                let values = match v {
                    Value::Tuple(items) => items.to_vec(),
                    other => vec![other.clone()],
                };
                let fmt = format!("<{}", typ.to_string().repeat(values.len()));
                super::structs::pack(self, &fmt, &values)?
            }
        })
    }
}
