//! Shared review operations. A batch resolves on a candidate publication, then commits once.
use std::collections::BTreeSet;
use std::sync::Arc;

use designcraft_doc::{
    ChangeMark, Document, Selection, Story, StoryId, TextSel,
    publication::{
        annotations::{PathStep, at_path_mut},
        numbering::TextTarget,
        review::{ChangeStatus, Comment},
    },
};
use serde_json::{Value, json};

use super::{CommandSpec, Result, Session, bad, cmd, has_publication, mutate, optional_id, str_param, view};

pub(crate) mod manuscript;
mod proofs;

pub(super) fn specs() -> Vec<CommandSpec> {
    let mut specs = vec![
        cmd!(query "publication.review.list", "Publication Review", [], None, "{} → tracking, author, pending ranges, comments and proof summaries; no layout", has_publication, list),
        cmd!(
            "publication.review.resolve",
            "Resolve Publication Changes",
            [],
            None,
            "{ids:[change], accept:bool} — accept/reject across all components as one undo step",
            has_publication,
            |s, p| {
                let ids: Vec<u64> = serde_json::from_value(p.get("ids").cloned().ok_or_else(|| bad("publication.review.resolve", "ids required"))?)
                    .map_err(|e| bad("publication.review.resolve", e.to_string()))?;
                let accept = p["accept"].as_bool().ok_or_else(|| bad("publication.review.resolve", "accept must be true or false"))?;
                resolve(s, ids, accept)
            }
        ),
        cmd!(
            "publication.comment.add",
            "Add Anchored Comment",
            [],
            None,
            "{text, author?, story?, start?, end?, path?:[cell|footnote|endnote]} — defaults to selection; retains quote when passage is removed",
            has_publication,
            comment
        ),
        cmd!(
            "publication.comment.edit",
            "Edit Publication Comment",
            [],
            None,
            "{id, text?, resolved?:bool} — retains original attribution and anchors",
            has_publication,
            |s, p| {
                let id = required(p, "id")?;
                mutate(s, |root, _| {
                    let c = root.review.comments.get_mut(&id).ok_or_else(|| bad("publication.comment.edit", "unknown comment"))?;
                    if let Some(text) = str_param(p, "text") {
                        c.text = text.into();
                    }
                    if let Some(resolved) = p.get("resolved") {
                        c.resolved = resolved.as_bool().ok_or_else(|| bad("publication.comment.edit", "resolved must be a boolean"))?;
                    }
                    root.revision = root.revision.saturating_add(1);
                    Ok(json!({"id":id}))
                })
            }
        ),
        cmd!(noundo "publication.review.goTo", "Go to Review Passage", [], None, "{id:change|comment} — select anchored text, including nested cells/notes; returns component, story, path and page", has_publication, go_to),
        cmd!(
            "publication.review.replace",
            "Edit Review Text",
            [],
            None,
            "{id:change|comment,start,end,text} — edit byte range in the review record's text leaf with normal tracking",
            has_publication,
            |s, p| {
                let id = required(p, "id")?;
                let v = view(s)?;
                let (component, story, path, _, _) =
                    v.root.review_address(id)?.ok_or_else(|| bad("publication.review.replace", "review passage no longer resolves"))?;
                let start = offset(p, "start")?.ok_or_else(|| bad("publication.review.replace", "start required"))?;
                let end = offset(p, "end")?.ok_or_else(|| bad("publication.review.replace", "end required"))?;
                let text = str_param(p, "text").ok_or_else(|| bad("publication.review.replace", "text required"))?;
                mutate(s, |root, v| {
                    let mut d = root.project(v.edition, component)?;
                    let leaf = d
                        .story_mut(story)
                        .and_then(|s| at_path_mut(s, &path))
                        .ok_or_else(|| bad("publication.review.replace", "text path no longer resolves"))?;
                    if start > end || end > leaf.len() || !leaf.text.is_char_boundary(start) || !leaf.text.is_char_boundary(end) {
                        return Err(bad("publication.review.replace", "range must use ordered UTF-8 boundaries"));
                    }
                    let pos = super::super::changes::replace(leaf, start..end, text, root.review.tracking);
                    root.capture(v.edition, component, &d)?;
                    Ok(json!({"position":pos}))
                })
            }
        ),
    ];
    specs.extend(proofs::specs());
    specs.extend(manuscript::specs());
    specs
}

