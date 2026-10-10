use std::collections::BTreeMap;
use std::sync::Arc;

use designcraft_doc::{
    SemanticId, StoryId,
    publication::{
        Publication,
        numbering::{NumberingSequence, ObjectKind, SemanticObject, TextTarget},
    },
};
use serde_json::{Value, json};

use super::{CommandSpec, Result, Session, bad, cmd, has_publication, mutate, optional_id, str_param, view};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "publication.metadata",
            "Publication Metadata and Values",
            [],
            None,
            "{title?, metadata?: {key: string|null}, values?: {key: string|null}} — patch shared metadata/reusable values; null removes a key",
            has_publication,
            metadata
        ),
        cmd!(query "publication.numbering.list", "Publication Numbering Sequences", [], None,
        "{} → {sequences,objects} — definitions and registered stable heading/figure/table targets",has_publication,|s,_| {
            let v=view(s)?;Ok(json!({"sequences":v.root.numbering,"objects":v.root.semantic}))
        }),
        cmd!(
            "publication.numbering.define",
            "Define Publication Numbering",
            [],
            None,
            "{name,kind?:chapter|heading|figure|table,style?:arabic|lowerRoman|upperRoman|lowerLetters|upperLetters|arabicLeadingZero|symbols,start?:1,prefix?,suffix?,restart?:continuous|component|parent|role,scope?:component,role?:frontMatter|chapter|backMatter,level?:1..9,restartAt?:{component:start}}",
            has_publication,
            |s, p| {
                let sequence: NumberingSequence =
                    serde_json::from_value(p.clone()).map_err(|e| bad("publication.numbering.define", e.to_string()))?;
                mutate(s, |root, _| {
                    sequence.check(root)?;
                    root.numbering.insert(sequence.name.clone(), sequence.clone());
                    root.numbers(&sequence.name)?;
                    root.revision = root.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(query "publication.numbering.resolve", "Resolve Publication Numbers", [], None,
        "{name} → [{component,object,title,number,label,resolved}] — source order with explicit unresolved targets, no layout",has_publication,|s,p| {
            let name=str_param(p,"name").ok_or_else(||bad("publication.numbering.resolve","name required"))?;
            serde_json::to_value(view(s)?.root.numbers(name)?).map_err(|e|bad("publication.numbering.resolve",e.to_string()))
        }),
        cmd!(
            "publication.numbering.mark",
            "Mark Numbered Heading, Figure or Table",
            [],
            None,
            "{kind:heading|figure|table,story?,pos?,level?:1,title?} — bind an object to the current paragraph, including a selected table cell or footnote; omitted title follows edited paragraph text",
            has_publication,
            mark
        ),
        cmd!(
            "publication.value.insert",
            "Insert Publication Value",
            [],
            None,
            "{key, metadata?:false} — insert a bound shared value at the text selection",
            has_publication,
            |s, p| {
                let key = str_param(p, "key").ok_or_else(|| bad("publication.value.insert", "key required"))?;
                let metadata = p.get("metadata").and_then(Value::as_bool).unwrap_or(false);
                let v = view(s)?;
                if !(if metadata { &v.root.metadata } else { &v.root.values }).contains_key(key) {
                    return Err(bad("publication.value.insert", "unknown value"));
                }
                insert_variable(s, designcraft_doc::vars::VarKind::PublicationValue { key: key.into(), metadata }, key)
            }
        ),
        cmd!(
            "publication.numbering.insert",
            "Insert Publication Number",
            [],
            None,
            "{name,object?} — insert a bound number at the text selection; omitted object is the current chapter",
            has_publication,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("publication.numbering.insert", "name required"))?;
                let object = optional_id(p, "object")?.map(SemanticId);
                let v = view(s)?;
                if !v.root.numbers(name)?.iter().any(|n| n.resolved && n.object == object && (object.is_some() || n.component == v.component)) {
                    return Err(bad("publication.numbering.insert", "no resolved target in this sequence"));
                }
                insert_variable(s, designcraft_doc::vars::VarKind::PublicationNumber { sequence: name.into(), object }, name)
            }
        ),
    ]
}

fn insert_variable(s: &mut Session, kind: designcraft_doc::vars::VarKind, name: &str) -> Result<Value> {
    s.edit(|doc, selection| {
        let mut caret = selection.text.ok_or_else(|| bad("publication.insert", "select text first"))?;
        let index = if let Some(index) = doc.text_variables.iter().position(|v| v.kind == kind && v.before.is_empty() && v.after.is_empty()) {
            index
        } else {
            if doc.text_variables.len() >= designcraft_doc::vars::VAR_MAX {
                return Err(bad("publication.insert", "too many text variables"));
            }
            let base = format!("Publication: {name}");
            let name = designcraft_doc::Styles::unique_name(|n| doc.text_variables.iter().any(|v| v.name == n), &base);
            doc.text_variables.push(designcraft_doc::vars::TextVariable::new(&name, kind));
            doc.text_variables.len() - 1
        };
        let marker = designcraft_doc::vars::var_char(index).ok_or_else(|| bad("publication.insert", "invalid variable index"))?;
        let story = doc.text_story_mut(caret.story, caret.cell).ok_or_else(|| bad("publication.insert", "unknown text selection"))?;
        let range = caret.range();
        if !story.text.is_char_boundary(range.start) || !story.text.is_char_boundary(range.end) {
            return Err(bad("publication.insert", "selection is outside the text"));
        }
        story.replace(range.clone(), &marker.to_string());
        caret.anchor = range.start + marker.len_utf8();
        caret.focus = caret.anchor;
        selection.text = Some(caret);
        Ok(json!({"variable":index}))
    })
}

