//! Just enough .xlsx reading for iPart tables: one zip entry, the shared strings and the cells
//! of a worksheet. The XML is scanned with the reference reader's regular-expression rules
//! (`<row ...>...</row>` on one line, `<c ...>[<v>...</v>]</c>`), so cells it skips (formulas,
//! inline strings, self-closing cells) are skipped here too.

use std::io::Read;

/// End of central directory record signature.
const EOCD_SIG: &[u8] = b"PK\x05\x06";
/// Central directory file header signature.
const CDH_SIG: &[u8] = b"PK\x01\x02";

/// The uncompressed bytes of entry `name` (stored or deflated) of the zip archive `zip`.
/// Like Python's `zipfile`, data prepended to the archive shifts every offset it records.
pub(super) fn zip_entry(zip: &[u8], name: &str) -> Option<Vec<u8>> {
    let eocd = zip.windows(4).rposition(|w| w == EOCD_SIG)?;
    let cd_size = u32_at(zip, eocd + 12)? as usize;
    let cd_offset = u32_at(zip, eocd + 16)? as usize;
    let shift = eocd.checked_sub(cd_size + cd_offset)?;
    let mut pos = cd_offset + shift;
    while zip.get(pos..pos + 4) == Some(CDH_SIG) {
        let name_len = u16_at(zip, pos + 28)? as usize;
        let skip = name_len + u16_at(zip, pos + 30)? as usize + u16_at(zip, pos + 32)? as usize;
        if zip.get(pos + 46..pos + 46 + name_len)? == name.as_bytes() {
            return entry_data(zip, pos, shift);
        }
        pos += 46 + skip;
    }
    None
}

/// The data of the entry whose central directory header is at `cdh`.
fn entry_data(zip: &[u8], cdh: usize, shift: usize) -> Option<Vec<u8>> {
    let method = u16_at(zip, cdh + 10)?;
    let size = u32_at(zip, cdh + 20)? as usize;
    let local = u32_at(zip, cdh + 42)? as usize + shift;
    let start = local + 30 + u16_at(zip, local + 26)? as usize + u16_at(zip, local + 28)? as usize;
    let data = zip.get(start..start + size)?;
    match method {
        0 => Some(data.to_vec()),
        8 => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .ok()?;
            Some(out)
        }
        _ => None,
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The `<t>` texts of sharedStrings.xml, entities `&lt;`, `&gt;`, `&amp;` decoded.
pub(super) fn shared_strings(xml: &str) -> Vec<String> {
    tag_bodies(xml, "<t", "</t>")
        .into_iter()
        .map(|(_, body)| {
            body.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        })
        .collect()
}

/// The rows of a worksheet as text; shared-string cells (`t="s"`) resolved through `strings`.
pub(super) fn sheet_rows(xml: &str, strings: &[String]) -> Vec<Vec<String>> {
    tag_bodies(xml, "<row", "</row>")
        .into_iter()
        .map(|(_, row)| {
            row_cells(row)
                .into_iter()
                .map(|(attrs, v)| cell_text(attrs, v, strings))
                .collect()
        })
        .collect()
}

fn cell_text(attrs: &str, value: &str, strings: &[String]) -> String {
    if !attrs.contains("t=\"s\"") || value.is_empty() {
        return value.to_string();
    }
    let shared = value
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|k| strings.get(k));
    shared.cloned().unwrap_or_else(|| value.to_string())
}

/// Every non-overlapping `<open[^>]*>(.*?)</close>` match as (attributes, body); the body does
/// not span a line break.
fn tag_bodies<'a>(text: &'a str, open: &str, close: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(open).map(|k| from + k) {
        let attrs_start = at + open.len();
        let matched = text[attrs_start..].find('>').and_then(|gt| {
            let body_start = attrs_start + gt + 1;
            let body = line_until(&text[body_start..], close)?;
            Some((
                (&text[attrs_start..attrs_start + gt], body),
                body_start + body.len() + close.len(),
            ))
        });
        match matched {
            Some((found, end)) => {
                out.push(found);
                from = end;
            }
            None => from = at + 1,
        }
    }
    out
}

/// The text before the first `close` in `rest`, when no line break comes first.
fn line_until<'a>(rest: &'a str, close: &str) -> Option<&'a str> {
    let end = rest.find(close)?;
    let body = &rest[..end];
    (!body.contains('\n')).then_some(body)
}

/// The cells of a row: `<c attrs><v>value</v></c>` or `<c attrs></c>` (value "").
fn row_cells(row: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = row[from..].find("<c ").map(|k| from + k) {
        let attrs_start = at + 3;
        let matched = row[attrs_start..].find('>').and_then(|gt| {
            let rest_start = attrs_start + gt + 1;
            let (value, len) = cell_value(&row[rest_start..])?;
            Some((
                (&row[attrs_start..attrs_start + gt], value),
                rest_start + len,
            ))
        });
        match matched {
            Some((cell, end)) => {
                out.push(cell);
                from = end;
            }
            None => from = at + 1,
        }
    }
    out
}

/// After a cell's start tag: the value and the length through `</c>`.
fn cell_value(rest: &str) -> Option<(&str, usize)> {
    let valued = rest
        .strip_prefix("<v>")
        .and_then(|inner| line_until(inner, "</v></c>"));
    if let Some(value) = valued {
        return Some((value, 3 + value.len() + "</v></c>".len()));
    }
    rest.starts_with("</c>").then_some(("", "</c>".len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cells_like_the_reference_regexes() {
        let sheet = r#"<sheetData><row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1"><v>2.5</v></c><c r="C1"></c><c r="D1"/></row></sheetData>"#;
        let strings = vec!["a".to_string(), "b".to_string()];
        assert_eq!(sheet_rows(sheet, &strings), vec![vec!["b", "2.5", ""]]);
    }

    #[test]
    fn decodes_shared_string_entities() {
        let xml = "<sst><si><t>a &lt;b&gt; &amp; c</t></si><si><t xml:space=\"preserve\"> x</t></si></sst>";
        assert_eq!(shared_strings(xml), vec!["a <b> & c", " x"]);
    }
}