fn required(p: &Value, key: &str) -> Result<u64> {
    optional_id(p, key)?.ok_or_else(|| bad("publication.review", format!("{key} required")))
}

pub(crate) fn configure(s: &mut Session, p: &Value) -> Result<Value> {
    mutate(s, |root, _| {
        let on = match p.get("on") {
            Some(v) => v.as_bool().ok_or_else(|| bad("changes.track", "on must be boolean"))?,
            None => !root.review.tracking,
        };
        if let Some(v) = p.get("author") {
            let author =
                v.as_str().filter(|s| !s.trim().is_empty() && s.len() <= 256).ok_or_else(|| bad("changes.track", "author must be 1–256 bytes"))?;
            root.review.author = author.into();
        }
        root.review.tracking = on;
        root.revision = root.revision.saturating_add(1);
        Ok(json!({"on":on,"author":root.review.author()}))
    })
}

pub(crate) fn resolve_one(s: &mut Session, p: &Value, accept: bool) -> Result<Value> {
    let v = view(s)?;
    let id = match optional_id(p, "id")? {
        Some(id) => id,
        None => {
            let sid = StoryId(required(p, "story")?);
            let start = offset(p, "start")?.ok_or_else(|| bad("changes.accept", "start required"))?;
            let path: Vec<PathStep> =
                serde_json::from_value(p.get("path").cloned().unwrap_or(json!([]))).map_err(|e| bad("changes.accept", e.to_string()))?;
            v.root
                .change_ranges()?
                .into_iter()
                .find(|r| r.component == v.component && r.story == sid && r.path == path && r.start <= start && start < r.end)
                .map(|r| r.id)
                .ok_or_else(|| bad("changes.accept", "no pending change at that position"))?
        }
    };
    resolve(s, vec![id], accept)
}

