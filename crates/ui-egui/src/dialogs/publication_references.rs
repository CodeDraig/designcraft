//! Reference selectors use the semantic command only; opening a dialog never requests layout.
use super::{Dialog, Value, combo, json, text_field};
use crate::DesignApp;

pub(super) fn open(app: &mut DesignApp, p: &Value, fields: &mut Value) -> Result<(), String> {
    let state = app.session.doc().map_err(|e| e.to_string())?;
    let v = state.doc.publication.clone().ok_or("no publication open")?;
    fields["targetRevision"] = json!(state.revision);
    fields["targetSelection"] = serde_json::to_value(&state.selection).map_err(|e| e.to_string())?;
    fields["templates"] = json!(v.root.references.templates.keys().collect::<Vec<_>>());
    fields["paraStyles"] = json!(state.doc.styles.paragraph.iter().map(|s| &s.name).collect::<Vec<_>>());
    fields["charStyles"] = json!(state.doc.styles.character.iter().map(|s| &s.name).collect::<Vec<_>>());
    fields["components"] = json!(
        v.root
            .ordered_components()
            .map_err(|e| e.to_string())?
            .iter()
            .filter_map(|id| v.root.components.get(id))
            .map(|c| json!({"id":c.id,"title":c.title,"role":c.role,"children":c.children}))
            .collect::<Vec<_>>()
    );
    match fields["mode"].as_str().unwrap_or("") {
        "template" => {
            let name = p["name"].as_str().unwrap_or("Title and page");
            fields["definition"] = serde_json::to_value(v.root.references.templates.get(name)).map_err(|e| e.to_string())?;
            if fields["definition"].is_null() {
                fields["definition"] = json!({"name":"New template","parts":[{"token":{"type":"title"},"format":{}}]});
            }
        }
        "reference" | "generated" => {
            fields["objects"] = app.run("publication.query", json!({}))?;
            fields["template"] =
                p.get("template").cloned().unwrap_or(json!(if fields["mode"] == "generated" { "Contents entry" } else { "Title and page" }));
            fields["query"] = p.get("query").cloned().unwrap_or(json!({"kind":"heading","ids":[]}));
            fields["use"] = json!("first");
            fields["separator"] = json!(", ");
            fields["suppressOpening"] = json!(true);
            fields["name"] = json!("Contents");
            fields["title"] = json!("Contents");
            fields["titleStyle"] = json!(designcraft_doc::story::BASIC_PARAGRAPH);
            fields["paragraphStyle"] = fields["titleStyle"].clone();
            fields["levelStyles"] = json!({});
            if let Some(id) = p["id"].as_u64()
                && let Some(f) = v.root.references.fields.get(&id)
            {
                fields["id"] = json!(id);
                fields["query"] = json!(f.query);
                fields["template"] = json!(f.template);
                fields["use"] = json!(f.use_);
                fields["separator"] = json!(f.separator);
                fields["suppressOpening"] = json!(f.suppress_opening);
            }
            if let Some(name) = p["name"].as_str()
                && let Some(g) = v.root.references.sections.get(name)
            {
                fields["name"] = json!(g.name);
                fields["query"] = json!(g.query);
                fields["template"] = json!(g.template);
                fields["title"] = json!(g.title);
                fields["titleStyle"] = json!(g.title_style);
                fields["paragraphStyle"] = json!(g.paragraph_style);
                fields["levelStyles"] = json!(g.level_styles);
            }
        }
        "scan" => {
            fields["tei"] = json!(true);
            fields["style"] = json!("");
            fields["kind"] = json!("heading");
            fields["level"] = json!(1);
        }
        _ => return Err("unknown reference dialog".into()),
    }
    Ok(())
}

pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    match d.s("mode").as_str() {
        "template" => template(ui, d),
        "scan" => {
            ui.label("Register source headings or paragraphs with a chosen style as reusable destinations.");
            let mut tei = d.fields.get("tei").and_then(Value::as_bool).unwrap_or(true);
            ui.checkbox(&mut tei, "Use preserved TEI headings");
            d.fields.insert("tei".into(), json!(tei));
            ui.label("Optional paragraph style");
            names(ui, d, "style", "paraStyles", true);
            combo(ui, d, "kind", &[("heading", "Heading"), ("figure", "Figure caption"), ("table", "Table caption")]);
            let mut level = d.fields.get("level").and_then(Value::as_u64).unwrap_or(1);
            ui.add(egui::DragValue::new(&mut level).range(1..=9).prefix("Level "));
            d.fields.insert("level".into(), json!(level));
            ui.label("Already registered anchors keep their identities.");
        }
        _ => {
            let generated = d.s("mode") == "generated";
            if generated {
                ui.label("Section name");
                text_field(ui, d, "name", 350.);
                ui.label("Printed title");
                text_field(ui, d, "title", 350.);
            }
            ui.label("Template");
            names(ui, d, "template", "templates", false);
            if !generated {
                combo(
                    ui,
                    d,
                    "use",
                    &[("first", "First matching destination"), ("join", "All matching destinations"), ("runningHead", "Running head")],
                );
                if d.s("use") == "join" {
                    ui.label("Separate entries with");
                    text_field(ui, d, "separator", 140.);
                }
                if d.s("use") == "runningHead" {
                    let mut on = d.fields.get("suppressOpening").and_then(Value::as_bool).unwrap_or(true);
                    ui.checkbox(&mut on, "Suppress on the chapter's opening page");
                    d.fields.insert("suppressOpening".into(), json!(on));
                    ui.label("Uses the latest qualifying heading before the page's first body content.");
                }
            }
            query(ui, d);
            if generated {
                ui.label("Title paragraph style");
                names(ui, d, "titleStyle", "paraStyles", false);
                ui.label("Entry paragraph style");
                names(ui, d, "paragraphStyle", "paraStyles", false);
                ui.collapsing("Entry styles by level", |ui| {
                    let styles = d.fields.get("paraStyles").and_then(Value::as_array).cloned().unwrap_or_default();
                    for level in 1..=9 {
                        let key = level.to_string();
                        let mut name = d.fields.get("levelStyles").and_then(|v| v.get(&key)).and_then(Value::as_str).unwrap_or("").to_string();
                        egui::ComboBox::from_id_salt(("level-style", level))
                            .selected_text(format!("Level {level}: {}", if name.is_empty() { "Entry style" } else { &name }))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut name, String::new(), "Entry style");
                                for s in &styles {
                                    if let Some(s) = s.as_str() {
                                        ui.selectable_value(&mut name, s.into(), s);
                                    }
                                }
                            });
                        let levels = d.fields.entry("levelStyles").or_insert(json!({}));
                        if let Some(map) = levels.as_object_mut() {
                            if name.is_empty() {
                                map.remove(&key);
                            } else {
                                map.insert(key, json!(name));
                            }
                        }
                    }
                });
            }
        }
    }
}
fn names(ui: &mut egui::Ui, d: &mut Dialog, key: &str, source: &str, empty: bool) {
    let mut value = d.s(key);
    let values = d.fields.get(source).and_then(Value::as_array).cloned().unwrap_or_default();
    egui::ComboBox::from_id_salt(key).selected_text(if value.is_empty() { "None" } else { &value }).show_ui(ui, |ui| {
        if empty {
            ui.selectable_value(&mut value, String::new(), "None");
        }
        for v in values {
            if let Some(s) = v.as_str() {
                ui.selectable_value(&mut value, s.to_string(), s);
            }
        }
    });
    d.fields.insert(key.into(), json!(value));
}
fn query(ui: &mut egui::Ui, d: &mut Dialog) {
    ui.separator();
    ui.strong("Destinations");
    let components = d.fields.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
    let objects = d.fields.get("objects").and_then(Value::as_array).cloned().unwrap_or_default();
    let Some(q) = d.fields.get_mut("query") else { return };
    let mut kind = q["kind"].as_str().unwrap_or("").to_string();
    egui::ComboBox::from_id_salt("query-kind").selected_text(if kind.is_empty() { "All types" } else { &kind }).show_ui(ui, |ui| {
        for (k, label) in [("", "All types"), ("chapter", "Chapters"), ("heading", "Headings"), ("figure", "Figures"), ("table", "Tables")] {
            ui.selectable_value(&mut kind, k.into(), label);
        }
    });
    q["kind"] = if kind.is_empty() { Value::Null } else { json!(kind) };
    let mut component = q["component"].as_u64();
    let label = components.iter().find(|c| c["id"].as_u64() == component).and_then(|c| c["title"].as_str()).unwrap_or("Whole publication");
    egui::ComboBox::from_id_salt("query-component").selected_text(label).show_ui(ui, |ui| {
        ui.selectable_value(&mut component, None, "Whole publication");
        for c in &components {
            ui.selectable_value(&mut component, c["id"].as_u64(), c["title"].as_str().unwrap_or(""));
        }
    });
    q["component"] = json!(component);
    let mut subtree = q["subtree"].as_bool().unwrap_or(false);
    ui.checkbox(&mut subtree, "Include child chapters");
    q["subtree"] = json!(subtree);
    ui.collapsing("Role, levels and range", |ui| {
        let mut role = q["role"].as_str().unwrap_or("").to_string();
        egui::ComboBox::from_id_salt("query-role").selected_text(if role.is_empty() { "All book parts" } else { &role }).show_ui(ui, |ui| {
            for (value, label) in [("", "All book parts"), ("frontMatter", "Front matter"), ("chapter", "Chapters"), ("backMatter", "Back matter")] {
                ui.selectable_value(&mut role, value.into(), label);
            }
        });
        q["role"] = if role.is_empty() { Value::Null } else { json!(role) };
        ui.horizontal(|ui| {
            for (key, label) in [("minLevel", "From level "), ("maxLevel", "To level ")] {
                let mut level = q[key].as_u64().unwrap_or(0);
                ui.add(egui::DragValue::new(&mut level).range(0..=9).prefix(label));
                q[key] = if level == 0 { Value::Null } else { json!(level) };
            }
            ui.small("0 = all");
        });
        let mut range = q["between"].is_array();
        ui.checkbox(&mut range, "Limit to a range in reading order");
        if range {
            let mut pair = [
                q["between"][0].as_u64().or_else(|| objects.first().and_then(|o| o["id"].as_u64())),
                q["between"][1].as_u64().or_else(|| objects.last().and_then(|o| o["id"].as_u64())),
            ];
            for (i, label) in ["From", "Through"].iter().enumerate() {
                let title = objects.iter().find(|o| o["id"].as_u64() == pair[i]).and_then(|o| o["title"].as_str()).unwrap_or("Choose destination");
                egui::ComboBox::from_id_salt(("range", i)).selected_text(format!("{label}: {title}")).show_ui(ui, |ui| {
                    for o in &objects {
                        ui.selectable_value(&mut pair[i], o["id"].as_u64(), o["title"].as_str().unwrap_or(""));
                    }
                });
            }
            q["between"] = json!(pair);
        } else {
            q["between"] = Value::Null;
        }
    });
    ui.label("Leave the list unselected to keep a query that follows matching objects as the book changes.");
    let mut ids: Vec<u64> = q["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    let mut scope = std::collections::BTreeSet::new();
    let mut pending: Vec<_> = component.into_iter().collect();
    while let Some(id) = pending.pop() {
        if scope.insert(id) && subtree {
            pending.extend(
                components
                    .iter()
                    .find(|c| c["id"].as_u64() == Some(id))
                    .into_iter()
                    .flat_map(|c| c["children"].as_array())
                    .flatten()
                    .filter_map(Value::as_u64),
            );
        }
    }
    let start = q["between"][0].as_u64().and_then(|id| objects.iter().position(|o| o["id"].as_u64() == Some(id))).unwrap_or(0);
    let end = q["between"][1].as_u64().and_then(|id| objects.iter().position(|o| o["id"].as_u64() == Some(id))).unwrap_or(usize::MAX);
    egui::ScrollArea::vertical().max_height(220.).show(ui, |ui| {
        for (_, o) in objects.iter().enumerate().filter(|(i, o)| {
            (kind.is_empty() || o["kind"] == kind)
                && (component.is_none() || o["component"].as_u64().is_some_and(|c| scope.contains(&c)))
                && q["role"].as_str().is_none_or(|role| components.iter().find(|c| c["id"] == o["component"]).is_some_and(|c| c["role"] == role))
                && q["minLevel"].as_u64().is_none_or(|n| o["level"].as_u64().unwrap_or(0) >= n)
                && q["maxLevel"].as_u64().is_none_or(|n| o["level"].as_u64().unwrap_or(0) <= n)
                && *i >= start
                && *i <= end
        }) {
            let Some(id) = o["id"].as_u64() else { continue };
            let mut on = ids.contains(&id);
            if ui
                .checkbox(&mut on, format!("{}{}", o["title"].as_str().unwrap_or(""), if o["resolved"] == false { " (unresolved)" } else { "" }))
                .changed()
            {
                if on { ids.push(id) } else { ids.retain(|i| *i != id) }
            }
        }
    });
    q["ids"] = json!(ids);
}
fn template(ui: &mut egui::Ui, d: &mut Dialog) {
    let styles = d.fields.get("charStyles").and_then(Value::as_array).cloned().unwrap_or_default();
    let Some(def) = d.fields.get_mut("definition") else { return };
    let mut name = def["name"].as_str().unwrap_or("").to_string();
    ui.label("Template name");
    ui.text_edit_singleline(&mut name);
    def["name"] = json!(name);
    ui.label("Parts appear in this order. Each part can use a character style and size override.");
    let Some(parts) = def["parts"].as_array_mut() else { return };
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(380.).show(ui, |ui| {
        for (i, p) in parts.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    let mut kind = p["token"]["type"].as_str().unwrap_or("title").to_string();
                    egui::ComboBox::from_id_salt("token").selected_text(&kind).show_ui(ui, |ui| {
                        for k in ["literal", "title", "componentTitle", "publicationTitle", "number", "page", "globalPage", "metadata", "value"] {
                            ui.selectable_value(&mut kind, k.to_string(), k);
                        }
                    });
                    if p["token"]["type"] != kind {
                        p["token"] = json!({"type":kind});
                    }
                    let key = match kind.as_str() {
                        "literal" => Some("text"),
                        "number" => Some("sequence"),
                        "metadata" | "value" => Some("key"),
                        _ => None,
                    };
                    if let Some(key) = key {
                        let mut value = p["token"][key].as_str().unwrap_or("").to_string();
                        ui.add(egui::TextEdit::singleline(&mut value).desired_width(160.).hint_text(key));
                        p["token"][key] = json!(value);
                    }
                    if ui.button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    let mut style = p["format"]["style"].as_str().unwrap_or("").to_string();
                    egui::ComboBox::from_id_salt("style").selected_text(if style.is_empty() { "Surrounding text" } else { &style }).show_ui(
                        ui,
                        |ui| {
                            ui.selectable_value(&mut style, String::new(), "Surrounding text");
                            for s in &styles {
                                if let Some(s) = s.as_str() {
                                    ui.selectable_value(&mut style, s.to_string(), s);
                                }
                            }
                        },
                    );
                    p["format"]["style"] = json!(style);
                    let mut size = p["format"]["over"]["size"].as_f64().unwrap_or(0.0);
                    ui.add(egui::DragValue::new(&mut size).range(0.0..=200.0).prefix("Size "));
                    ui.small("0 inherits");
                    if size > 0.0 {
                        p["format"]["over"]["size"] = json!(size);
                    } else if let Some(over) = p["format"]["over"].as_object_mut() {
                        over.remove("size");
                    }
                });
                ui.separator();
            });
        }
    });
    if let Some(i) = remove {
        parts.remove(i);
    }
    if ui.button("Add part").clicked() {
        parts.push(json!({"token":{"type":"literal","text":""},"format":{}}));
    }
}

pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    match d.s("mode").as_str() {
        "template"=>app.run("publication.template.define",d.fields.get("definition").cloned().unwrap_or(Value::Null)),
        "scan"=>{
            let rules=if d.s("style").is_empty(){vec![]}else {vec![json!({"style":d.s("style"),"kind":d.s("kind"),"level":d.fields.get("level")})]};
            app.run("publication.semantic.scan",json!({"tei":d.fields.get("tei"),"styles":rules}))
        }
        "generated"=>app.run("publication.generated.define",json!({"name":d.s("name"),"query":d.fields.get("query"),"template":d.s("template"),"title":d.s("title"),"titleStyle":d.s("titleStyle"),"paragraphStyle":d.s("paragraphStyle"),"levelStyles":d.fields.get("levelStyles")})),
        _=>{
            let state=app.session.doc().map_err(|e|e.to_string())?;
            if d.fields.get("targetRevision")!=Some(&json!(state.revision)) || d.fields.get("targetSelection")!=Some(&serde_json::to_value(&state.selection).map_err(|e|e.to_string())?) {
                return Err("The insertion point changed. Select it and open the reference dialog again.".into());
            }
            let mut p=json!({"query":d.fields.get("query"),"template":d.s("template"),"use":d.s("use"),"separator":d.s("separator"),"suppressOpening":d.fields.get("suppressOpening")});
            let command=if let Some(id)=d.fields.get("id"){p["id"]=id.clone();"publication.reference.edit"}else{"publication.reference.insert"};
            app.run(command,p)
        }
    }
}
