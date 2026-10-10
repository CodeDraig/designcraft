//! Publication-wide named numbering over stable semantic identities and moving text anchors.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{ComponentRole, Publication, invalid};
use crate::{ComponentId, ContentId, NumberStyle, Result, SemanticId, Story, StoryId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    #[default]
    Chapter,
    Heading,
    Figure,
    Table,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextTarget {
    pub component: ComponentId,
    pub content: ContentId,
    pub anchor: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticObject {
    pub id: SemanticId,
    pub kind: ObjectKind,
    pub target: TextTarget,
    pub level: u8,
    /// None follows the anchored paragraph as the author edits it.
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Restart {
    #[default]
    Continuous,
    Component,
    Parent,
    Role,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct NumberingSequence {
    pub name: String,
    pub kind: ObjectKind,
    pub style: NumberStyle,
    pub start: u32,
    pub prefix: String,
    pub suffix: String,
    pub restart: Restart,
    /// A component subtree; None covers the whole publication.
    pub scope: Option<ComponentId>,
    pub role: Option<ComponentRole>,
    pub level: Option<u8>,
    /// An explicit restart at a component's first matching object.
    pub restart_at: BTreeMap<ComponentId, u32>,
}

impl Default for NumberingSequence {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ObjectKind::Chapter,
            style: NumberStyle::Arabic,
            start: 1,
            prefix: String::new(),
            suffix: String::new(),
            restart: Restart::Continuous,
            scope: None,
            role: None,
            level: None,
            restart_at: BTreeMap::new(),
        }
    }
}

impl NumberingSequence {
    pub fn check(&self, root: &Publication) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 256 || self.prefix.len() > 1024 || self.suffix.len() > 1024 {
            return Err(invalid("numbering needs a name (up to 256 bytes) and prefix/suffix up to 1024 bytes"));
        }
        if self.start == 0 || self.start > 100_000 || self.restart_at.values().any(|n| *n == 0 || *n > 100_000) {
            return Err(invalid("numbering starts must be between 1 and 100000"));
        }
        if self.scope.is_some_and(|id| !root.components.contains_key(&id)) || self.restart_at.keys().any(|id| !root.components.contains_key(id)) {
            return Err(invalid("numbering refers to an unknown component"));
        }
        if self.level.is_some_and(|n| n == 0 || n > 9) {
            return Err(invalid("heading level must be 1–9"));
        }
        Ok(())
    }

    fn label(&self, n: u32) -> Result<String> {
        // Repeating-letter/symbol formats otherwise allocate in proportion to arbitrary input.
        if n > 100_000
            || matches!(self.style, NumberStyle::Symbols) && n > 600
            || matches!(self.style, NumberStyle::UpperLetters | NumberStyle::LowerLetters) && n > 2600
        {
            return Err(invalid(format!(
                "sequence {} exceeds the supported range for {:?}; choose a more compact numeral style",
                self.name, self.style
            )));
        }
        Ok(format!("{}{}{}", self.prefix, self.style.format(n), self.suffix))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberedObject {
    pub component: ComponentId,
    pub object: Option<SemanticId>,
    pub title: String,
    pub number: Option<u32>,
    pub label: String,
    pub resolved: bool,
}

#[derive(Clone, Debug)]
pub struct TargetText {
    pub story: StoryId,
    pub path: Vec<usize>,
    pub title: String,
}

impl Publication {
    pub fn target_text(&self, target: &TextTarget) -> Option<TargetText> {
        let component = self.components.get(&target.component)?;
        let (&sid, _) = component.stories.iter().find(|(_, id)| **id == target.content)?;
        let mut top = Story::new(sid);
        top.content = self.content.get(&target.content)?.clone();
        let mut pending = vec![(&top, Vec::new())];
        while let Some((story, path)) = pending.pop() {
            if let Some(pos) = story.anchor_pos(target.anchor) {
                let range = story.para_ranges().get(story.para_at(pos))?.clone();
                let mut path = path;
                path.push(pos);
                return Some(TargetText { story: sid, path, title: crate::xref::clean(story.text.get(range)?) });
            }
            for (id, table) in &story.tables {
                let at = story.table_anchor(*id)?;
                for (index, cell) in table.cells.iter().enumerate() {
                    let mut child = path.clone();
                    child.extend([at, index]);
                    pending.push((&cell.text, child));
                }
            }
            for (marks, notes) in [(crate::FOOTNOTE_REF, &story.notes), (crate::endnotes::ENDNOTE_REF, &story.endnotes)] {
                for ((at, _), note) in story.text.match_indices(marks).zip(notes) {
                    let mut child = path.clone();
                    child.push(at);
                    pending.push((&note.text, child));
                }
            }
        }
        None
    }

    pub fn numbers(&self, name: &str) -> Result<Vec<NumberedObject>> {
        let sequence = self.numbering.get(name).ok_or_else(|| invalid(format!("unknown numbering sequence {name}")))?;
        sequence.check(self)?;
        let order = self.ordered_components()?;
        let mut allowed: BTreeSet<_> = order.iter().copied().collect();
        if let Some(scope) = sequence.scope {
            allowed.clear();
            let mut pending = vec![scope];
            while let Some(id) = pending.pop() {
                if !allowed.insert(id) {
                    return Err(invalid("cyclic numbering scope"));
                }
                pending.extend(self.components.get(&id).ok_or_else(|| invalid("unknown scope"))?.children.iter().copied());
            }
        }
        let mut out = Vec::new();
        let mut counters: BTreeMap<u64, u32> = BTreeMap::new();
        for component in order.into_iter().filter(|id| allowed.contains(id)) {
            let c = self.components.get(&component).ok_or_else(|| invalid("unknown component"))?;
            if sequence.role.is_some_and(|role| role != c.role) {
                continue;
            }
            let group = match sequence.restart {
                Restart::Continuous => 0,
                Restart::Component => component.0,
                Restart::Parent => self.components.values().find(|c| c.children.contains(&component)).map_or(0, |c| c.id.0),
                Restart::Role => match c.role {
                    ComponentRole::FrontMatter => 0,
                    ComponentRole::Chapter => 1,
                    ComponentRole::BackMatter => 2,
                },
            };
            let mut hits = Vec::new();
            if sequence.kind == ObjectKind::Chapter {
                hits.push((0, Vec::new(), None, c.title.clone(), true));
            } else {
                for object in self
                    .semantic
                    .values()
                    .filter(|o| o.target.component == component && o.kind == sequence.kind && sequence.level.is_none_or(|l| l == o.level))
                {
                    let target = self.target_text(&object.target);
                    let story_order = target.as_ref().and_then(|t| c.story_order.iter().position(|id| *id == t.story)).unwrap_or(usize::MAX);
                    let title =
                        object.title.clone().or_else(|| target.as_ref().map(|t| t.title.clone())).unwrap_or_else(|| "Unresolved passage".into());
                    hits.push((story_order, target.as_ref().map(|t| t.path.clone()).unwrap_or_default(), Some(object.id), title, target.is_some()));
                }
                hits.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
            }
            let mut first = true;
            for (_, _, object, title, resolved) in hits {
                let (number, label) = if resolved {
                    let next = counters.entry(group).or_insert(sequence.start);
                    if first && let Some(restart) = sequence.restart_at.get(&component) {
                        *next = *restart;
                    }
                    first = false;
                    let n = *next;
                    *next = next.checked_add(1).ok_or_else(|| invalid("numbering overflow"))?;
                    (Some(n), sequence.label(n)?)
                } else {
                    (None, String::new())
                };
                out.push(NumberedObject { component, object, title, number, label, resolved });
            }
        }
        Ok(out)
    }
}