pub(crate) fn resolve_all(s: &mut Session, p: &Value, accept: bool) -> Result<Value> {
    let v = view(s)?;
    let only = optional_id(p, "story")?.map(StoryId);
    let ids = v
        .root
        .change_ranges()?
        .into_iter()
        .filter(|r| only.is_none_or(|sid| r.component == v.component && r.story == sid))
        .map(|r| r.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    resolve(s, ids, accept)
}

fn resolve(s: &mut Session, ids: Vec<u64>, accept: bool) -> Result<Value> {
    let selected: BTreeSet<_> = ids.iter().copied().collect();
    if selected.len() != ids.len() {
        return Err(bad("publication.review.resolve", "duplicate change identity"));
    }
    if ids.is_empty() {
        return Ok(json!({"changes":0}));
    }
    let result = mutate(s, |root, v| {
        for id in &ids {
            if !root.review.changes.get(id).is_some_and(|c| c.status == ChangeStatus::Pending) {
                return Err(bad("publication.review.resolve", format!("change {id} is missing or already resolved")));
            }
        }
        let ranges = root.change_ranges()?;
        let components: BTreeSet<_> = ranges.iter().filter(|r| selected.contains(&r.id)).map(|r| r.component).collect();
        for component in components {
            let mut d = root.project(v.edition, component)?;
            for (sid, st) in &mut d.stories {
                if d.endnote_story == Some(*sid) || root.generated_destinations().any(|(_, c, s, _)| c == component && s == *sid) {
                    continue;
                }
                Arc::make_mut(st).for_each_text_mut(&mut |leaf| {
                    let changes: Vec<_> =
                        leaf.runs().filter_map(|(r, f)| f.over.change_id.filter(|id| selected.contains(id)).map(|_| (r, f.over.change))).collect();
                    for (range, kind) in changes.into_iter().rev() {
                        if (kind == Some(ChangeMark::Deleted)) == accept {
                            leaf.delete(range);
                        } else {
                            leaf.format_chars(range, |f| {
                                f.over.change = None;
                                f.over.change_id = None;
                            });
                        }
                    }
                });
            }
            d.sync_endnote_story();
            root.capture(v.edition, component, &d)?;
        }
        for id in &ids {
            if let Some(c) = root.review.changes.get_mut(id) {
                c.status = if accept { ChangeStatus::Accepted } else { ChangeStatus::Rejected };
            }
        }
        root.revision = root.revision.saturating_add(1);
        Ok(json!({"changes":ids.len(),"accepted":accept}))
    })?;
    let st = s.doc_mut()?;
    if let Some(t) = &mut st.selection.text {
        if let Some(text) = st.doc.text_story(t.story, t.cell) {
            t.anchor = designcraft_doc::story::floor_char_boundary(&text.text, t.anchor.min(text.len()));
            t.focus = designcraft_doc::story::floor_char_boundary(&text.text, t.focus.min(text.len()));
        } else {
            st.selection.text = None;
        }
    }
    Ok(result)
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let v = view(s)?;
    let comments = v
        .root
        .review
        .comments
        .values()
        .map(|c| {
            let start = v.root.review_target(&c.start)?;
            Ok(json!({"comment":c,"unresolved":v.root.review_address(c.id)?.is_none(),"location":start}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"tracking":v.root.review.tracking,"author":v.root.review.author(),"changes":v.root.change_ranges()?,"records":v.root.review.changes,"comments":comments,
        "proofs":v.root.review.proofs.values().map(|p|json!({"id":p.id,"name":p.name,"author":p.author,"revision":p.revision,"edition":p.edition.id,"approval":v.root.review.approvals.get(&p.id),"diagnostics":p.diagnostics})).collect::<Vec<_>>()}),
    )
}

fn offset(p: &Value, key: &str) -> Result<Option<usize>> {
    p.get(key)
        .map(|v| v.as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| bad("publication.review", format!("{key} must be a byte offset"))))
        .transpose()
}

fn selection_path(d: &Document, sid: StoryId, cell: Option<designcraft_doc::CellAddr>) -> Result<Vec<PathStep>> {
    let Some(cell) = cell else {
        return Ok(Vec::new());
    };
    if let Some(id) = cell.review_id() {
        let v = d.publication.as_ref().ok_or_else(|| bad("publication.review", "no publication"))?;
        return v
            .root
            .review_address(id)?
            .filter(|(c, s, _, _, _)| *c == v.component && *s == sid)
            .map(|(_, _, p, _, _)| p)
            .ok_or_else(|| bad("publication.review", "unresolved review selection"));
    }
    if let Some(id) = cell.annotation_id() {
        let v = d.publication.as_ref().ok_or_else(|| bad("publication.review", "no publication"))?;
        let e = v
            .root
            .annotation_entries()?
            .into_iter()
            .find(|e| e.component == v.component && e.story == sid && e.identity.id == id)
            .ok_or_else(|| bad("publication.review", "unresolved note selection"))?;
        let mut path = e.path;
        path.push(if e.endnote { PathStep::Endnote { id: e.note.id } } else { PathStep::Footnote { id: e.note.id } });
        return Ok(path);
    }
    Ok(vec![if let Some(id) = cell.footnote_id() {
        PathStep::Footnote { id }
    } else {
        PathStep::Cell { table: cell.table, row: cell.row, col: cell.col }
    }])
}

fn comment(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text")
        .filter(|t| !t.trim().is_empty() && t.len() <= 64 * 1024)
        .ok_or_else(|| bad("publication.comment.add", "text required, at most 64 KiB"))?
        .to_string();
    s.edit(|d, selection| {
        let mut v = d.publication.clone().ok_or_else(|| bad("publication.comment.add", "no publication"))?;
        let mut root = (*v.root).clone();
        let caret = selection.text;
        let sid = optional_id(p, "story")?
            .map(StoryId)
            .or(caret.map(|t| t.story))
            .ok_or_else(|| bad("publication.comment.add", "story or selection required"))?;
        let path: Vec<PathStep> = match p.get("path") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad("publication.comment.add", e.to_string()))?,
            None => selection_path(d, sid, caret.filter(|t| t.story == sid).and_then(|t| t.cell))?,
        };
        let content =
            *root.components.get(&v.component).and_then(|c| c.stories.get(&sid)).ok_or_else(|| bad("publication.comment.add", "unknown story"))?;
        if d.endnote_story == Some(sid) || root.generated_destinations().any(|(_, c, s, _)| c == v.component && s == sid) {
            return Err(bad("publication.comment.add", "comment on authored text, not generated output"));
        }
        let start = offset(p, "start")?.or(caret.filter(|t| t.story == sid).map(|t| t.range().start)).unwrap_or(0);
        let end = offset(p, "end")?.or(caret.filter(|t| t.story == sid).map(|t| t.range().end)).unwrap_or(start);
        let first = reserve_anchor(&mut d.next_id)?;
        let last = reserve_anchor(&mut d.next_id)?;
        let leaf = d.story_mut(sid).and_then(|s| at_path_mut(s, &path)).ok_or_else(|| bad("publication.comment.add", "text path not found"))?;
        if start > end || end > leaf.len() || !leaf.text.is_char_boundary(start) || !leaf.text.is_char_boundary(end) {
            return Err(bad("publication.comment.add", "range must be ordered UTF-8 boundaries"));
        }
        let quote = leaf.text.get(start..end).unwrap_or_default().to_owned();
        if quote.len() > 64 * 1024 {
            return Err(bad("publication.comment.add", "quoted passage exceeds 64 KiB"));
        }
        let end = if end > start {
            insert_anchor(leaf, end, "Comment end", last);
            Some(TextTarget { component: v.component, content, anchor: last })
        } else {
            None
        };
        insert_anchor(leaf, start, "Comment start", first);
        let id = root.alloc()?;
        root.review.comments.insert(
            id,
            Comment {
                id,
                text,
                author: str_param(p, "author").map(str::to_owned).unwrap_or_else(|| root.review.author()),
                revision: root.revision,
                start: TextTarget { component: v.component, content, anchor: first },
                end,
                quote,
                resolved: false,
            },
        );
        root.revision = root.revision.saturating_add(1);
        v.root = Arc::new(root);
        d.publication = Some(v);
        if let Some(t) = selection.text.as_mut().filter(|t| t.story == sid) {
            t.anchor = start + designcraft_doc::ANCHOR_MARK.len_utf8();
            t.focus = t.anchor;
        }
        Ok(json!({"id":id}))
    })
}

