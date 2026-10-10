//! Named note streams and edition placement. Rich note text is edited at its canonical owner.
use super::{CommandSpec, Result, Session, Value, bad, cmd, has_publication, mutate, optional_id, str_param, view};
use designcraft_doc::{
    ComponentId, ParaFormat, StoryId,
    publication::{Publication, annotations::*},
};
use serde_json::json;
use std::sync::Arc;

mod apparatus;
mod outputs;
pub(super) use apparatus::diagnostics;
pub(super) use outputs::{locations, number, prepare};

fn overlay<T: serde::Serialize + serde::de::DeserializeOwned>(current: &T, patch: &Value) -> Result<T> {
    let mut v = serde_json::to_value(current).map_err(|e| bad("publication.notes", e.to_string()))?;
    if let (Some(base), Some(p)) = (v.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            base.insert(k.clone(), v.clone());
        }
    }
    serde_json::from_value(v).map_err(|e| bad("publication.notes", e.to_string()))
}
pub(super) fn specs() -> Vec<CommandSpec> {
    let mut commands = vec![
        cmd!(query "publication.notes.list","Publication Notes",[],None,"{stream?} → streams, edition policies, labels, canonical note owners and rich content",has_publication,list),
        cmd!(
            "publication.notes.stream",
            "Define Note Stream",
            [],
            None,
            "{name, style?, startAt?, restart?: publication|component|page|spread|section, prefix?, suffix?, affixIn?: none|reference|text|both, refPosition?, refCharStyle?, paraStyle?, preserveParagraphStyles?, heading?, headingStyle?, separator?, defaultPlacement?}",
            has_publication,
            |s, p| {
                let name = str_param(p, "name")
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| bad("publication.notes.stream", "name required"))?
                    .to_string();
                mutate(s, |r, _| {
                    let old = r.note_streams.get(&name).cloned().unwrap_or_else(|| Stream {
                        name: name.clone(),
                        heading: name.clone(),
                        ..Default::default()
                    });
                    let next: Stream = overlay(&old, p)?;
                    next.check()?;
                    r.note_streams.insert(name, next);
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.notes.policy",
            "Edition Note Placement",
            [],
            None,
            "{stream, placement?: footnote|componentEnd|publicationEnd|sidenote|omit, columns?, gutter?, runIn?, betweenNotes?, maxFraction?, spaceBefore?, spaceBetween?, sideWidth?, sideGap?, side?: left|right|outer}",
            has_publication,
            |s, p| {
                let name = str_param(p, "stream").ok_or_else(|| bad("publication.notes.policy", "stream required"))?.to_string();
                let mut patch = p.clone();
                if let Some(o) = patch.as_object_mut() {
                    o.remove("stream");
                }
                mutate(s, |r, v| {
                    if !r.note_streams.contains_key(&name) {
                        return Err(bad("publication.notes.policy", "unknown stream"));
                    }
                    let next: Policy = overlay(&r.note_policy(v.edition, &name), &patch)?;
                    next.check()?;
                    Arc::make_mut(r.editions.get_mut(&v.edition).ok_or_else(|| bad("publication.notes.policy", "unknown edition"))?)
                        .note_policies
                        .insert(name, next);
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.note.insert",
            "Insert Publication Note",
            [],
            None,
            "{stream, text?, story?, at?, path?: [{type:cell,table,row,col}|{type:footnote|endnote,id}]} — inserts at the selected caret by default",
            has_publication,
            insert
        ),
        cmd!(
            "publication.note.edit",
            "Edit Publication Note",
            [],
            None,
            "{id, text?, stream?} — rich editing is also available through note.select and normal text commands",
            has_publication,
            edit
        ),
        cmd!(noundo "publication.note.select","Select Publication Note Text",[],None,"{id, anchor?, focus?} — select canonical rich note text for normal text and formatting commands",has_publication,select),
        cmd!(noundo "publication.note.goTo","Go to Publication Note",[],None,"{id, target?: note|callout} → resolved component and zero-based page; does not recompute layout",has_publication,go_to),
        cmd!(
            "publication.notes.migrate",
            "Migrate Publication Notes",
            [],
            None,
            "{} — give legacy notes persistent identities and separate their content from placement",
            has_publication,
            |s, _| mutate(s, |r, v| {
                prepare(r, v.edition)?;
                Ok(Value::Null)
            })
        ),
    ];
    commands.extend(apparatus::specs());
    commands
}
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let v = view(s)?;
    let entries = v.root.annotation_entries()?;
    let notes:Vec<_>=entries.into_iter().filter(|e|str_param(p,"stream").is_none_or(|x|x==e.identity.stream)).map(|e|json!({
        "id":e.identity.id,"callout":e.identity.callout,"stream":e.identity.stream,"component":e.component,"content":e.content,"story":e.story,
        "path":e.path,"at":e.at,"endnote":e.endnote,"localId":e.note.id,"text":e.note.text,"apparatus":e.note.apparatus,"label":v.root.note_label(v.edition,e.identity.id),
        "policy":v.root.note_policy(v.edition,&e.identity.stream)
    })).collect();
    Ok(json!({"streams":v.root.note_streams,"policies":v.root.editions.get(&v.edition).map(|e|&e.note_policies),"notes":notes}))
}
fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "stream").ok_or_else(|| bad("publication.note.insert", "stream required"))?.to_string();
    let stream = view(s)?.root.note_streams.get(&name).cloned().ok_or_else(|| bad("publication.note.insert", "unknown stream"))?;
    let selection = s.doc()?.selection.text;
    let sid = optional_id(p, "story")?
        .map(StoryId)
        .or(selection.map(|t| t.story))
        .ok_or_else(|| bad("publication.note.insert", "story or text selection required"))?;
    let mut path: Vec<PathStep> =
        serde_json::from_value(p.get("path").cloned().unwrap_or(json!([]))).map_err(|e| bad("publication.note.insert", e.to_string()))?;
    if p.get("path").is_none()
        && p.get("story").is_none()
        && let Some(cell) = selection.and_then(|t| t.cell)
    {
        if let Some(id) = cell.annotation_id() {
            let e = entry(&view(s)?.root, &json!({"id":id}))?;
            path = e.path;
            path.push(if e.endnote { PathStep::Endnote { id: e.note.id } } else { PathStep::Footnote { id: e.note.id } });
        } else if let Some(id) = cell.footnote_id() {
            path.push(PathStep::Footnote { id });
        } else {
            path.push(PathStep::Cell { table: cell.table, row: cell.row, col: cell.col });
        }
    }
    if path.len() > 32 {
        return Err(bad("publication.note.insert", "path exceeds 32 levels"));
    }
    let at = match p.get("at") {
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("publication.note.insert", "invalid offset"))?,
        None => selection.map_or(0, |t| t.focus),
    };
    let text = str_param(p, "text").unwrap_or("");
    let local = s.edit(|d, _| {
        let root = d.publication.as_ref().ok_or_else(|| bad("publication.note.insert", "no publication"))?;
        if root.root.generated_destinations().any(|(_, c, story, _)| c == root.component && story == sid) {
            return Err(bad("publication.note.insert", "cannot insert into generated content"));
        }
        let source = d.story_mut(sid).ok_or_else(|| bad("publication.note.insert", "unknown story"))?;
        let source = at_path_mut(source, &path).ok_or_else(|| bad("publication.note.insert", "path not found"))?;
        if !source.text.is_char_boundary(at) {
            return Err(bad("publication.note.insert", "offset must be a UTF-8 boundary within the text"));
        }
        let id = source.insert_note(at, text, ParaFormat { style: stream.para_style.clone(), ..Default::default() });
        source.note_mut(id).ok_or_else(|| bad("publication.note.insert", "missing new note"))?.annotation =
            Some(Identity { id: 0, callout: 0, stream: name.clone() });
        Ok(id)
    })?;
    let v = view(s)?;
    let e = v
        .root
        .annotation_entries()?
        .into_iter()
        .find(|e| e.component == v.component && e.story == sid && e.path == path && e.note.id == local && !e.endnote)
        .ok_or_else(|| bad("publication.note.insert", "new note identity missing"))?;
    Ok(json!({"id":e.identity.id,"callout":e.identity.callout,"localId":local}))
}
fn entry(r: &Publication, p: &Value) -> Result<Entry> {
    let id = optional_id(p, "id")?.ok_or_else(|| bad("publication.note", "id required"))?;
    r.annotation_entries()?.into_iter().find(|e| e.identity.id == id).ok_or_else(|| bad("publication.note", "note no longer exists"))
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    mutate(s, |r, v| {
        let e = entry(r, p)?;
        if let Some(name) = str_param(p, "stream")
            && !r.note_streams.contains_key(name)
        {
            return Err(bad("publication.note.edit", "unknown stream"));
        }
        let mut d = r.project(v.edition, e.component)?;
        let st = d.story_mut(e.story).and_then(|st| at_path_mut(st, &e.path)).ok_or_else(|| bad("publication.note.edit", "note owner missing"))?;
        let n =
            if e.endnote { st.endnote_mut(e.note.id) } else { st.note_mut(e.note.id) }.ok_or_else(|| bad("publication.note.edit", "note missing"))?;
        if let Some(text) = str_param(p, "text") {
            let old = &n.text.text;
            let pre = old.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
            let pre = (0..=pre).rev().find(|&i| old.is_char_boundary(i) && text.is_char_boundary(i)).unwrap_or(0);
            let max = old.len().min(text.len()) - pre;
            let suf = old.bytes().rev().zip(text.bytes().rev()).take(max).take_while(|(a, b)| a == b).count();
            let suf = (0..=suf).rev().find(|&i| old.is_char_boundary(old.len() - i) && text.is_char_boundary(text.len() - i)).unwrap_or(0);
            let end = old.len() - suf;
            super::super::changes::replace(&mut n.text, pre..end, &text[pre..text.len() - suf], r.review.tracking);
        }
        if let Some(name) = str_param(p, "stream")
            && let Some(a) = &mut n.annotation
        {
            a.stream = name.into();
        }
        r.capture(v.edition, e.component, &d)?;
        Ok(Value::Null)
    })
}
fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let mut v = view(s)?;
    let e = entry(&v.root, p)?;
    v.component = e.component;
    let len = e.note.text.len();
    let offset = |key: &str, default| -> Result<usize> {
        match p.get(key) {
            None => Ok(default),
            Some(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| e.note.text.text.is_char_boundary(*n))
                .ok_or_else(|| bad("publication.note.select", "invalid UTF-8 selection")),
        }
    };
    let anchor = offset("anchor", len)?;
    let focus = offset("focus", anchor)?;
    let st = s.doc_mut()?;
    st.doc = Arc::new(v.project()?);
    st.selection = designcraft_doc::Selection::text(designcraft_doc::TextSel {
        story: e.story,
        anchor,
        focus,
        frame: None,
        cell: Some(designcraft_doc::CellAddr::annotation(e.identity.id)),
    });
    st.revision += 1;
    Ok(json!({"id":e.identity.id,"component":e.component,"story":e.story}))
}
fn go_to(s: &mut Session, p: &Value) -> Result<Value> {
    let mut v = view(s)?;
    let e = entry(&v.root, p)?;
    let id = match str_param(p, "target").unwrap_or("note") {
        "note" => e.identity.id,
        "callout" => e.identity.callout,
        _ => return Err(bad("publication.note.goTo", "target is note or callout")),
    };
    let location = v
        .root
        .editions
        .get(&v.edition)
        .and_then(|e| e.references.locations.get(&id))
        .cloned()
        .ok_or_else(|| bad("publication.note.goTo", "note has no resolved page; update the edition or change its placement"))?;
    v.component = location.component;
    let st = s.doc_mut()?;
    st.doc = Arc::new(v.project()?);
    st.selection = Default::default();
    st.revision += 1;
    Ok(json!({"component":location.component,"page":location.page,"id":id}))
}

#[cfg(test)]
mod tests;
