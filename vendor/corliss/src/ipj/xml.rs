//! A lossless XML element tree for project files: every byte of the source is kept (tag
//! spacing, attribute quoting and order, self-closing form, entities), so an unchanged tree
//! writes back the bytes it was read from, whatever the layout (spec 19.1, Table 19-1).

/// One attribute exactly as written: ` name="value"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// Whitespace before the name.
    pub lead: String,
    /// Attribute name.
    pub name: String,
    /// `=` with any whitespace around it.
    pub equals: String,
    /// The value with its quotes, entities not expanded.
    pub quoted: String,
}

impl Attribute {
    /// The value with entities expanded.
    pub fn value(&self) -> String {
        let inner = self
            .quoted
            .get(1..self.quoted.len().saturating_sub(1))
            .unwrap_or_default();
        unescape(inner)
    }

    fn write(&self, out: &mut String) {
        for part in [&self.lead, &self.name, &self.equals, &self.quoted] {
            out.push_str(part);
        }
    }
}

/// A piece of element content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// A child element.
    Element(Element),
    /// Character data as written (entities not expanded), whitespace included.
    Text(String),
    /// A comment, processing instruction or CDATA section, kept as written.
    Markup(String),
}

/// An element with everything needed to write it back unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Element name.
    pub name: String,
    /// Attributes in source order.
    pub attributes: Vec<Attribute>,
    /// Whitespace before `>` or `/>` in the start tag.
    pub tag_space: String,
    /// Written `<X/>` rather than `<X></X>`.
    pub self_closing: bool,
    /// Content in source order.
    pub children: Vec<Content>,
    /// Whitespace inside the end tag (`</X  >`).
    pub end_space: String,
}

impl Element {
    /// An empty element `<name>` with no attributes.
    pub fn new(name: &str) -> Self {
        Element {
            name: name.to_string(),
            attributes: Vec::new(),
            tag_space: String::new(),
            self_closing: false,
            children: Vec::new(),
            end_space: String::new(),
        }
    }

    /// The value of attribute `name`, entities expanded.
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(Attribute::value)
    }

    /// Sets attribute `name`, keeping its place and quoting style when it exists.
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        let quoted = format!("\"{}\"", escape(value, true));
        match self.attributes.iter_mut().find(|a| a.name == name) {
            Some(a) => a.quoted = quoted,
            None => self.attributes.push(Attribute {
                lead: " ".into(),
                name: name.into(),
                equals: "=".into(),
                quoted,
            }),
        }
    }

    /// Child elements in order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Content::Element(e) => Some(e),
            _ => None,
        })
    }

    /// The first child element named `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// The first child element named `name`, mutable.
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|c| match c {
            Content::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// The character data, entities expanded; empty for `<X/>` and `<X></X>` alike.
    pub fn text(&self) -> String {
        let raw = self.children.iter().filter_map(|c| match c {
            Content::Text(t) => Some(t.as_str()),
            _ => None,
        });
        raw.map(unescape).collect()
    }

    /// Replaces the content with character data `value`. An empty value keeps the element's
    /// `<X/>` or `<X></X>` form; other bytes of the file stay as they were.
    pub fn set_text(&mut self, value: &str) {
        self.children = if value.is_empty() {
            Vec::new()
        } else {
            vec![Content::Text(escape(value, false))]
        };
        self.self_closing &= value.is_empty();
    }

    /// The element as XML text, exactly as read (plus any changes).
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    pub(super) fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        self.attributes.iter().for_each(|a| a.write(out));
        out.push_str(&self.tag_space);
        if self.self_closing {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for c in &self.children {
            match c {
                Content::Element(e) => e.write(out),
                Content::Text(t) | Content::Markup(t) => out.push_str(t),
            }
        }
        out.push_str(&format!("</{}{}>", self.name, self.end_space));
    }
}

/// Expands the five predefined entities and character references; anything else stays.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match entity(rest) {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character an entity at the start of `s` stands for, and the entity's length.
fn entity(s: &str) -> Option<(char, usize)> {
    let end = s.find(';')?;
    let name = &s[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => char_reference(name)?,
    };
    Some((c, end + 1))
}

fn char_reference(name: &str) -> Option<char> {
    let code = match name.strip_prefix("#x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => name.strip_prefix('#')?.parse().ok()?,
    };
    char::from_u32(code)
}

/// Escapes `&`, `<` and `>` (and `"` in attribute values), as the reference writer does.
pub fn escape(s: &str, attribute: bool) -> String {
    let s = s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    if attribute {
        s.replace('"', "&quot;")
    } else {
        s
    }
}
