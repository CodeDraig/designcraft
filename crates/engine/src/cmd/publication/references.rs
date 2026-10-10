//! Semantic selection is a read-only model operation. Explicit updates and live command commits
//! share the same bounded composition transaction.
use super::{CommandSpec, Result, Session, bad, cmd, has_publication, mutate, optional_id, str_param, view};
use designcraft_doc::{ComponentId, ParaFormat, SpreadRef, publication::references::*};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "publication.semantic.scan",
            "Register Styled and TEI Destinations",
            [],
            None,
            "{component?,tei?:true,styles?:[{style,kind:heading|figure|table,level?:1}]} — register existing TEI headings or explicitly mapped paragraph styles once, retaining stable identities",
            has_publication,
            super::semantic::scan
        ),
        cmd!(query "publication.query", "Find Publication Destinations", [], None,
        "{query?:{ids?,kind?,component?,subtree?,role?,minLevel?,maxLevel?,between?}} → semantic objects in source order; does not compose pages",has_publication,|s,p| {
            let v=view(s)?; let q:Query=parse(p.get("query").cloned().unwrap_or(json!({})))?;
            Ok(json!(SemanticIndex::build(&v.root)?.query(&q)?))
        }),
        cmd!(query "publication.references", "Inspect Publication References", [], None,
        "{} → templates, fields, generated sections, locations, dependencies, diagnostics, mode and stale status",has_publication,|s,_| {
            let v=view(s)?; let e=v.root.editions.get(&v.edition).ok_or_else(||bad("publication.references","missing edition"))?;
            Ok(json!({"definitions":v.root.references,"layout":e.references,"revision":v.root.revision,"stale":e.references.source_revision!=Some(v.root.revision)}))
        }),
        cmd!(
            "publication.template.define",
            "Define Reference Template",
            [],
            None,
            "{name,parts:[{token:{type:literal|title|componentTitle|publicationTitle|number|page|globalPage|metadata|value,text?|sequence?|key?},format?:{style?,over?}}]}",
            has_publication,
            |s, p| {
                let template: Template = parse(p.clone())?;
                template.check()?;
                mutate(s, |r, _| {
                    r.references.templates.insert(template.name.clone(), template);
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.reference.insert",
            "Insert Publication Reference",
            [],
            None,
            "{query,template,use?:first|join|runningHead,separator?,suppressOpening?:true} — insert one source-bound rich field at the caret; captions and running heads use the same binding",
            has_publication,
            insert
        ),
        cmd!(
            "publication.reference.edit",
            "Edit Publication Reference",
            [],
            None,
            "{id,query,template,use?:first|join|runningHead,separator?,suppressOpening?:true} — replace a definition, retaining its identity",
            has_publication,
            |s, p| {
                let f: Field = parse(p.clone())?;
                mutate(s, |r, _| {
                    if !r.references.fields.contains_key(&f.id) {
                        return Err(bad("publication.reference.edit", "unknown field"));
                    }
                    r.references.fields.insert(f.id, f);
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.generated.define",
            "Create or Update Generated Section",
            [],
            None,
            "{name,query,template?:Contents entry,title?,titleStyle?,paragraphStyle?,levelStyles?,component?} — create a dedicated generated story on a new page; existing named sections retain their destination",
            has_publication,
            section
        ),
        cmd!(
            "publication.generated.detach",
            "Detach Generated Section",
            [],
            None,
            "{name} — keep editable bound entries but stop query-driven regeneration",
            has_publication,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("publication.generated.detach", "name required"))?;
                mutate(s, |r, _| {
                    if r.references.sections.remove(name).is_none() {
                        return Err(bad("publication.generated.detach", "unknown section"));
                    }
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.updateMode",
            "Set Reference Update Mode",
            [],
            None,
            "{mode:live|deferred} — edition setting; deferred values show a stale flag; explicit update and export always resolve current content",
            has_publication,
            |s, p| {
                let mode: UpdateMode = parse(p.get("mode").cloned().unwrap_or(Value::Null))?;
                mutate(s, |r, v| {
                    Arc::make_mut(r.editions.get_mut(&v.edition).ok_or_else(|| bad("publication.updateMode", "missing edition"))?).references.mode =
                        mode;
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.update",
            "Update Publication References",
            [],
            None,
            "{} — compose the active edition and generated sections to a fixed point (at most 8 passes); errors preserve the previous document",
            has_publication,
            |s, _| {
                let mut d = (*s.doc()?.doc).clone();
                let report = super::update::refresh(&mut d, true, &s.cache)?;
                let st = s.doc_mut()?;
                st.doc = Arc::new(d);
                st.revision = st.revision.saturating_add(1);
                Ok(report)
            }
        ),
        cmd!(noundo "publication.reference.goTo","Go to Publication Reference Destination",[],None,
        "{object} — activate the destination chapter and resolved page",has_publication,|s,p| {
            let id=optional_id(p,"object")?.ok_or_else(||bad("publication.reference.goTo","object required"))?;
            let mut v=view(s)?;
            let index=SemanticIndex::build(&v.root)?;
            let exists=index.by_id.get(&id).and_then(|i|index.entries.get(*i)).is_some_and(|e|e.resolved)
                || v.root.indexes.values().any(|i|i.topics.contains_key(&id)||i.occurrences.contains_key(&id))
                || v.root.annotation_entries()?.iter().any(|n|n.identity.id==id||n.identity.callout==id);
            if !exists{return Err(bad("publication.reference.goTo","unresolved object"));}
            let location=v.root.editions.get(&v.edition).and_then(|e|e.references.locations.get(&id)).ok_or_else(||bad("publication.reference.goTo","destination has no resolved page; update the edition first"))?;
            v.component=location.component;
            let page=location.page;
            let d=v.project()?;let st=s.doc_mut()?;st.doc=Arc::new(d);st.selection=Default::default();
            Ok(json!({"component":v.component,"page":page}))
        }),
    ]
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| bad("publication.references", e.to_string()))
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, selection| {
        let mut v = doc.publication.clone().ok_or_else(|| bad("publication.reference.insert", "no publication"))?;
        let mut r = (*v.root).clone();
        let id = r.alloc()?;
        let mut params = p.clone();
        params["id"] = json!(id);
        let field: Field = parse(params)?;
        r.references.fields.insert(id, field);
        r.revision = r.revision.saturating_add(1);
        let mut caret = selection.text.ok_or_else(|| bad("publication.reference.insert", "select text first"))?;
        let story = doc.text_story_mut(caret.story, caret.cell).ok_or_else(|| bad("publication.reference.insert", "missing story"))?;
        let range = caret.range();
        if !story.text.is_char_boundary(range.start) || !story.text.is_char_boundary(range.end) {
            return Err(bad("publication.reference.insert", "invalid text selection"));
        }
        story.replace(range.clone(), "");
        story.insert_xref(range.start, designcraft_doc::xref::CrossRef { target: 0, format: String::new(), field: Some(id) });
        caret.anchor = range.start + designcraft_doc::XREF_MARK.len_utf8();
        caret.focus = caret.anchor;
        selection.text = Some(caret);
        v.root = Arc::new(r);
        doc.publication = Some(v);
        Ok(json!({"field":id}))
    })
}

fn section(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("publication.generated.define", "name required"))?.to_string();
    let query: Query = parse(p.get("query").cloned().unwrap_or(json!({})))?;
    let template = str_param(p, "template").unwrap_or("Contents entry").to_string();
    let title = str_param(p, "title").unwrap_or(&name).to_string();
    let title_style = str_param(p, "titleStyle").unwrap_or(designcraft_doc::story::BASIC_PARAGRAPH).to_string();
    let paragraph_style = str_param(p, "paragraphStyle").unwrap_or(designcraft_doc::story::BASIC_PARAGRAPH).to_string();
    let level_styles: std::collections::BTreeMap<u8, String> = parse(p.get("levelStyles").cloned().unwrap_or(json!({})))?;
    let requested = optional_id(p, "component")?.map(ComponentId);
    mutate(s, |root, v| {
        let existing = root.references.sections.get(&name).cloned();
        if existing.as_ref().is_some_and(|g| requested.is_some_and(|c| c != g.component)) {
            return Err(bad("publication.generated.define", "an existing section retains its component; use a new name"));
        }
        let component = existing.as_ref().map(|g| g.component).or(requested).unwrap_or(v.component);
        let mut doc = root.project(v.edition, component)?;
        for style in std::iter::once(&title_style).chain(std::iter::once(&paragraph_style)).chain(level_styles.values()) {
            if doc.styles.para(style).is_none() {
                return Err(bad("publication.generated.define", format!("unknown paragraph style {style}")));
            }
        }
        let (story, content, fields) = if let Some(g) = existing {
            if g.title_style != title_style || g.paragraph_style != paragraph_style || g.level_styles != level_styles {
                let story = doc.story_mut(g.story).ok_or_else(|| bad("publication.generated.define", "missing generated destination"))?;
                if let Some(p) = story.paras.first_mut() {
                    p.style = title_style.clone();
                }
                let entries: Vec<_> =
                    story.text.match_indices(designcraft_doc::XREF_MARK).zip(&story.xrefs).map(|((pos, _), x)| (pos, x.field)).collect();
                for (pos, id) in entries {
                    let level = id
                        .and_then(|id| root.references.fields.get(&id))
                        .and_then(|f| f.query.ids.first())
                        .and_then(|id| root.semantic.get(&designcraft_doc::SemanticId(*id)))
                        .map_or(1, |o| o.level);
                    let i = story.para_at(pos);
                    if let Some(p) = story.paras.get_mut(i) {
                        p.style = level_styles.get(&level).unwrap_or(&paragraph_style).clone();
                    }
                }
                root.capture(v.edition, component, &doc)?;
            }
            (g.story, g.content, g.fields)
        } else {
            let page = doc.page_count();
            doc.insert_pages(page.checked_sub(1), 1, None)?;
            let (si, pi) = doc.page_loc(page).ok_or_else(|| bad("publication.generated.define", "missing new page"))?;
            let rect = doc.spreads[si].pages[pi].margin_rect();
            let layer = doc.default_layer();
            let (_, story) = doc.add_text_frame(SpreadRef::Doc(si), rect, layer, "", ParaFormat::default())?;
            root.capture(v.edition, component, &doc)?;
            let content = *root
                .components
                .get(&component)
                .and_then(|c| c.stories.get(&story))
                .ok_or_else(|| bad("publication.generated.define", "missing new story"))?;
            (story, content, Default::default())
        };
        root.references.sections.insert(
            name.clone(),
            GeneratedSection {
                name: name.clone(),
                component,
                story,
                content,
                query,
                template,
                title,
                title_style,
                paragraph_style,
                level_styles,
                fields,
            },
        );
        root.revision = root.revision.saturating_add(1);
        Ok(json!({"name":name,"component":component,"story":story}))
    })
}