fn patch(map: &mut BTreeMap<String, String>, value: Option<&Value>) -> Result<()> {
    let Some(value) = value else { return Ok(()) };
    let entries = value.as_object().ok_or_else(|| bad("publication.metadata", "metadata and values must be objects"))?;
    for (key, value) in entries {
        if key.trim().is_empty() || key.len() > 256 {
            return Err(bad("publication.metadata", "keys need 1–256 bytes"));
        }
        if value.is_null() {
            map.remove(key);
        } else {
            let text = value.as_str().ok_or_else(|| bad("publication.metadata", "each value must be text or null"))?;
            if text.len() > 65536 {
                return Err(bad("publication.metadata", "a value exceeds 65536 bytes"));
            }
            map.insert(key.clone(), text.into());
        }
    }
    if map.len() > 1024 {
        return Err(bad("publication.metadata", "at most 1024 keys are supported"));
    }
    Ok(())
}

fn metadata(s: &mut Session, p: &Value) -> Result<Value> {
    mutate(s, |root, _| {
        if let Some(title) = str_param(p, "title") {
            if title.trim().is_empty() || title.len() > 4096 {
                return Err(bad("publication.metadata", "title needs 1–4096 bytes"));
            }
            root.title = title.into();
        }
        patch(&mut root.metadata, p.get("metadata"))?;
        patch(&mut root.values, p.get("values"))?;
        root.revision = root.revision.saturating_add(1);
        Ok(json!({"title":root.title,"metadata":root.metadata,"values":root.values}))
    })
}

fn mark(s: &mut Session, p: &Value) -> Result<Value> {
    let kind: ObjectKind =
        serde_json::from_value(p.get("kind").cloned().unwrap_or(json!("heading"))).map_err(|e| bad("publication.numbering.mark", e.to_string()))?;
    if kind == ObjectKind::Chapter {
        return Err(bad("publication.numbering.mark", "chapters already have stable identities"));
    }
    let selected = s.doc()?.selection.text;
    let sid = optional_id(p, "story")?
        .map(StoryId)
        .or_else(|| selected.map(|t| t.story))
        .ok_or_else(|| bad("publication.numbering.mark", "select text or supply story and pos"))?;
    let cell = if p.get("story").is_none() { selected.and_then(|t| t.cell) } else { None };
    let pos = p
        .get("pos")
        .map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("publication.numbering.mark", "invalid position")))
        .transpose()?
        .unwrap_or_else(|| selected.map_or(0, |t| t.focus));
    let level = p
        .get("level")
        .map(|v| v.as_u64().filter(|n| *n > 0 && *n <= 9).ok_or_else(|| bad("publication.numbering.mark", "level must be 1–9")))
        .transpose()?
        .unwrap_or(1) as u8;
    let title = str_param(p, "title").map(str::to_string);
    s.edit(|doc, selection| {
        let v = doc.publication.clone().ok_or_else(|| bad("publication.numbering.mark", "no publication"))?;
        let story = doc.text_story_mut(sid, cell).ok_or_else(|| bad("publication.numbering.mark", "unknown story or cell"))?;
        if !story.text.is_char_boundary(pos) {
            return Err(bad("publication.numbering.mark", "position must be a UTF-8 text boundary"));
        }
        let range = story.para_ranges().get(story.para_at(pos)).cloned().ok_or_else(|| bad("publication.numbering.mark", "no paragraph"))?;
        let label = title.clone().unwrap_or_else(|| designcraft_doc::xref::clean(story.text.get(range.clone()).unwrap_or("")));
        let anchor = story.insert_anchor(range.start, &label);
        if let Some(t) = selection.text.as_mut()
            && t.story == sid
            && t.cell == cell
        {
            if t.anchor >= range.start {
                t.anchor = t.anchor.saturating_add(designcraft_doc::xref::ANCHOR_MARK.len_utf8());
            }
            if t.focus >= range.start {
                t.focus = t.focus.saturating_add(designcraft_doc::xref::ANCHOR_MARK.len_utf8());
            }
        }
        let mut root: Publication = (*v.root).clone();
        root.capture(v.edition, v.component, doc)?;
        let content = *root
            .components
            .get(&v.component)
            .and_then(|c| c.stories.get(&sid))
            .ok_or_else(|| bad("publication.numbering.mark", "missing canonical story"))?;
        let id = SemanticId(root.alloc()?);
        root.semantic.insert(id, SemanticObject { id, kind, target: TextTarget { component: v.component, content, anchor }, level, title });
        root.revision = root.revision.saturating_add(1);
        root.check()?;
        doc.publication = Some(designcraft_doc::publication::PublicationView { root: Arc::new(root), ..v });
        Ok(json!({"object":id,"anchor":anchor}))
    })
}
