//! Publication settings and reviewed synchronization. Dialog fields are presentation state;
//! all validation and document edits run through the engine's public commands.
use super::{Dialog, Value, combo, json, text_field};
use crate::DesignApp;

pub(super) fn open(app: &mut DesignApp, p: &Value, fields: &mut Value) -> Result<(), String> {
    let v = app.session.doc().map_err(|e| e.to_string())?.doc.publication.clone().ok_or("no publication open")?;
    match fields["mode"].as_str().unwrap_or("") {
        "settings" => {
            fields["title"] = json!(v.root.title);
            for (key, map) in [("metadata", &v.root.metadata), ("values", &v.root.values)] {
                fields[key] = json!(map.iter().map(|(key, value)| json!({"key":key,"value":value})).collect::<Vec<_>>());
                fields[format!("original_{key}")] = json!(map.keys().collect::<Vec<_>>());
            }
        }
        "styles" => {
            let preview = app.run("publication.styles.preview", p.clone())?;
            fields["preview"] = preview["preview"].clone();
            fields["rows"] = preview["rows"].clone();
            if let Some(rows) = fields["rows"].as_array_mut() {
                for row in rows {
                    row["selected"] = json!(row["changed"] == true && row["overridden"] == false);
                }
            }
            fields["replaceOverrides"] = json!(false);
        }
        "numbering" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            fields["sequence"]=v.root.numbering.get(name).map(serde_json::to_value).transpose().map_err(|e|e.to_string())?
                .unwrap_or_else(||json!({"name":"Chapters","kind":"chapter","style":"arabic","start":1,"prefix":"","suffix":"","restart":"continuous","role":null,"scope":null,"level":null,"restartAt":{}}));
            fields["components"] = json!(
                v.root
                    .ordered_components()
                    .map_err(|e| e.to_string())?
                    .iter()
                    .filter_map(|id| v.root.components.get(id))
                    .map(|c| json!({"id":c.id,"title":c.title}))
                    .collect::<Vec<_>>()
            );
        }
        "mark" => {
            if app.session.doc().map_err(|e| e.to_string())?.selection.text.is_none() {
                return Err("Select a heading, figure caption or table caption first.".into());
            }
            fields["kind"] = json!("heading");
            fields["title"] = json!("");
            fields["level"] = json!(1);
            let state = app.session.doc().map_err(|e| e.to_string())?;
            fields["targetSelection"] = serde_json::to_value(&state.selection).map_err(|e| e.to_string())?;
            fields["targetRevision"] = json!(state.revision);
        }
        _ => return Err("unknown publication settings dialog".into()),
    }
    Ok(())
}

pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    match d.s("mode").as_str() {
        "settings" => {
            ui.label("Publication title");
            text_field(ui, d, "title", 360.0);
            for (key, label) in [("metadata", "Metadata"), ("values", "Reusable values")] {
                ui.separator();
                ui.strong(label);
                if let Some(rows) = d.fields.get_mut(key).and_then(Value::as_array_mut) {
                    let mut remove = None;
                    for (index, row) in rows.iter_mut().enumerate() {
                        ui.push_id((key, index), |ui| {
                            ui.horizontal(|ui| {
                                let mut key = row["key"].as_str().unwrap_or("").to_string();
                                let mut value = row["value"].as_str().unwrap_or("").to_string();
                                ui.add(egui::TextEdit::singleline(&mut key).desired_width(130.0).hint_text("Name"));
                                ui.add(egui::TextEdit::singleline(&mut value).desired_width(240.0).hint_text("Value"));
                                row["key"] = json!(key);
                                row["value"] = json!(value);
                                if ui.button("Remove").clicked() {
                                    remove = Some(index);
                                }
                            })
                        });
                    }
                    if let Some(index) = remove {
                        rows.remove(index);
                    }
                    if ui.button(format!("Add {}", label.to_lowercase())).clicked() {
                        rows.push(json!({"key":"","value":""}));
                    }
                }
            }
            ui.label("Insert these with a Publication Value text variable. Changing a value updates every edition.");
        }
        "styles" => styles(ui, d),
        "numbering" => numbering(ui, d),
        "mark" => {
            ui.label("Number this paragraph as");
            combo(ui, d, "kind", &[("heading", "Heading"), ("figure", "Figure"), ("table", "Table")]);
            ui.label("Display title (leave blank to follow the paragraph text)");
            text_field(ui, d, "title", 360.0);
            let mut level = d.fields.get("level").and_then(Value::as_u64).unwrap_or(1) as u8;
            ui.horizontal(|ui| {
                ui.label("Heading level");
                ui.add(egui::DragValue::new(&mut level).range(1..=9));
            });
            d.fields.insert("level".into(), json!(level));
        }
        _ => {}
    }
}

