//! Loads the grammar snapshot written by the reference implementation's export tooling.

use std::collections::HashMap;
use std::sync::Arc;

use super::ast::FuncDef;
use super::astconv::Kinds;
use super::value::{Key, ObjId, Value};
use crate::bytes::ByteReader;
use crate::error::{CorlissError, Result};

const STRUCTURE: &str = "grammar snapshot";
const MAGIC: &[u8; 16] = b"CORLISS-GRAMMARS";
const FORMAT_VERSION: u32 = 1;

/// A function body.
pub(crate) struct Code {
    pub def: FuncDef,
}

/// A function object of the snapshot: code plus the values Python had bound to it.
pub(crate) struct SnapFn {
    pub qualname: Arc<str>,
    pub name: Arc<str>,
    pub module: Arc<str>,
    pub code: usize,
    pub globals: ObjId,
    pub defaults: Vec<ObjId>,
    pub kw_defaults: Vec<(Arc<str>, ObjId)>,
    pub cells: HashMap<Arc<str>, ObjId>,
    pub attrs: HashMap<Arc<str>, ObjId>,
}

/// A snapshot object.
pub(crate) enum Obj {
    /// A number, string, bytes, None, bool or tuple: its value is in `Program::consts`.
    Plain,
    List(Vec<ObjId>),
    Set(Vec<ObjId>),
    Dict(Vec<(ObjId, ObjId)>, HashMap<Key, usize>),
    Function(SnapFn),
    Namespace(HashMap<Arc<str>, ObjId>),
    EmptyCell,
}

/// The whole snapshot.
pub(crate) struct Program {
    pub codes: Vec<Code>,
    pub objects: Vec<Obj>,
    pub consts: Vec<Value>,
    pub modules: HashMap<String, ObjId>,
    pub readers: HashMap<String, ObjId>,
    pub segtypes: HashMap<String, String>,
    pub doctypes: HashMap<String, i64>,
}

/// Raw records before tuples are resolved to values.
enum Raw {
    Value(Value),
    Tuple(Vec<ObjId>),
    Obj(Obj),
    Native(Arc<str>),
    Unsupported(Arc<str>),
}

impl Program {
    /// Parses an uncompressed snapshot.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = ByteReader::new(data, STRUCTURE);
        if r.take(16, "magic")? != MAGIC {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "magic",
                0,
                "other bytes",
                "CORLISS-GRAMMARS",
            ));
        }
        let version = r.u32("version")?;
        if version != FORMAT_VERSION {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "version",
                16,
                version,
                FORMAT_VERSION,
            ));
        }
        let kinds = Kinds::read(&mut r)?;
        let codes = read_codes(&mut r, &kinds)?;
        let raws = read_objects(&mut r)?;
        let (objects, consts) = resolve(raws);
        let mut program = Program {
            codes,
            objects,
            consts,
            modules: HashMap::new(),
            readers: HashMap::new(),
            segtypes: HashMap::new(),
            doctypes: HashMap::new(),
        };
        program.read_roots(&mut r)?;
        r.finish()?;
        Ok(program)
    }

    fn read_roots(&mut self, r: &mut ByteReader<'_>) -> Result<()> {
        for _ in 0..r.count("modules", 8)? {
            let name = read_str(r)?;
            self.modules.insert(name, r.u32("module")?);
        }
        for _ in 0..r.count("readers", 8)? {
            let name = read_str(r)?;
            self.readers.insert(name, r.u32("table")?);
        }
        for _ in 0..r.count("segtypes", 8)? {
            let segtype = read_str(r)?;
            self.segtypes.insert(segtype, read_str(r)?);
        }
        for _ in 0..r.count("doctypes", 12)? {
            let ext = read_str(r)?;
            self.doctypes.insert(ext, r.i64("doctype")?);
        }
        Ok(())
    }

    /// The value form of object `id`.
    pub fn value(&self, id: ObjId) -> Value {
        self.consts.get(id as usize).cloned().unwrap_or(Value::None)
    }

    /// The object `id`.
    pub fn obj(&self, id: ObjId) -> Option<&Obj> {
        self.objects.get(id as usize)
    }

    /// Looks up `key` in the snapshot dict `id`.
    pub fn dict_get(&self, id: ObjId, key: &Value) -> Option<Value> {
        let Some(Obj::Dict(items, index)) = self.obj(id) else {
            return None;
        };
        let at = index.get(&Key::of(key)?)?;
        Some(self.value(items[*at].1))
    }
}

fn read_str(r: &mut ByteReader<'_>) -> Result<String> {
    Ok(String::from_utf8_lossy(&r.text8("str")?).into_owned())
}

fn read_arc(r: &mut ByteReader<'_>) -> Result<Arc<str>> {
    Ok(Arc::from(read_str(r)?))
}

fn read_codes(r: &mut ByteReader<'_>, kinds: &Kinds) -> Result<Vec<Code>> {
    let n = r.count("codes", 6)?;
    let mut codes = Vec::with_capacity(n);
    for _ in 0..n {
        // The origin (source file and line) only helps when reading the snapshot by hand.
        read_str(r)?;
        let node = kinds.read_node(r)?;
        codes.push(Code {
            def: kinds.convert_def(&node)?,
        });
    }
    Ok(codes)
}

