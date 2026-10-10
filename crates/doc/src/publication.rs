//! Canonical publication content and independent edition layouts.
//!
//! The editor works on a component projection. Its stories share the canonical content Arcs;
//! committing a projection replaces the publication root in the same undo snapshot. Edition
//! page models contain no authored stories or attachments. Legacy ids are local to a component;
//! archive-wide content/resource ids do not change when components move in the publication tree.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub mod annotations;
mod appearance;
pub mod editions;
pub mod indexes;
pub mod numbering;
pub mod references;
pub mod review;
pub mod styles;
pub mod text_edits;

use crate::{
    Asset, AssetId, ComponentId, ContentId, DocError, Document, EditionId, ItemId, ResourceId, Result, SourceId, Story, StoryContent, StoryId,
    TextDirection,
};

pub const MAX_COMPONENTS: usize = 10_000;
pub const MAX_EDITIONS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComponentRole {
    FrontMatter,
    #[default]
    Chapter,
    BackMatter,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub id: ComponentId,
    pub title: String,
    pub role: ComponentRole,
    pub children: Vec<ComponentId>,
    pub stories: BTreeMap<StoryId, ContentId>,
    pub assets: BTreeMap<AssetId, ResourceId>,
    pub sources: Vec<SourceId>,
    /// Shared allocation high-water mark prevents one edition reusing another edition's ids.
    pub next_local_id: u64,
    #[serde(default)]
    pub story_order: Vec<StoryId>,
    /// Variable marker indices refer to one shared definition list in every edition.
    #[serde(default)]
    pub variables: Vec<crate::vars::TextVariable>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryBinding {
    pub frames: Vec<ItemId>,
    pub direction: TextDirection,
    pub vertical: bool,
    pub revision: u64,
    pub link: Option<(StoryId, u64)>,
}

impl StoryBinding {
    fn capture(s: &Story) -> Self {
        Self { frames: s.frames.clone(), direction: s.direction, vertical: s.vertical, revision: s.rev, link: s.link }
    }

    fn project(&self, id: StoryId, content: Arc<StoryContent>) -> Story {
        Story { id, content, frames: self.frames.clone(), direction: self.direction, vertical: self.vertical, rev: self.revision, link: self.link }
    }
}

/// Only layout and edition configuration are retained in `pages`. It is private so callers
/// cannot introduce a second editable copy of canonical content. Capture/project are the sole
/// conversion boundary; validation rejects archives with hidden authored stories or attachments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentLayout {
    pages: Document,
    bindings: BTreeMap<StoryId, StoryBinding>,
    #[serde(default)]
    appearance: BTreeMap<StoryId, appearance::StoryAppearance>,
    #[serde(default)]
    style_baselines: BTreeMap<String, styles::Definition>,
}

impl ComponentLayout {
    fn capture(doc: &Document) -> Result<Self> {
        let mut pages = doc.clone();
        let bindings = pages.stories.iter().map(|(id, s)| (*id, StoryBinding::capture(s))).collect();
        let appearance = pages.stories.iter().map(|(id, s)| Ok((*id, appearance::StoryAppearance::capture(s)?))).collect::<Result<_>>()?;
        pages.stories.clear();
        pages.assets.clear();
        pages.tei_sources.clear();
        pages.text_variables.clear();
        pages.publication = None;
        Ok(Self { pages, bindings, appearance, style_baselines: BTreeMap::new() })
    }

