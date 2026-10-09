//! Paragraph and character style creation, application, and renaming.

use super::{CharAttrs, CharacterStyle, ParaAttrs, ParagraphStyle, Result, Session, Styles, Value, attrs, bad, json, ok, str_param};

pub(super) fn apply_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.apply", "missing name"))?.to_string();
    let clear = p.get("clearOverrides").and_then(Value::as_bool).unwrap_or(false);
    if s.doc()?.doc.styles.para(&name).is_none() {
        return Err(bad("style.paragraph.apply", format!("no paragraph style `{name}`")));
    }
    let targets = super::super::text::format_targets(s);
    s.edit(|d, _| {
        for t in &targets {
            let r = &t.range;
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                st.format_paras(r.clone(), |f| {
                    f.style = name.clone();
                    if clear {
                        f.para = ParaAttrs::default();
                        f.chars = CharAttrs::default();
                    }
                });
                if clear {
                    let (a, b) = (st.para_ranges()[st.para_at(r.start)].start, st.para_ranges()[st.para_at(r.end)].end);
                    st.format_chars(a..b, |f| f.over = CharAttrs::default());
                }
            }
        }
        ok()
    })
}

pub(super) fn apply_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.character.apply", "missing name"))?.to_string();
    if s.doc()?.doc.styles.char_style(&name).is_none() {
        return Err(bad("style.character.apply", format!("no character style `{name}`")));
    }
    let targets = super::super::text::format_targets(s);
    s.edit(|d, _| {
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                st.format_chars(t.range.clone(), |f| f.style = name.clone());
            }
        }
        ok()
    })
}

pub(super) fn create_para(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Paragraph Style 1").to_string();
    let mut para: ParaAttrs = attrs(p.get("para"), |a: &mut ParaAttrs, k, v| a.set_json(k, v))?;
    let mut chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false)
        && let Ok(cur) = s.execute("type.selectionAttrs", &json!({}))
        && !cur.is_null()
    {
        if let Ok(pp) = serde_json::from_value::<designcraft_doc::ParaProps>(cur["para"].clone()) {
            let mut full = ParaProps_to_attrs(&pp);
            full.merge(&para);
            para = full;
        }
        if let Ok(cp) = serde_json::from_value::<designcraft_doc::CharProps>(cur["chars"].clone()) {
            let mut full = CharProps_to_attrs(&cp);
            full.merge(&chars);
            chars = full;
        }
    }
    let based_on = str_param(p, "basedOn").map(str::to_string);
    let next = str_param(p, "nextStyle").map(str::to_string);
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.styles.para(n).is_some(), &base);
        if let Some(b) = &based_on
            && d.styles.para(b).is_none()
        {
            return Err(bad("style.paragraph.create", format!("no style `{b}`")));
        }
        d.styles_mut().paragraph.push(ParagraphStyle { name: name.clone(), based_on, next_style: next, para, chars, shortcut: String::new() });
        Ok(json!({"name": name}))
    })
}

#[allow(non_snake_case)]
fn ParaProps_to_attrs(p: &designcraft_doc::ParaProps) -> ParaAttrs {
    designcraft_doc::ParaProps::common([p])
}
#[allow(non_snake_case)]
fn CharProps_to_attrs(p: &designcraft_doc::CharProps) -> CharAttrs {
    designcraft_doc::CharProps::common([p])
}

pub(super) fn edit_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.edit", "missing name"))?.to_string();
    let para: ParaAttrs = attrs(p.get("para"), |a: &mut ParaAttrs, k, v| a.set_json(k, v))?;
    let chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    let rename = str_param(p, "rename").map(str::to_string);
    let based = p.get("basedOn").cloned();
    s.edit(|d, _| {
        if let Some(Value::String(b)) = &based
            && d.styles.para_based_on_cycles(&name, b)
        {
            return Err(bad("style.paragraph.edit", "based-on would create a cycle"));
        }
        let st = d.styles_mut().para_mut(&name).ok_or_else(|| bad("style.paragraph.edit", format!("no style `{name}`")))?;
        st.para.merge(&para);
        st.chars.merge(&chars);
        match based {
            Some(Value::String(b)) => st.based_on = Some(b),
            Some(Value::Null) => st.based_on = None,
            _ => {}
        }
        if let Some(n) = rename.clone() {
            // Every use: other styles, stories, table cells, footnotes, object styles.
            rename_style(d, true, &name, &n);
        }
        ok()
    })
}

/// Rename a paragraph or character style everywhere it's used.
pub(super) fn rename_style(d: &mut designcraft_doc::Document, para: bool, from: &str, to: &str) {
    let st = d.styles_mut();
    if para {
        for s in &mut st.paragraph {
            if s.name == from {
                s.name = to.to_string();
            }
            for r in [&mut s.based_on, &mut s.next_style] {
                if r.as_deref() == Some(from) {
                    *r = Some(to.to_string());
                }
            }
        }
        for o in &mut st.object {
            if o.paragraph_style.as_deref() == Some(from) {
                o.paragraph_style = Some(to.to_string());
            }
        }
    } else {
        for s in &mut st.character {
            if s.name == from {
                s.name = to.to_string();
            }
            if s.based_on.as_deref() == Some(from) {
                s.based_on = Some(to.to_string());
            }
        }
    }
    for sid in d.stories.keys().copied().collect::<Vec<_>>() {
        let Some(story) = d.story_mut(sid) else { continue };
        story.for_each_text_mut(&mut |st| {
            if para {
                for f in st.paras.iter_mut().filter(|f| f.style == from) {
                    f.style = to.to_string();
                }
            } else {
                for r in st.chars.iter_mut().filter(|r| r.format.style == from) {
                    r.format.style = to.to_string();
                }
            }
            st.rev += 1;
        });
    }
}

