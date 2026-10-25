//! Reading XML text into the lossless tree of [`super::xml`], as the reference reader parses it.
//! The grammar is the subset project files use (spec 19.1): elements, attributes, character
//! data, and comments, processing instructions and CDATA kept as written.

use super::xml::{Attribute, Content, Element};
use crate::error::{CorlissError, Result};

const STRUCTURE: &str = "project file";

/// Text before the root element, the root, and text after it.
pub(super) struct ParsedXml {
    pub prolog: String,
    pub root: Element,
    pub epilog: String,
}

/// Parses a whole project file's text.
pub(super) fn parse_xml(text: &str) -> Result<ParsedXml> {
    let mut cur = XmlCursor { text, pos: 0 };
    cur.skip_prolog();
    let prolog = text[..cur.pos].to_string();
    if !cur.at("<") || cur.at("</") {
        return Err(cur.error("root", "a root element after the declaration"));
    }
    let root = cur.element()?;
    Ok(ParsedXml {
        prolog,
        root,
        epilog: text[cur.pos..].to_string(),
    })
}

struct XmlCursor<'t> {
    text: &'t str,
    pos: usize,
}

impl XmlCursor<'_> {
    fn rest(&self) -> &str {
        &self.text[self.pos..]
    }

    fn at(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn error(&self, field: &'static str, expected: &str) -> CorlissError {
        let found: String = self.rest().chars().take(30).collect();
        CorlissError::invalid(STRUCTURE, field, self.pos, format!("{found:?}"), expected)
    }

    /// Whitespace, the declaration, processing instructions and comments.
    fn skip_prolog(&mut self) {
        loop {
            let before = self.pos;
            self.take_while(char::is_whitespace);
            for (open, close) in [("<?", "?>"), ("<!--", "-->")] {
                if self.at(open)
                    && let Some(end) = self.rest().find(close)
                {
                    self.pos += end + close.len();
                }
            }
            if self.pos == before {
                return;
            }
        }
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &str {
        let start = self.pos;
        let len = self
            .rest()
            .find(|c: char| !keep(c))
            .unwrap_or(self.rest().len());
        self.pos += len;
        &self.text[start..self.pos]
    }

    /// An element from its start tag to its end tag.
    fn element(&mut self) -> Result<Element> {
        let mut e = self.start_tag()?;
        if e.self_closing {
            return Ok(e);
        }
        while !self.at("</") {
            let content = self.content()?;
            e.children.push(content);
        }
        e.end_space = self.end_tag(&e.name)?;
        Ok(e)
    }

    fn content(&mut self) -> Result<Content> {
        for (open, close) in [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>")] {
            if self.at(open) {
                let end = self
                    .rest()
                    .find(close)
                    .ok_or_else(|| self.error("markup", close))?;
                let raw = self.rest()[..end + close.len()].to_string();
                self.pos += raw.len();
                return Ok(Content::Markup(raw));
            }
        }
        if self.at("<") {
            return Ok(Content::Element(self.element()?));
        }
        let end = self
            .rest()
            .find('<')
            .ok_or_else(|| self.error("text", "an end tag after the character data"))?;
        let text = self.rest()[..end].to_string();
        self.pos += end;
        Ok(Content::Text(text))
    }

    fn start_tag(&mut self) -> Result<Element> {
        self.pos += 1;
        let name = self.take_while(is_name_char).to_string();
        if name.is_empty() {
            return Err(self.error("start tag", "an element name after '<'"));
        }
        let mut e = Element::new(&name);
        while let Some(a) = self.attribute() {
            e.attributes.push(a);
        }
        e.tag_space = self.take_while(char::is_whitespace).to_string();
        e.self_closing = self.at("/>");
        let close = if e.self_closing { 2 } else { 1 };
        if !e.self_closing && !self.at(">") {
            return Err(self.error("start tag", "'>' or '/>' closing the start tag"));
        }
        self.pos += close;
        Ok(e)
    }

    /// ` name = "value"`; `None` (position unchanged) when no attribute follows.
    fn attribute(&mut self) -> Option<Attribute> {
        let start = self.pos;
        let found = self.try_attribute();
        if found.is_none() {
            self.pos = start;
        }
        found
    }

    fn try_attribute(&mut self) -> Option<Attribute> {
        let lead = self.take_while(char::is_whitespace).to_string();
        let first = self.rest().chars().next()?;
        if lead.is_empty() || !(first.is_ascii_alphabetic() || first == '_' || first == ':') {
            return None;
        }
        let name = self.take_while(is_name_char).to_string();
        let eq_start = self.pos;
        self.take_while(char::is_whitespace);
        self.at("=").then(|| self.pos += 1)?;
        self.take_while(char::is_whitespace);
        let equals = self.text[eq_start..self.pos].to_string();
        let quote = self
            .rest()
            .chars()
            .next()
            .filter(|q| *q == '"' || *q == '\'')?;
        let len = self.rest()[1..].find(quote)? + 2;
        let quoted = self.rest()[..len].to_string();
        self.pos += len;
        Some(Attribute {
            lead,
            name,
            equals,
            quoted,
        })
    }

    /// `</name  >` closing `name`; returns the whitespace before `>`.
    fn end_tag(&mut self, name: &str) -> Result<String> {
        self.pos += 2;
        let found = self.take_while(is_name_char).to_string();
        let space = self.take_while(char::is_whitespace).to_string();
        if found != name || !self.at(">") {
            return Err(self.error("end tag", &format!("the end tag </{name}>")));
        }
        self.pos += 1;
        Ok(space)
    }
}

/// `[\w.:-]`: the name characters the reference reader accepts.
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')
}
