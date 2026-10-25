//! Checking a project file against spec clause 19, as the reference checker does: encoding, the
//! declaration, the root, schema revisions, unknown elements and attributes, enumerations,
//! integers, Name26 GUIDs, legacy elements and entries Inventor ignores. Findings are what a
//! reader may want to report; none stops Corliss from reading the file [R-14].

use super::schema::{AttrRule, ElementRule, Shape, rule_for};
use super::xml::{Content, Element};
use super::{ProjectFile, TextEncoding};
use crate::naming::guid_from_name;

/// Findings for `file`, in document order, worded as the reference checker words them.
pub(super) fn check(file: &ProjectFile) -> Vec<String> {
    let mut out = Vec::new();
    if file.encoding != TextEncoding::Utf16Le || file.bom.is_empty() {
        out.push("encoding: not UTF-16LE with BOM".to_string());
    }
    check_declaration(&file.prolog, &mut out);
    if file.root.name != "InventorProject" {
        out.push(format!(
            "root: {} (expected InventorProject)",
            file.root.name
        ));
    }
    let revision = file.root.attribute("schemarevid");
    check_revision(&file.root, revision.as_deref(), &mut out);
    let ctx = CheckContext {
        revision: revision
            .as_deref()
            .filter(|r| is_digits(r))
            .and_then(|r| r.parse().ok()),
    };
    ctx.walk(&file.root, "InventorProject", &mut out);
    out
}

fn check_declaration(prolog: &str, out: &mut Vec<String>) {
    let decl = prolog.trim();
    let forms = [
        "<?xml version=\"1.0\" encoding=\"utf-16\"?>",
        "<?xml version=\"1.0\" encoding=\"utf-16\" standalone=\"no\" ?>",
    ];
    if !forms.iter().any(|f| decl.starts_with(f)) {
        let head: String = decl.chars().take(60).collect();
        out.push(format!("declaration: unexpected form {}", py_repr(&head)));
    }
}

fn check_revision(root: &Element, revision: Option<&str>, out: &mut Vec<String>) {
    match revision {
        None if root.attribute("version").is_none() => {
            out.push("root: neither schemarevid nor version (Inventor rejects the file)".into())
        }
        Some(rev) if !(is_digits(rev) && (10..=14).contains(&rev.parse::<i64>().unwrap_or(-1))) => {
            out.push(format!(
                "root: schemarevid={rev} outside 10..14 (Inventor rejects the file)"
            ))
        }
        _ => {}
    }
}

struct CheckContext {
    /// `schemarevid` when it is a number.
    revision: Option<i64>,
}

impl CheckContext {
    fn walk(&self, e: &Element, path: &str, out: &mut Vec<String>) {
        let Some(rule) = rule_for(path) else {
            out.push(format!("unknown element {path}"));
            return;
        };
        if matches!(rule.shape, Shape::Opaque | Shape::OpaqueInner) {
            return self.walk_opaque(e, path, rule, out);
        }
        check_attributes(e, path, rule.attrs, out);
        if rule.shape == Shape::Text {
            check_text(e, path, rule, out);
        }
        self.check_rule_flags(path, rule, out);
        e.elements()
            .for_each(|c| self.walk(c, &format!("{path}/{}", c.name), out));
        check_stray_text(e, path, rule.shape, out);
        check_ignored_entries(e, path, out);
    }

    /// Opaque content is the add-in's own: only the listed attributes and the listed inner
    /// elements are checked.
    fn walk_opaque(&self, e: &Element, path: &str, rule: &ElementRule, out: &mut Vec<String>) {
        let known: Vec<&str> = rule.attrs.iter().map(|a| a.name).collect();
        for a in e
            .attributes
            .iter()
            .filter(|a| !known.is_empty() && !known.contains(&a.name.as_str()))
        {
            out.push(format!("unknown attribute {path}@{}", a.name));
        }
        for c in e.elements() {
            let sub = format!("{path}/{}", c.name);
            if rule_for(&sub).is_some_and(|r| r.shape != Shape::OpaqueInner) {
                self.walk(c, &sub, out);
            }
        }
    }

    fn check_rule_flags(&self, path: &str, rule: &ElementRule, out: &mut Vec<String>) {
        if let (Some(since), Some(rev)) = (rule.since, self.revision)
            && rev < since
        {
            out.push(format!(
                "{path}: ignored by Inventor below schemarevid {since}"
            ));
        }
        if rule.legacy {
            out.push(format!(
                "legacy element {path} (read only by migration, never written)"
            ));
        }
    }
}

fn check_attributes(e: &Element, path: &str, rules: &[AttrRule], out: &mut Vec<String>) {
    for a in &e.attributes {
        let Some(rule) = rules.iter().find(|r| r.name == a.name) else {
            out.push(format!("unknown attribute {path}@{}", a.name));
            continue;
        };
        let value = a.value();
        if let Some(values) = rule.values.filter(|v| !v.contains(&value.as_str())) {
            out.push(format!(
                "value {path}@{}={} not in {}",
                a.name,
                py_repr(&value),
                py_list(values)
            ));
        }
        if rule.name26 && guid_from_name(&value).is_none() {
            out.push(format!(
                "{path}@{}: not a 26-character GUID name: {}",
                a.name,
                py_repr(&value)
            ));
        }
    }
}

fn check_text(e: &Element, path: &str, rule: &ElementRule, out: &mut Vec<String>) {
    if e.elements().next().is_some() {
        out.push(format!("{path}: element content in a text element"));
    }
    let text = e.text();
    if let Some(values) = rule
        .values
        .filter(|v| !text.is_empty() && !v.contains(&text.as_str()))
    {
        let case = values
            .iter()
            .any(|v| v.to_lowercase() == text.to_lowercase());
        let note = if case {
            " (case differs; Inventor compares case-insensitively)"
        } else {
            ""
        };
        out.push(format!(
            "value {path}={} not in {}{note}",
            py_repr(&text),
            py_list(values)
        ));
    }
    let digits = text.strip_prefix('-').unwrap_or(&text);
    if rule.integer && !text.is_empty() && !is_digits(digits) {
        out.push(format!("value {path}={} is not an integer", py_repr(&text)));
    }
}

fn check_stray_text(e: &Element, path: &str, shape: Shape, out: &mut Vec<String>) {
    if !matches!(shape, Shape::Struct | Shape::List) {
        return;
    }
    for c in &e.children {
        let Content::Text(t) = c else { continue };
        let t = t.trim();
        if !t.is_empty() {
            out.push(format!(
                "{path}: stray character data {}",
                py_repr(&t.chars().take(30).collect::<String>())
            ));
        }
    }
}

/// Entries Inventor skips (19.3.4, 19.3.5 [F-12]).
fn check_ignored_entries(e: &Element, path: &str, out: &mut Vec<String>) {
    let empty = |name: &str| e.child(name).is_none_or(|c| c.text().is_empty());
    if path.ends_with("/ProjectPath") && (empty("PathName") || empty("Path")) {
        out.push(format!(
            "{path}: empty PathName or Path (Inventor ignores the entry)"
        ));
    }
    if !path.ends_with("/ProteinLibrary") {
        return;
    }
    for k in ["ProteinLibraryType", "ActiveProteinLibrary"]
        .into_iter()
        .filter(|k| empty(k))
    {
        out.push(format!("{path}: empty {k} (Inventor ignores the entry)"));
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Python's `repr` of a string, so findings read as the reference's do.
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => out.extend(['\\', c]),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn py_list(values: &[&str]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|v| py_repr(v))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
