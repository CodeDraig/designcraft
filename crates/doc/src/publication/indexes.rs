//! Independent indexes over persistent publication passages. Preview uses existing pagination;
//! generating the destination is a separate operation in the bounded reference update cycle.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{Publication, invalid, numbering::TextTarget, references::Location};
use crate::{CharFormat, ComponentId, ContentId, Result, StoryId};

mod render;
mod sorting;
pub use render::{OccurrencePreview, Preview, Row, preview, render_entries};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortMode {
    #[default]
    Alphabetical,
    Numeric,
    Biblical,
    Archival,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    #[default]
    Recto,
    Verso,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SortKey {
    Alphabetical {
        key: String,
    },
    Numeric {
        value: i64,
    },
    Biblical {
        book: String,
        chapter: u32,
        verse: u32,
        #[serde(default)]
        end_chapter: Option<u32>,
        #[serde(default)]
        end_verse: Option<u32>,
    },
    Archival {
        repository: String,
        collection: String,
        shelfmark: String,
        folio: u32,
        side: Side,
    },
}
impl Default for SortKey {
    fn default() -> Self {
        Self::Alphabetical { key: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Topic {
    pub id: u64,
    /// Display spelling is preserved; sorting and aliases never rewrite it.
    pub display: String,
    #[serde(default)]
    pub parent: Option<u64>,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default)]
    pub see: Vec<u64>,
    #[serde(default)]
    pub see_also: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub id: u64,
    pub target: TextTarget,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Occurrence {
    pub id: u64,
    pub topic: u64,
    pub start: TextTarget,
    #[serde(default)]
    pub end: Option<Endpoint>,
    #[serde(default)]
    pub suppress_pages: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageLabels {
    #[default]
    Global,
    Local,
    Component,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Formatting {
    pub title: String,
    pub title_style: String,
    pub section_style: String,
    pub level_styles: BTreeMap<usize, String>,
    #[serde(default, deserialize_with = "super::references::read_format")]
    pub topic_format: CharFormat,
    #[serde(default, deserialize_with = "super::references::read_format")]
    pub page_format: CharFormat,
    pub section_headings: bool,
    pub run_in: bool,
    pub following_topic: String,
    pub between_pages: String,
    pub page_range: String,
    pub between_entries: String,
    pub before_cross_ref: String,
    pub page_labels: PageLabels,
}
impl Default for Formatting {
    fn default() -> Self {
        Self {
            title: "Index".into(),
            title_style: "[Basic Paragraph]".into(),
            section_style: "[Basic Paragraph]".into(),
            level_styles: BTreeMap::new(),
            topic_format: Default::default(),
            page_format: Default::default(),
            section_headings: true,
            run_in: false,
            following_topic: "  ".into(),
            between_pages: ", ".into(),
            page_range: "–".into(),
            between_entries: "; ".into(),
            before_cross_ref: ". ".into(),
            page_labels: PageLabels::Global,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub component: ComponentId,
    pub story: StoryId,
    pub content: ContentId,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub sorting: SortMode,
    /// Explicit canonical book names in the requested order; no implicit canon.
    pub book_order: Vec<String>,
    /// Alternative input keys map to configured names, without altering Topic.display.
    pub book_aliases: BTreeMap<String, String>,
    pub format: Formatting,
    pub topics: BTreeMap<u64, Topic>,
    pub occurrences: BTreeMap<u64, Occurrence>,
    pub destination: Option<Destination>,
    /// Persistent generated-field bindings also survive detaching an output story.
    pub fields: BTreeMap<u64, u64>,
    /// Component identities already migrated, including components containing no markers.
    pub migrated: BTreeSet<ComponentId>,
}

impl Definition {
    pub fn check(&self, root: &Publication) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 256 || self.topics.len() > 100_000 || self.occurrences.len() > 100_000 {
            return Err(invalid("index needs a name up to 256 bytes, at most 100000 topics and 100000 occurrences"));
        }
        let books: BTreeSet<_> = self.book_order.iter().collect();
        if books.len() != self.book_order.len()
            || self.book_order.len() > 1024
            || self.book_order.iter().any(|b| b.trim().is_empty() || b.len() > 256)
            || (self.sorting == SortMode::Biblical && books.is_empty())
            || self.book_aliases.iter().any(|(a, b)| a.is_empty() || a.len() > 256 || !books.contains(b))
        {
            return Err(invalid("biblical indexes require a unique explicit book order; aliases must name a configured book"));
        }
        for (id, t) in &self.topics {
            if *id != t.id || t.display.trim().is_empty() || t.display.len() > 4096 || t.see.len() > 128 || t.see_also.len() > 128 {
                return Err(invalid("invalid topic identity, display (1–4096 bytes) or aliases"));
            }
            self.sort_key(t)?;
            let mut seen = BTreeSet::new();
            let mut current = Some(t.id);
            while let Some(id) = current {
                if !seen.insert(id) || seen.len() > 16 {
                    return Err(invalid("index topic hierarchy is cyclic or exceeds 16 levels"));
                }
                current = self.topics.get(&id).ok_or_else(|| invalid("missing parent topic"))?.parent;
            }
            for target in t.see.iter().chain(&t.see_also) {
                if !self.topics.contains_key(target) {
                    return Err(invalid("index alias refers to an unknown topic"));
                }
            }
            let mut pending = t.see.clone();
            let mut visited = BTreeSet::new();
            while let Some(id) = pending.pop() {
                if id == t.id {
                    return Err(invalid("index See aliases form a cycle"));
                }
                if visited.insert(id) {
                    pending.extend(self.topics.get(&id).into_iter().flat_map(|v| &v.see).copied());
                }
            }
        }
        for (id, o) in &self.occurrences {
            if *id != o.id
                || !self.topics.contains_key(&o.topic)
                || !root.components.contains_key(&o.start.component)
                || o.end.as_ref().is_some_and(|e| !root.components.contains_key(&e.target.component))
            {
                return Err(invalid("index occurrence has an invalid identity, topic or component"));
            }
            // Missing content/anchors remain diagnosable after deletion; never rebind by offset.
        }
        if let Some(d) = &self.destination
            && root.components.get(&d.component).and_then(|c| c.stories.get(&d.story)) != Some(&d.content)
        {
            return Err(invalid("index destination or generated topic binding is missing"));
        }
        if self.fields.keys().any(|id| !self.topics.contains_key(id)) {
            return Err(invalid("index field refers to a missing topic"));
        }
        if self.format.level_styles.keys().any(|n| *n == 0 || *n > 16)
            || [
                &self.format.title,
                &self.format.following_topic,
                &self.format.between_pages,
                &self.format.page_range,
                &self.format.between_entries,
                &self.format.before_cross_ref,
            ]
            .iter()
            .any(|s| s.len() > 4096)
        {
            return Err(invalid("index formatting exceeds supported levels or text length"));
        }
        Ok(())
    }

    /// Hierarchy order with siblings sorted by typed keys. Display text is untouched.
    pub fn ordered_topics(&self) -> Result<Vec<(u64, usize)>> {
        let mut children: BTreeMap<Option<u64>, Vec<_>> = BTreeMap::new();
        for t in self.topics.values() {
            children.entry(t.parent).or_default().push((self.sort_key(t)?, t.id));
        }
        for group in children.values_mut() {
            group.sort();
        }
        let mut pending: Vec<_> = children.get(&None).into_iter().flatten().rev().map(|(_, id)| (*id, 1usize)).collect();
        let mut out = Vec::new();
        let mut visited = BTreeSet::new();
        while let Some((id, level)) = pending.pop() {
            if !visited.insert(id) || level > 16 {
                return Err(invalid("index hierarchy exceeds bounds"));
            }
            out.push((id, level));
            pending.extend(children.get(&Some(id)).into_iter().flatten().rev().map(|(_, id)| (*id, level + 1)));
        }
        if out.len() != self.topics.len() {
            return Err(invalid("index contains disconnected topics"));
        }
        Ok(out)
    }

    pub fn identities(&self) -> impl Iterator<Item = u64> + '_ {
        self.topics
            .keys()
            .copied()
            .chain(self.occurrences.keys().copied())
            .chain(self.occurrences.values().filter_map(|o| o.end.as_ref().map(|e| e.id)))
            .chain(self.fields.values().copied())
    }

    pub fn targets(&self) -> impl Iterator<Item = (u64, &TextTarget)> {
        self.occurrences.values().flat_map(|o| std::iter::once((o.id, &o.start)).chain(o.end.iter().map(|e| (e.id, &e.target))))
    }
}

impl Publication {
    pub fn index_field_ids(&self) -> BTreeSet<u64> {
        self.indexes.values().flat_map(|i| i.fields.values().copied()).collect()
    }

    pub fn generated_destinations(&self) -> impl Iterator<Item = (&str, ComponentId, StoryId, ContentId)> {
        self.references
            .sections
            .values()
            .map(|g| (g.name.as_str(), g.component, g.story, g.content))
            .chain(self.indexes.values().filter_map(|i| i.destination.as_ref().map(|d| (i.name.as_str(), d.component, d.story, d.content))))
            .chain(self.note_outputs.values().map(|o| (o.stream.as_str(), o.component, o.story, o.content)))
    }
}

fn page_label(root: &Publication, format: &Formatting, location: &Location) -> String {
    match format.page_labels {
        PageLabels::Global => (location.global_page + 1).to_string(),
        PageLabels::Local => location.label.clone(),
        PageLabels::Component => format!("{}: {}", root.components.get(&location.component).map_or("?", |c| c.title.as_str()), location.label),
    }
}

#[cfg(test)]
mod tests;
