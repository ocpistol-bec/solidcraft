//! The canonical text and FNV-1a fingerprint of decoded nodes.
//!
//! The text is the one the reference implementation's export tooling writes for the reference
//! engine, so equal fingerprints mean Corliss decoded every field, name, value and byte the
//! same way.

use std::fmt::Write;

use super::tree::{DecodeStatus, DecodedNode, Field, FieldValue, OtherValue};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 over `data`, continuing from `h`.
pub fn fnv1a(data: &[u8], mut h: u64) -> u64 {
    for b in data {
        h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
    }
    h
}

/// The fingerprint of a segment's decoded nodes, in order.
///
/// ```
/// assert_eq!(corliss::grammar::fingerprint(&[]), 0xcbf2_9ce4_8422_2325);
/// ```
pub fn fingerprint(nodes: &[DecodedNode]) -> u64 {
    nodes.iter().fold(FNV_OFFSET, |h, n| {
        let mut text = node_text(n);
        text.push('\n');
        fnv1a(text.as_bytes(), h)
    })
}

/// The canonical text of one node.
pub fn node_text(n: &DecodedNode) -> String {
    let flag = match n.status {
        DecodeStatus::NoGrammar => return format!("{:08X}:nogrammar", n.type_id.0),
        DecodeStatus::Full => "full",
        DecodeStatus::Partial => "partial",
    };
    let mut out = format!("{:08X}:{flag}:", n.type_id.0);
    n.fields.iter().for_each(|f| write_field(&mut out, f));
    out
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .fold(String::with_capacity(2 * b.len()), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

fn write_field(out: &mut String, f: &Field) {
    let name = f.name.as_deref().unwrap_or("~");
    if let FieldValue::List(l) = &f.value {
        let typ = l.element_type.map_or("None".to_string(), |t| t.to_string());
        let meta = l.meta.as_deref().map_or("n".to_string(), hex);
        let _ = write!(
            out,
            "L:{name};C:{};Y:{typ};F:{};M:{meta};[",
            l.code,
            hex(&l.framing)
        );
        for el in &l.elements {
            out.push('{');
            el.iter().for_each(|x| write_field(out, x));
            out.push('}');
        }
        out.push(']');
        return;
    }
    let (kind, value) = kind_and_value(&f.value);
    let _ = write!(out, "K:{kind};N:{name};V:{value};R:{}", hex(&f.raw));
    if let FieldValue::Ref(_, k) = &f.value {
        let _ = write!(out, ";E:{}", k.code());
    }
    out.push('|');
}

fn float(x: f64) -> String {
    format!("f{:016x}", x.to_bits())
}

fn tuple<T: Copy>(items: &[T], each: impl Fn(T) -> String) -> String {
    format!(
        "({})",
        items.iter().map(|x| each(*x)).collect::<Vec<_>>().join(",")
    )
}

fn int(i: impl Into<i64>) -> String {
    format!("i{}", i.into())
}

fn text(s: &str) -> String {
    format!("s{}:{s}", s.len())
}

fn kind_and_value(v: &FieldValue) -> (String, String) {
    let (k, s) = match v {
        FieldValue::U8(x) => ("u8", int(*x)),
        FieldValue::I8(x) => ("s8", int(*x)),
        FieldValue::U16(x) => ("u16", int(*x)),
        FieldValue::I16(x) => ("s16", int(*x)),
        FieldValue::U32(x) => ("u32", int(*x)),
        FieldValue::I32(x) => ("s32", int(*x)),
        FieldValue::F32(x) => ("f32", float(f64::from(*x))),
        FieldValue::F64(x) => ("f64", float(*x)),
        FieldValue::U8s(x) => ("u8", tuple(x, int)),
        FieldValue::I8s(x) => ("s8", tuple(x, int)),
        FieldValue::U16s(x) => ("u16", tuple(x, int)),
        FieldValue::I16s(x) => ("s16", tuple(x, int)),
        FieldValue::U32s(x) => ("u32", tuple(x, int)),
        FieldValue::I32s(x) => ("s32", tuple(x, int)),
        FieldValue::F32s(x) => ("f32", tuple(x, |y| float(f64::from(y)))),
        FieldValue::F64s(x) => ("f64", tuple(x, float)),
        FieldValue::Bool(b) => ("bool", if *b { "T".into() } else { "F".into() }),
        FieldValue::Guid(g) => ("uuid", format!("b{}", hex(g.as_bytes()))),
        FieldValue::Text(s) => ("text16", text(s)),
        FieldValue::Text8(s) => ("text8", text(s)),
        FieldValue::Text8Bytes(b) => ("text8", format!("b{}", hex(b))),
        FieldValue::Ref(r, _) => ("ref", int(r.0)),
        FieldValue::Color(c) => ("color", tuple(c, |y| float(f64::from(y)))),
        FieldValue::Raw => ("raw", "n".into()),
        FieldValue::List(_) => ("list", String::new()),
        FieldValue::Other(kind, o) => return (kind.clone(), other(o)),
    };
    (k.to_string(), s)
}

fn other(o: &OtherValue) -> String {
    match o {
        OtherValue::None => "n".into(),
        OtherValue::Bool(b) => {
            if *b {
                "T".into()
            } else {
                "F".into()
            }
        }
        OtherValue::Int(i) => int(*i),
        OtherValue::Float(f) => float(*f),
        OtherValue::Str(s) => text(s),
        OtherValue::Bytes(b) => format!("b{}", hex(b)),
        OtherValue::Tuple(t) => format!("({})", t.iter().map(other).collect::<Vec<_>>().join(",")),
    }
}
