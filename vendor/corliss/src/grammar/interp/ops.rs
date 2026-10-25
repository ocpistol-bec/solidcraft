//! Operators, truthiness, equality, ordering and string conversion.

use std::cmp::Ordering;

use super::{HeapObj, Interp, Res};
use crate::grammar::ast::{BinOp, CmpOp};
use crate::grammar::snapshot::Obj;
use crate::grammar::value::Value;

/// Python's type name of a value, for messages and `type()` checks.
pub(crate) fn type_name(it: &Interp, v: &Value) -> &'static str {
    match v {
        Value::None => "NoneType",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "str",
        Value::Bytes(_) => "bytes",
        Value::Tuple(_) => "tuple",
        Value::Native(_) | Value::Method(_) => "builtin_function_or_method",
        Value::Obj(id) => match it.prog.obj(*id) {
            Some(Obj::List(_)) => "list",
            Some(Obj::Set(_)) => "set",
            Some(Obj::Dict(..)) => "dict",
            Some(Obj::Function(_)) => "function",
            Some(Obj::Namespace(_)) => "module",
            _ => "object",
        },
        Value::Heap(id) => match it.obj(*id) {
            HeapObj::List(_) => "list",
            HeapObj::Dict(_) => "dict",
            HeapObj::Set(_) => "set",
            HeapObj::Token(_) => "Tok",
            HeapObj::Parser(_) => "Parser",
            HeapObj::Closure(_) => "function",
            HeapObj::Exception(_) => "Exception",
            HeapObj::Iter(..) => "iterator",
            HeapObj::Env(_) | HeapObj::Attrs(_) => "object",
        },
    }
}

impl Interp {
    pub fn truthy(&mut self, v: &Value) -> Res<bool> {
        Ok(match v {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::Obj(_) | Value::Heap(_) => match self.len_of(v) {
                Some(n) => n > 0,
                None => true,
            },
            Value::Native(_) | Value::Method(_) => true,
        })
    }

