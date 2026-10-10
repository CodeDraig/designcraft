//! Namespace-aware, bounded XML parsing. No external resources are expanded.
use crate::ImportError;
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};
use std::collections::{BTreeMap, BTreeSet};

pub const NS: &str = "http://www.tei-c.org/ns/1.0";
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_NODES: usize = 1_000_000;
const MAX_DEPTH: usize = 128;

#[derive(Clone, Debug)]
pub(super) enum Node {
    Element(Element),
    Text(String),
}
#[derive(Clone, Debug, Default)]
pub(super) struct Element {
    pub name: String,
    pub namespace: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Node>,
    pub line: usize,
    pub column: usize,
}
impl Element {
    pub fn attr(&self, key: &str) -> &str {
        self.attrs.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }
    pub fn is(&self, name: &str) -> bool {
        self.namespace == NS && self.name == name
    }
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.is(name))
    }
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.text_into(&mut out);
        out
    }
    fn text_into(&self, out: &mut String) {
        for node in &self.children {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.text_into(out),
            }
        }
    }
    pub fn visit(&self, f: &mut impl FnMut(&Element)) {
        f(self);
        for child in self.elements() {
            child.visit(f);
        }
    }
}
pub(super) fn err(message: impl Into<String>) -> ImportError {
    ImportError::Corrupt(format!("TEI: {}", message.into()))
}
fn namespace(ns: ResolveResult<'_>) -> Result<String, ImportError> {
    match ns {
        ResolveResult::Bound(n) => String::from_utf8(n.as_ref().to_vec()).map_err(|e| err(e.to_string())),
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Unknown(p) => Err(err(format!("undeclared namespace prefix {}", String::from_utf8_lossy(&p)))),
    }
}
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}
// XML 1.0 (Fifth Edition), productions 4 and 4a, without the namespace colon.
fn name_start(c: char) -> bool {
    matches!(c, 'A'..='Z' | '_' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' |
        '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' |
        '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' |
        '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' |
        '\u{10000}'..='\u{effff}')
}
fn ncname(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(name_start) && chars.all(name_char)
}
fn name_char(c: char) -> bool {
    name_start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
}
fn qname(name: &str) -> bool {
    name.split_once(':').map_or_else(|| ncname(name), |(prefix, local)| ncname(prefix) && ncname(local))
}
fn decode(bytes: &[u8]) -> Result<String, ImportError> {
    if bytes.len() > MAX_BYTES {
        return Err(err("source exceeds 64 MiB"));
    }
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]);
    let text = if utf16 {
        let little = bytes.starts_with(&[0xff, 0xfe]);
        let tail = &bytes[2..];
        if !tail.len().is_multiple_of(2) {
            return Err(err("incomplete UTF-16 code unit"));
        }
        let units: Vec<u16> =
            tail.chunks_exact(2).map(|c| if little { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16(&units).map_err(|e| err(e.to_string()))?
    } else {
        let utf8 =
            std::str::from_utf8(bytes).map_err(|_| ImportError::Unsupported("TEI encoding: use UTF-8 or UTF-16 with a byte-order mark".into()))?;
        utf8.strip_prefix('\u{feff}').unwrap_or(utf8).to_string()
    };
    if !text.chars().all(xml_char) {
        return Err(err("invalid XML character"));
    }
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Probe only the root, including its namespace. A file extension is never proof of TEI.
pub fn is_tei(bytes: &[u8]) -> bool {
    let Ok(text) = decode(bytes) else { return false };
    let mut reader = NsReader::from_str(&text);
    loop {
        match reader.read_resolved_event() {
            Ok((ns, Event::Start(e) | Event::Empty(e))) => {
                return namespace(ns).is_ok_and(|s| s == NS) && matches!(e.local_name().as_ref(), b"TEI" | b"teiCorpus");
            }
            Ok((_, Event::Eof)) | Err(_) => return false,
            _ => {}
        }
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<Element, ImportError> {
    let text = decode(bytes)?;
    let mut reader = NsReader::from_str(&text);
    reader.config_mut().check_comments = true;
    let lines: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let mut ids = BTreeSet::new();
    let mut nodes = 0usize;
    let mut doctype = false;
    loop {
        let offset = reader.buffer_position() as usize;
        let (ns, event) = reader.read_resolved_event().map_err(|e| err(format!("at byte {offset}: {e}")))?;
        let ns = namespace(ns)?;
        nodes += 1;
        if nodes > MAX_NODES {
            return Err(err("XML exceeds one million nodes/events"));
        }
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if !qname(std::str::from_utf8(e.name().as_ref()).map_err(|e| err(e.to_string()))?) {
                    return Err(err(format!("invalid element name at byte {offset}")));
                }
                if stack.len() >= MAX_DEPTH {
                    return Err(err("XML nesting exceeds 128 levels"));
                }
                let mut attrs = BTreeMap::new();
                for a in e.attributes() {
                    let a = a.map_err(|e| err(e.to_string()))?;
                    let raw = std::str::from_utf8(a.key.as_ref()).map_err(|e| err(e.to_string()))?;
                    if !qname(raw) || a.value.contains(&b'<') {
                        return Err(err(format!("invalid attribute {raw} at byte {offset}")));
                    }
                    let normalized = std::str::from_utf8(&a.value).map_err(|e| err(e.to_string()))?.replace(['\t', '\r', '\n'], " ");
                    let value = quick_xml::escape::unescape(&normalized).map_err(|e| err(e.to_string()))?.into_owned();
                    if !value.chars().all(xml_char) {
                        return Err(err(format!("invalid XML character reference in attribute {raw}")));
                    }
                    if raw == "xmlns" || raw.starts_with("xmlns:") {
                        continue;
                    }
                    let (ans, local) = reader.resolve_attribute(a.key);
                    let ans = namespace(ans)?;
                    let local = std::str::from_utf8(local.as_ref()).map_err(|e| err(e.to_string()))?.to_string();
                    let key = if ans == "http://www.w3.org/XML/1998/namespace" {
                        format!("xml:{local}")
                    } else if ans.is_empty() {
                        local
                    } else {
                        format!("{{{ans}}}{local}")
                    };
                    if attrs.insert(key.clone(), value).is_some() {
                        return Err(err(format!("duplicate attribute {key}")));
                    }
                }
                if let Some(id) = attrs.get("xml:id")
                    && (!ncname(id) || !ids.insert(id.clone()))
                {
                    return Err(err(format!("empty, invalid, or duplicate xml:id {id:?}")));
                }
                let li = lines.partition_point(|&p| p <= offset).saturating_sub(1);
                let column = offset.saturating_sub(lines.get(li).copied().unwrap_or(0)) + 1;
                let el = Element {
                    name: String::from_utf8(e.local_name().as_ref().to_vec()).map_err(|e| err(e.to_string()))?,
                    namespace: ns,
                    attrs,
                    children: Vec::new(),
                    line: li + 1,
                    column,
                };
                if empty {
                    attach(el, &mut stack, &mut root)?;
                } else {
                    stack.push(el);
                }
            }
            Event::End(_) => {
                let el = stack.pop().ok_or_else(|| err("unexpected closing element"))?;
                attach(el, &mut stack, &mut root)?;
            }
            Event::Text(t) => {
                let s = t.xml_content().map_err(|e| err(e.to_string()))?.into_owned();
                if s.contains("]]>") {
                    return Err(err("CDATA terminator outside a CDATA section"));
                }
                text_node(s, &mut stack)?;
            }
            Event::CData(t) => {
                if stack.is_empty() {
                    return Err(err("CDATA outside the root element"));
                }
                text_node(t.decode().map_err(|e| err(e.to_string()))?.into_owned(), &mut stack)?;
            }
            Event::GeneralRef(e) => {
                if stack.is_empty() {
                    return Err(err("entity reference outside the root element"));
                }
                let reference = format!("&{};", e.decode().map_err(|e| err(e.to_string()))?);
                let s = quick_xml::escape::unescape(&reference).map_err(|e| err(format!("unresolved or invalid entity: {e}")))?.into_owned();
                if !s.chars().all(xml_char) {
                    return Err(err("invalid XML character reference"));
                }
                text_node(s, &mut stack)?;
            }
            Event::Decl(d) => {
                if offset != 0 {
                    return Err(err("XML declaration must be first in the document"));
                }
                if d.version().map_err(|e| err(e.to_string()))?.as_ref() != b"1.0" {
                    return Err(ImportError::Unsupported("TEI XML version: use XML 1.0".into()));
                }
                let declaration = BytesStart::from_content(std::str::from_utf8(&d).map_err(|e| err(e.to_string()))?, 3);
                let mut previous = 0;
                for attr in declaration.attributes() {
                    let attr = attr.map_err(|e| err(e.to_string()))?;
                    let order = match attr.key.as_ref() {
                        b"version" => 1,
                        b"encoding" => 2,
                        b"standalone" if matches!(attr.value.as_ref(), b"yes" | b"no") => 3,
                        _ => return Err(err("invalid XML declaration attribute")),
                    };
                    if order <= previous {
                        return Err(err("repeated or out-of-order XML declaration attribute"));
                    }
                    previous = order;
                }
                if let Some(enc) = d.encoding() {
                    let enc = enc.map_err(|e| err(e.to_string()))?;
                    let enc = String::from_utf8_lossy(&enc).to_ascii_lowercase();
                    let le = bytes.starts_with(&[0xff, 0xfe]);
                    let be = bytes.starts_with(&[0xfe, 0xff]);
                    if enc.starts_with("utf-16") && !(le || be)
                        || matches!(enc.as_str(), "utf-8" | "utf8" | "us-ascii") && (le || be)
                        || enc == "utf-16le" && !le
                        || enc == "utf-16be" && !be
                        || enc == "us-ascii" && !text.is_ascii()
                    {
                        return Err(err("declared encoding does not match source bytes"));
                    }
                    if !matches!(enc.as_str(), "utf-8" | "utf8" | "utf-16" | "utf-16le" | "utf-16be" | "us-ascii") {
                        return Err(ImportError::Unsupported(format!("TEI encoding {enc}; use UTF-8 or UTF-16")));
                    }
                }
            }
            Event::DocType(_) => {
                if doctype || root.is_some() || !stack.is_empty() {
                    return Err(err("DOCTYPE must occur once, before the root element"));
                }
                doctype = true; // Deliberately never load or expand a DTD.
            }
            Event::PI(pi) => {
                let target = std::str::from_utf8(pi.target()).map_err(|e| err(e.to_string()))?;
                if target.eq_ignore_ascii_case("xml")
                    || target.is_empty()
                    || !target.chars().enumerate().all(|(i, c)| c == ':' || if i == 0 { name_start(c) } else { name_char(c) })
                {
                    return Err(err("invalid processing-instruction target"));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(err("unclosed element"));
    }
    let root = root.ok_or_else(|| err("missing root element"))?;
    if !root.is("TEI") && !root.is("teiCorpus") {
        return Err(ImportError::Unsupported("expected a TEI P5 TEI or teiCorpus root in http://www.tei-c.org/ns/1.0".into()));
    }
    Ok(root)
}
fn attach(el: Element, stack: &mut [Element], root: &mut Option<Element>) -> Result<(), ImportError> {
    if let Some(p) = stack.last_mut() {
        p.children.push(Node::Element(el));
    } else if root.replace(el).is_some() {
        return Err(err("multiple root elements"));
    }
    Ok(())
}
fn text_node(s: String, stack: &mut [Element]) -> Result<(), ImportError> {
    if let Some(p) = stack.last_mut() {
        p.children.push(Node::Text(s));
    } else if !s.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
        return Err(err("text outside the root element"));
    }
    Ok(())
}
