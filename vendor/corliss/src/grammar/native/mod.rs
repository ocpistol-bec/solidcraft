//! Natives: the engine objects grammars use (the reference grammar engine's module, `struct`,
//! `importlib`, the grammar table lookup) and the methods of built-in types.

pub(crate) mod args;
mod builtins;
mod list_walkers;
mod lists;
mod methods;
mod methods_text;
pub(crate) mod parser;
mod parser_methods;
mod structs;
pub(crate) mod token;

use std::sync::Arc;

use args::Args;
use token::Token;

use crate::grammar::interp::{Flow, Interp, Res};
use crate::grammar::value::Value;

/// Calls the native `name`.
pub(crate) fn call_native(
    it: &mut Interp,
    name: Arc<str>,
    pos: Vec<Value>,
    kw: Vec<(Arc<str>, Value)>,
) -> Res<Value> {
    let a = Args::new(pos, kw);
    let (module, item) = name.split_once('.').unwrap_or(("builtins", &name));
    match module {
        "builtins" => it.builtin(item, a),
        "dcgram" => dcgram(it, item, a),
        "struct" => struct_call(it, item, &a),
        "importlib" if item == "import_module" => {
            let n = a.or(0, "name", Value::None);
            match it.module(n.as_str().unwrap_or("")) {
                Some(m) => Ok(m),
                None => it.raise(
                    "builtins.ModuleNotFoundError",
                    format!("no module {}", n.as_str().unwrap_or("?")),
                ),
            }
        }
        "registry" if item == "table_for" => table_for(it, &a),
        _ => Err(Flow::Fatal(format!("native {name} is not implemented"))),
    }
}

/// Calls method `name` of `recv`.
pub(crate) fn call_method(
    it: &mut Interp,
    recv: Value,
    name: Arc<str>,
    pos: Vec<Value>,
    kw: Vec<(Arc<str>, Value)>,
) -> Res<Value> {
    it.method(recv, &name, Args::new(pos, kw))
}

/// An attribute of a native object or module.
pub(crate) fn native_attr(_it: &Interp, native: &Arc<str>, name: &Arc<str>) -> Value {
    if let Some(module) = native.strip_prefix("module:") {
        let short = module.rsplit('.').next().unwrap_or(module);
        return Value::Native(Arc::from(format!("{short}.{name}")));
    }
    match name.as_ref() {
        "__name__" | "__qualname__" => Value::str(native.rsplit('.').next().unwrap_or(native)),
        _ => Value::Native(Arc::from(format!("{native}.{name}"))),
    }
}

fn bytes_arg(it: &mut Interp, a: &Args, k: usize, name: &str) -> Res<Arc<[u8]>> {
    match a.get(k, name) {
        Some(Value::Bytes(b)) => Ok(b),
        _ => it.raise("builtins.TypeError", format!("{name} must be bytes")),
    }
}

fn dcgram(it: &mut Interp, item: &str, a: Args) -> Res<Value> {
    match item {
        "Parser" => {
            let data = bytes_arg(it, &a, 0, "data")?;
            let ctx = a.or(1, "ctx", Value::None);
            Ok(it.new_parser(data, ctx))
        }
        "Tok" => {
            let kind = a.or(0, "kind", Value::str("raw"));
            let raw = match a.get(3, "raw") {
                Some(Value::Bytes(b)) => b.to_vec(),
                _ => Vec::new(),
            };
            let mut t = Token::new(
                kind.as_str().unwrap_or("raw"),
                a.or(1, "name", Value::None),
                a.or(2, "value", Value::None),
                &raw,
                None,
            );
            t.typ = a.or(4, "typ", Value::None);
            t.extra = a.or(5, "extra", Value::None);
            Ok(it.new_token(t))
        }
        "GrammarError" => {
            let msg = match a.get(0, "msg") {
                Some(v) => it.py_str(&v)?,
                None => String::new(),
            };
            it.raise_value("dcgram.GrammarError", msg)
        }
        _ => dcgram_reader(it, item, &a),
    }
}

/// The grammar engine's module-level readers: each returns `(token, next offset)`.
fn dcgram_reader(it: &mut Interp, item: &str, a: &Args) -> Res<Value> {
    let buf = bytes_arg(it, a, 0, "buf")?;
    let i = a.int(1, "i").unwrap_or(0).max(0) as usize;
    let (tok, next) = match item {
        "read_scalar" => {
            let typ = a
                .or(2, "typ", Value::None)
                .as_str()
                .and_then(|s| s.chars().next())
                .unwrap_or('I');
            it.read_scalar(&buf, i, typ, &a.or(3, "name", Value::None))?
        }
        "read_text8" => it.read_text8(&buf, i, &a.or(2, "name", Value::None))?,
        "read_text16" => it.read_text16(&buf, i, &a.or(2, "name", Value::None))?,
        "read_uuid" => it.read_uuid(&buf, i, &a.or(2, "name", Value::None))?,
        "read_bool" => it.read_bool(&buf, i, &a.or(2, "name", Value::None))?,
        "read_ref" => it.read_ref(
            &buf,
            i,
            a.int(2, "refkind").unwrap_or(1),
            &a.or(3, "name", Value::None),
        )?,
        "parse_list" => {
            let code = a.int(2, "code").unwrap_or(2);
            let typ = a.int(3, "typ").unwrap_or(0);
            let ctx = a.or(4, "ctx", Value::None);
            let size = a.int(6, "arraysize").unwrap_or(1);
            it.parse_list(
                &buf,
                i,
                code,
                typ,
                &ctx,
                &a.or(5, "name", Value::None),
                size,
            )?
        }
        "_need" => {
            it.need(&buf, i, a.int(2, "n").unwrap_or(0).max(0) as usize)?;
            return Ok(Value::None);
        }
        other => return Err(Flow::Fatal(format!("dcgram.{other} is not implemented"))),
    };
    let tok = it.new_token(tok);
    Ok(Value::tuple(vec![tok, Value::Int(next as i64)]))
}

fn struct_call(it: &mut Interp, item: &str, a: &Args) -> Res<Value> {
    let fmt = a
        .or(0, "fmt", Value::None)
        .as_str()
        .unwrap_or("")
        .to_string();
    match item {
        "unpack_from" | "unpack" => {
            let buf = bytes_arg(it, a, 1, "buffer")?;
            let offset = a.int(2, "offset").unwrap_or(0).max(0) as usize;
            structs::unpack_from(it, &fmt, &buf, offset)
        }
        "pack" => {
            let values = a.pos.get(1..).unwrap_or_default().to_vec();
            Ok(Value::bytes(&structs::pack(it, &fmt, &values)?))
        }
        "calcsize" => Ok(Value::Int(structs::calcsize(it, &fmt)? as i64)),
        other => Err(Flow::Fatal(format!("struct.{other} is not implemented"))),
    }
}

/// The grammar table lookup `table_for(segtype)` → (reader name, {TypeID: grammar}).
fn table_for(it: &mut Interp, a: &Args) -> Res<Value> {
    let segtype = a.or(0, "segtype", Value::None);
    let segtype = segtype.as_str().unwrap_or("");
    let prog = it.prog;
    let Some(reader) = prog.segtypes.get(segtype) else {
        return Ok(Value::tuple(vec![Value::None, it.new_dict(Vec::new())?]));
    };
    let table = prog
        .readers
        .get(reader)
        .map_or(Value::None, |id| Value::Obj(*id));
    Ok(Value::tuple(vec![Value::str(reader), table]))
}
