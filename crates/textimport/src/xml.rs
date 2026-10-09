//! Bounded XML element trees for Office document parts.

use crate::ImportError;
use quick_xml::events::Event;

/// A minimal element tree (local names, attributes by local name).
#[derive(Debug, Default, Clone)]
pub(crate) struct El {
    pub(crate) name: String,
    attrs: Vec<(String, String)>,
    pub(crate) kids: Vec<Node>,
}

#[derive(Debug, Clone)]
pub(crate) enum Node {
    El(El),
    Text(String),
}

impl El {
    pub(crate) fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub(crate) fn els(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
    }
    pub(crate) fn child(&self, name: &str) -> Option<&El> {
        self.els().find(|e| e.name == name)
    }
    pub(crate) fn text(&self) -> String {
        self.kids
            .iter()
            .map(|n| match n {
                Node::Text(t) => t.clone(),
                Node::El(e) => e.text(),
            })
            .collect()
    }
    /// `<w:x w:val="…"/>` → the value; a bare `<w:b/>` counts as "on".
    pub(crate) fn val(&self, name: &str) -> Option<String> {
        self.child(name).map(|e| e.attr("val").unwrap_or("true").to_string())
    }
    pub(crate) fn on(&self, name: &str) -> Option<bool> {
        self.val(name).map(|v| !matches!(v.as_str(), "0" | "false" | "none"))
    }
}

fn local(n: &[u8]) -> String {
    let s = String::from_utf8_lossy(n);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

pub(crate) fn parse(xml: &str) -> Result<El, ImportError> {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<El> = vec![El::default()];
    loop {
        match r.read_event().map_err(|e| ImportError::Corrupt(e.to_string()))? {
            Event::Start(e) => {
                let el = El {
                    name: local(e.name().as_ref()),
                    attrs: e
                        .attributes()
                        .flatten()
                        .map(|a| (local(a.key.as_ref()), a.unescape_value().map(|v| v.to_string()).unwrap_or_default()))
                        .collect(),
                    kids: vec![],
                };
                stack.push(el);
            }
            Event::Empty(e) => {
                let el = El {
                    name: local(e.name().as_ref()),
                    attrs: e
                        .attributes()
                        .flatten()
                        .map(|a| (local(a.key.as_ref()), a.unescape_value().map(|v| v.to_string()).unwrap_or_default()))
                        .collect(),
                    kids: vec![],
                };
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::El(el));
                }
            }
            Event::End(_) => {
                let el = stack.pop().unwrap_or_default();
                match stack.last_mut() {
                    Some(p) => p.kids.push(Node::El(el)),
                    None => return Ok(el),
                }
            }
            Event::Text(t) => {
                let s = t.decode().map(|s| s.to_string()).unwrap_or_default();
                let s = quick_xml::escape::unescape(&s).map(|s| s.to_string()).unwrap_or(s);
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Text(s));
                }
            }
            Event::GeneralRef(e) => {
                let name = String::from_utf8_lossy(&e).to_string();
                let s = match name.as_str() {
                    "amp" => "&".to_string(),
                    "lt" => "<".into(),
                    "gt" => ">".into(),
                    "quot" => "\"".into(),
                    "apos" => "'".into(),
                    n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
                    n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
                    _ => String::new(),
                };
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Text(s));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut root = stack.pop().unwrap_or_default();
    Ok(root.kids.drain(..).find_map(|n| if let Node::El(e) = n { Some(e) } else { None }).unwrap_or_default())
}
