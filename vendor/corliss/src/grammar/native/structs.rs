//! The `struct` module subset grammars use: `unpack_from`, `unpack`, `pack`, `calcsize`.

use super::parser::scalar_value;
use crate::grammar::interp::{Interp, Res};
use crate::grammar::value::Value;

/// One expanded format item.
enum Item {
    Scalar(char, usize),
    Pad,
    Bytes(usize),
}

fn width(c: char) -> Option<usize> {
    Some(match c {
        'B' | 'b' | 'x' | '?' | 'c' | 's' => 1,
        'H' | 'h' => 2,
        'I' | 'i' | 'L' | 'l' | 'f' => 4,
        'Q' | 'q' | 'd' => 8,
        _ => return None,
    })
}

fn items(it: &mut Interp, fmt: &str) -> Res<Vec<Item>> {
    let body = fmt.trim_start_matches(['<', '>', '=', '!', '@']);
    let mut out = Vec::new();
    let mut count = String::new();
    for c in body.chars() {
        if c.is_ascii_digit() {
            count.push(c);
            continue;
        }
        let n: usize = if count.is_empty() {
            1
        } else {
            count.parse().unwrap_or(1)
        };
        count.clear();
        let Some(w) = width(c) else {
            return it.raise("struct.error", format!("bad char in struct format: {c}"));
        };
        match c {
            's' => out.push(Item::Bytes(n)),
            'x' => (0..n).for_each(|_| out.push(Item::Pad)),
            _ => (0..n).for_each(|_| out.push(Item::Scalar(c, w))),
        }
    }
    Ok(out)
}

/// `struct.calcsize`.
pub(crate) fn calcsize(it: &mut Interp, fmt: &str) -> Res<usize> {
    Ok(items(it, fmt)?
        .iter()
        .map(|i| match i {
            Item::Scalar(_, w) => *w,
            Item::Pad => 1,
            Item::Bytes(n) => *n,
        })
        .sum())
}

/// `struct.unpack_from(fmt, buffer, offset)`.
pub(crate) fn unpack_from(it: &mut Interp, fmt: &str, buf: &[u8], offset: usize) -> Res<Value> {
    let size = calcsize(it, fmt)?;
    if offset + size > buf.len() {
        let msg = format!(
            "unpack_from requires a buffer of at least {} bytes",
            offset + size
        );
        return it.raise("struct.error", msg);
    }
    let mut at = offset;
    let mut out = Vec::new();
    for item in items(it, fmt)? {
        match item {
            Item::Scalar(c, w) => {
                out.push(decode(c, &buf[at..at + w]));
                at += w;
            }
            Item::Pad => at += 1,
            Item::Bytes(n) => {
                out.push(Value::bytes(&buf[at..at + n]));
                at += n;
            }
        }
    }
    Ok(Value::tuple(out))
}

fn decode(c: char, b: &[u8]) -> Value {
    match c {
        'L' => scalar_value('I', b),
        'l' => scalar_value('i', b),
        'Q' => {
            let v = u64::from_le_bytes(b.try_into().unwrap_or([0; 8]));
            i64::try_from(v).map_or(Value::Float(v as f64), Value::Int)
        }
        'q' => Value::Int(i64::from_le_bytes(b.try_into().unwrap_or([0; 8]))),
        '?' => Value::Bool(b[0] != 0),
        'c' => Value::bytes(b),
        _ => scalar_value(c, b),
    }
}

/// `struct.pack(fmt, *values)`.
pub(crate) fn pack(it: &mut Interp, fmt: &str, values: &[Value]) -> Res<Vec<u8>> {
    let mut out = Vec::new();
    let mut next = values.iter();
    for item in items(it, fmt)? {
        match item {
            Item::Pad => out.push(0),
            Item::Bytes(n) => {
                let mut b = match next.next() {
                    Some(Value::Bytes(b)) => b.to_vec(),
                    _ => Vec::new(),
                };
                b.resize(n, 0);
                out.extend(b);
            }
            Item::Scalar(c, w) => {
                let v = next.next().cloned().unwrap_or(Value::Int(0));
                out.extend(encode(c, w, &v));
            }
        }
    }
    Ok(out)
}

fn encode(c: char, w: usize, v: &Value) -> Vec<u8> {
    match c {
        'f' => (super::super::interp::ops::as_f64(v).unwrap_or(0.0) as f32)
            .to_le_bytes()
            .to_vec(),
        'd' => super::super::interp::ops::as_f64(v)
            .unwrap_or(0.0)
            .to_le_bytes()
            .to_vec(),
        _ => v.as_int().unwrap_or(0).to_le_bytes()[..w].to_vec(),
    }
}
