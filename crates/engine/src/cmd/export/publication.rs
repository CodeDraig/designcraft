//! Export a chosen edition without changing the user's active component or selection.
use super::*;
use designcraft_doc::{Document, EditionId, publication::PublicationView};

pub(super) struct Output {
    pub documents: Vec<Document>,
    pub warnings: Vec<String>,
    pub metadata: designcraft_doc::publication::editions::OutputSettings,
}

pub(super) fn prepare(s: &Session, p: &Value) -> Result<Output> {
    let mut selected = (*s.doc()?.doc).clone();
    let scope = str_param(p, "scope").unwrap_or(if selected.publication.is_some() { "publication" } else { "component" });
    if !matches!(scope, "publication" | "component") {
        return Err(bad("file.export", "scope must be publication or component"));
    }
    if let Some(id) = p.get("edition") {
        let id = id.as_u64().filter(|n| *n > 0).ok_or_else(|| bad("file.export", "edition must be a positive identity"))?;
        let v = selected.publication.as_ref().ok_or_else(|| bad("file.export", "edition selection requires a publication"))?;
        selected = PublicationView { edition: EditionId(id), ..v.clone() }.project()?;
    }
    super::super::publication::update::refresh(&mut selected, true, &s.cache)?;
    let mut metadata = Default::default();
    let mut warnings = Vec::new();
    let documents = if let Some(v) = &selected.publication {
        let e = v.root.editions.get(&v.edition).ok_or_else(|| bad("file.export", "missing edition"))?;
        metadata = e.output.clone();
        if metadata.title.is_none() {
            metadata.title = Some(v.root.title.clone());
        }
        if metadata.author.is_none() {
            metadata.author = v.root.metadata.get("author").cloned();
        }
        if metadata.language.is_none() {
            metadata.language = v.root.metadata.get("language").cloned();
        }
        if metadata.identifier.is_none() {
            metadata.identifier = v.root.metadata.get("identifier").cloned();
        }
        warnings.extend(e.references.diagnostics.iter().cloned());
        if scope == "publication" {
            v.root
                .ordered_components()?
                .into_iter()
                .map(|component| PublicationView { component, ..v.clone() }.project())
                .collect::<designcraft_doc::Result<Vec<_>>>()?
        } else {
            vec![selected]
        }
    } else {
        vec![selected]
    };
    for d in &documents {
        for st in d.stories.values().filter(|st| d.publication_story_included(st.id)) {
            let c = s.cache.get(d, st.id, None);
            if let Some(at) = c.overset_at {
                warnings.push(format!("Chapter {}: story {} has overset text from byte {at}", d.title, st.id.0));
            }
            if st.frames.is_empty() && !st.text.is_empty() {
                warnings.push(format!("Chapter {}: story {} has no frames; page output cannot show it", d.title, st.id.0));
            }
            for id in &c.annotation_overflow {
                warnings.push(format!("Chapter {}: note {id} is overset", d.title));
            }
            warnings.extend(c.diagnostics.iter().map(|v| format!("Chapter {}: {v}", d.title)));
        }
    }
    warnings.sort();
    warnings.dedup();
    if p["strict"] == true && !warnings.is_empty() {
        return Err(bad("file.export", format!("Export diagnostics: {}", warnings.join("; "))));
    }
    Ok(Output { documents, warnings, metadata })
}

pub(super) fn defaults(p: &Value, m: &designcraft_doc::publication::editions::OutputSettings) -> Value {
    let mut p = p.as_object().cloned().unwrap_or_default();
    for (name, value) in [("title", &m.title), ("author", &m.author), ("language", &m.language), ("identifier", &m.identifier)] {
        if !p.contains_key(name)
            && let Some(value) = value
        {
            p.insert(name.into(), json!(value));
        }
    }
    Value::Object(p)
}

pub(super) fn selected_spreads(docs: &[Document], opts: &PdfOptions) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut first = 0;
    for (part, doc) in docs.iter().enumerate() {
        let mut local = opts.clone();
        local.pages =
            opts.pages.as_ref().map(|pages| pages.iter().filter(|p| **p >= first && **p < first + doc.page_count()).map(|p| *p - first).collect());
        if local.pages.as_ref().is_none_or(|p| !p.is_empty()) {
            out.extend(designcraft_pdf::sheet_spreads(doc, &local).into_iter().map(|si| (part, si)));
        }
        first += doc.page_count();
    }
    out
}
