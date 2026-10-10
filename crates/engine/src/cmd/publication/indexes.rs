//! Publication index commands. Preview is read-only; generated output joins the same atomic
//! bounded layout update as cross-references and contents lists.
use super::{CommandSpec, Result, Session, bad, cmd, has_publication, mutate, optional_id, str_param, view};
use designcraft_doc::{
    ComponentId, Document, Story, StoryId,
    publication::{Publication, indexes::*, numbering::TextTarget, references::*},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::sync::Arc;

mod generate;
pub(in crate::cmd) mod legacy;
mod migration;
pub(super) use generate::{evaluate, regenerate};
pub(super) use migration::migrate_legacy;

fn parse<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| bad("publication.index", e.to_string()))
}
fn name(p: &Value) -> Result<&str> {
    str_param(p, "index").or_else(|| str_param(p, "name")).ok_or_else(|| bad("publication.index", "index name required"))
}
fn get<'a>(r: &'a Publication, name: &str) -> Result<&'a Definition> {
    r.indexes.get(name).ok_or_else(|| bad("publication.index", format!("unknown index {name}")))
}
fn get_mut<'a>(r: &'a mut Publication, name: &str) -> Result<&'a mut Definition> {
    r.indexes.get_mut(name).ok_or_else(|| bad("publication.index", format!("unknown index {name}")))
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "publication.index.list","Publication Indexes",[],None,"{} → named index definitions and destinations",has_publication,|s,_|Ok(json!(view(s)?.root.indexes))),
        cmd!(
            "publication.index.define",
            "Define Publication Index",
            [],
            None,
            "{name,rename?,sorting?:alphabetical|numeric|biblical|archival,bookOrder?:[names],bookAliases?:{alias:book},format?:{title,titleStyle,sectionStyle,levelStyles,topicFormat,pageFormat,runIn,sectionHeadings,pageLabels:global|local|component,followingTopic,betweenPages,pageRange,betweenEntries,beforeCrossRef}} — update settings without replacing topics or occurrences",
            has_publication,
            define
        ),
        cmd!(
            "publication.index.topic",
            "Define Index Topic",
            [],
            None,
            "{index,id?,display,parent?,sort?:{type:alphabetical,key}|{type:numeric,value}|{type:biblical,book,chapter,verse,endChapter?,endVerse?}|{type:archival,repository,collection,shelfmark,folio,side:recto|verso},see?:[topic ids],seeAlso?:[topic ids]} → stable topic id",
            has_publication,
            topic
        ),
        cmd!(
            "publication.index.mark",
            "Mark Index Occurrence",
            [],
            None,
            "{index,topic,story?,start?,end?,path?:[{type:cell,table,row,col}|{type:footnote,id}|{type:endnote,id}],suppressPages?:false} — use selected text or explicit UTF-8 range; anchors survive edits in prose, tables and notes",
            has_publication,
            mark
        ),
        cmd!(
            "publication.index.addOccurrence",
            "Link Index Occurrence",
            [],
            None,
            "{index,topic,start:{component,content,anchor},end?:{component,content,anchor},suppressPages?:false} — bind existing anchors, including cross-component ranges",
            has_publication,
            |s, p| {
                let name = name(p)?.to_string();
                let topic = optional_id(p, "topic")?.ok_or_else(|| bad("publication.index", "topic required"))?;
                let start: TextTarget = parse(p["start"].clone())?;
                let end: Option<TextTarget> = p.get("end").cloned().map(parse).transpose()?;
                mutate(s, |r, _| {
                    if r.target_text(&start).is_none() || end.as_ref().is_some_and(|t| r.target_text(t).is_none()) {
                        return Err(bad("publication.index", "requested anchor is unresolved"));
                    }
                    let id = r.alloc()?;
                    let end = end.map(|target| r.alloc().map(|id| Endpoint { id, target })).transpose()?;
                    get_mut(r, &name)?
                        .occurrences
                        .insert(id, Occurrence { id, topic, start, end, suppress_pages: p["suppressPages"].as_bool().unwrap_or(false) });
                    r.revision = r.revision.saturating_add(1);
                    Ok(json!({"occurrence":id}))
                })
            }
        ),
        cmd!(
            "publication.index.removeOccurrence",
            "Remove Index Occurrence",
            [],
            None,
            "{index,id} — remove the index binding; source text and anchor markers remain",
            has_publication,
            |s, p| {
                let name = name(p)?.to_string();
                let id = optional_id(p, "id")?.ok_or_else(|| bad("publication.index", "id required"))?;
                mutate(s, |r, _| {
                    if get_mut(r, &name)?.occurrences.remove(&id).is_none() {
                        return Err(bad("publication.index", "unknown occurrence"));
                    }
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(query "publication.index.preview","Preview Publication Index",[],None,"{index} → all topics, occurrences, excerpts, resolved locations, unresolved reasons and generated rows; never regenerates output or composes pages",has_publication,|s,p| {
            let v=view(s)?;let d=get(&v.root,name(p)?)?;let e=v.root.editions.get(&v.edition).ok_or_else(||bad("publication.index","missing edition"))?;
            Ok(json!(preview(&v.root,d,&e.references)?))
        }),
        cmd!(
            "publication.index.generate",
            "Generate Publication Index",
            [],
            None,
            "{index,component?,page?,rect?} — create a bound index story on a new page, or update its existing destination",
            has_publication,
            generate::destination
        ),
        cmd!(
            "publication.index.detach",
            "Detach Publication Index",
            [],
            None,
            "{index} — make the current generated text editable while retaining the index's topics and occurrences",
            has_publication,
            generate::detach
        ),
        cmd!(
            "publication.index.migrate",
            "Migrate Existing Index",
            [],
            None,
            "{} — convert legacy component markers and their generated indexes into named publication indexes once",
            has_publication,
            |s, _| mutate(s, |r, v| Ok(json!({"migrated":migrate_legacy(r,v.edition)?})))
        ),
        cmd!(noundo "publication.index.goTo","Go to Index Occurrence",[],None,"{index,id,end?:false} — activate the resolved occurrence's component and page",has_publication,|s,p| {
            let mut v=view(s)?;let d=get(&v.root,name(p)?)?;let id=optional_id(p,"id")?.ok_or_else(||bad("publication.index","id required"))?;
            let o=d.occurrences.get(&id).ok_or_else(||bad("publication.index","unknown occurrence"))?;
            let id=if p["end"].as_bool().unwrap_or(false){o.end.as_ref().map_or(id,|e|e.id)}else{id};
            let target=d.targets().find(|(key,_)|*key==id).map(|(_,t)|t).ok_or_else(||bad("publication.index","missing target"))?;
            if v.root.target_text(target).is_none(){return Err(bad("publication.index","indexed source passage was removed"));}
            let location=v.root.editions.get(&v.edition).and_then(|e|e.references.locations.get(&id)).cloned().ok_or_else(||bad("publication.index","destination is unplaced; update publication first"))?;
            v.component=location.component;let doc=v.project()?;let st=s.doc_mut()?;st.doc=Arc::new(doc);st.selection=Default::default();Ok(json!({"component":location.component,"page":location.page}))
        }),
    ]
}

fn define(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name(p)?.to_string();
    mutate(s, |r, v| {
        let mut index = r.indexes.get(&name).cloned().unwrap_or_else(|| Definition {
            name: name.clone(),
            format: Formatting { title: name.clone(), ..Default::default() },
            ..Default::default()
        });
        if let Some(value) = p.get("sorting") {
            index.sorting = parse(value.clone())?;
        }
        if let Some(value) = p.get("bookOrder") {
            index.book_order = parse(value.clone())?;
        }
        if let Some(value) = p.get("bookAliases") {
            index.book_aliases = parse(value.clone())?;
        }
        if let Some(value) = p.get("format") {
            let mut format = json!(index.format);
            for (k, value) in value.as_object().ok_or_else(|| bad("publication.index", "format must be an object"))? {
                format[k] = value.clone();
            }
            index.format = parse(format)?;
        }
        if let Some(new_name) = str_param(p, "rename") {
            if new_name != name && r.indexes.contains_key(new_name) {
                return Err(bad("publication.index", "an index already has the requested new name"));
            }
            index.name = new_name.into();
        }
        index.check(r)?;
        if let Some(destination) = &index.destination {
            let mut d = r.project(v.edition, destination.component)?;
            generate::check_styles(&d, &index.format)?;
            if let Some(st) = d.story_mut(destination.story) {
                if let Some(first) = st.paras.first_mut() {
                    first.style = index.format.title_style.clone();
                }
                let rows = index.ordered_topics()?;
                let entries: Vec<_> = st.text.match_indices(designcraft_doc::XREF_MARK).zip(&st.xrefs).map(|((at, _), x)| (at, x.field)).collect();
                for (at, field) in entries {
                    let level = index
                        .fields
                        .iter()
                        .find(|(_, id)| Some(**id) == field)
                        .and_then(|(topic, _)| rows.iter().find(|(id, _)| id == topic))
                        .map_or(1, |(_, level)| *level);
                    let pi = st.para_at(at);
                    if let Some(para) = st.paras.get_mut(pi) {
                        para.style = index.format.level_styles.get(&level).cloned().unwrap_or_else(|| designcraft_doc::story::BASIC_PARAGRAPH.into());
                    }
                }
            }
            r.capture(v.edition, destination.component, &d)?;
        }
        r.indexes.remove(&name);
        r.indexes.insert(index.name.clone(), index);
        r.revision = r.revision.saturating_add(1);
        Ok(Value::Null)
    })
}
fn topic(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name(p)?.to_string();
    let requested = optional_id(p, "id")?;
    mutate(s, |r, _| {
        let id = match requested {
            Some(id) => {
                if !get(r, &name)?.topics.contains_key(&id) {
                    return Err(bad("publication.index", "unknown topic id"));
                }
                id
            }
            None => r.alloc()?,
        };
        let mut value = get(r, &name)?
            .topics
            .get(&id)
            .map(|t| json!(t))
            .unwrap_or(json!({"id":id,"display":"","parent":null,"sort":{"type":"alphabetical","key":""},"see":[],"seeAlso":[]}));
        for key in ["display", "parent", "sort", "see", "seeAlso"] {
            if let Some(v) = p.get(key) {
                value[key] = v.clone();
            }
        }
        let topic: Topic = parse(value)?;
        get_mut(r, &name)?.topics.insert(id, topic);
        r.revision = r.revision.saturating_add(1);
        Ok(json!({"topic":id}))
    })
}

#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Step {
    Cell { table: u64, row: usize, col: usize },
    Footnote { id: u64 },
    Endnote { id: u64 },
}
fn leaf<'a>(story: &'a mut Story, path: &[Step]) -> Result<&'a mut Story> {
    let Some((step, rest)) = path.split_first() else { return Ok(story) };
    if path.len() > 32 {
        return Err(bad("publication.index", "text path exceeds 32 levels"));
    }
    let next = match step {
        Step::Cell { table, row, col } => story.table_mut(*table).and_then(|t| t.cell_mut(*row, *col)).map(|c| &mut c.text),
        Step::Footnote { id } => story.note_mut(*id).map(|n| &mut n.text),
        Step::Endnote { id } => story.endnotes.iter_mut().find(|n| n.id == *id).map(|n| &mut Arc::make_mut(n).text),
    }
    .ok_or_else(|| bad("publication.index", "text path does not resolve"))?;
    leaf(next, rest)
}
// Reserve an unused story-sized anchor namespace in the component's allocation high-water
// mark. Nested notes often have StoryId(0), so their local insert_anchor ids alone collide.
fn reserve_anchor(next_local: &mut u64) -> Result<u64> {
    *next_local = next_local.checked_add(1).ok_or_else(|| bad("publication.index", "anchor identity exhausted"))?;
    next_local.checked_mul(1 << 24).and_then(|n| n.checked_add(1)).ok_or_else(|| bad("publication.index", "anchor identity exhausted"))
}
fn insert_anchor(story: &mut Story, at: usize, name: &str, id: u64) -> u64 {
    let local = story.insert_anchor(at, name);
    if let Some(anchor) = story.anchors.iter_mut().find(|a| a.id == local) {
        Arc::make_mut(anchor).id = id;
    }
    id
}
fn mark(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name(p)?.to_string();
    let topic = optional_id(p, "topic")?.ok_or_else(|| bad("publication.index", "topic required"))?;
    s.edit(|d, selection| {
        let mut v = d.publication.clone().ok_or_else(|| bad("publication.index", "no publication"))?;
        let mut root = (*v.root).clone();
        get(&root, &name)?;
        let caret = selection.text;
        let sid = optional_id(p, "story")?
            .map(StoryId)
            .or(caret.map(|t| t.story))
            .ok_or_else(|| bad("publication.index", "story or text selection required"))?;
        let mut path: Vec<Step> = parse(p.get("path").cloned().unwrap_or(json!([])))?;
        if p.get("path").is_none()
            && p.get("story").is_none()
            && let Some(cell) = caret.and_then(|t| t.cell)
        {
            path.push(if cell.table == designcraft_doc::FOOTNOTE_TABLE {
                Step::Footnote { id: cell.row as u64 }
            } else {
                Step::Cell { table: cell.table, row: cell.row, col: cell.col }
            });
        }
        let content =
            *root.components.get(&v.component).and_then(|c| c.stories.get(&sid)).ok_or_else(|| bad("publication.index", "unknown story"))?;
        if d.endnote_story == Some(sid) || root.generated_destinations().any(|(_, c, s, _)| c == v.component && s == sid) {
            return Err(bad("publication.index", "index authored text, not a bound generated story"));
        }
        let first_anchor = reserve_anchor(&mut d.next_id)?;
        let last_anchor = reserve_anchor(&mut d.next_id)?;
        let st = leaf(d.story_mut(sid).ok_or_else(|| bad("publication.index", "unknown story"))?, &path)?;
        let position = |key: &str| -> Result<Option<usize>> {
            p.get(key)
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| bad("publication.index", format!("{key} must be a nonnegative byte offset")))
                })
                .transpose()
        };
        let start = position("start")?.or(caret.filter(|t| t.story == sid).map(|t| t.range().start)).unwrap_or(0);
        let end = position("end")?.or(caret.filter(|t| t.story == sid).map(|t| t.range().end)).unwrap_or(start);
        if start > end || end > st.len() || !st.text.is_char_boundary(start) || !st.text.is_char_boundary(end) {
            return Err(bad("publication.index", "range must be ordered UTF-8 boundaries within this text"));
        }
        let id = root.alloc()?;
        let end = if end > start {
            Some(Endpoint {
                id: root.alloc()?,
                target: TextTarget { component: v.component, content, anchor: insert_anchor(st, end, "Index range end", last_anchor) },
            })
        } else {
            None
        };
        let anchor = insert_anchor(st, start, "Index occurrence", first_anchor);
        get_mut(&mut root, &name)?.occurrences.insert(
            id,
            Occurrence {
                id,
                topic,
                start: TextTarget { component: v.component, content, anchor },
                end,
                suppress_pages: p["suppressPages"].as_bool().unwrap_or(false),
            },
        );
        root.revision = root.revision.saturating_add(1);
        v.root = Arc::new(root);
        d.publication = Some(v);
        if p.get("story").is_none()
            && p.get("path").is_none()
            && let Some(t) = selection.text.as_mut()
            && t.story == sid
        {
            t.anchor = start + designcraft_doc::ANCHOR_MARK.len_utf8();
            t.focus = t.anchor;
        }
        Ok(json!({"occurrence":id}))
    })
}

#[cfg(test)]
mod tests;
