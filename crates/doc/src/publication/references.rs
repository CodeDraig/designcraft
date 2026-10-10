//! Shared semantic queries and rich templates. Resolving page locations belongs to composition;
//! selecting a destination or inspecting a binding never requires layout.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{
    ComponentRole, Publication, invalid,
    numbering::{ObjectKind, TextTarget},
};

mod render;
use crate::{CharFormat, ComponentId, ContentId, Result, StoryId};
pub use render::RenderContext;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Query {
    /// Publication entity identities: component ids for chapters, semantic ids for passages.
    pub ids: Vec<u64>,
    pub kind: Option<ObjectKind>,
    pub component: Option<ComponentId>,
    pub subtree: bool,
    pub role: Option<ComponentRole>,
    pub min_level: Option<u8>,
    pub max_level: Option<u8>,
    /// Inclusive endpoints in canonical reading order, each a persistent entity identity.
    pub between: Option<[u64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub id: u64,
    pub kind: ObjectKind,
    pub component: ComponentId,
    pub level: u8,
    pub title: String,
    pub target: Option<TextTarget>,
    pub resolved: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SemanticIndex {
    pub entries: Vec<Entity>,
    pub by_id: BTreeMap<u64, usize>,
    components: BTreeMap<ComponentId, (ComponentRole, Vec<ComponentId>)>,
}

impl SemanticIndex {
    pub fn build(root: &Publication) -> Result<Self> {
        let mut index = Self::default();
        for component in root.ordered_components()? {
            let c = root.components.get(&component).ok_or_else(|| invalid("missing semantic component"))?;
            index.components.insert(component, (c.role, c.children.clone()));
            index.entries.push(Entity {
                id: component.0,
                kind: ObjectKind::Chapter,
                component,
                level: 1,
                title: c.title.clone(),
                target: None,
                resolved: true,
            });
            let mut objects = Vec::new();
            for o in root.semantic.values().filter(|o| o.target.component == component) {
                let target = root.target_text(&o.target);
                let order = target.as_ref().and_then(|t| c.story_order.iter().position(|id| *id == t.story)).unwrap_or(usize::MAX);
                let path = target.as_ref().map(|t| t.path.clone()).unwrap_or_default();
                let title = o.title.clone().or_else(|| target.as_ref().map(|t| t.title.clone())).unwrap_or_else(|| "Unresolved passage".into());
                objects.push((
                    order,
                    path,
                    Entity { id: o.id.0, kind: o.kind, component, level: o.level, title, target: Some(o.target.clone()), resolved: target.is_some() },
                ));
            }
            objects.sort_by(|a, b| (&a.0, &a.1, a.2.id).cmp(&(&b.0, &b.1, b.2.id)));
            index.entries.extend(objects.into_iter().map(|(_, _, e)| e));
        }
        index.by_id = index.entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
        Ok(index)
    }

    pub fn query(&self, query: &Query) -> Result<Vec<Entity>> {
        if query.ids.len() > 100_000
            || query.min_level.is_some_and(|n| n == 0 || n > 9)
            || query.max_level.is_some_and(|n| n == 0 || n > 9)
            || matches!((query.min_level,query.max_level),(Some(a),Some(b)) if a>b)
        {
            return Err(invalid("query supports at most 100000 identities and heading levels 1–9 in ascending order"));
        }
        let mut scope = BTreeSet::new();
        if let Some(component) = query.component {
            if !self.components.contains_key(&component) {
                return Err(invalid("unknown query component"));
            }
            let mut pending = vec![component];
            while let Some(id) = pending.pop() {
                if !scope.insert(id) {
                    return Err(invalid("cyclic query scope"));
                }
                if query.subtree {
                    pending.extend(self.components.get(&id).into_iter().flat_map(|(_, children)| children).copied());
                }
            }
        }
        let range = query
            .between
            .map(|[a, b]| {
                let a = self.by_id.get(&a).copied().ok_or_else(|| invalid("query range start is unresolved"))?;
                let b = self.by_id.get(&b).copied().ok_or_else(|| invalid("query range end is unresolved"))?;
                if a > b {
                    return Err(invalid("query range endpoints are reversed in reading order"));
                }
                Ok(a..=b)
            })
            .transpose()?;
        let ids: BTreeSet<_> = query.ids.iter().copied().collect();
        let mut out: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                (ids.is_empty() || ids.contains(&e.id))
                    && query.kind.is_none_or(|k| k == e.kind)
                    && (scope.is_empty() || scope.contains(&e.component))
                    && query.role.is_none_or(|role| self.components.get(&e.component).is_some_and(|(r, _)| *r == role))
                    && query.min_level.is_none_or(|n| e.level >= n)
                    && query.max_level.is_none_or(|n| e.level <= n)
                    && range.as_ref().is_none_or(|r| r.contains(i))
            })
            .map(|(_, e)| e.clone())
            .collect();
        // Explicit references keep vanished identities visible instead of changing their target.
        for id in ids.iter().filter(|id| !self.by_id.contains_key(id)) {
            out.push(Entity {
                id: *id,
                kind: query.kind.unwrap_or(ObjectKind::Heading),
                component: query.component.unwrap_or(ComponentId(0)),
                level: 1,
                title: format!("Unresolved object {id}"),
                target: None,
                resolved: false,
            });
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Token {
    Literal { text: String },
    Title,
    ComponentTitle,
    PublicationTitle,
    Number { sequence: String },
    Page,
    GlobalPage,
    Metadata { key: String },
    Value { key: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplatePart {
    pub token: Token,
    #[serde(default, deserialize_with = "read_format")]
    pub format: CharFormat,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Template {
    pub name: String,
    pub parts: Vec<TemplatePart>,
}

impl Template {
    pub fn check(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 256 || self.parts.is_empty() || self.parts.len() > 128 {
            return Err(invalid("template needs a name (1–256 bytes) and 1–128 parts"));
        }
        for part in &self.parts {
            let value = match &part.token {
                Token::Literal { text } => Some(text),
                Token::Number { sequence } => Some(sequence),
                Token::Metadata { key } | Token::Value { key } => Some(key),
                _ => None,
            };
            if value.is_some_and(|s| s.len() > 65536 || s.chars().any(|c| ('\u{E000}'..='\u{F8FF}').contains(&c))) {
                return Err(invalid("template text exceeds 65536 bytes or contains private document markers"));
            }
        }
        Ok(())
    }
    pub fn uses_pages(&self) -> bool {
        self.parts.iter().any(|p| matches!(p.token, Token::Page | Token::GlobalPage))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldUse {
    #[default]
    First,
    Join,
    RunningHead,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    pub id: u64,
    pub query: Query,
    pub template: String,
    #[serde(default, rename = "use")]
    pub use_: FieldUse,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "yes")]
    pub suppress_opening: bool,
}
fn default_separator() -> String {
    ", ".into()
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichRun {
    pub text: String,
    pub format: CharFormat,
    pub target: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldText {
    pub runs: Vec<RichRun>,
    pub unresolved: Vec<String>,
}
impl FieldText {
    pub fn plain(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub component: ComponentId,
    pub page: usize,
    pub global_page: usize,
    pub label: String,
    pub order: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FieldValues {
    pub text: FieldText,
    pub pages: BTreeMap<ComponentId, BTreeMap<usize, FieldText>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateMode {
    #[default]
    Live,
    Deferred,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReferenceLayout {
    pub mode: UpdateMode,
    pub source_revision: Option<u64>,
    pub locations: BTreeMap<u64, Location>,
    pub first_body: BTreeMap<ComponentId, BTreeMap<usize, Vec<usize>>>,
    pub fields: BTreeMap<u64, FieldValues>,
    pub dependencies: BTreeMap<u64, Vec<Dependency>>,
    pub passes: usize,
    pub generation: u64,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum Dependency {
    Entity(u64),
    ReadingOrder,
    Pagination(ComponentId),
    Template(String),
    Numbering(String),
    Metadata(String),
    Value(String),
    PublicationTitle,
    Index(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedSection {
    pub name: String,
    pub component: ComponentId,
    pub story: StoryId,
    pub content: ContentId,
    pub query: Query,
    pub template: String,
    pub title: String,
    pub title_style: String,
    pub paragraph_style: String,
    #[serde(default)]
    pub level_styles: BTreeMap<u8, String>,
    /// An existing entry keeps its field identity as the query's reading order changes.
    #[serde(default)]
    pub fields: BTreeMap<u64, u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct References {
    pub templates: BTreeMap<String, Template>,
    pub fields: BTreeMap<u64, Field>,
    pub sections: BTreeMap<String, GeneratedSection>,
}

pub fn defaults() -> References {
    let make = |name: &str, tokens: Vec<Token>| Template {
        name: name.into(),
        parts: tokens.into_iter().map(|token| TemplatePart { token, format: CharFormat::default() }).collect(),
    };
    let templates = [
        make("Title", vec![Token::Title]),
        make("Title and page", vec![Token::Title, Token::Literal { text: ", p. ".into() }, Token::Page]),
        make("Contents entry", vec![Token::Title, Token::Literal { text: "\t".into() }, Token::Page]),
        make("Page", vec![Token::Literal { text: "page ".into() }, Token::Page]),
    ]
    .into_iter()
    .map(|t| (t.name.clone(), t))
    .collect();
    References { templates, ..Default::default() }
}

impl References {
    pub fn check(&self, root: &Publication) -> Result<()> {
        if self.fields.len() > 100_000 || self.templates.len() > 1024 || self.sections.len() > 1024 {
            return Err(invalid("publication supports 100000 generated fields, 1024 templates and 1024 generated sections"));
        }
        for (name, template) in &self.templates {
            if name != &template.name {
                return Err(invalid("template name mismatch"));
            }
            template.check()?;
        }
        // Query selection is independent of pagination, including unresolved explicit targets.
        let index = SemanticIndex::build(root)?;
        for (id, field) in &self.fields {
            if *id != field.id || !self.templates.contains_key(&field.template) {
                return Err(invalid("invalid generated field identity or missing template"));
            }
            index.query(&field.query)?;
            if field.separator.len() > 1024 {
                return Err(invalid("field separator exceeds 1024 bytes"));
            }
        }
        let index_fields = root.index_field_ids();
        for content in root.content.values() {
            if field_ids(content).iter().any(|id| !self.fields.contains_key(id) && !index_fields.contains(id)) {
                return Err(invalid("content refers to an unknown publication field"));
            }
        }
        for (name, section) in &self.sections {
            if name != &section.name
                || name.trim().is_empty()
                || name.len() > 256
                || !self.templates.contains_key(&section.template)
                || root.components.get(&section.component).and_then(|c| c.stories.get(&section.story)) != Some(&section.content)
                || section.fields.values().any(|id| !self.fields.contains_key(id))
            {
                return Err(invalid("invalid generated section name, destination, template or field"));
            }
            index.query(&section.query)?;
        }
        Ok(())
    }
}

impl crate::Document {
    /// Read the edition's last completed update. Deferred edits deliberately retain these values;
    /// the publication panel exposes their stale revision until an explicit update or export.
    pub fn publication_field(&self, id: u64, page: Option<usize>) -> FieldText {
        self.publication
            .as_ref()
            .and_then(|v| {
                v.root
                    .editions
                    .get(&v.edition)
                    .and_then(|e| e.references.fields.get(&id))
                    .map(|f| page.and_then(|p| f.pages.get(&v.component).and_then(|pages| pages.get(&p))).unwrap_or(&f.text).clone())
            })
            .unwrap_or_else(|| FieldText {
                runs: vec![RichRun { text: format!("[Unupdated reference {id}]"), format: Default::default(), target: None }],
                unresolved: vec![format!("Reference {id} needs an update")],
            })
    }
}

/// All field bindings in content, including tables and notes. Iterative traversal is also used
/// at clipboard/persistence boundaries so nested fields cannot silently bind to another book.
pub fn field_ids(content: &crate::StoryContent) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    let mut pending = vec![content];
    while let Some(s) = pending.pop() {
        out.extend(s.xrefs.iter().filter_map(|x| x.field));
        pending.extend(s.tables.values().flat_map(|t| t.cells.iter()).map(|c| c.text.content.as_ref()));
        pending.extend(s.notes.iter().chain(&s.endnotes).map(|n| n.text.content.as_ref()));
    }
    out
}

#[cfg(test)]
mod tests;

/// Preserve a generated entry's edition formatting by field identity when membership or
/// reading order changes. A new entry keeps the destination's requested defaults.
pub fn carry_entry_formats(previous: &crate::Story, next: &mut crate::Story) {
    let mut formats = BTreeMap::new();
    for ((pos, _), xref) in previous.text.match_indices(crate::XREF_MARK).zip(&previous.xrefs) {
        if let Some(id) = xref.field
            && let Some(para) = previous.paras.get(previous.para_at(pos))
        {
            formats.insert(id, (para.clone(), previous.format_after(pos).clone()));
        }
    }
    let entries: Vec<_> =
        next.text.match_indices(crate::XREF_MARK).zip(&next.xrefs).filter_map(|((pos, _), x)| x.field.map(|id| (pos, id))).collect();
    for (pos, id) in entries {
        if let Some((para, chars)) = formats.get(&id) {
            let i = next.para_at(pos);
            if let Some(p) = next.paras.get_mut(i) {
                *p = para.clone();
            }
            next.format_chars(pos..pos + crate::XREF_MARK.len_utf8(), |f| *f = chars.clone());
        }
    }
}

pub(crate) fn read_format<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<CharFormat, D::Error> {
    #[derive(Deserialize, Default)]
    #[serde(default, deny_unknown_fields)]
    struct Format {
        style: String,
        over: crate::CharAttrs,
    }
    let value = Format::deserialize(d)?;
    Ok(CharFormat { style: value.style, over: value.over })
}
