//! Existing Index menu/command entry points use the publication model when a book is active.
use super::*;

fn ensure(s: &mut Session, name: &str) -> Result<()> {
    if !view(s)?.root.indexes.contains_key(name) {
        define(s, &json!({"name":name}))?;
    }
    if view(s)?.root.indexes.get(name).is_some_and(|d| d.sorting != SortMode::Alphabetical) {
        return Err(bad("index.addReference", "use publication.index.topic with a typed key for this index"));
    }
    Ok(())
}
fn path(s: &mut Session, index: &str, parts: &[String], sort: &[String]) -> Result<u64> {
    let mut parent = None;
    for (i, display) in parts.iter().enumerate() {
        let existing =
            view(s)?.root.indexes.get(index).and_then(|d| d.topics.values().find(|t| t.display == *display && t.parent == parent)).map(|t| t.id);
        parent=Some(match existing{Some(id)=>id,None=>topic(s,&json!({"index":index,"display":display,"parent":parent,"sort":{"type":"alphabetical","key":sort.get(i).cloned().unwrap_or_default()}}))?["topic"].as_u64().ok_or_else(||bad("index.addReference","topic not created"))?});
    }
    parent.ok_or_else(|| bad("index.addReference", "select text or supply a topic"))
}
pub(in crate::cmd) fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let index = str_param(p, "index").unwrap_or("Index");
    let caret = s.doc()?.selection.text.ok_or_else(|| bad("index.addReference", "select text first"))?;
    let story = s.doc()?.doc.text_story(caret.story, caret.cell).ok_or_else(|| bad("index.addReference", "missing text"))?;
    let selected = designcraft_doc::xref::clean(story.slice(caret.range()));
    let topics: Vec<String> = if let Some(topics) = p.get("topics") {
        parse(topics.clone())?
    } else {
        str_param(p, "topic").unwrap_or(&selected).split('>').map(|s| s.trim().to_string()).collect()
    };
    let sort: Vec<String> = parse(p.get("sort").cloned().unwrap_or(json!([])))?;
    let mode = str_param(p, "range").unwrap_or("currentPage");
    let start = caret.range().start;
    let end = match mode {
        "currentPage" | "see" | "seeAlso" | "suppressPageRange" => start,
        "toEndOfStory" => story.len(),
        "nextParagraphs" => {
            let ranges = story.para_ranges();
            ranges
                .get(story.para_at(start).saturating_add(p["paragraphs"].as_u64().unwrap_or(1) as usize).min(ranges.len().saturating_sub(1)))
                .map_or(start, |r| r.end)
        }
        _ => return Err(bad("index.addReference", "unknown index range")),
    };
    ensure(s, index)?;
    let id = path(s, index, &topics, &sort)?;
    if matches!(mode, "see" | "seeAlso") {
        let parts: Vec<_> = str_param(p, "target").unwrap_or("").split('>').map(|s| s.trim().to_string()).collect();
        let target = path(s, index, &parts, &[])?;
        let key = if mode == "see" { "see" } else { "seeAlso" };
        let v = view(s)?;
        let topic = get(&v.root, index)?.topics.get(&id).ok_or_else(|| bad("index.addReference", "topic missing"))?;
        let mut aliases = if mode == "see" { topic.see.clone() } else { topic.see_also.clone() };
        if !aliases.contains(&target) {
            aliases.push(target);
        }
        let mut params = json!({"index":index,"id":id});
        params[key] = json!(aliases);
        super::topic(s, &params)?;
        if mode == "see" {
            return Ok(json!({"topic":id}));
        }
    }
    super::mark(s, &json!({"index":index,"topic":id,"start":start,"end":end,"suppressPages":mode=="suppressPageRange"}))
}
pub(in crate::cmd) fn generate(s: &mut Session, p: &Value) -> Result<Value> {
    let index = str_param(p, "index").unwrap_or("Index");
    if !view(s)?.root.indexes.contains_key(index) {
        define(s, &json!({"name":index}))?;
    }
    let mut format = serde_json::Map::new();
    for key in ["title", "sectionHeadings", "runIn"] {
        if let Some(value) = p.get(key) {
            format.insert(key.into(), value.clone());
        }
    }
    if !format.is_empty() {
        define(s, &json!({"name":index,"format":format}))?;
    }
    let mut params = p.clone();
    params["index"] = json!(index);
    super::generate::destination(s, &params)
}
pub(in crate::cmd) fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let v = view(s)?;
    let e = v.root.editions.get(&v.edition).ok_or_else(|| bad("index.list", "missing edition"))?;
    let mut out = Vec::new();
    for index in v.root.indexes.values().filter(|d| str_param(p, "index").is_none_or(|name| name == d.name)) {
        for o in preview(&v.root, index, &e.references)?.occurrences {
            let source = index.occurrences.get(&o.id).and_then(|o| v.root.target_text(&o.start));
            out.push(json!({"index":index.name,"id":o.id,"topic":o.topic,"story":source.as_ref().map(|t|t.story),"pos":source.as_ref().and_then(|t|t.path.last()),"page":o.start.as_ref().map(|l|&l.label),"occurrence":o}));
        }
    }
    Ok(json!(out))
}
