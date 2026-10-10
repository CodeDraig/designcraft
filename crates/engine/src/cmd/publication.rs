//! Publication creation, conversion and component/edition navigation. All mutations prepare
//! a complete candidate root before replacing the active snapshot.

use std::sync::Arc;

use designcraft_doc::{
    ComponentId, Document, EditionId, Selection,
    build::NewDocument,
    publication::{ComponentRole, Publication, PublicationView},
};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, has_doc, str_param};
use crate::{DocState, Result, Session};

mod annotations;
mod editions;
pub(super) mod indexes;
mod layout;
mod references;
pub(crate) mod review;
mod semantic;
mod settings;
pub(crate) mod styles;
pub(crate) mod update;

fn has_publication(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|st| st.doc.publication.is_some()) { Ok(()) } else { Err("no publication open".into()) }
}

fn view(s: &Session) -> Result<PublicationView> {
    s.doc()?.doc.publication.clone().ok_or_else(|| bad("publication", "no publication open"))
}

fn role(p: &Value) -> Result<ComponentRole> {
    serde_json::from_value(p.get("role").cloned().unwrap_or(json!("chapter"))).map_err(|e| bad("publication", e.to_string()))
}

fn optional_id(p: &Value, key: &str) -> Result<Option<u64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().filter(|n| *n > 0).map(Some).ok_or_else(|| bad("publication", format!("{key} must be a positive integer"))),
    }
}

fn open_new(s: &mut Session, mut view: PublicationView, fonts: Vec<Arc<crate::FontScope>>) -> Result<Value> {
    indexes::migrate_legacy(Arc::make_mut(&mut view.root), view.edition)?;
    annotations::prepare(Arc::make_mut(&mut view.root), view.edition)?;
    view.root.check()?;
    let mut state = DocState::new(view.project()?, None);
    state.publication_fonts = fonts;
    state.saved_doc = Arc::new(Document::new(&Default::default()));
    let i = s.add_document(state);
    Ok(json!({"index":i, "component":view.component, "edition":view.edition}))
}

/// Reuse the actual open commands in an isolated session, so TEI/IDML/native validation,
/// resource resolution and font loading are identical without touching the live session.
fn import_component(p: &Value) -> Result<DocState> {
    let mut candidate = Session::new();
    if p.get("base64").is_some() {
        candidate.execute("file.openBytes", p)?;
    } else if p.get("path").is_some() {
        candidate.execute("file.open", p)?;
    } else {
        candidate.execute("file.new", p)?;
    }
    let mut state = candidate.doc()?.clone();
    if state.doc.publication.is_some() {
        return Err(bad("publication.addComponent", "choose a standalone document, not a publication archive"));
    }
    if let Some(title) = str_param(p, "title") {
        Arc::make_mut(&mut state.doc).title = title.into();
    }
    Ok(state)
}

fn mutate<T>(s: &mut Session, edit: impl FnOnce(&mut Publication, &mut PublicationView) -> Result<T>) -> Result<T> {
    s.edit(|doc, selection| {
        doc.sync_publication()?;
        let mut view = doc.publication.clone().ok_or_else(|| bad("publication", "no publication open"))?;
        let previous = (view.component, view.edition);
        let mut root = (*view.root).clone();
        let result = edit(&mut root, &mut view)?;
        root.check()?;
        view.root = Arc::new(root);
        *doc = view.project()?;
        if previous != (view.component, view.edition) {
            *selection = Selection::default();
        }
        Ok(result)
    })
}