fn read_ids(r: &mut ByteReader<'_>) -> Result<Vec<ObjId>> {
    let n = r.count("ids", 4)?;
    (0..n).map(|_| r.u32("id")).collect()
}

fn read_pairs(r: &mut ByteReader<'_>) -> Result<Vec<(Arc<str>, ObjId)>> {
    let n = r.count("pairs", 8)?;
    (0..n).map(|_| Ok((read_arc(r)?, r.u32("id")?))).collect()
}

fn read_objects(r: &mut ByteReader<'_>) -> Result<Vec<Raw>> {
    let n = r.count("objects", 1)?;
    (0..n).map(|_| read_object(r)).collect()
}

fn read_object(r: &mut ByteReader<'_>) -> Result<Raw> {
    let at = r.position();
    Ok(match r.u8("tag")? {
        0 => Raw::Value(Value::None),
        1 => Raw::Value(Value::Bool(true)),
        2 => Raw::Value(Value::Bool(false)),
        3 => Raw::Value(Value::Int(r.i64("int")?)),
        4 => Raw::Value(Value::Float(r.f64("float")?)),
        5 => Raw::Value(Value::Str(read_arc(r)?)),
        6 => Raw::Value(Value::bytes(&r.text8("bytes")?)),
        7 => Raw::Tuple(read_ids(r)?),
        8 => Raw::Obj(Obj::List(read_ids(r)?)),
        9 => Raw::Obj(read_dict(r)?),
        10 => Raw::Obj(Obj::Set(read_ids(r)?)),
        11 => Raw::Obj(Obj::Function(read_function(r)?)),
        12 => {
            // The namespace's module name is informational; lookups go through `modules`.
            read_arc(r)?;
            Raw::Obj(Obj::Namespace(read_pairs(r)?.into_iter().collect()))
        }
        13 => Raw::Native(read_arc(r)?),
        14 => Raw::Unsupported(read_arc(r)?),
        15 => Raw::Obj(Obj::EmptyCell),
        tag => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "tag",
                at,
                tag,
                "an object tag 0-15",
            ));
        }
    })
}

fn read_dict(r: &mut ByteReader<'_>) -> Result<Obj> {
    let n = r.count("dict", 8)?;
    let items = (0..n)
        .map(|_| Ok((r.u32("key")?, r.u32("value")?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(Obj::Dict(items, HashMap::new()))
}

fn read_function(r: &mut ByteReader<'_>) -> Result<SnapFn> {
    let qualname = read_arc(r)?;
    let name = read_arc(r)?;
    let module = read_arc(r)?;
    let code = r.u32("code")? as usize;
    let globals = r.u32("globals")?;
    let defaults = read_ids(r)?;
    let kw_defaults = read_pairs(r)?;
    let cells = read_pairs(r)?.into_iter().collect();
    let attrs = read_pairs(r)?.into_iter().collect();
    Ok(SnapFn {
        qualname,
        name,
        module,
        code,
        globals,
        defaults,
        kw_defaults,
        cells,
        attrs,
    })
}

/// Turns raw records into objects plus the value form of every object; builds dict indexes.
fn resolve(raws: Vec<Raw>) -> (Vec<Obj>, Vec<Value>) {
    let mut consts: Vec<Option<Value>> = vec![None; raws.len()];
    for id in 0..raws.len() {
        const_of(&raws, &mut consts, id);
    }
    let consts: Vec<Value> = consts
        .into_iter()
        .map(|v| v.unwrap_or(Value::None))
        .collect();
    let objects = raws
        .into_iter()
        .map(|raw| match raw {
            Raw::Obj(Obj::Dict(items, _)) => {
                let index = items
                    .iter()
                    .enumerate()
                    .filter_map(|(k, (key, _))| Some((Key::of(&consts[*key as usize])?, k)))
                    .collect();
                Obj::Dict(items, index)
            }
            Raw::Obj(obj) => obj,
            _ => Obj::Plain,
        })
        .collect();
    (objects, consts)
}

fn const_of(raws: &[Raw], consts: &mut Vec<Option<Value>>, id: usize) -> Value {
    if let Some(v) = &consts[id] {
        return v.clone();
    }
    let v = match &raws[id] {
        Raw::Value(v) => v.clone(),
        Raw::Tuple(items) => {
            // Mark first so a (never expected) cyclic tuple cannot recurse forever.
            consts[id] = Some(Value::None);
            Value::tuple(
                items
                    .iter()
                    .map(|i| const_of(raws, consts, *i as usize))
                    .collect(),
            )
        }
        Raw::Native(name) => Value::Native(name.clone()),
        Raw::Unsupported(what) => Value::Native(Arc::from(format!("unsupported:{what}"))),
        Raw::Obj(_) => Value::Obj(id as ObjId),
    };
    consts[id] = Some(v.clone());
    v
}