    pub fn page_count(&self) -> usize {
        self.pages.page_count()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edition {
    pub id: EditionId,
    pub name: String,
    pub components: BTreeMap<ComponentId, Arc<ComponentLayout>>,
    #[serde(default)]
    pub references: references::ReferenceLayout,
    #[serde(default)]
    pub note_policies: BTreeMap<String, annotations::Policy>,
    #[serde(default)]
    pub notes: annotations::Layout,
    #[serde(default)]
    pub apparatus_witness: Option<String>,
    #[serde(default)]
    pub output: editions::OutputSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub title: String,
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub numbering: BTreeMap<String, numbering::NumberingSequence>,
    #[serde(default)]
    pub semantic: BTreeMap<crate::SemanticId, numbering::SemanticObject>,
    #[serde(default = "references::defaults")]
    pub references: references::References,
    #[serde(default)]
    pub indexes: BTreeMap<String, indexes::Definition>,
    #[serde(default)]
    pub note_streams: BTreeMap<String, annotations::Stream>,
    #[serde(default)]
    pub note_outputs: BTreeMap<String, annotations::Output>,
    #[serde(default)]
    pub review: review::Review,
    pub roots: Vec<ComponentId>,
    pub components: BTreeMap<ComponentId, Arc<Component>>,
    pub content: BTreeMap<ContentId, Arc<StoryContent>>,
    pub assets: BTreeMap<ResourceId, Arc<Asset>>,
    pub sources: BTreeMap<SourceId, Arc<crate::tei::TeiSource>>,
    pub editions: BTreeMap<EditionId, Arc<Edition>>,
    pub next_id: u64,
    pub revision: u64,
}

/// Runtime view kept with every document snapshot, including history and interaction previews.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicationView {
    pub root: Arc<Publication>,
    pub edition: EditionId,
    pub component: ComponentId,
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

impl Publication {
    pub fn alloc(&mut self) -> Result<u64> {
        if self.next_id >= crate::MAX_NEXT_ID {
            return Err(invalid("publication identity space exhausted"));
        }
        self.next_id += 1;
        Ok(self.next_id)
    }

    pub fn from_document(doc: &Document, title: &str) -> Result<PublicationView> {
        let mut root = Self {
            title: title.into(),
            metadata: BTreeMap::new(),
            values: BTreeMap::new(),
            numbering: BTreeMap::new(),
            semantic: BTreeMap::new(),
            references: references::defaults(),
            indexes: BTreeMap::new(),
            note_streams: BTreeMap::new(),
            note_outputs: BTreeMap::new(),
            review: Default::default(),
            roots: Vec::new(),
            components: BTreeMap::new(),
            content: BTreeMap::new(),
            assets: BTreeMap::new(),
            sources: BTreeMap::new(),
            editions: BTreeMap::new(),
            next_id: 0,
            revision: 0,
        };
        let edition = EditionId(root.alloc()?);
        root.editions.insert(
            edition,
            Arc::new(Edition {
                id: edition,
                name: "Print".into(),
                components: BTreeMap::new(),
                references: Default::default(),
                note_policies: Default::default(),
                notes: Default::default(),
                apparatus_witness: None,
                output: Default::default(),
            }),
        );
        let component = root.add_document(doc, ComponentRole::Chapter, None)?;
        Ok(PublicationView { root: Arc::new(root), edition, component })
    }

    /// Add a complete standalone component to every edition, retaining its source layout and
    /// style definitions. The caller works on an unpublished candidate until validation succeeds.
    pub fn add_document(&mut self, doc: &Document, role: ComponentRole, parent: Option<ComponentId>) -> Result<ComponentId> {
        doc.check()?;
        let mut normalized = doc.clone();
        for story in normalized.stories.values_mut() {
            appearance::assign_cell_ids(Arc::make_mut(story))?;
        }
        self.normalize_annotations(None, &mut normalized)?;
        self.normalize_changes(true, &mut normalized)?;
        let doc = &normalized;
        if doc.publication.is_some() {
            return Err(invalid("choose a standalone document; a publication cannot be nested in another publication"));
        }
        if self.components.len() >= MAX_COMPONENTS {
            return Err(invalid("publication component limit exceeded (10000)"));
        }
        if parent.is_some_and(|p| !self.components.contains_key(&p)) {
            return Err(invalid("unknown parent component"));
        }
        let id = ComponentId(self.alloc()?);
        let mut component = Component {
            id,
            title: doc.title.clone(),
            role,
            children: Vec::new(),
            stories: BTreeMap::new(),
            assets: BTreeMap::new(),
            sources: Vec::new(),
            next_local_id: doc.next_id,
            story_order: Vec::new(),
            variables: doc.text_variables.clone(),
        };
        self.capture_content(&mut component, doc)?;
        self.components.insert(id, Arc::new(component));
        if let Some(parent) = parent {
            let p = self.components.get_mut(&parent).ok_or_else(|| invalid("unknown parent component"))?;
            Arc::make_mut(p).children.push(id);
        } else {
            self.roots.push(id);
        }
        let layout = Arc::new(ComponentLayout::capture(doc)?);
        for edition in self.editions.values_mut() {
            Arc::make_mut(edition).components.insert(id, layout.clone());
        }
        self.revision = self.revision.saturating_add(1);
        Ok(id)
    }

    fn capture_content(&mut self, c: &mut Component, doc: &Document) -> Result<()> {
        c.title = doc.title.clone();
        c.next_local_id = c.next_local_id.max(doc.next_id);
        c.variables = doc.text_variables.clone();
        c.story_order.retain(|id| doc.stories.contains_key(id));
        // Establish semantic story order from initial frames. Later frame moves in an edition
        // do not silently reorder the shared publication's reading sequence.
        let mut positions = Vec::new();
        for (si, spread) in doc.spreads.iter().enumerate() {
            for top in &spread.items {
                top.walk(&mut |item| {
                    if let Some(frame) = item.text_frame() {
                        let bounds = item.bounds();
                        positions.push((si, (bounds.y0 * 10.0) as i64, (bounds.x0 * 10.0) as i64, frame.story));
                    }
                });
            }
        }
        positions.sort();
        for id in positions.iter().map(|p| p.3).chain(doc.stories.keys().copied()) {
            if !c.story_order.contains(&id) {
                c.story_order.push(id);
            }
        }
        c.stories.retain(|id, _| doc.stories.contains_key(id));
        for (&id, story) in &doc.stories {
            let key = match c.stories.get(&id) {
                Some(key) => *key,
                None => ContentId(self.alloc()?),
            };
            if self.content.get(&key).is_none_or(|old| **old != *story.content) {
                self.content.insert(key, story.content.clone());
            }
            c.stories.insert(id, key);
        }
        c.assets.retain(|id, _| doc.assets.contains_key(id));
        for (&id, asset) in &doc.assets {
            let key = match c.assets.get(&id) {
                Some(key) => *key,
                None => ResourceId(self.alloc()?),
            };
            self.assets.insert(key, asset.clone());
            c.assets.insert(id, key);
        }
        let mut sources = Vec::new();
        for source in &doc.tei_sources {
            let key = match c.sources.iter().find(|key| self.sources.get(key).is_some_and(|s| s.id == source.id)) {
                Some(key) => *key,
                None => SourceId(self.alloc()?),
            };
            if let Some(previous) = self.sources.get(&key)
                && previous != source
            {
                return Err(invalid(format!("preserved TEI source {} is immutable", source.id)));
            }
            self.sources.insert(key, source.clone());
            sources.push(key);
        }
        if c.sources.iter().any(|key| !sources.contains(key)) {
            return Err(invalid("cannot discard a preserved TEI source"));
        }
        c.sources = sources;
        Ok(())
    }

    pub fn ordered_components(&self) -> Result<Vec<ComponentId>> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut pending: Vec<_> = self.roots.iter().rev().copied().collect();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                return Err(invalid(format!("component {id} appears twice or forms a cycle")));
            }
            let c = self.components.get(&id).ok_or_else(|| invalid(format!("missing component {id}")))?;
            out.push(id);
            pending.extend(c.children.iter().rev().copied());
        }
        if out.len() != self.components.len() {
            return Err(invalid("publication has components outside its tree"));
        }
        Ok(out)
    }

    /// Materialize one edition component without making another authoritative content copy.
    pub fn project(&self, edition: EditionId, component: ComponentId) -> Result<Document> {
        let c = self.components.get(&component).ok_or_else(|| invalid(format!("unknown component {component}")))?;
        let layout = self
            .editions
            .get(&edition)
            .and_then(|e| e.components.get(&component))
            .ok_or_else(|| invalid(format!("missing layout for {edition}/{component}")))?;
        if !layout.pages.stories.is_empty()
            || !layout.pages.assets.is_empty()
            || !layout.pages.tei_sources.is_empty()
            || layout.pages.publication.is_some()
        {
            return Err(invalid("edition layout contains duplicate canonical content"));
        }
        if layout.bindings.len() != c.stories.len() {
            return Err(invalid("edition story bindings do not match component content"));
        }
        let mut doc = layout.pages.clone();
        doc.title = c.title.clone();
        doc.next_id = c.next_local_id;
        if !c.variables.is_empty() || doc.text_variables.is_empty() {
            doc.text_variables = c.variables.clone();
        }
        for (&id, key) in &c.stories {
            let content = self.content.get(key).ok_or_else(|| invalid(format!("missing content {key}")))?;
            let binding = layout.bindings.get(&id).ok_or_else(|| invalid(format!("missing story binding {id}")))?;
            let mut story = binding.project(id, content.clone());
            if let Some(appearance) = layout.appearance.get(&id) {
                appearance.apply(&mut story)?;
            }
            doc.stories.insert(id, Arc::new(story));
        }
        for (&id, key) in &c.assets {
            let asset = self.assets.get(key).ok_or_else(|| invalid(format!("missing asset {key}")))?;
            if asset.id != id {
                return Err(invalid("asset identity does not match its component mapping"));
            }
            doc.assets.insert(id, asset.clone());
        }
        for key in &c.sources {
            doc.tei_sources.push(self.sources.get(key).ok_or_else(|| invalid(format!("missing source {key}")))?.clone());
        }
        doc.check()?;
        Ok(doc)
    }

    /// Capture one edited projection. Other editions retain their frames and styles, and see
    /// the new canonical text on their next projection. Deleted stories remove their frames in
    /// every edition; new stories are explicitly unplaced in editions without a binding yet.
    pub fn capture(&mut self, edition: EditionId, component: ComponentId, doc: &Document) -> Result<()> {
        doc.check()?;
        let mut normalized = doc.clone();
        for story in normalized.stories.values_mut() {
            appearance::assign_cell_ids(Arc::make_mut(story))?;
        }
        self.normalize_annotations(Some(component), &mut normalized)?;
        self.normalize_changes(false, &mut normalized)?;
        let doc = &normalized;
        let original = self.components.get(&component).ok_or_else(|| invalid("unknown component"))?.clone();
        if !self.editions.contains_key(&edition) {
            return Err(invalid("unknown edition"));
        }
        let mut comparable = doc.clone();
        comparable.publication = None;
        let current = self.project(edition, component)?;
        comparable.font_scope = current.font_scope;
        if current == comparable {
            return Ok(());
        }
        let mut other_layouts = Vec::new();
        for &other in self.editions.keys().filter(|id| **id != edition) {
            let mut projected = self.project(other, component)?;
            for id in original.stories.keys().filter(|id| !doc.stories.contains_key(id)) {
                let frames = projected.story(*id).map(|s| s.frames.clone()).unwrap_or_default();
                for frame in frames {
                    projected.remove_item(frame)?;
                }
                projected.stories.remove(id);
            }
            for (&id, story) in &doc.stories {
                if let Some(existing) = projected.story_mut(id) {
                    let mut carried = appearance::carry(existing, story)?;
                    if self.generated_destinations().any(|(_, c, s, _)| c == component && s == id) {
                        references::carry_entry_formats(existing, &mut carried);
                    }
                    *existing = carried;
                } else {
                    let mut new = (**story).clone();
                    new.frames.clear();
                    projected.stories.insert(id, Arc::new(new));
                }
            }
            let mut layout = ComponentLayout::capture(&projected)?;
            if let Some(old) = self.editions.get(&other).and_then(|e| e.components.get(&component)) {
                layout.style_baselines = old.style_baselines.clone();
            }
            other_layouts.push((other, Arc::new(layout)));
        }
        let mut c = (*original).clone();
        self.capture_content(&mut c, doc)?;
        // Threading moves stable anchors into another canonical story. Retain the semantic
        // object identity and update only its content owner; absent anchors remain unresolved.
        let mut owners = BTreeMap::new();
        for (&sid, story) in &doc.stories {
            if doc.endnote_story == Some(sid) {
                continue;
            }
            let Some(&content) = c.stories.get(&sid) else { continue };
            let mut pending = vec![story.as_ref()];
            while let Some(s) = pending.pop() {
                for anchor in &s.anchors {
                    owners.insert(anchor.id, content);
                }
                pending.extend(s.tables.values().flat_map(|t| t.cells.iter()).map(|cell| &cell.text));
                pending.extend(s.notes.iter().chain(&s.endnotes).map(|n| &n.text));
            }
        }
        for index in self.indexes.values_mut() {
            for occurrence in index.occurrences.values_mut() {
                for target in std::iter::once(&mut occurrence.start).chain(occurrence.end.iter_mut().map(|e| &mut e.target)) {
                    if target.component == component
                        && let Some(content) = owners.get(&target.anchor)
                    {
                        target.content = *content;
                    }
                }
            }
        }
        for object in self.semantic.values_mut().filter(|o| o.target.component == component) {
            if let Some(content) = owners.get(&object.target.anchor) {
                object.target.content = *content;
            }
        }
        for comment in self.review.comments.values_mut() {
            for target in std::iter::once(&mut comment.start).chain(comment.end.iter_mut()) {
                if target.component == component
                    && let Some(content) = owners.get(&target.anchor)
                {
                    target.content = *content;
                }
            }
        }
        self.components.insert(component, Arc::new(c));
        let mut layout = ComponentLayout::capture(doc)?;
        if let Some(old) = self.editions.get(&edition).and_then(|e| e.components.get(&component)) {
            layout.style_baselines = old.style_baselines.clone();
        }
        other_layouts.push((edition, Arc::new(layout)));
        for (id, layout) in other_layouts {
            let e = self.editions.get_mut(&id).ok_or_else(|| invalid("unknown edition"))?;
            Arc::make_mut(e).components.insert(component, layout);
        }
        let referenced: BTreeSet<_> = self.components.values().flat_map(|c| c.stories.values().copied()).collect();
        self.content.retain(|id, _| referenced.contains(id));
        let referenced: BTreeSet<_> = self.components.values().flat_map(|c| c.assets.values().copied()).collect();
        self.assets.retain(|id, _| referenced.contains(id));
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn duplicate_edition(&mut self, source: EditionId, name: &str) -> Result<EditionId> {
        if self.editions.len() >= MAX_EDITIONS || name.trim().is_empty() {
            return Err(invalid("edition needs a name; at most 64 editions are supported"));
        }
        let mut edition = (**self.editions.get(&source).ok_or_else(|| invalid("unknown source edition"))?).clone();
        edition.id = EditionId(self.alloc()?);
        edition.name = name.into();
        let id = edition.id;
        self.editions.insert(id, Arc::new(edition));
        self.revision = self.revision.saturating_add(1);
        Ok(id)
    }

    pub fn check(&self) -> Result<()> {
        if self.components.is_empty() || self.components.len() > MAX_COMPONENTS || self.editions.is_empty() || self.editions.len() > MAX_EDITIONS {
            return Err(invalid("publication needs 1–10000 components and 1–64 editions"));
        }
        self.ordered_components()?;
        let annotations = self.annotation_entries()?;
        let mut ids = BTreeSet::new();
        for id in self
            .components
            .keys()
            .map(|id| id.0)
            .chain(self.editions.keys().map(|id| id.0))
            .chain(self.content.keys().map(|id| id.0))
            .chain(self.assets.keys().map(|id| id.0))
            .chain(self.sources.keys().map(|id| id.0))
            .chain(self.semantic.keys().map(|id| id.0))
            .chain(self.references.fields.keys().copied())
            .chain(self.indexes.values().flat_map(|i| i.identities()))
            .chain(annotations.iter().flat_map(|n| [n.identity.id, n.identity.callout]))
            .chain(self.review.identities())
        {
            if id == 0 || id > self.next_id || !ids.insert(id) {
                return Err(invalid("invalid or duplicate publication identity"));
            }
        }
        if self.next_id > crate::MAX_NEXT_ID {
            return Err(invalid("publication identity counter out of range"));
        }
        for (id, c) in &self.components {
            if *id != c.id || c.next_local_id > crate::MAX_NEXT_ID {
                return Err(invalid("invalid component identity"));
            }
            let unique: BTreeSet<_> = c.story_order.iter().copied().collect();
            if unique.len() != c.story_order.len() || unique.iter().any(|id| !c.stories.contains_key(id)) {
                return Err(invalid("invalid component reading order"));
            }
        }
        for (id, object) in &self.semantic {
            if *id != object.id || !self.components.contains_key(&object.target.component) || object.level == 0 || object.level > 9 {
                return Err(invalid("invalid semantic object identity, component or level"));
            }
        }
        for (name, sequence) in &self.numbering {
            if name != &sequence.name {
                return Err(invalid("numbering sequence name mismatch"));
            }
            sequence.check(self)?;
        }
        if self.indexes.len() > 1024 {
            return Err(invalid("at most 1024 indexes are supported"));
        }
        for (name, index) in &self.indexes {
            if name != &index.name {
                return Err(invalid("index name mismatch"));
            }
            index.check(self)?;
        }
        self.references.check(self)?;
        self.check_annotations()?;
        self.review.check(self)?;
        for (id, e) in &self.editions {
            e.output.check(self)?;
            if *id != e.id || e.components.len() != self.components.len() {
                return Err(invalid("invalid edition identity or component set"));
            }
            for &component in self.components.keys() {
                self.project(*id, component)?;
            }
        }
        Ok(())
    }
}