fn styles(ui: &mut egui::Ui, d: &mut Dialog) {
    ui.label("Use the current chapter as the style source. Select definitions to adopt in the other chapters.");
    let mut replace = d.fields.get("replaceOverrides").and_then(Value::as_bool).unwrap_or(false);
    ui.checkbox(&mut replace, "Replace selected chapter overrides");
    d.fields.insert("replaceOverrides".into(), json!(replace));
    let mut identical = d.fields.get("showIdentical").and_then(Value::as_bool).unwrap_or(false);
    ui.checkbox(&mut identical, "Show identical definitions (select to keep synchronized)");
    d.fields.insert("showIdentical".into(), json!(identical));
    let Some(rows) = d.fields.get_mut("rows").and_then(Value::as_array_mut) else { return };
    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        for row in rows.iter_mut().filter(|r| identical || r["changed"] == true) {
            let index = row["row"].as_u64().unwrap_or(0);
            ui.push_id(index, |ui| {
                let label = format!(
                    "{} — {}{}",
                    row["componentTitle"].as_str().unwrap_or(""),
                    row["key"].as_str().unwrap_or(""),
                    if row["overridden"] == true { " (chapter override)" } else { "" }
                );
                let mut selected = row["selected"].as_bool().unwrap_or(false);
                ui.checkbox(&mut selected, label);
                row["selected"] = json!(selected);
                ui.collapsing("Changes", |ui| {
                    let before = &row["before"]["definition"];
                    let after = &row["after"]["definition"];
                    show_difference(ui, "", before, after);
                    if let Some(deps) = row["dependencies"].as_array()
                        && !deps.is_empty()
                    {
                        ui.label(format!("Uses: {}", deps.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
                    }
                });
            });
        }
    });
}

fn show_difference(ui: &mut egui::Ui, prefix: &str, before: &Value, after: &Value) {
    // Bounded depth: style definitions have a fixed schema. A stack also handles nested attrs.
    let mut pending = vec![(prefix.to_string(), before, after)];
    while let Some((key, a, b)) = pending.pop() {
        if a == b {
            continue;
        }
        if let Some(fields) = b.as_object() {
            for (name, value) in fields.iter().rev() {
                pending.push((if key.is_empty() { name.clone() } else { format!("{key}.{name}") }, a.get(name).unwrap_or(&Value::Null), value));
            }
            if let Some(old) = a.as_object() {
                for name in old.keys().filter(|name| !fields.contains_key(*name)) {
                    ui.label(format!("{key}.{name}: cleared"));
                }
            }
        } else {
            let text = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| if v.is_null() { "unset".into() } else { v.to_string() });
            ui.label(format!("{key}: {} → {}", text(a), text(b)));
        }
    }
}