pub fn specs() -> Vec<CommandSpec> {
    let mut commands = vec![
        cmd!(noundo "publication.new", "New Publication", ["File", "New"], None,
        "{title?, componentTitle?} — new unsaved publication with a print edition and one chapter", always, |s,p| {
            let title = str_param(p,"title").unwrap_or("Untitled Publication");
            let d = Document::new(&NewDocument { title:str_param(p,"componentTitle").unwrap_or("Chapter 1").into(), ..Default::default() });
            open_new(s, Publication::from_document(&d,title)?, Vec::new())
        }),
        cmd!(noundo "publication.convert", "Convert to Publication", ["File"], None,
        "{title?} — copy the active standalone document into a new unsaved publication, preserving the original", has_doc, |s,p| {
            let st = s.doc()?;
            let fonts = st.fonts.iter().cloned().collect();
            let v = Publication::from_document(&st.doc, str_param(p,"title").unwrap_or(&st.doc.title))?;
            open_new(s,v,fonts)
        }),
        cmd!(noundo "publication.importBook", "Convert Book to Publication", [], None,
            "{path? | base64 + name, documents?: {bookPath: {name, base64}}, title?} — copy every .dcbook chapter into one unsaved archive; omitted input uses the open Book", always, import_book),
        cmd!(query "publication.list", "Publication Structure", [], None,
            "{} → ordered components, editions, persistent content ids and revision; does not compose pages", has_publication, list),
        cmd!(
            "publication.addComponent",
            "Add Publication Component",
            [],
            None,
            "{path? | base64 + name, title?, role?: frontMatter|chapter|backMatter, parent?, tei?, resources?} — import a standalone document; no file creates a blank chapter; one undo step",
            has_publication,
            |s, p| {
                let imported = import_component(p)?;
                let role = role(p)?;
                let parent = optional_id(p, "parent")?.map(ComponentId);
                let result = mutate(s, |root, v| {
                    let id = root.add_document(&imported.doc, role, parent)?;
                    indexes::migrate_legacy(root, v.edition)?;
                    v.component = id;
                    Ok(json!({"component":id}))
                })?;
                if let Some(fonts) = imported.fonts {
                    s.doc_mut()?.publication_fonts.push(fonts);
                }
                Ok(result)
            }
        ),
        cmd!(noundo "publication.activate", "Activate Publication Component", [], None,
            "{component?, edition?} — switch editor projection without changing content or creating an undo step", has_publication, activate),
        cmd!(
            "publication.component",
            "Publication Component Properties",
            [],
            None,
            "{component?, title?, role?: frontMatter|chapter|backMatter}",
            has_publication,
            |s, p| {
                let component = ComponentId(optional_id(p, "component")?.unwrap_or(view(s)?.component.0));
                let new_role = if p.get("role").is_some() { Some(role(p)?) } else { None };
                mutate(s, |root, _| {
                    let c = Arc::make_mut(root.components.get_mut(&component).ok_or_else(|| bad("publication.component", "unknown component"))?);
                    if let Some(title) = str_param(p, "title") {
                        c.title = title.into();
                    }
                    if let Some(role) = new_role {
                        c.role = role;
                    }
                    root.revision = root.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.moveComponent",
            "Move Publication Component",
            [],
            None,
            "{component, parent?: component|null, at?: zero-based sibling index} — move a subtree without changing its identities",
            has_publication,
            move_component
        ),
        cmd!(
            "publication.edition.create",
            "Create Edition",
            [],
            None,
            "{name, source?: edition} — copy the selected edition's layouts; canonical text and assets remain shared",
            has_publication,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("publication.edition.create", "name required"))?;
                let source = EditionId(optional_id(p, "source")?.unwrap_or(view(s)?.edition.0));
                mutate(s, |root, v| {
                    let id = root.duplicate_edition(source, name)?;
                    v.edition = id;
                    Ok(json!({"edition":id}))
                })
            }
        ),
    ];
    commands.extend(styles::specs());
    commands.extend(settings::specs());
    commands.extend(references::specs());
    commands.extend(indexes::specs());
    commands.extend(annotations::specs());
    commands.extend(editions::specs());
    commands.extend(review::specs());
    commands.extend(layout::specs());
    commands
}

fn activate(s: &mut Session, p: &Value) -> Result<Value> {
    if s.doc()?.interaction.is_some() {
        return Err(bad("publication.activate", "finish or cancel the current interaction before switching components"));
    }
    let mut v = view(s)?;
    v.component = ComponentId(optional_id(p, "component")?.unwrap_or(v.component.0));
    v.edition = EditionId(optional_id(p, "edition")?.unwrap_or(v.edition.0));
    let mut d = v.project()?;
    if d.font_scope == 0 {
        d.font_scope = s.doc()?.fonts.as_ref().map_or(0, |f| f.0);
    }
    s.cancel_tool_gesture();
    let st = s.doc_mut()?;
    st.doc = Arc::new(d);
    st.selection = Selection::default();
    st.active_layer = st.doc.default_layer();
    st.editing_parents = false;
    st.revision = st.revision.saturating_add(1);
    Ok(json!({"component":v.component,"edition":v.edition}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let v = view(s)?;
    let components:Vec<_> = v.root.ordered_components()?.iter().filter_map(|id| {
        let c=v.root.components.get(id)?;
        let layout=v.root.editions.get(&v.edition)?.components.get(id)?;
        let parent=v.root.components.values().find(|p|p.children.contains(id)).map(|p|p.id);
        Some(json!({"id":id,"title":c.title,"role":c.role,"parent":parent,"children":c.children,"pages":layout.page_count(),"stories":c.stories,"sources":c.sources}))
    }).collect();
    Ok(json!({"title":v.root.title,"revision":v.root.revision,"activeComponent":v.component,"activeEdition":v.edition,"components":components,
        "editions":v.root.editions.values().map(|e|json!({"id":e.id,"name":e.name})).collect::<Vec<_>>(),"metadata":v.root.metadata,"values":v.root.values}))
}

fn move_component(s: &mut Session, p: &Value) -> Result<Value> {
    let id = ComponentId(optional_id(p, "component")?.ok_or_else(|| bad("publication.moveComponent", "component required"))?);
    let parent = optional_id(p, "parent")?.map(ComponentId);
    let at = match p.get("at") {
        None => None,
        Some(v) => Some(v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("publication.moveComponent", "invalid sibling index"))?),
    };
    mutate(s, |root, _| {
        if !root.components.contains_key(&id) || parent.is_some_and(|id| !root.components.contains_key(&id)) {
            return Err(bad("publication.moveComponent", "unknown component or parent"));
        }
        root.roots.retain(|c| *c != id);
        for c in root.components.values_mut() {
            Arc::make_mut(c).children.retain(|c| *c != id);
        }
        let siblings = match parent {
            Some(parent) => {
                &mut Arc::make_mut(root.components.get_mut(&parent).ok_or_else(|| bad("publication.moveComponent", "missing parent"))?).children
            }
            None => &mut root.roots,
        };
        let at = at.unwrap_or(siblings.len());
        if at > siblings.len() {
            return Err(bad("publication.moveComponent", "sibling index is out of range"));
        }
        siblings.insert(at, id);
        root.ordered_components()?;
        root.revision = root.revision.saturating_add(1);
        Ok(Value::Null)
    })
}

fn import_book(s: &mut Session, p: &Value) -> Result<Value> {
    let book = if p.get("path").is_some() || p.get("base64").is_some() {
        let bytes = if let Some(base64) = str_param(p, "base64") {
            super::tei::decoded(base64, 4 * 1024 * 1024)?
        } else {
            super::tei::read_path(str_param(p, "path").unwrap_or(""), 4 * 1024 * 1024)?
        };
        let mut book: super::book::Book = serde_json::from_slice(&bytes).map_err(|e| bad("publication.importBook", e.to_string()))?;
        book.path = str_param(p, "path").unwrap_or("").into();
        book
    } else {
        s.book.clone().ok_or_else(|| bad("publication.importBook", "give a .dcbook file or open a Book first"))?
    };
    if book.documents.is_empty() || book.documents.len() > designcraft_doc::publication::MAX_COMPONENTS {
        return Err(bad("publication.importBook", "book must contain 1–10000 documents"));
    }
    let mut documents = Vec::new();
    for path in &book.documents {
        let params = if let Some(mapped) = p.get("documents").and_then(|d| d.get(path)) {
            mapped.clone()
        } else {
            if p.get("base64").is_some() {
                return Err(bad("publication.importBook", format!("missing byte mapping for {path}")));
            }
            let base = std::path::Path::new(&book.path).parent().unwrap_or_else(|| std::path::Path::new("."));
            json!({"path":base.join(path).to_string_lossy()})
        };
        documents.push(import_component(&params)?);
    }
    let first = documents.first().ok_or_else(|| bad("publication.importBook", "empty book"))?;
    let mut v = Publication::from_document(&first.doc, str_param(p, "title").unwrap_or("Imported Publication"))?;
    let mut root = (*v.root).clone();
    for st in documents.iter().skip(1) {
        root.add_document(&st.doc, ComponentRole::Chapter, None)?;
    }
    v.root = Arc::new(root);
    let fonts = documents.iter().filter_map(|st| st.fonts.clone()).collect();
    open_new(s, v, fonts)
}

#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod references_tests;
