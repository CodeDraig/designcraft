//! TEI P5 reading editions. XML originals are retained independently of editable layout text.
mod render;
mod styles;
#[cfg(test)]
mod tests;
mod xml;

use crate::{ImportError, Imported};
use designcraft_doc::{
    Hyperlink,
    tei::{TeiDiagnostic, TeiMetadata, TeiOptions, TeiResource, TeiSource},
};
use std::collections::BTreeMap;
use std::sync::Arc;
use xml::{Element, NS, err};
pub use xml::{MAX_BYTES, is_tei};

/// Actual persistent source identifiers, distinguished from generated navigation labels.
pub fn source_ids(bytes: &[u8]) -> Result<std::collections::BTreeSet<String>, ImportError> {
    let root = xml::parse(bytes)?;
    let mut ids = std::collections::BTreeSet::new();
    root.visit(&mut |e| {
        if !e.attr("xml:id").is_empty() {
            ids.insert(e.attr("xml:id").to_owned());
        }
    });
    Ok(ids)
}

/// A graphic request. The engine resolves bytes and builds native anchored objects; this
/// parser stays independent of filesystem and renderer APIs.
#[derive(Clone, Debug)]
pub struct Graphic {
    pub item: u64,
    pub url: String,
    pub description: String,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub location: TeiDiagnostic,
}
#[derive(Clone, Debug)]
pub struct Payload {
    pub source: TeiSource,
    pub graphics: Vec<Graphic>,
    pub links: Vec<Hyperlink>,
    pub witnesses: BTreeMap<String, String>,
}

pub fn import(name: &str, bytes: &[u8], options: &TeiOptions) -> Result<Imported, ImportError> {
    let root = xml::parse(bytes)?;
    let mut ctx = render::Context::new(options.clone());
    root.visit(&mut |e| {
        if e.is("witness") && !e.attr("xml:id").is_empty() {
            ctx.witnesses.insert(e.attr("xml:id").into(), words(&e.text()));
        }
        if e.is("rendition") && !e.attr("xml:id").is_empty() {
            ctx.renditions.insert(e.attr("xml:id").into(), (e.attr("scheme").into(), e.text()));
        }
        if e.is("char")
            && !e.attr("xml:id").is_empty()
            && let Some(m) = e.elements().find(|m| m.is("mapping") && matches!(m.attr("type"), "unicode" | "standard"))
        {
            ctx.characters.insert(e.attr("xml:id").into(), m.text());
        }
    });
    if let Some(w) = &options.witness
        && !ctx.witnesses.contains_key(w.trim_start_matches('#'))
    {
        ctx.warn(&root, "unknown-witness", &format!("Witness {w} is not declared in listWit; matching @wit tokens will still be used."));
    }
    let mut metadata = Vec::new();
    root.visit(&mut |e| {
        if e.is("teiHeader") {
            metadata.push(metadata_of(e));
        }
    });
    let mut out = render::Builder::new(ctx.next_story());
    ctx.render(&root, &mut out, &render::State::default())?;
    let story = out.finish();
    if ctx.texts == 0 {
        return Err(err("the document contains no text element"));
    }
    ctx.resolve_links();
    story.check().map_err(err)?;
    let warnings = ctx.diagnostics.iter().map(|d| format!("{}:{} {}: {}", d.line, d.column, d.code, d.message)).collect();
    let resources = ctx.graphics.iter().map(|g| TeiResource { url: g.url.clone(), asset: None }).collect();
    let source = TeiSource {
        id: 0,
        name: name.into(),
        byte_length: bytes.len(),
        bytes: Arc::new(bytes.to_vec()),
        metadata,
        options: options.clone(),
        import_settings: Default::default(),
        diagnostics: ctx.diagnostics,
        navigation: ctx.navigation,
        resources,
    };
    Ok(Imported {
        story,
        para_styles: styles::paragraphs(),
        char_styles: vec![],
        warnings,
        tei: Some(Payload { source, graphics: ctx.graphics, links: ctx.links, witnesses: ctx.witnesses }),
    })
}

fn words(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn structured_metadata(e: &Element) -> String {
    let mut out = String::new();
    for node in &e.children {
        match node {
            xml::Node::Text(t) => out.push_str(t),
            xml::Node::Element(c) => {
                let separate = matches!(
                    c.name.as_str(),
                    "p" | "title"
                        | "author"
                        | "editor"
                        | "name"
                        | "resp"
                        | "publisher"
                        | "pubPlace"
                        | "date"
                        | "availability"
                        | "idno"
                        | "edition"
                        | "bibl"
                        | "witness"
                );
                if separate {
                    out.push(' ');
                }
                out.push_str(&structured_metadata(c));
                if separate {
                    out.push(' ');
                }
            }
        }
    }
    out
}
fn metadata_of(header: &Element) -> TeiMetadata {
    let mut m = TeiMetadata::default();
    if let Some(fd) = header.child("fileDesc") {
        if let Some(ts) = fd.child("titleStmt") {
            for e in ts.elements() {
                match e.name.as_str() {
                    "title" => m.titles.push(words(&e.text())),
                    "author" | "editor" | "respStmt" | "sponsor" | "funder" | "principal" => m.contributors.push(words(&structured_metadata(e))),
                    _ => {}
                }
            }
        }
        if let Some(e) = fd.child("editionStmt") {
            m.edition.push(words(&structured_metadata(e)));
        }
        if let Some(e) = fd.child("publicationStmt") {
            m.publication.push(words(&structured_metadata(e)));
        }
        if let Some(e) = fd.child("sourceDesc") {
            m.sources.push(words(&structured_metadata(e)));
        }
    }
    header.visit(&mut |e| {
        if e.namespace != NS {
            return;
        }
        match e.name.as_str() {
            "idno" => m.identifiers.push(words(&e.text())),
            "language" => m.languages.push(if e.attr("ident").is_empty() { words(&e.text()) } else { e.attr("ident").into() }),
            "availability" => m.rights.push(words(&e.text())),
            _ => {}
        }
    });
    m
}

fn location(e: &Element, code: &str, message: &str) -> TeiDiagnostic {
    TeiDiagnostic {
        code: code.into(),
        element: e.name.clone(),
        xml_id: (!e.attr("xml:id").is_empty()).then(|| e.attr("xml:id").into()),
        line: e.line,
        column: e.column,
        message: message.into(),
    }
}