fn numbering(ui: &mut egui::Ui, d: &mut Dialog) {
    let value = d.fields.get("sequence").cloned().unwrap_or(json!({}));
    let mut fields = Dialog::new("numberingFields", value);
    for (key, label) in [("name", "Sequence name"), ("prefix", "Prefix"), ("suffix", "Suffix")] {
        ui.label(label);
        text_field(ui, &mut fields, key, 280.0);
    }
    ui.label("Objects");
    combo(ui, &mut fields, "kind", &[("chapter", "Chapters"), ("heading", "Headings"), ("figure", "Figures"), ("table", "Tables")]);
    ui.label("Numerals");
    combo(
        ui,
        &mut fields,
        "style",
        &[
            ("arabic", "1, 2, 3"),
            ("lowerRoman", "i, ii, iii"),
            ("upperRoman", "I, II, III"),
            ("upperLetters", "A, B, C"),
            ("lowerLetters", "a, b, c"),
            ("arabicLeadingZero", "01, 02, 03"),
            ("symbols", "*, †, ‡"),
        ],
    );
    ui.label("Restart");
    combo(
        ui,
        &mut fields,
        "restart",
        &[
            ("continuous", "Continue throughout"),
            ("component", "Each chapter"),
            ("parent", "Each parent division"),
            ("role", "Front matter / chapters / back matter"),
        ],
    );
    let mut start = fields.fields.get("start").and_then(Value::as_u64).unwrap_or(1) as u32;
    ui.horizontal(|ui| {
        ui.label("Start at");
        ui.add(egui::DragValue::new(&mut start).range(1..=100000));
    });
    fields.fields.insert("start".into(), json!(start));
    if fields.fields.get("role").is_none_or(Value::is_null) {
        fields.fields.insert("role".into(), json!("all"));
    }
    ui.label("Part of the publication");
    combo(
        ui,
        &mut fields,
        "role",
        &[("all", "All components"), ("frontMatter", "Front matter"), ("chapter", "Chapters"), ("backMatter", "Back matter")],
    );
    if fields.s("role") == "all" {
        fields.fields.insert("role".into(), Value::Null);
    }
    let components = d.fields.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut scope = fields.fields.get("scope").and_then(Value::as_u64);
    let label = scope.and_then(|id| components.iter().find(|c| c["id"] == id)).and_then(|c| c["title"].as_str()).unwrap_or("Whole publication");
    ui.label("Scope");
    egui::ComboBox::from_id_salt("numberingScope").selected_text(label).show_ui(ui, |ui| {
        ui.selectable_value(&mut scope, None, "Whole publication");
        for c in &components {
            if let Some(id) = c["id"].as_u64() {
                ui.selectable_value(&mut scope, Some(id), c["title"].as_str().unwrap_or(""));
            }
        }
    });
    fields.fields.insert("scope".into(), json!(scope));
    if fields.s("kind") == "heading" {
        let mut level = fields.fields.get("level").and_then(Value::as_u64).unwrap_or(0) as u8;
        ui.horizontal(|ui| {
            ui.label("Heading level (0 = all)");
            ui.add(egui::DragValue::new(&mut level).range(0..=9));
        });
        fields.fields.insert("level".into(), if level == 0 { Value::Null } else { json!(level) });
    }
    ui.collapsing("Explicit chapter restarts", |ui| {
        let value = fields.fields.entry("restartAt").or_insert(json!({}));
        if let Some(restarts) = value.as_object_mut() {
            for c in &components {
                let Some(id) = c["id"].as_u64() else { continue };
                let key = id.to_string();
                let mut enabled = restarts.contains_key(&key);
                let mut start = restarts.get(&key).and_then(Value::as_u64).unwrap_or(1) as u32;
                ui.horizontal(|ui| {
                    ui.checkbox(&mut enabled, c["title"].as_str().unwrap_or(""));
                    if enabled {
                        ui.label("starts at");
                        ui.add(egui::DragValue::new(&mut start).range(1..=100000));
                    }
                });
                if enabled {
                    restarts.insert(key, json!(start));
                } else {
                    restarts.remove(&key);
                }
            }
        }
    });
    ui.label("A separate front-matter sequence can use Roman numerals. Heading, figure and table sequences follow marked paragraphs.");
    d.fields.insert("sequence".into(), Value::Object(fields.fields));
}

pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    match d.s("mode").as_str() {
        "settings"=> {
            let mut params=json!({"title":d.s("title")});
            for key in ["metadata","values"] {
                let mut map=serde_json::Map::new();
                for old in d.fields.get(&format!("original_{key}")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {map.insert(old.into(),Value::Null);}
                let mut seen=std::collections::BTreeSet::new();
                for row in d.fields.get(key).and_then(Value::as_array).into_iter().flatten() {
                    let name=row["key"].as_str().unwrap_or("");
                    if !seen.insert(name) {return Err(format!("Duplicate {key} name: {name}"));}
                    map.insert(name.into(),row["value"].clone());
                }
                params[key]=Value::Object(map);
            }
            app.run("publication.metadata",params)
        }
        "styles"=>app.run("publication.styles.apply",json!({"preview":d.fields.get("preview"),"replaceOverrides":d.fields.get("replaceOverrides"),
            "rows":d.fields.get("rows").and_then(Value::as_array).into_iter().flatten().filter(|r|r["selected"]==true).map(|r|r["row"].clone()).collect::<Vec<_>>() })),
        "numbering"=>app.run("publication.numbering.define",d.fields.get("sequence").cloned().unwrap_or(Value::Null)),
        "mark"=> {
            let state=app.session.doc().map_err(|e|e.to_string())?;
            if d.fields.get("targetRevision")!=Some(&json!(state.revision)) || d.fields.get("targetSelection")!=Some(&serde_json::to_value(&state.selection).map_err(|e|e.to_string())?) {
                return Err("The selected passage changed. Select it and open the mark dialog again.".into());
            }
            let mut p=json!({"kind":d.s("kind"),"level":d.fields.get("level")});
            if !d.s("title").trim().is_empty() {p["title"]=json!(d.s("title"));}
            app.run("publication.numbering.mark",p)
        }
        _=>Err("unknown publication settings dialog".into()),
    }
}