    /// Python `==`.
    pub fn py_eq(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::None, Value::None) => true,
            (Value::Str(x), Value::Str(y)) => x == y,
            (Value::Bytes(x), Value::Bytes(y)) => x == y,
            (Value::Tuple(x), Value::Tuple(y)) => {
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| self.py_eq(p, q))
            }
            (Value::Float(x), _) => b
                .as_int()
                .map(|i| *x == i as f64)
                .unwrap_or(matches!(b, Value::Float(y) if x == y)),
            (_, Value::Float(y)) => a.as_int().is_some_and(|i| i as f64 == *y),
            (Value::Obj(x), Value::Obj(y)) => x == y,
            (Value::Native(x), Value::Native(y)) => x == y,
            (Value::Heap(x), Value::Heap(y)) => x == y || self.heap_eq(*x, *y),
            _ => match (a.as_int(), b.as_int()) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            },
        }
    }

    fn heap_eq(&self, x: u32, y: u32) -> bool {
        match (self.obj(x), self.obj(y)) {
            (HeapObj::List(p), HeapObj::List(q)) => {
                p.len() == q.len() && p.iter().zip(q).all(|(a, b)| self.py_eq(a, b))
            }
            _ => false,
        }
    }

    /// Python `is`: identity for objects, equality for the small immutable values grammars compare.
    pub fn py_is(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::None, Value::None) => true,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Obj(x), Value::Obj(y)) => x == y,
            (Value::Heap(x), Value::Heap(y)) => x == y,
            (Value::Native(x), Value::Native(y)) => x == y,
            (Value::Int(x), Value::Int(y)) => x == y,
            (Value::Str(x), Value::Str(y)) => x == y,
            _ => false,
        }
    }

    fn order(&mut self, a: &Value, b: &Value) -> Res<Ordering> {
        let ord = match (a, b) {
            (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
            (Value::Bytes(x), Value::Bytes(y)) => Some(x.cmp(y)),
            (Value::Tuple(x), Value::Tuple(y)) => return self.order_seq(x, y),
            (Value::Float(_), _) | (_, Value::Float(_)) => {
                let (x, y) = (as_f64(a), as_f64(b));
                x.zip(y).and_then(|(x, y)| x.partial_cmp(&y))
            }
            _ => a.as_int().zip(b.as_int()).map(|(x, y)| x.cmp(&y)),
        };
        match ord {
            Some(o) => Ok(o),
            None => self.raise(
                "builtins.TypeError",
                "'<' not supported between these types",
            ),
        }
    }

    fn order_seq(&mut self, x: &[Value], y: &[Value]) -> Res<Ordering> {
        for (p, q) in x.iter().zip(y) {
            let o = self.order(p, q)?;
            if o != Ordering::Equal {
                return Ok(o);
            }
        }
        Ok(x.len().cmp(&y.len()))
    }

    pub fn compare(&mut self, op: CmpOp, a: &Value, b: &Value) -> Res<bool> {
        Ok(match op {
            CmpOp::Eq => self.py_eq(a, b),
            CmpOp::NotEq => !self.py_eq(a, b),
            CmpOp::Is => self.py_is(a, b),
            CmpOp::IsNot => !self.py_is(a, b),
            CmpOp::In => self.contains(b, a)?,
            CmpOp::NotIn => !self.contains(b, a)?,
            CmpOp::Lt => self.order(a, b)? == Ordering::Less,
            CmpOp::LtE => self.order(a, b)? != Ordering::Greater,
            CmpOp::Gt => self.order(a, b)? == Ordering::Greater,
            CmpOp::GtE => self.order(a, b)? != Ordering::Less,
        })
    }

    pub fn binop(&mut self, op: BinOp, a: &Value, b: &Value) -> Res<Value> {
        match (op, a, b) {
            (BinOp::Add, Value::Str(x), Value::Str(y)) => Ok(Value::str(&format!("{x}{y}"))),
            (BinOp::Add, Value::Bytes(x), Value::Bytes(y)) => {
                Ok(Value::bytes(&[x.as_ref(), y.as_ref()].concat()))
            }
            (BinOp::Add, Value::Tuple(x), Value::Tuple(y)) => {
                Ok(Value::tuple(x.iter().chain(y.iter()).cloned().collect()))
            }
            (BinOp::Mod, Value::Str(fmt), _) => {
                let s = self.percent_format(fmt, b)?;
                Ok(Value::str(&s))
            }
            (BinOp::Mul, Value::Str(s), _) => Ok(Value::str(
                &s.repeat(b.as_int().unwrap_or(0).max(0) as usize),
            )),
            (BinOp::Mul, Value::Bytes(s), _) => Ok(Value::bytes(
                &s.repeat(b.as_int().unwrap_or(0).max(0) as usize),
            )),
            (BinOp::Mul, Value::Tuple(t), _) => {
                let n = b.as_int().unwrap_or(0).max(0) as usize;
                Ok(Value::tuple(
                    t.iter().cloned().cycle().take(t.len() * n).collect(),
                ))
            }
            (BinOp::Add, Value::Heap(_) | Value::Obj(_), Value::Heap(_) | Value::Obj(_)) => {
                let mut items = self.iterate(a)?;
                items.extend(self.iterate(b)?);
                Ok(self.new_list(items))
            }
            _ => self.arith(op, a, b),
        }
    }

    fn arith(&mut self, op: BinOp, a: &Value, b: &Value) -> Res<Value> {
        if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
            return self.int_op(op, x, y);
        }
        let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) else {
            let msg = format!(
                "unsupported operand types for {op:?}: {} and {}",
                type_name(self, a),
                type_name(self, b)
            );
            return self.raise("builtins.TypeError", msg);
        };
        Ok(Value::Float(match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => x / y,
            BinOp::FloorDiv => (x / y).floor(),
            BinOp::Mod => x - y * (x / y).floor(),
            BinOp::Pow => return self.raise("builtins.TypeError", "float power is not supported"),
            _ => return self.raise("builtins.TypeError", "bit operation on float"),
        }))
    }

    fn int_op(&mut self, op: BinOp, x: i64, y: i64) -> Res<Value> {
        let overflow = || "integer overflow".to_string();
        let v = match op {
            BinOp::Add => x.checked_add(y),
            BinOp::Sub => x.checked_sub(y),
            BinOp::Mul => x.checked_mul(y),
            BinOp::Div => {
                if y == 0 {
                    return self.raise("builtins.ZeroDivisionError", "division by zero");
                }
                return Ok(Value::Float(x as f64 / y as f64));
            }
            BinOp::FloorDiv | BinOp::Mod if y == 0 => {
                return self.raise("builtins.ZeroDivisionError", "division by zero");
            }
            BinOp::FloorDiv => Some(x / y - i64::from(x % y != 0 && ((x < 0) != (y < 0)))),
            BinOp::Mod => Some(((x % y) + y) % y),
            BinOp::Pow => u32::try_from(y).ok().and_then(|e| x.checked_pow(e)),
            BinOp::LShift => u32::try_from(y)
                .ok()
                .and_then(|s| x.checked_shl(s))
                .filter(|v| v >> y == x),
            BinOp::RShift => Some(if y >= 64 {
                if x < 0 { -1 } else { 0 }
            } else {
                x >> y
            }),
            BinOp::BitOr => Some(x | y),
            BinOp::BitXor => Some(x ^ y),
            BinOp::BitAnd => Some(x & y),
        };
        match v {
            Some(v) => Ok(Value::Int(v)),
            None => self.raise("builtins.OverflowError", overflow()),
        }
    }

    /// `str(v)`.
    pub fn py_str(&mut self, v: &Value) -> Res<String> {
        match v {
            Value::Str(s) => Ok(s.to_string()),
            _ => self.repr(v),
        }
    }

    /// `repr(v)` for the values grammars print in messages.
    pub fn repr(&mut self, v: &Value) -> Res<String> {
        Ok(match v {
            Value::None => "None".into(),
            Value::Bool(b) => {
                if *b {
                    "True".into()
                } else {
                    "False".into()
                }
            }
            Value::Int(i) => i.to_string(),
            Value::Float(f) => float_repr(*f),
            Value::Str(s) => format!("'{s}'"),
            Value::Bytes(b) => format!(
                "b'{}'",
                b.iter().map(|c| format!("\\x{c:02x}")).collect::<String>()
            ),
            Value::Tuple(t) => {
                let parts = t.iter().map(|x| self.repr(x)).collect::<Res<Vec<_>>>()?;
                if parts.len() == 1 {
                    format!("({},)", parts[0])
                } else {
                    format!("({})", parts.join(", "))
                }
            }
            Value::Heap(id) if matches!(self.obj(*id), HeapObj::Exception(_)) => {
                let HeapObj::Exception(e) = self.obj(*id) else {
                    return Ok(String::new());
                };
                e.message.clone()
            }
            Value::Heap(_) | Value::Obj(_) => match self.iterate(v) {
                Ok(items) => {
                    let parts = items
                        .iter()
                        .map(|x| self.repr(x))
                        .collect::<Res<Vec<_>>>()?;
                    format!("[{}]", parts.join(", "))
                }
                Err(_) => format!("<{}>", type_name(self, v)),
            },
            Value::Native(n) => format!("<{n}>"),
            Value::Method(m) => format!("<method {}>", m.1),
        })
    }

    /// `fmt % args` with the conversions grammars use (%d %i %s %r %x %X %f %g %c %%, flags and width).
    pub fn percent_format(&mut self, fmt: &str, args: &Value) -> Res<String> {
        let values: Vec<Value> = match args {
            Value::Tuple(t) => t.to_vec(),
            other => vec![other.clone()],
        };
        let mut next = values.into_iter();
        let mut out = String::new();
        let mut chars = fmt.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            let mut spec = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() || matches!(d, '-' | '+' | ' ' | '#' | '.') {
                    spec.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            let conv = chars.next().unwrap_or('%');
            if conv == '%' {
                out.push('%');
                continue;
            }
            let v = next.next().unwrap_or(Value::None);
            let text = self.convert(conv, &v)?;
            out.push_str(&pad(&text, &spec, conv));
        }
        Ok(out)
    }

    fn convert(&mut self, conv: char, v: &Value) -> Res<String> {
        Ok(match conv {
            'd' | 'i' | 'u' => match v {
                Value::Float(f) => (f.trunc() as i64).to_string(),
                _ => v
                    .as_int()
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "?".into()),
            },
            'x' => v.as_int().map(|i| hex(i, false)).unwrap_or_default(),
            'X' => v.as_int().map(|i| hex(i, true)).unwrap_or_default(),
            'f' | 'F' | 'g' | 'e' => as_f64(v).map(|f| format!("{f:.6}")).unwrap_or_default(),
            'r' => self.repr(v)?,
            'c' => v
                .as_int()
                .and_then(|i| char::from_u32(i as u32))
                .map(String::from)
                .unwrap_or_default(),
            _ => self.py_str(v)?,
        })
    }
}