fn reserve_anchor(next: &mut u64) -> Result<u64> {
    *next = next.checked_add(1).ok_or_else(|| bad("publication.review", "anchor identity exhausted"))?;
    next.checked_mul(1 << 24).and_then(|n| n.checked_add(1)).ok_or_else(|| bad("publication.review", "anchor identity exhausted"))
}
fn insert_anchor(s: &mut Story, at: usize, name: &str, id: u64) {
    let old = s.insert_anchor(at, name);
    if let Some(a) = s.anchors.iter_mut().find(|a| a.id == old) {
        Arc::make_mut(a).id = id;
    }
    let end = at.saturating_add(designcraft_doc::ANCHOR_MARK.len_utf8());
    s.format_chars(at..end, |f| {
        f.over.change = None;
        f.over.change_id = None;
    });
}

fn go_to(s: &mut Session, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    let mut v = view(s)?;
    let (component, story, path, start, end) =
        v.root.review_address(id)?.ok_or_else(|| bad("publication.review.goTo", "passage was removed or change is resolved"))?;
    v.component = component;
    let d = v.project()?;
    let outer = d.story(story).ok_or_else(|| bad("publication.review.goTo", "story missing"))?;
    let at = path
        .first()
        .and_then(|step| match step {
            PathStep::Cell { table, .. } => outer.table_anchor(*table),
            PathStep::Footnote { id } => outer.note_anchor(*id),
            PathStep::Endnote { id } => outer
                .endnotes
                .iter()
                .position(|n| n.id == *id)
                .and_then(|i| outer.text.match_indices(designcraft_doc::ENDNOTE_REF).nth(i).map(|(p, _)| p)),
        })
        .unwrap_or(start);
    let composed = s.cache.get(&d, story, None);
    let frame =
        composed.frames.iter().find(|f| f.range.contains(&at) || f.range.end == at).map(|f| f.frame).or_else(|| outer.frames.first().copied());
    let page = frame.and_then(|f| d.page_of_item(f));
    let cell = (!path.is_empty()).then_some(designcraft_doc::CellAddr::review(id));
    let state = s.doc_mut()?;
    state.doc = Arc::new(d);
    state.selection = Selection::text(TextSel { story, anchor: start, focus: end, frame, cell });
    state.revision = state.revision.saturating_add(1);
    Ok(json!({"id":id,"component":component,"story":story,"path":path,"start":start,"end":end,"page":page}))
}

#[cfg(test)]
mod tests;
