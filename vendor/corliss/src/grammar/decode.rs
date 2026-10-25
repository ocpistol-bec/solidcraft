//! Runs node grammars over a segment's payloads and converts their tokens to typed fields.

use std::collections::HashMap;
use std::sync::Arc;

use super::interp::{Flow, HeapObj, Interp};
use super::snapshot::Program;
use super::tree::{DecodeStatus, DecodedNode, Field, FieldValue, ListField, OtherValue, RefKind};
use super::value::{HeapId, Value};
use crate::guid::{Guid, TypeId};
use crate::node::NodeRef;

/// The version context grammars read (the reference grammar engine's context object).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeContext {
    /// Release year the grammars switch on (1996 + the RSeDb major, 2027 for major 31).
    pub file_version: i64,
    /// Release major.
    pub seg_major: i64,
    /// Document type code the grammar tables are keyed by (1 assembly, 2 drawing, 3 part,
    /// 4 presentation).
    pub doctype: i64,
    /// False in template segments, whose nodes have no object-id head (spec 11.6.2).
    pub rse_ided: bool,
}

/// Decodes `nodes` (TypeID, payload) of a segment of type `segtype` with one interpreter session.
pub(crate) fn decode_nodes(
    prog: &'static Program,
    segtype: &str,
    ctx: DecodeContext,
    nodes: &[(TypeId, &[u8])],
) -> Vec<DecodedNode> {
    let table = prog
        .segtypes
        .get(segtype)
        .and_then(|reader| prog.readers.get(reader))
        .copied();
    let mut it = Interp::new(prog);
    let ctx_value = make_ctx(&mut it, ctx);
    nodes
        .iter()
        .map(|(tid, payload)| {
            let grammar = table.and_then(|t| prog.dict_get(t, &Value::Int(i64::from(tid.0))));
            match grammar {
                Some(g) => decode_one(&mut it, &g, &ctx_value, *tid, payload),
                None => no_grammar(*tid, payload),
            }
        })
        .collect()
}

fn make_ctx(it: &mut Interp, ctx: DecodeContext) -> Value {
    let attrs = HashMap::from([
        (Arc::from("file"), Value::Int(ctx.file_version)),
        (Arc::from("major"), Value::Int(ctx.seg_major)),
        (Arc::from("doctype"), Value::Int(ctx.doctype)),
        (Arc::from("rse_ided"), Value::Bool(ctx.rse_ided)),
    ]);
    it.alloc(HeapObj::Attrs(attrs))
}

fn no_grammar(tid: TypeId, payload: &[u8]) -> DecodedNode {
    let field = Field {
        name: None,
        value: FieldValue::Raw,
        raw: payload.to_vec(),
        aliases: Vec::new(),
    };
    DecodedNode {
        type_id: tid,
        status: DecodeStatus::NoGrammar,
        fields: vec![field],
        error: None,
    }
}

fn decode_one(
    it: &mut Interp,
    grammar: &Value,
    ctx: &Value,
    tid: TypeId,
    payload: &[u8],
) -> DecodedNode {
    let mark = it.heap.len();
    let data: Arc<[u8]> = Arc::from(payload);
    let parser = it.new_parser(data, ctx.clone());
    let outcome = it.call(grammar, vec![parser.clone(), ctx.clone()], Vec::new());
    let Value::Heap(pid) = parser else {
        return no_grammar(tid, payload);
    };
    let mut error = match outcome {
        Ok(_) => None,
        Err(Flow::Exception(e)) => Some(exception_text(it, &e)),
        Err(Flow::Fatal(msg)) => Some(format!("interpreter: {msg}")),
    };
    let consumed = it.parser(pid).map_or(-1, |p| p.i);
    if error.is_none() && consumed != payload.len() as i64 {
        error = Some(format!(
            "grammar read {consumed} of {} bytes",
            payload.len()
        ));
    }
    let toks = top_tokens(it, pid);
    let node = match error {
        None => DecodedNode {
            type_id: tid,
            status: DecodeStatus::Full,
            fields: fields(it, &toks),
            error: None,
        },
        Some(e) => resync(it, &toks, tid, payload, e),
    };
    release(it, mark);
    node
}

fn exception_text(it: &Interp, e: &Value) -> String {
    match e {
        Value::Heap(id) => match it.obj(*id) {
            HeapObj::Exception(x) => format!("{}: {}", x.class, x.message),
            _ => "exception".into(),
        },
        _ => "exception".into(),
    }
}