pub(super) fn group_styles(s: &mut Session, p: &Value) -> Result<Value> {
    let para = match str_param(p, "kind").unwrap_or("paragraph") {
        "paragraph" => true,
        "character" => false,
        k => return Err(bad("style.group", format!("unknown kind `{k}`"))),
    };
    let names: Vec<String> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let group = str_param(p, "group").unwrap_or("").trim_matches('/').to_string();
    s.edit(|d, _| {
        let mut renamed = serde_json::Map::new();
        for n in &names {
            if n.starts_with('[') {
                return Err(bad("style.group", format!("`{n}` is built in")));
            }
            let exists = if para { d.styles.para(n).is_some() } else { d.styles.character.iter().any(|c| c.name == *n) };
            if !exists {
                return Err(bad("style.group", format!("no style `{n}`")));
            }
            let base = n.rsplit('/').next().unwrap_or(n);
            let mut to = if group.is_empty() { base.to_string() } else { format!("{group}/{base}") };
            let taken = |d: &designcraft_doc::Document, x: &str| {
                if para { d.styles.para(x).is_some() } else { d.styles.character.iter().any(|c| c.name == x) }
            };
            if to != *n {
                to = Styles::unique_name(|x| taken(d, x), &to);
                rename_style(d, para, n, &to);
                renamed.insert(n.clone(), json!(to));
            }
        }
        Ok(json!({"renamed": renamed}))
    })
}

pub(super) fn delete_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("").to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(designcraft_doc::BASIC_PARAGRAPH).to_string();
    if name.starts_with('[') {
        return Err(bad("style.paragraph.delete", "built-in styles can't be deleted"));
    }
    s.edit(|d, _| {
        d.styles_mut().paragraph.retain(|x| x.name != name);
        for sid in d.stories.keys().copied().collect::<Vec<_>>() {
            if let Some(story) = d.story_mut(sid) {
                for f in &mut story.paras {
                    if f.style == name {
                        f.style = repl.clone();
                    }
                }
            }
        }
        ok()
    })
}

pub(super) fn create_char(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Character Style 1").to_string();
    let chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    let based_on = str_param(p, "basedOn").map(str::to_string);
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.styles.char_style(n).is_some(), &base);
        d.styles_mut().character.push(CharacterStyle { name: name.clone(), based_on, chars, shortcut: String::new() });
        Ok(json!({"name": name}))
    })
}

pub(super) fn edit_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.character.edit", "missing name"))?.to_string();
    let chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    s.edit(|d, _| {
        let st = d.styles_mut().char_style_mut(&name).ok_or_else(|| bad("style.character.edit", "no such style"))?;
        st.chars.merge(&chars);
        ok()
    })
}

pub(super) fn style_composite_font_set(s: &mut Session, p: &Value) -> Result<Value> {
    let f: designcraft_doc::cjk::CompositeFont = serde_json::from_value(p.clone()).map_err(|e| bad("style.compositeFont.set", e.to_string()))?;
    if f.name.trim().is_empty()
        || f.entries.is_empty()
        || f.entries.len() > 1024
        || !f.entries.iter().any(|e| e.characters.is_empty())
        || f.entries.iter().any(|e| {
            e.family.trim().is_empty()
                || ![e.relative_size, e.horizontal_scale, e.vertical_scale].iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 100.0)
                || !e.baseline_shift.is_finite()
                || e.baseline_shift.abs() > 100.0
        })
    {
        return Err(bad("style.compositeFont.set", "invalid name, base entry, font, scale or baseline shift"));
    }
    s.edit(|d, _| {
        let fonts = &mut d.styles_mut().composite_fonts;
        if let Some(old) = fonts.iter_mut().find(|old| old.name == f.name) {
            *old = f.clone();
        } else {
            fonts.push(f.clone());
        }
        Ok(json!({"name": f.name}))
    })
}

pub(super) fn style_export_tag(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "style").ok_or_else(|| bad("style.exportTag", "`style` required"))?.to_string();
    let character = p.get("character").and_then(Value::as_bool).unwrap_or(false);
    let exists = if character { s.doc()?.doc.styles.char_style(&name).is_some() } else { s.doc()?.doc.styles.para(&name).is_some() };
    if !exists {
        return Err(bad("style.exportTag", format!("no style `{name}`")));
    }
    let key = format!("{}:{name}", if character { "c" } else { "p" });
    let (tag, class) = (str_param(p, "tag").map(str::to_string), str_param(p, "class").map(str::to_string));
    s.edit(|d, _| {
        let st = d.styles_mut();
        let e = st.export_tags.entry(key.clone()).or_default();
        if let Some(t) = tag.clone() {
            e.tag = t;
        }
        if let Some(c) = class.clone() {
            e.class = c;
        }
        let out = serde_json::to_value(&*e).unwrap_or_default();
        if e.tag.is_empty() && e.class.is_empty() {
            st.export_tags.remove(&key);
        }
        Ok(out)
    })
}

pub(super) fn style_list(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.doc()?;
    Ok(json!({
        "paragraph": st.doc.styles.paragraph.iter().map(|p| &p.name).collect::<Vec<_>>(),
        "character": st.doc.styles.character.iter().map(|p| &p.name).collect::<Vec<_>>(),
        "object": st.doc.styles.object.iter().map(|p| &p.name).collect::<Vec<_>>(),
        "cell": st.doc.styles.cell.iter().map(|p| &p.name).collect::<Vec<_>>(),
        "table": st.doc.styles.table.iter().map(|p| &p.name).collect::<Vec<_>>(),
    }))
}