impl PublicationView {
    pub fn project(&self) -> Result<Document> {
        let mut doc = self.root.project(self.edition, self.component)?;
        doc.publication = Some(self.clone());
        Ok(doc)
    }
}

impl Document {
    /// Synchronize a candidate before publishing it to session/history or serialization. No
    /// mutation reaches the previous root if capture or validation fails.
    pub fn sync_publication(&mut self) -> Result<()> {
        let Some(view) = self.publication.clone() else { return Ok(()) };
        for (name, _, story, content) in view.root.generated_destinations().filter(|(_, c, _, _)| *c == view.component) {
            let old = view.root.content.get(&content);
            let current = self.story(story);
            if !matches!((old,current),(Some(a),Some(b)) if a.text==b.text && a.xrefs==b.xrefs && a.tables==b.tables && a.notes==b.notes && a.endnotes==b.endnotes && a.objects==b.objects)
            {
                return Err(invalid(format!("{} is a generated section; change its query/template or detach it before editing its text", name)));
            }
        }
        let mut root = (*view.root).clone();
        root.capture(view.edition, view.component, self)?;
        if root.revision == view.root.revision {
            return Ok(());
        }
        root.check()?;
        self.stories = root.project(view.edition, view.component)?.stories;
        self.publication = Some(PublicationView { root: Arc::new(root), ..view });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