fn hex(i: i64, upper: bool) -> String {
    let s = if i < 0 {
        format!("-{:x}", -(i as i128))
    } else {
        format!("{i:x}")
    };
    if upper { s.to_uppercase() } else { s }
}

/// Applies `%[flags][width][.precision]` to converted text.
fn pad(text: &str, spec: &str, conv: char) -> String {
    let left = spec.contains('-');
    let zero = spec
        .trim_start_matches(['-', '+', ' ', '#'])
        .starts_with('0');
    let (width_part, precision) = match spec.split_once('.') {
        Some((w, p)) => (w.to_string(), p.parse::<usize>().ok()),
        None => (spec.to_string(), None),
    };
    let width: usize = width_part
        .trim_start_matches(['-', '+', ' ', '#', '0'])
        .parse()
        .unwrap_or(0);
    let mut body = text.to_string();
    if let (Some(p), 'f' | 'F') = (precision, conv)
        && let Ok(f) = text.parse::<f64>()
    {
        body = format!("{f:.p$}");
    }
    if body.len() >= width {
        return body;
    }
    let fill = width - body.len();
    if left {
        return format!("{body}{}", " ".repeat(fill));
    }
    if zero && conv != 's' && conv != 'r' {
        let (sign, digits) = body.split_at(usize::from(body.starts_with('-')));
        return format!("{sign}{}{digits}", "0".repeat(fill));
    }
    format!("{}{body}", " ".repeat(fill))
}

pub(crate) fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        _ => v.as_int().map(|i| i as f64),
    }
}

/// Python's shortest round-trip float repr.
pub(crate) fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let s = format!("{f:?}");
    if s.contains('e') || s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}
