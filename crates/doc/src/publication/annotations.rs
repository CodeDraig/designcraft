//! A note's canonical rich content stays with its callout. Identity and stream membership do
//! not encode where an edition prints it; empty output stories carry generated endnote areas.
use super::{Publication, invalid};
use crate::{ComponentId, ContentId, NumberStyle, Position, Result, Story, StoryId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

mod catalog;
pub use catalog::{Entry, PathStep, at_path, at_path_mut};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: u64,
    pub callout: u64,
    pub stream: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Restart {
    #[default]
    Publication,
    Component,
    Page,
    Spread,
    Section,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Placement {
    #[default]
    Footnote,
    ComponentEnd,
    PublicationEnd,
    Sidenote,
    Omit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Stream {
    pub name: String,
    pub style: NumberStyle,
    pub start_at: u32,
    pub restart: Restart,
    pub prefix: String,
    pub suffix: String,
    pub affix_in: crate::notes::AffixIn,
    pub ref_position: Position,
    pub ref_char_style: String,
    pub para_style: String,
    pub preserve_paragraph_styles: bool,
    pub heading: String,
    pub heading_style: String,
    pub separator: String,
    pub default_placement: Placement,
}
impl Default for Stream {
    fn default() -> Self {
        Self {
            name: "Notes".into(),
            style: NumberStyle::Arabic,
            start_at: 1,
            restart: Restart::Publication,
            prefix: String::new(),
            suffix: String::new(),
            ref_position: Position::Superscript,
            affix_in: crate::notes::AffixIn::Both,
            ref_char_style: crate::NO_CHAR_STYLE.into(),
            para_style: crate::BASIC_PARAGRAPH.into(),
            preserve_paragraph_styles: false,
            heading: "Notes".into(),
            heading_style: crate::BASIC_PARAGRAPH.into(),
            separator: "  ".into(),
            default_placement: Placement::Footnote,
        }
    }
}
impl Stream {
    pub fn number(&self, n: u32) -> Result<String> {
        if n > 100_000
            || matches!(self.style, NumberStyle::Symbols) && n > 600
            || matches!(self.style, NumberStyle::UpperLetters | NumberStyle::LowerLetters) && n > 2600
        {
            return Err(invalid(format!("note stream {} exceeds the supported numeral range; choose a compact numbering style", self.name)));
        }
        Ok(self.style.format(n))
    }
    pub fn decorate(&self, number: &str, text: bool) -> String {
        use crate::notes::AffixIn;
        if matches!((self.affix_in, text), (AffixIn::Both, _) | (AffixIn::Text, true) | (AffixIn::Reference, false)) {
            format!("{}{number}{}", self.prefix, self.suffix)
        } else {
            number.into()
        }
    }
    pub fn check(&self) -> Result<()> {
        self.number(self.start_at)?;
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.start_at == 0
            || self.start_at > 100_000
            || [&self.prefix, &self.suffix, &self.separator, &self.heading].iter().any(|s| s.len() > 4096)
        {
            return Err(invalid("note stream needs a name of at most 256 bytes, numbering 1–100000, and labels of at most 4096 bytes"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Policy {
    pub placement: Placement,
    pub columns: u32,
    pub gutter: f64,
    pub run_in: bool,
    pub between_notes: String,
    pub max_fraction: f64,
    pub space_before: f64,
    pub space_between: f64,
    pub side_width: f64,
    pub side_gap: f64,
    pub side: Side,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Left,
    Right,
    #[default]
    Outer,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            placement: Placement::Footnote,
            columns: 1,
            gutter: 12.0,
            run_in: false,
            between_notes: "   ".into(),
            max_fraction: 0.45,
            space_before: 8.0,
            space_between: 3.0,
            side_width: 72.0,
            side_gap: 12.0,
            side: Side::Outer,
        }
    }
}
impl Policy {
    pub fn check(&self) -> Result<()> {
        if !(1..=12).contains(&self.columns)
            || !(0.05..=0.9).contains(&self.max_fraction)
            || [self.gutter, self.space_before, self.space_between, self.side_gap].iter().any(|n| !n.is_finite() || !(0.0..=1000.0).contains(n))
            || !self.side_width.is_finite()
            || !(12.0..=2000.0).contains(&self.side_width)
            || self.between_notes.len() > 4096
        {
            return Err(invalid("note area requires 1–12 columns, a 0.05–0.9 page fraction, finite spacing 0–1000 pt and sidenote width 12–2000 pt"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub stream: String,
    /// A component's endnotes, or every component when None.
    pub source: Option<ComponentId>,
    pub component: ComponentId,
    pub story: StoryId,
    pub content: ContentId,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub labels: BTreeMap<u64, String>,
    #[serde(default)]
    pub streams: BTreeMap<u64, String>,
    pub diagnostics: Vec<String>,
}

/// Source apparatus remains structured even when its selected main-text passage is edited.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Apparatus {
    pub source_key: String,
    #[serde(default)]
    pub source: u64,
    pub start: u64,
    pub end: u64,
    pub imported_reading: usize,
    pub imported_text: String,
    pub readings: Vec<Reading>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub lemma: bool,
    pub witnesses: Vec<String>,
    pub witness_labels: Vec<String>,
    pub text: Story,
}

impl Apparatus {
    pub fn check(&self) -> std::result::Result<(), String> {
        if self.readings.is_empty()
            || self.readings.len() > 2048
            || self.imported_reading >= self.readings.len()
            || self.source_key.len() > 4096
            || self.imported_text.len() > 16 * 1024 * 1024
        {
            return Err("apparatus requires 1–2048 readings, a valid imported reading, and bounded source labels/text".into());
        }
        let mut bytes = 0usize;
        for reading in &self.readings {
            bytes = bytes.saturating_add(reading.text.len());
            if bytes > 64 * 1024 * 1024
                || reading.witnesses.len() > 4096
                || reading.witnesses.iter().chain(&reading.witness_labels).any(|s| s.len() > 4096)
            {
                return Err("apparatus exceeds 64 MiB or witness label limits".into());
            }
            reading.text.check()?;
        }
        Ok(())
    }
    pub fn choose(&self, witness: &str) -> Option<usize> {
        self.readings
            .iter()
            .position(|r| r.witnesses.iter().any(|w| w.trim_start_matches('#') == witness.trim_start_matches('#')))
            .or_else(|| self.readings.iter().position(|r| r.lemma))
            .or_else(|| (!self.readings.is_empty()).then_some(0))
    }
    pub fn range(&self, story: &Story) -> Option<std::ops::Range<usize>> {
        let start = story.anchor_pos(self.start)?.checked_add(crate::ANCHOR_MARK.len_utf8())?;
        let end = story.anchor_pos(self.end)?;
        (start <= end).then_some(start..end)
    }
    pub fn edited(&self, story: &Story) -> bool {
        self.range(story).and_then(|r| story.text.get(r)).is_none_or(|s| s != self.imported_text)
    }
}

impl Publication {
    pub fn note_policy(&self, edition: crate::EditionId, stream: &str) -> Policy {
        self.editions.get(&edition).and_then(|e| e.note_policies.get(stream)).cloned().unwrap_or_else(|| Policy {
            placement: self.note_streams.get(stream).map_or(Placement::Footnote, |s| s.default_placement),
            ..Default::default()
        })
    }
    pub fn clear_legacy_endnote_output(&mut self, component: ComponentId) {
        for edition in self.editions.values_mut() {
            if let Some(layout) = Arc::make_mut(edition).components.get_mut(&component) {
                Arc::make_mut(layout).pages.endnote_story = None;
            }
        }
    }
    pub fn note_output(&self, component: ComponentId, story: StoryId) -> Option<&Output> {
        self.note_outputs.values().find(|o| o.component == component && o.story == story)
    }
    pub fn note_label(&self, edition: crate::EditionId, id: u64) -> Option<String> {
        self.annotation_label(edition, id, false)
    }
    pub fn note_text_label(&self, edition: crate::EditionId, id: u64) -> Option<String> {
        self.annotation_label(edition, id, true)
    }
    fn annotation_label(&self, edition: crate::EditionId, id: u64, text: bool) -> Option<String> {
        if let Some(layout) = self.editions.get(&edition).map(|e| &e.notes)
            && let (Some(number), Some(stream)) = (layout.labels.get(&id), layout.streams.get(&id).and_then(|s| self.note_streams.get(s)))
        {
            return Some(stream.decorate(number, text));
        }
        let entries = self.annotation_entries().ok()?;
        let entry = entries.iter().find(|e| e.identity.id == id)?;
        let stream = self.note_streams.get(&entry.identity.stream)?;
        self.editions.get(&edition).and_then(|e| e.notes.labels.get(&id)).map(|n| stream.decorate(n, text)).or_else(|| {
            let mut counters = BTreeMap::new();
            for entry in self.annotation_entries().ok()? {
                let stream = self.note_streams.get(&entry.identity.stream)?;
                let component = (stream.restart != Restart::Publication).then_some(entry.component);
                let n = counters.entry((stream.name.clone(), component)).or_insert(stream.start_at);
                if entry.identity.id == id {
                    return Some(stream.decorate(&stream.number(*n).ok()?, text));
                }
                *n = n.saturating_add(1);
            }
            None
        })
    }
    pub(super) fn check_annotations(&self) -> Result<()> {
        if self.note_streams.len() > 1024 {
            return Err(invalid("at most 1024 note streams are supported"));
        }
        for (name, s) in &self.note_streams {
            s.check()?;
            if name != &s.name {
                return Err(invalid("note stream name mismatch"));
            }
        }
        for e in self.editions.values() {
            for (name, p) in &e.note_policies {
                p.check()?;
                if !self.note_streams.contains_key(name) {
                    return Err(invalid("note policy refers to missing stream"));
                }
            }
        }
        for output in self.note_outputs.values() {
            if !self.note_streams.contains_key(&output.stream)
                || output.source.is_some_and(|id| !self.components.contains_key(&id))
                || self.components.get(&output.component).and_then(|c| c.stories.get(&output.story)) != Some(&output.content)
                || self.content.get(&output.content).is_none_or(|c| !c.text.is_empty())
            {
                return Err(invalid("invalid generated note output"));
            }
        }
        for entry in self.annotation_entries()? {
            if !self.note_streams.contains_key(&entry.identity.stream) {
                return Err(invalid("note refers to missing stream"));
            }
        }
        Ok(())
    }
}
