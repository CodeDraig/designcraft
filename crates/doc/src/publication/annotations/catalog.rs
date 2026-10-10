use super::*;
use crate::{Document, ENDNOTE_REF, FOOTNOTE_REF, Footnote, TABLE_ANCHOR};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PathStep {
    Cell { table: u64, row: usize, col: usize },
    Footnote { id: u64 },
    Endnote { id: u64 },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub identity: Identity,
    pub component: ComponentId,
    pub content: ContentId,
    pub story: StoryId,
    pub path: Vec<PathStep>,
    pub at: usize,
    pub endnote: bool,
    pub note: Arc<Footnote>,
}

impl Publication {
    pub fn annotation_entries(&self) -> Result<Vec<Entry>> {
        let mut out = Vec::new();
        for id in self.ordered_components()? {
            let Some(c) = self.components.get(&id) else { continue };
            let legacy = self.editions.values().find_map(|e| e.components.get(&id)).and_then(|l| l.pages.endnote_story);
            for sid in c.story_order.iter().chain(c.stories.keys().filter(|s| !c.story_order.contains(s))) {
                if legacy == Some(*sid) || self.note_output(id, *sid).is_some() {
                    continue;
                }
                let Some(key) = c.stories.get(sid) else { continue };
                let Some(content) = self.content.get(key) else { continue };
                let context = Entry {
                    identity: Default::default(),
                    component: id,
                    content: *key,
                    story: *sid,
                    path: Vec::new(),
                    at: 0,
                    endnote: false,
                    note: Arc::new(Footnote { id: 0, text: Story::new(StoryId(0)), annotation: None, apparatus: None }),
                };
                collect(content, &context, &mut out, 0)?;
            }
        }
        Ok(out)
    }