fn top_tokens(it: &mut Interp, pid: HeapId) -> Vec<HeapId> {
    let Ok(list) = it.parser(pid).map(|p| p.toks) else {
        return Vec::new();
    };
    match it.obj(list) {
        HeapObj::List(items) => items
            .iter()
            .filter_map(|v| match v {
                Value::Heap(id) if it.token(*id).is_some() => Some(*id),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Keeps the longest token prefix that reproduces the payload, then one raw tail, as the
/// reference grammar engine resynchronises.
fn resync(
    it: &mut Interp,
    toks: &[HeapId],
    tid: TypeId,
    payload: &[u8],
    error: String,
) -> DecodedNode {
    let mut done = 0;
    let mut keep = Vec::new();
    for id in toks {
        let Ok(b) = it.serialize_token(*id) else {
            break;
        };
        if payload.get(done..done + b.len()) != Some(b.as_slice()) {
            break;
        }
        done += b.len();
        keep.push(*id);
    }
    let mut out = fields(it, &keep);
    if done < payload.len() {
        let raw = payload[done..].to_vec();
        out.push(Field {
            name: Some("partial_tail".into()),
            value: FieldValue::Raw,
            raw,
            aliases: Vec::new(),
        });
    }
    DecodedNode {
        type_id: tid,
        status: DecodeStatus::Partial,
        fields: out,
        error: Some(error),
    }
}

/// Drops the node's heap objects unless a module global still reaches one of them.
fn release(it: &mut Interp, mark: usize) {
    let roots: Vec<Value> = it.globals.values().cloned().collect();
    let mut stack = roots;
    let mut seen = std::collections::HashSet::new();
    while let Some(v) = stack.pop() {
        let Value::Heap(id) = v else { continue };
        if id as usize >= mark {
            return;
        }
        if !seen.insert(id) {
            continue;
        }
        match it.obj(id) {
            HeapObj::List(items) => stack.extend(items.iter().cloned()),
            HeapObj::Dict(d) | HeapObj::Set(d) => d
                .items
                .iter()
                .for_each(|(k, v)| stack.extend([k.clone(), v.clone()])),
            HeapObj::Attrs(m) => stack.extend(m.values().cloned()),
            _ => {}
        }
    }
    it.heap.truncate(mark);
}

fn fields(it: &mut Interp, toks: &[HeapId]) -> Vec<Field> {
    toks.iter().filter_map(|id| field(it, *id)).collect()
}

fn field(it: &mut Interp, id: HeapId) -> Option<Field> {
    let t = it.token(id)?.clone();
    let name = t.name.as_str().map(str::to_string);
    let aliases = match &t.alias {
        Value::Str(a) => vec![a.to_string()],
        Value::Tuple(items) => items
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    if t.kind.as_ref() == "list" {
        let list = list_field(it, &t.value, &t.extra);
        return Some(Field {
            name,
            value: FieldValue::List(list),
            raw: Vec::new(),
            aliases,
        });
    }
    let value = typed(&t.kind, &t.value, &t.extra);
    Some(Field {
        name,
        value,
        raw: t.raw.to_vec(),
        aliases,
    })
}

fn list_field(it: &mut Interp, value: &Value, extra: &Value) -> ListField {
    let get = |it: &mut Interp, k: &str| -> Value {
        let key = Value::str(k);
        match it.contains(extra, &key) {
            Ok(true) => it.getitem(extra, &key).unwrap_or(Value::None),
            _ => Value::None,
        }
    };
    let bytes = |v: Value| match v {
        Value::Bytes(b) => Some(b.to_vec()),
        _ => None,
    };
    let code = get(it, "code").as_int().unwrap_or(0);
    let element_type = get(it, "typ").as_int();
    let framing = bytes(get(it, "framing_raw")).unwrap_or_default();
    let meta = bytes(get(it, "meta_raw"));
    let elements = it.iterate(value).unwrap_or_default();
    let elements = elements
        .iter()
        .map(|el| {
            let toks: Vec<HeapId> = it
                .iterate(el)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| match v {
                    Value::Heap(id) => Some(id),
                    _ => None,
                })
                .collect();
            fields(it, &toks)
        })
        .collect();
    ListField {
        code,
        element_type,
        framing,
        meta,
        elements,
    }
}

fn ints(v: &Value) -> Option<Vec<i64>> {
    match v {
        Value::Tuple(t) => t.iter().map(Value::as_int).collect(),
        _ => None,
    }
}

fn floats(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Tuple(t) => t
            .iter()
            .map(|x| match x {
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// The typed value of a non-list token; tokens of unexpected shape become `Other`.
fn typed(kind: &str, value: &Value, extra: &Value) -> FieldValue {
    typed_known(kind, value, extra)
        .unwrap_or_else(|| FieldValue::Other(kind.to_string(), other(value)))
}

fn typed_known(kind: &str, value: &Value, extra: &Value) -> Option<FieldValue> {
    if matches!(value, Value::Tuple(_)) {
        return typed_array(kind, value);
    }
    let i = value.as_int().filter(|_| !matches!(value, Value::Bool(_)));
    Some(match (kind, value) {
        ("u8", _) => FieldValue::U8(u8::try_from(i?).ok()?),
        ("s8", _) => FieldValue::I8(i8::try_from(i?).ok()?),
        ("u16", _) => FieldValue::U16(u16::try_from(i?).ok()?),
        ("s16", _) => FieldValue::I16(i16::try_from(i?).ok()?),
        ("u32", _) => FieldValue::U32(u32::try_from(i?).ok()?),
        ("s32", _) => FieldValue::I32(i32::try_from(i?).ok()?),
        ("f32", Value::Float(f)) if f64::from(*f as f32).to_bits() == f.to_bits() => {
            FieldValue::F32(*f as f32)
        }
        ("f64", Value::Float(f)) => FieldValue::F64(*f),
        ("bool", Value::Bool(b)) => FieldValue::Bool(*b),
        ("uuid", Value::Bytes(b)) => {
            FieldValue::Guid(Guid::from_bytes(b.as_ref().try_into().ok()?))
        }
        ("text16", Value::Str(s)) => FieldValue::Text(s.to_string()),
        ("text8", Value::Str(s)) => FieldValue::Text8(s.to_string()),
        ("text8", Value::Bytes(b)) => FieldValue::Text8Bytes(b.to_vec()),
        ("ref", _) => FieldValue::Ref(
            NodeRef(u32::try_from(i?).ok()?),
            RefKind::from_code(extra.as_int()?),
        ),
        ("raw", Value::None) => FieldValue::Raw,
        _ => return None,
    })
}

fn typed_array(kind: &str, value: &Value) -> Option<FieldValue> {
    Some(match kind {
        "u8" => FieldValue::U8s(
            ints(value)?
                .into_iter()
                .map(|x| u8::try_from(x).ok())
                .collect::<Option<_>>()?,
        ),
        "s8" => FieldValue::I8s(
            ints(value)?
                .into_iter()
                .map(|x| i8::try_from(x).ok())
                .collect::<Option<_>>()?,
        ),
        "u16" => FieldValue::U16s(
            ints(value)?
                .into_iter()
                .map(|x| u16::try_from(x).ok())
                .collect::<Option<_>>()?,
        ),
        "s16" => FieldValue::I16s(
            ints(value)?
                .into_iter()
                .map(|x| i16::try_from(x).ok())
                .collect::<Option<_>>()?,
        ),
        "u32" => FieldValue::U32s(
            ints(value)?
                .into_iter()
                .map(|x| u32::try_from(x).ok())
                .collect::<Option<_>>()?,
        ),
        "s32" => FieldValue::I32s(
            ints(value)?
                .into_iter()
                .map(|x| i32::try_from(x).ok())
                .collect::<Option<_>>()?,
        ),
        "f32" => {
            let f = floats(value)?;
            if f.iter()
                .any(|x| f64::from(*x as f32).to_bits() != x.to_bits())
            {
                return None;
            }
            FieldValue::F32s(f.into_iter().map(|x| x as f32).collect())
        }
        "f64" => FieldValue::F64s(floats(value)?),
        "color" => {
            let f = floats(value)?;
            let c: [f32; 4] = f
                .iter()
                .map(|x| *x as f32)
                .collect::<Vec<_>>()
                .try_into()
                .ok()?;
            if f.iter()
                .zip(c)
                .any(|(x, y)| f64::from(y).to_bits() != x.to_bits())
            {
                return None;
            }
            FieldValue::Color(c)
        }
        _ => return None,
    })
}

fn other(v: &Value) -> OtherValue {
    match v {
        Value::None => OtherValue::None,
        Value::Bool(b) => OtherValue::Bool(*b),
        Value::Int(i) => OtherValue::Int(*i),
        Value::Float(f) => OtherValue::Float(*f),
        Value::Str(s) => OtherValue::Str(s.to_string()),
        Value::Bytes(b) => OtherValue::Bytes(b.to_vec()),
        Value::Tuple(t) => OtherValue::Tuple(t.iter().map(other).collect()),
        _ => OtherValue::Str("<object>".into()),
    }
}
