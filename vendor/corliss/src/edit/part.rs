//! Part document edits on the PmDCSegment.

use super::Editor;
use super::field::node_ref;
use crate::error::{CorlissError, Result};
use crate::grammar::{Field, FieldValue, ListField, RefKind};

const T_PARAMETER: u32 = 0x9087_4D26;
const T_COMPONENT: u32 = 0x9087_4D63;
const T_USERPARAM_MARK: u32 = 0x2F39_A056;
const SEGMENT: &str = "PmDCSegment";
const STRUCTURE: &str = "part parameters";

/// Adds user parameter `name` = `value` (database units: cm, rad) to a part that already has
/// one: the first user parameter's owned subgraph is cloned, renamed and linked into the
/// component node the way Inventor 2027 does. Returns the new parameter's node index.
///
/// ```no_run
/// let doc = corliss::Document::open("part.ipt")?;
/// let mut ed = corliss::edit::Editor::new(&doc)?;
/// corliss::edit::add_user_parameter(&mut ed, "width", 3.0)?;
/// ed.finish()?.save("edited.ipt")?;
/// # Ok::<(), corliss::CorlissError>(())
/// ```
pub fn add_user_parameter(ed: &mut Editor, name: &str, value: f64) -> Result<usize> {
    let seg = ed.segment(SEGMENT)?;
    let comp = only_component(&seg.of_type(T_COMPONENT))?;
    ensure_new_name(seg, comp, name)?;
    let src = first_user_parameter(seg)?;
    let mapping = seg.clone_subgraph(src)?;
    let new = mapping
        .get(&src)
        .copied()
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "cloned parameter"))?;
    let index = seg
        .node(comp)
        .and_then(|n| n.node.get("index"))
        .cloned()
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "component index"))?;
    set_identity(seg, new, name, value, &index)?;
    let comp_node = &mut seg
        .node_mut(comp)
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "component"))?
        .node;
    comp_node.set("index", with_int(&index, index.as_int().unwrap_or(0) + 1)?)?;
    link(comp_node, name, new)?;
    Ok(new)
}

fn only_component(found: &[usize]) -> Result<usize> {
    match found {
        [one] => Ok(*one),
        _ => Err(CorlissError::invalid(
            STRUCTURE,
            "component",
            0,
            format!("{} component nodes", found.len()),
            "exactly one component node (90874D63)",
        )),
    }
}

fn ensure_new_name(seg: &super::EditSegment, comp: usize, name: &str) -> Result<()> {
    let params = seg.node(comp).and_then(|n| n.node.get("parameters"));
    if !params.is_some_and(|v| map_keys(v).iter().any(|k| k == name)) {
        return Ok(());
    }
    let expected = "a parameter name not yet in the part";
    Err(CorlissError::invalid(
        STRUCTURE, "name", comp, name, expected,
    ))
}

/// The text keys of a name map.
fn map_keys(v: &FieldValue) -> Vec<String> {
    let FieldValue::List(l) = v else {
        return Vec::new();
    };
    l.elements
        .iter()
        .filter_map(|el| el.first())
        .filter_map(|f| match &f.value {
            FieldValue::Text(s) | FieldValue::Text8(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Parameters that own a user-parameter marker child.
fn first_user_parameter(seg: &super::EditSegment) -> Result<usize> {
    let is_marker = |i: Option<usize>| {
        i.and_then(|i| seg.node(i))
            .is_some_and(|n| n.node.type_id.0 == T_USERPARAM_MARK)
    };
    seg.of_type(T_PARAMETER)
        .into_iter()
        .find(|p| {
            seg.node(*p).is_some_and(|n| {
                n.node
                    .refs()
                    .iter()
                    .any(|(r, k)| *k == RefKind::Child && is_marker(r.index()))
            })
        })
        .ok_or_else(|| {
            CorlissError::missing(
                "edit",
                STRUCTURE,
                "a user parameter to clone (create one in Inventor first)",
            )
        })
}

/// Name, values and object index of the new parameter, and the value of its formula.
fn set_identity(
    seg: &mut super::EditSegment,
    new: usize,
    name: &str,
    value: f64,
    index: &FieldValue,
) -> Result<()> {
    let p = &mut seg
        .node_mut(new)
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, "new parameter"))?
        .node;
    p.set("name", FieldValue::Text(name.to_string()))?;
    p.set("valueNominal", FieldValue::F64(value))?;
    p.set("valueModel", FieldValue::F64(value))?;
    p.set("index", index.clone())?;
    let formula = match p.get("formula") {
        Some(FieldValue::Ref(r, _)) => r.index(),
        _ => None,
    };
    let Some(f) = formula.and_then(|i| seg.node_mut(i)) else {
        return Ok(());
    };
    if f.node.field("value").is_some() {
        f.node.set("value", FieldValue::F64(value))?;
    }
    Ok(())
}

/// Lists the parameter in the component's objects and, in key order, its name map.
fn link(comp: &mut crate::grammar::DecodedNode, name: &str, new: usize) -> Result<()> {
    let objects = list(comp, "objects")?;
    objects.push(vec![Field::encoded(
        "objects",
        node_ref(new, true, RefKind::Child),
    )?]);
    let entry = vec![
        Field::encoded("parameters", FieldValue::Text(name.to_string()))?,
        Field::encoded("parameters", node_ref(new, true, RefKind::Child))?,
    ];
    let params = list(comp, "parameters")?;
    let keys = map_keys(&FieldValue::List(params.clone()));
    let at = keys.iter().take_while(|k| k.as_str() < name).count();
    params.insert(at, entry);
    Ok(())
}

fn list<'a>(node: &'a mut crate::grammar::DecodedNode, name: &str) -> Result<&'a mut ListField> {
    node.list_mut(name)
        .ok_or_else(|| CorlissError::missing("edit", STRUCTURE, format!("list {name}")))
}

/// `v` in the integer width of `like`.
fn with_int(like: &FieldValue, v: i64) -> Result<FieldValue> {
    Ok(match like {
        FieldValue::U8(_) => FieldValue::U8(v as u8),
        FieldValue::I8(_) => FieldValue::I8(v as i8),
        FieldValue::U16(_) => FieldValue::U16(v as u16),
        FieldValue::I16(_) => FieldValue::I16(v as i16),
        FieldValue::U32(_) => FieldValue::U32(v as u32),
        FieldValue::I32(_) => FieldValue::I32(v as i32),
        other => {
            return Err(CorlissError::invalid(
                STRUCTURE,
                "index",
                0,
                format!("{other:?}"),
                "an integer field",
            ));
        }
    })
}
