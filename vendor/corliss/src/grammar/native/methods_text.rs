//! Methods of `str` and `bytes`.

use std::sync::Arc;

use super::args::Args;
use crate::grammar::interp::{Flow, Interp, Res};
use crate::grammar::value::Value;

impl Interp {
    pub(crate) fn str_method(&mut self, s: Arc<str>, name: &str, a: &Args) -> Res<Value> {
        let arg_str = |k: usize| a.get(k, "").and_then(|v| v.as_str().map(str::to_string));
        Ok(match name {
            "startswith" | "endswith" => {
                let probes = match a.or(0, "prefix", Value::None) {
                    Value::Tuple(t) => t
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect(),
                    v => v.as_str().map(|x| vec![x.to_string()]).unwrap_or_default(),
                };
                let hit = probes.iter().any(|p| {
                    if name == "startswith" {
                        s.starts_with(p.as_str())
                    } else {
                        s.ends_with(p.as_str())
                    }
                });
                Value::Bool(hit)
            }
            "split" | "rsplit" => {
                self.str_split(&s, arg_str(0), a.int(1, "maxsplit"), name == "rsplit")
            }
            "partition" | "rpartition" => {
                let sep = arg_str(0).unwrap_or_default();
                let found = if name == "partition" {
                    s.split_once(sep.as_str())
                } else {
                    s.rsplit_once(sep.as_str())
                };
                let parts = match found {
                    Some((x, y)) => [x.to_string(), sep.clone(), y.to_string()],
                    None if name == "partition" => [s.to_string(), String::new(), String::new()],
                    None => [String::new(), String::new(), s.to_string()],
                };
                Value::tuple(parts.iter().map(|p| Value::str(p)).collect())
            }
            "find" | "rfind" | "index" => {
                let sub = arg_str(0).unwrap_or_default();
                let at = if name == "rfind" {
                    s.rfind(sub.as_str())
                } else {
                    s.find(sub.as_str())
                };
                match at {
                    Some(b) => Value::Int(s[..b].chars().count() as i64),
                    None if name == "index" => {
                        return self.raise("builtins.ValueError", "substring not found");
                    }
                    None => Value::Int(-1),
                }
            }
            "lower" => Value::str(&s.to_lowercase()),
            "upper" => Value::str(&s.to_uppercase()),
            "strip" | "rstrip" | "lstrip" => Value::str(&strip(&s, name, arg_str(0))),
            "replace" => Value::str(&s.replace(
                arg_str(0).unwrap_or_default().as_str(),
                &arg_str(1).unwrap_or_default(),
            )),
            "join" => {
                let parts = self.iterate(&a.or(0, "it", Value::None))?;
                let texts = parts
                    .iter()
                    .map(|p| self.py_str(p))
                    .collect::<Res<Vec<_>>>()?;
                Value::str(&texts.join(&s))
            }
            "encode" => Value::bytes(&self.encode_str(&s, arg_str(0))),
            "isdigit" => Value::Bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())),
            "isalpha" => Value::Bool(!s.is_empty() && s.chars().all(char::is_alphabetic)),
            "isascii" => Value::Bool(s.is_ascii()),
            "count" => {
                Value::Int(s.matches(arg_str(0).unwrap_or_default().as_str()).count() as i64)
            }
            "format" => {
                let mut out = s.to_string();
                for v in &a.pos {
                    let text = self.py_str(v)?;
                    out = out.replacen("{}", &text, 1);
                }
                Value::str(&out)
            }
            other => return Err(Flow::Fatal(format!("str.{other} is not implemented"))),
        })
    }

    pub(crate) fn str_split(
        &mut self,
        s: &str,
        sep: Option<String>,
        max: Option<i64>,
        right: bool,
    ) -> Value {
        let max = max.filter(|m| *m >= 0).map(|m| m as usize);
        let parts: Vec<String> = match (sep, max, right) {
            (None, _, _) => s.split_whitespace().map(str::to_string).collect(),
            (Some(sep), Some(m), false) => {
                s.splitn(m + 1, sep.as_str()).map(str::to_string).collect()
            }
            (Some(sep), Some(m), true) => {
                let mut v: Vec<String> =
                    s.rsplitn(m + 1, sep.as_str()).map(str::to_string).collect();
                v.reverse();
                v
            }
            (Some(sep), None, _) => s.split(sep.as_str()).map(str::to_string).collect(),
        };
        let items = parts.iter().map(|p| Value::str(p)).collect();
        self.new_list(items)
    }

    pub(crate) fn encode_str(&self, s: &str, encoding: Option<String>) -> Vec<u8> {
        match encoding.as_deref().map(str::to_lowercase).as_deref() {
            Some("utf-16-le" | "utf_16_le" | "utf-16le") => {
                s.encode_utf16().flat_map(u16::to_le_bytes).collect()
            }
            Some("latin-1" | "latin1" | "cp1252") => s
                .chars()
                .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
                .collect(),
            _ => s.as_bytes().to_vec(),
        }
    }

    pub(crate) fn bytes_method(&mut self, b: Arc<[u8]>, name: &str, a: &Args) -> Res<Value> {
        Ok(match name {
            "hex" => Value::str(&b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
            "find" | "index" | "count" => {
                let sub = match a.or(0, "sub", Value::None) {
                    Value::Bytes(x) => x.to_vec(),
                    v => v.as_int().map(|i| vec![i as u8]).unwrap_or_default(),
                };
                let start = a.int(1, "start").unwrap_or(0).max(0) as usize;
                if name == "count" {
                    let n = if sub.is_empty() {
                        0
                    } else {
                        b.windows(sub.len())
                            .filter(|w| *w == sub.as_slice())
                            .count()
                    };
                    return Ok(Value::Int(n as i64));
                }
                let hay = &b[start.min(b.len())..];
                let at = if sub.is_empty() {
                    Some(0)
                } else {
                    hay.windows(sub.len()).position(|w| w == sub.as_slice())
                };
                match at {
                    Some(p) => Value::Int((p + start) as i64),
                    None if name == "index" => {
                        return self.raise("builtins.ValueError", "subsection not found");
                    }
                    None => Value::Int(-1),
                }
            }
            "startswith" => Value::Bool(
                matches!(a.or(0, "prefix", Value::None), Value::Bytes(p) if b.starts_with(&p)),
            ),
            "decode" => self.decode_bytes(&b, a)?,
            other => return Err(Flow::Fatal(format!("bytes.{other} is not implemented"))),
        })
    }

    pub(crate) fn decode_bytes(&mut self, b: &[u8], a: &Args) -> Res<Value> {
        let encoding = a
            .get(0, "encoding")
            .and_then(|v| v.as_str().map(str::to_lowercase))
            .unwrap_or_else(|| "utf-8".into());
        let lenient = a.get(1, "errors").is_some();
        let text = match encoding.as_str() {
            "utf-16-le" | "utf_16_le" | "utf-16le" => {
                let units: Vec<u16> = b
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                if lenient {
                    Ok(String::from_utf16_lossy(&units))
                } else {
                    String::from_utf16(&units).map_err(|_| ())
                }
            }
            "latin-1" | "latin1" | "cp1252" | "ascii" => {
                Ok(b.iter().map(|c| char::from(*c)).collect())
            }
            _ => {
                if lenient {
                    Ok(String::from_utf8_lossy(b).into_owned())
                } else {
                    String::from_utf8(b.to_vec()).map_err(|_| ())
                }
            }
        };
        match text {
            Ok(t) => Ok(Value::str(&t)),
            Err(()) => self.raise(
                "builtins.UnicodeDecodeError",
                format!("cannot decode {encoding}"),
            ),
        }
    }
}

fn strip(s: &str, how: &str, chars: Option<String>) -> String {
    let pred = |c: char| match &chars {
        Some(set) => set.contains(c),
        None => c.is_whitespace(),
    };
    match how {
        "lstrip" => s.trim_start_matches(pred).to_string(),
        "rstrip" => s.trim_end_matches(pred).to_string(),
        _ => s.trim_matches(pred).to_string(),
    }
}