    /// Assign identities once on entry into canonical content. Copies acquire new identities;
    /// a cut whose original no longer exists keeps its identity when inserted elsewhere.
    pub(in super::super) fn normalize_annotations(&mut self, component: Option<ComponentId>, doc: &mut Document) -> Result<()> {
        let mut seen: BTreeSet<_> = self
            .annotation_entries()?
            .into_iter()
            .filter(|e| Some(e.component) != component)
            .flat_map(|e| [e.identity.id, e.identity.callout])
            .collect();
        for (&sid, story) in &mut doc.stories {
            if doc.endnote_story == Some(sid) || component.is_some_and(|c| self.note_output(c, sid).is_some()) {
                continue;
            }
            let mut error = None;
            Arc::make_mut(story).for_each_text_mut(&mut |s| {
                let s = Arc::make_mut(&mut s.content);
                for (end, note) in s.notes.iter_mut().map(|n| (false, n)).chain(s.endnotes.iter_mut().map(|n| (true, n))) {
                    if error.is_some() {
                        break;
                    }
                    let n = Arc::make_mut(note);
                    let stream = n.annotation.as_ref().map(|i| i.stream.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| {
                        if n.apparatus.is_some() {
                            "Apparatus"
                        } else if end {
                            "Endnotes"
                        } else {
                            "Footnotes"
                        }
                        .into()
                    });
                    self.note_streams.entry(stream.clone()).or_insert_with(|| {
                        if end {
                            Stream {
                                name: stream.clone(),
                                heading: if n.apparatus.is_some() { stream.clone() } else { doc.endnote_options.heading.clone() },
                                heading_style: doc.endnote_options.heading_style.clone(),
                                preserve_paragraph_styles: true,
                                restart: Restart::Component,
                                style: doc.endnote_options.style,
                                start_at: doc.endnote_options.start_at,
                                prefix: doc.endnote_options.prefix.clone(),
                                suffix: doc.endnote_options.suffix.clone(),
                                para_style: doc.endnote_options.para_style.clone(),
                                separator: doc.endnote_options.separator.clone(),
                                default_placement: Placement::ComponentEnd,
                                ..Default::default()
                            }
                        } else {
                            Stream {
                                name: stream.clone(),
                                heading: stream.clone(),
                                preserve_paragraph_styles: true,
                                style: doc.footnote_options.style,
                                start_at: doc.footnote_options.start_at,
                                restart: match doc.footnote_options.restart {
                                    crate::notes::NoteRestart::Never => Restart::Component,
                                    crate::notes::NoteRestart::Page => Restart::Page,
                                    crate::notes::NoteRestart::Spread => Restart::Spread,
                                    crate::notes::NoteRestart::Section => Restart::Section,
                                },
                                affix_in: doc.footnote_options.affix_in,
                                prefix: doc.footnote_options.prefix.clone(),
                                suffix: doc.footnote_options.suffix.clone(),
                                para_style: doc.footnote_options.para_style.clone(),
                                ref_char_style: doc.footnote_options.ref_char_style.clone(),
                                ref_position: doc.footnote_options.ref_position,
                                separator: doc.footnote_options.separator.clone(),
                                ..Default::default()
                            }
                        }
                    });
                    let valid = component.is_some()
                        && n.annotation.as_ref().is_some_and(|a| {
                            a.id > 0
                                && a.callout > 0
                                && a.id <= self.next_id
                                && a.callout <= self.next_id
                                && a.id != a.callout
                                && !seen.contains(&a.id)
                                && !seen.contains(&a.callout)
                        });
                    if !valid {
                        match self.alloc().and_then(|id| self.alloc().map(|callout| Identity { id, callout, stream: stream.clone() })) {
                            Ok(a) => n.annotation = Some(a),
                            Err(e) => {
                                error = Some(e);
                                break;
                            }
                        }
                    }
                    if let Some(a) = &n.annotation {
                        seen.extend([a.id, a.callout]);
                    }
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
        }
        Ok(())
    }
}

fn collect(s: &crate::StoryContent, base: &Entry, out: &mut Vec<Entry>, depth: usize) -> Result<()> {
    if depth > 32 || out.len() > 100_000 {
        return Err(invalid("annotations exceed 32 nesting levels or 100000 notes"));
    }
    let (mut foot, mut end) = (0usize, 0usize);
    for (at, ch) in s.text.char_indices() {
        match ch {
            FOOTNOTE_REF | ENDNOTE_REF => {
                let is_end = ch == ENDNOTE_REF;
                let (notes, k) = if is_end { (&s.endnotes, &mut end) } else { (&s.notes, &mut foot) };
                if let Some(note) = notes.get(*k) {
                    let mut e = Entry { at, endnote: is_end, note: note.clone(), ..base.clone() };
                    if let Some(identity) = &note.annotation {
                        e.identity = identity.clone();
                        out.push(e.clone());
                    }
                    e.path.push(if is_end { PathStep::Endnote { id: note.id } } else { PathStep::Footnote { id: note.id } });
                    collect(&note.text.content, &e, out, depth + 1)?;
                }
                *k += 1;
            }
            TABLE_ANCHOR => {
                let para = s.text[..at].matches('\n').count();
                if let Some(table) = s.paras.get(para).and_then(|p| p.table).and_then(|id| s.tables.get(&id)) {
                    for row in 0..table.nrows() {
                        for col in 0..table.ncols() {
                            if let Some(cell) = table.cell(row, col) {
                                if table.owner(row, col) != (row, col) {
                                    continue;
                                }
                                let mut e = base.clone();
                                e.path.push(PathStep::Cell { table: table.id, row, col });
                                collect(&cell.text.content, &e, out, depth + 1)?;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn at_path<'a>(story: &'a Story, path: &[PathStep]) -> Option<&'a Story> {
    let mut s = story;
    for step in path {
        s = match step {
            PathStep::Cell { table, row, col } => &s.tables.get(table)?.cell(*row, *col)?.text,
            PathStep::Footnote { id } => &s.note(*id)?.text,
            PathStep::Endnote { id } => &s.endnote(*id)?.text,
        };
    }
    Some(s)
}
pub fn at_path_mut<'a>(story: &'a mut Story, path: &[PathStep]) -> Option<&'a mut Story> {
    let mut s = story;
    for step in path {
        s = match step {
            PathStep::Cell { table, row, col } => &mut Arc::make_mut(s.tables.get_mut(table)?).cell_mut(*row, *col)?.text,
            PathStep::Footnote { id } => &mut s.note_mut(*id)?.text,
            PathStep::Endnote { id } => &mut s.endnote_mut(*id)?.text,
        };
    }
    Some(s)
}
