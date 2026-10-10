//! Publication review records. Change identities travel with character runs; comments bind
//! to persistent text anchors. A proof is an immutable snapshot, separate from live content.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Component, Edition, Publication, annotations::PathStep, invalid, numbering::TextTarget};
use crate::{ChangeMark, ComponentId, ContentId, Document, ItemId, Result, Story, StoryContent, StoryId};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Review {
    pub tracking: bool,
    /// User-entered attribution, never inferred from the machine account.
    pub author: String,
    pub changes: BTreeMap<u64, Change>,
    pub comments: BTreeMap<u64, Comment>,
    pub proofs: BTreeMap<u64, Arc<Proof>>,
    pub approvals: BTreeMap<u64, Approval>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: u64,
    pub author: String,
    pub revision: u64,
    pub kind: ChangeMark,
    pub status: ChangeStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub text: String,
    pub revision: u64,
    pub start: TextTarget,
    pub end: Option<TextTarget>,
    /// Keeps the comment interpretable when its anchored passage is removed.
    pub quote: String,
    pub resolved: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub author: String,
    pub revision: u64,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontInput {
    pub family: String,
    pub style: String,
    pub index: u32,
    pub coordinates: Vec<([u8; 4], f32)>,
    pub byte_length: usize,
    /// Deterministic byte fingerprint for detecting changed inputs, not a signature.
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofLine {
    pub start: usize,
    pub end: usize,
    pub paragraph: usize,
    pub baseline: f64,
    pub x: [f64; 2],
    pub glyphs: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofFrame {
    pub frame: ItemId,
    pub page: Option<usize>,
    pub range: [usize; 2],
    pub lines: Vec<ProofLine>,
    /// Nested table/note geometry and continuation state, captured in source order.
    pub nested: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofStory {
    pub component: ComponentId,
    pub content: ContentId,
    pub story: StoryId,
    pub frames: Vec<ProofFrame>,
    pub overset: Option<usize>,
    pub note_overflow: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofFigure {
    pub component: ComponentId,
    pub item: ItemId,
    pub content: Option<ContentId>,
    pub page: Option<usize>,
    pub bounds: [f64; 4],
    pub description: String,
    pub artwork: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proof {
    pub id: u64,
    pub name: String,
    pub author: String,
    pub revision: u64,
    pub title: String,
    pub metadata: BTreeMap<String, String>,
    pub values: BTreeMap<String, String>,
    pub order: Vec<ComponentId>,
    pub components: BTreeMap<ComponentId, Arc<Component>>,
    pub content: BTreeMap<ContentId, Arc<StoryContent>>,
    pub text_signatures: BTreeMap<ContentId, String>,
    pub formatting: BTreeMap<ContentId, String>,
    pub settings: BTreeMap<ComponentId, serde_json::Value>,
    pub edition: Arc<Edition>,
    pub semantic: BTreeMap<crate::SemanticId, super::numbering::SemanticObject>,
    pub indexes: BTreeMap<String, super::indexes::Definition>,
    pub stories: Vec<ProofStory>,
    pub figures: Vec<ProofFigure>,
    pub fonts: Vec<FontInput>,
    pub inputs: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRange {
    pub id: u64,
    pub component: ComponentId,
    pub content: ContentId,
    pub story: StoryId,
    pub path: Vec<PathStep>,
    pub start: usize,
    pub end: usize,
    pub kind: ChangeMark,
    pub text: String,
    pub author: String,
}

impl Review {
    pub fn author(&self) -> String {
        if self.author.trim().is_empty() { "Unattributed".into() } else { self.author.clone() }
    }

    pub fn identities(&self) -> impl Iterator<Item = u64> + '_ {
        self.changes.keys().chain(self.comments.keys()).chain(self.proofs.keys()).copied()
    }

    pub fn check(&self, root: &Publication) -> Result<()> {
        if self.author.len() > 256 || self.changes.len() > 100_000 || self.comments.len() > 100_000 || self.proofs.len() > 32 {
            return Err(invalid("review supports author names up to 256 bytes, 100000 changes/comments and 32 proof baselines"));
        }
        for (id, c) in &self.changes {
            if *id != c.id || c.author.len() > 256 || !matches!(c.kind, ChangeMark::Inserted | ChangeMark::Deleted) {
                return Err(invalid("invalid tracked-change record"));
            }
        }
        for (id, c) in &self.comments {
            if *id != c.id
                || c.author.len() > 256
                || c.text.len() > 64 * 1024
                || c.quote.len() > 64 * 1024
                || !root.components.contains_key(&c.start.component)
            {
                return Err(invalid("invalid anchored comment"));
            }
        }
        let mut names = BTreeSet::new();
        for (id, proof) in &self.proofs {
            if *id != proof.id
                || proof.name.trim().is_empty()
                || proof.name.len() > 256
                || proof.author.len() > 256
                || !names.insert(&proof.name)
                || proof.components.len() > super::MAX_COMPONENTS
                || proof.stories.len() > 100_000
                || proof.figures.len() > 100_000
                || proof.fonts.len() > 1024
            {
                return Err(invalid("invalid or oversized proof baseline"));
            }
            for content in proof.content.values() {
                let mut story = Story::new(StoryId(0));
                story.content = content.clone();
                story.check().map_err(invalid)?;
            }
            for story in &proof.stories {
                if story.frames.len() > 100_000 || !proof.content.contains_key(&story.content) {
                    return Err(invalid("invalid proof story"));
                }
                for frame in &story.frames {
                    if frame.lines.len() > 1_000_000 || frame.range[0] > frame.range[1] || frame.nested.len() > 1024 {
                        return Err(invalid("invalid proof composition frame"));
                    }
                    if frame
                        .lines
                        .iter()
                        .any(|l| l.start > l.end || !l.baseline.is_finite() || l.x.iter().any(|v| !v.is_finite()) || l.glyphs.len() > 128)
                    {
                        return Err(invalid("invalid proof composition line"));
                    }
                }
            }
            if proof.figures.iter().any(|f| f.bounds.iter().any(|v| !v.is_finite()) || f.artwork.len() > 128) {
                return Err(invalid("invalid proof figure"));
            }
        }
        if self.approvals.iter().any(|(id, a)| !self.proofs.contains_key(id) || a.author.len() > 256 || a.note.len() > 64 * 1024) {
            return Err(invalid("invalid proof approval"));
        }
        for range in root.change_ranges()? {
            if !self.changes.get(&range.id).is_some_and(|c| c.kind == range.kind && c.status == ChangeStatus::Pending) {
                return Err(invalid("tracked text has a missing or resolved change identity"));
            }
        }
        Ok(())
    }
}

/// Visit every editable text leaf with its stable component/content owner and nested address.
pub fn visit_text(story: &Story, path: &mut Vec<PathStep>, visit: &mut impl FnMut(&Story, &[PathStep]) -> Result<()>) -> Result<()> {
    if path.len() > 32 {
        return Err(invalid("review text nesting exceeds 32 levels"));
    }
    visit(story, path)?;
    for table in story.tables.values() {
        for row in 0..table.nrows() {
            for col in 0..table.ncols() {
                if table.owner(row, col) == (row, col)
                    && let Some(cell) = table.cell(row, col)
                {
                    path.push(PathStep::Cell { table: table.id, row, col });
                    visit_text(&cell.text, path, visit)?;
                    path.pop();
                }
            }
        }
    }
    for (end, note) in story.notes.iter().map(|n| (false, n)).chain(story.endnotes.iter().map(|n| (true, n))) {
        path.push(if end { PathStep::Endnote { id: note.id } } else { PathStep::Footnote { id: note.id } });
        visit_text(&note.text, path, visit)?;
        path.pop();
    }
    Ok(())
}

impl Publication {
    pub fn change_ranges(&self) -> Result<Vec<ChangeRange>> {
        let mut out = Vec::new();
        for cid in self.ordered_components()? {
            let component = self.components.get(&cid).ok_or_else(|| invalid("missing review component"))?;
            let legacy = self.editions.values().find_map(|e| e.components.get(&cid)).and_then(|l| l.pages.endnote_story);
            for (&sid, &content) in &component.stories {
                if legacy == Some(sid) || self.generated_destinations().any(|(_, c, s, _)| c == cid && s == sid) {
                    continue;
                }
                let mut story = Story::new(sid);
                story.content = self.content.get(&content).ok_or_else(|| invalid("missing review content"))?.clone();
                visit_text(&story, &mut Vec::new(), &mut |leaf, path| {
                    for (range, format) in leaf.runs() {
                        if let Some(kind @ (ChangeMark::Inserted | ChangeMark::Deleted)) = format.over.change {
                            let id = format.over.change_id.unwrap_or(0);
                            out.push(ChangeRange {
                                id,
                                component: cid,
                                content,
                                story: sid,
                                path: path.into(),
                                start: range.start,
                                end: range.end,
                                kind,
                                text: leaf.text.get(range).unwrap_or_default().into(),
                                author: self.review.changes.get(&id).map(|c| c.author.clone()).unwrap_or_else(|| "Unattributed".into()),
                            });
                            if out.len() > 100_000 {
                                return Err(invalid("review exceeds 100000 tracked ranges"));
                            }
                        }
                    }
                    Ok(())
                })?;
            }
        }
        Ok(out)
    }

    pub(super) fn normalize_changes(&mut self, importing: bool, doc: &mut Document) -> Result<()> {
        let author = self.review.author();
        let revision = self.revision.saturating_add(1);
        let mut error = None;
        for (sid, story) in &mut doc.stories {
            if doc.endnote_story == Some(*sid) {
                continue;
            }
            Arc::make_mut(story).for_each_text_mut(&mut |leaf| {
                if error.is_some() {
                    return;
                }
                let ranges: Vec<_> = leaf.runs().map(|(r, f)| (r, f.over.change, f.over.change_id)).collect();
                let mut previous = None;
                for (range, mark, id) in ranges {
                    if range.is_empty() {
                        continue;
                    }
                    let Some(kind @ (ChangeMark::Inserted | ChangeMark::Deleted)) = mark else {
                        if id.is_some() {
                            leaf.format_chars(range, |f| f.over.change_id = None);
                        }
                        previous = None;
                        continue;
                    };
                    if !importing
                        && id.and_then(|id| self.review.changes.get(&id)).is_some_and(|c| c.kind == kind && c.status == ChangeStatus::Pending)
                    {
                        previous = None;
                        continue;
                    }
                    let allocated = if let Some((_, id)) = previous.filter(|(k, _)| *k == kind) { Ok(id) } else { self.alloc() };
                    match allocated {
                        Ok(id) => {
                            self.review.changes.entry(id).or_insert_with(|| Change {
                                id,
                                author: author.clone(),
                                revision,
                                kind,
                                status: ChangeStatus::Pending,
                            });
                            leaf.format_chars(range, |f| f.over.change_id = Some(id));
                            previous = Some((kind, id));
                        }
                        Err(e) => {
                            error = Some(e);
                            break;
                        }
                    }
                }
            });
        }
        if let Some(e) = error {
            return Err(e);
        }
        Ok(())
    }

    pub fn review_target(&self, target: &TextTarget) -> Result<Option<(StoryId, Vec<PathStep>, usize)>> {
        let Some(component) = self.components.get(&target.component) else { return Ok(None) };
        let Some((&sid, _)) = component.stories.iter().find(|(_, c)| **c == target.content) else { return Ok(None) };
        let Some(content) = self.content.get(&target.content) else { return Ok(None) };
        let mut story = Story::new(sid);
        story.content = content.clone();
        let mut found = None;
        visit_text(&story, &mut Vec::new(), &mut |leaf, path| {
            if let Some(pos) = leaf.anchor_pos(target.anchor) {
                if found.is_some() {
                    return Err(invalid("ambiguous review anchor"));
                }
                found = Some((sid, path.to_vec(), pos));
            }
            Ok(())
        })?;
        Ok(found)
    }

    /// A change may have several formatting runs; navigation selects its first contiguous
    /// passage. The review listing exposes every fragment for explicit inspection.
    pub fn review_address(&self, id: u64) -> Result<Option<(ComponentId, StoryId, Vec<PathStep>, usize, usize)>> {
        if let Some(c) = self.review.comments.get(&id) {
            let Some((sid, path, at)) = self.review_target(&c.start)? else { return Ok(None) };
            let end = c.end.as_ref().map(|t| self.review_target(t)).transpose()?.flatten();
            if c.end.is_some() && end.is_none() {
                return Ok(None);
            }
            let start = at.saturating_add(crate::ANCHOR_MARK.len_utf8());
            let end = end.filter(|(s, p, _)| *s == sid && *p == path).map(|(_, _, p)| p).unwrap_or(start);
            if c.end.is_some() && !c.quote.is_empty() && end <= start {
                return Ok(None);
            }
            return Ok(Some((c.start.component, sid, path, start, end.max(start))));
        }
        Ok(self.change_ranges()?.into_iter().find(|r| r.id == id).map(|r| (r.component, r.story, r.path, r.start, r.end)))
    }

    /// Early version-3 archives predate review identities. Only an absent review section
    /// permits this migration; present but corrupt identities must fail validation.
    pub fn migrate_legacy_review(&mut self) -> Result<()> {
        let Some(edition) = self.editions.keys().next().copied() else {
            return Err(invalid("missing edition"));
        };
        for component in self.ordered_components()? {
            let mut doc = self.project(edition, component)?;
            self.review.tracking |= doc.settings.track_changes;
            self.normalize_changes(false, &mut doc)?;
            self.capture(edition, component, &doc)?;
        }
        Ok(())
    }
}
