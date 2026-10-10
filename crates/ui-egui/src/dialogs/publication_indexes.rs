//! Named index editing and cached occurrence inspection. The UI submits the same commands as
//! headless clients and never performs index generation merely to open a selector.
use super::{Dialog, Value, json};
use crate::DesignApp;
use designcraft_doc::publication::indexes::{Definition, SortMode};

pub(super) fn open(app: &mut DesignApp, p: &Value, fields: &mut Value) -> Result<(), String> {
    let state = app.session.doc().map_err(|e| e.to_string())?;
    let v = state.doc.publication.clone().ok_or("no publication open")?;
    fields["targetRevision"] = json!(state.revision);
    fields["targetSelection"] = serde_json::to_value(&state.selection).map_err(|e| e.to_string())?;
    fields["paraStyles"] = json!(state.doc.styles.paragraph.iter().map(|s| &s.name).collect::<Vec<_>>());
    fields["charStyles"] = json!(state.doc.styles.character.iter().map(|s| &s.name).collect::<Vec<_>>());
    let name = p["index"].as_str().or(p["name"].as_str()).unwrap_or("");
    let mode = fields["mode"].as_str().unwrap_or("").to_string();
    if mode == "index" {
        let index = v.root.indexes.get(name).cloned().unwrap_or_else(|| Definition { name: "New index".into(), ..Default::default() });
        fields["originalName"] = json!(if v.root.indexes.contains_key(name) { name } else { "" });
        fields["name"] = json!(index.name);
        fields["sorting"] = json!(index.sorting);
        fields["bookOrder"] = json!(index.book_order.join("\n"));
        fields["bookAliases"] = json!(index.book_aliases.iter().map(|(key, value)| json!({"key":key,"value":value})).collect::<Vec<_>>());
        fields["format"] = json!(index.format);
        return Ok(());
    }
    let index = v.root.indexes.get(name).ok_or("Choose an index in the Publication panel first.")?;
    fields["index"] = json!(name);
    fields["sorting"] = json!(index.sorting);
    fields["topics"] = json!(index.topics.values().collect::<Vec<_>>());
    fields["bookOrder"] = json!(index.book_order);
    match mode.as_str() {
        "indexTopic" => {
            fields["topic"]=p["id"].as_u64().and_then(|id|index.topics.get(&id)).map(|t|json!(t)).unwrap_or_else(||json!({"display":"New topic","parent":null,"sort":match index.sorting {
                SortMode::Alphabetical=>json!({"type":"alphabetical","key":""}),SortMode::Numeric=>json!({"type":"numeric","value":1}),
                SortMode::Biblical=>json!({"type":"biblical","book":index.book_order.first().cloned().unwrap_or_default(),"chapter":1,"verse":1,"endChapter":null,"endVerse":null}),
                SortMode::Archival=>json!({"type":"archival","repository":"","collection":"","shelfmark":"","folio":1,"side":"recto"})
            },"see":[],"seeAlso":[]}));
        }
        "indexMark" => {
            if state.selection.text.is_none() {
                return Err("Select text or place the caret before opening Mark occurrence.".into());
            }
            fields["topic"] = p["topic"].as_u64().or_else(|| index.topics.keys().next().copied()).map_or(Value::Null, |id| json!(id));
            fields["suppressPages"] = json!(false);
        }
        "indexPreview" => {
            fields["preview"] = app.run("publication.index.preview", json!({"index":name}))?;
            fields["selected"] = Value::Null;
        }
        _ => return Err("unknown index dialog".into()),
    }
    Ok(())
}
fn text(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str) {
    ui.label(label);
    let mut value = v[key].as_str().unwrap_or("").to_string();
    ui.add(egui::TextEdit::singleline(&mut value).desired_width(380.));
    v[key] = json!(value);
}
fn choice(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, choices: &[(&str, &str)]) {
    let mut selected = v[key].as_str().unwrap_or("").to_string();
    egui::ComboBox::from_id_salt((label, key))
        .selected_text(format!("{label}: {}", choices.iter().find(|(id, _)| *id == selected).map_or(selected.as_str(), |(_, name)| *name)))
        .show_ui(ui, |ui| {
            for (id, name) in choices {
                ui.selectable_value(&mut selected, id.to_string(), *name);
            }
        });
    v[key] = json!(selected);
}
fn number(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, min: i64) {
    let mut n = v[key].as_i64().unwrap_or(min.max(1));
    ui.add(egui::DragValue::new(&mut n).range(min..=1_000_000_000).prefix(format!("{label}: ")));
    v[key] = json!(n);
}
fn check(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str) {
    let mut on = v[key].as_bool().unwrap_or(false);
    ui.checkbox(&mut on, label);
    v[key] = json!(on);
}
fn styles(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, styles: &[Value]) {
    let mut selected = v[key].as_str().unwrap_or("").to_string();
    egui::ComboBox::from_id_salt((label, key)).selected_text(format!("{label}: {selected}")).show_ui(ui, |ui| {
        for name in styles.iter().filter_map(Value::as_str) {
            ui.selectable_value(&mut selected, name.into(), name);
        }
    });
    v[key] = json!(selected);
}
pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    let mode = d.s("mode");
    let mut fields = Value::Object(d.fields.clone());
    egui::ScrollArea::vertical().id_salt("index-dialog").max_height(520.).show(ui, |ui| match mode.as_str() {
        "index" => {
            text(ui, &mut fields, "name", "Index name");
            choice(
                ui,
                &mut fields,
                "sorting",
                "Sorting",
                &[("alphabetical", "Alphabetical"), ("numeric", "Numeric"), ("biblical", "Biblical passages"), ("archival", "Archival references")],
            );
            if fields["sorting"] == "biblical" {
                ui.label("Book order — one canonical name per line");
                let mut order = fields["bookOrder"].as_str().unwrap_or("").to_string();
                ui.add(egui::TextEdit::multiline(&mut order).desired_rows(5).desired_width(380.));
                fields["bookOrder"] = json!(order);
                ui.label("This order is explicit; printed topic names remain unchanged.");
                if let Some(rows) = fields["bookAliases"].as_array_mut() {
                    ui.collapsing("Book key aliases", |ui| {
                        let mut remove = None;
                        for (i, row) in rows.iter_mut().enumerate() {
                            ui.push_id(i, |ui| {
                                ui.horizontal(|ui| {
                                    let mut key = row["key"].as_str().unwrap_or("").to_string();
                                    let mut value = row["value"].as_str().unwrap_or("").to_string();
                                    ui.add(egui::TextEdit::singleline(&mut key).desired_width(150.).hint_text("Alias"));
                                    ui.add(egui::TextEdit::singleline(&mut value).desired_width(150.).hint_text("Canonical book"));
                                    row["key"] = json!(key);
                                    row["value"] = json!(value);
                                    if ui.button("Remove").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            });
                        }
                        if let Some(i) = remove {
                            rows.remove(i);
                        }
                        if ui.button("Add alias").clicked() {
                            rows.push(json!({"key":"","value":""}));
                        }
                    });
                }
            }
            let para = fields["paraStyles"].as_array().cloned().unwrap_or_default();
            let chars = fields["charStyles"].as_array().cloned().unwrap_or_default();
            let format = &mut fields["format"];
            ui.separator();
            text(ui, format, "title", "Printed title");
            check(ui, format, "sectionHeadings", "Alphabetical section headings");
            check(ui, format, "runIn", "Run subtopics into their parent paragraph");
            choice(
                ui,
                format,
                "pageLabels",
                "Page locators",
                &[("global", "Publication page number"), ("local", "Component page label"), ("component", "Component title and label")],
            );
            ui.collapsing("Styles and separators", |ui| {
                styles(ui, format, "titleStyle", "Title", &para);
                styles(ui, format, "sectionStyle", "Section heading", &para);
                for level in 1..=16 {
                    let key = level.to_string();
                    if format["levelStyles"][&key].is_null() {
                        format["levelStyles"][&key] = json!(designcraft_doc::story::BASIC_PARAGRAPH);
                    }
                    styles(ui, &mut format["levelStyles"], &key, &format!("Level {level}"), &para);
                }
                for (key, label) in [("topicFormat", "Topic character style"), ("pageFormat", "Locator character style")] {
                    styles(ui, &mut format[key], "style", label, &chars);
                }
                for (key, label) in [
                    ("followingTopic", "After topic"),
                    ("betweenPages", "Between locators"),
                    ("pageRange", "Range separator"),
                    ("betweenEntries", "Between run-in entries"),
                    ("beforeCrossRef", "Before See / See also"),
                ] {
                    text(ui, format, key, label);
                }
            });
        }
        "indexTopic" => {
            ui.strong(d.s("index"));
            let topics = fields["topics"].as_array().cloned().unwrap_or_default();
            let t = &mut fields["topic"];
            text(ui, t, "display", "Printed topic name");
            let mut parent = t["parent"].as_u64();
            egui::ComboBox::from_id_salt("topic-parent")
                .selected_text(format!(
                    "Parent: {}",
                    topics.iter().find(|x| x["id"].as_u64() == parent).and_then(|x| x["display"].as_str()).unwrap_or("Top level")
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut parent, None, "Top level");
                    for topic in &topics {
                        if topic["id"] != t["id"] {
                            ui.selectable_value(&mut parent, topic["id"].as_u64(), topic["display"].as_str().unwrap_or(""));
                        }
                    }
                });
            t["parent"] = json!(parent);
            let sort = &mut t["sort"];
            match sort["type"].as_str().unwrap_or("") {
                "numeric" => number(ui, sort, "value", "Sort value", i64::MIN),
                "biblical" => {
                    text(ui, sort, "book", "Book key or configured alias");
                    ui.horizontal(|ui| {
                        number(ui, sort, "chapter", "Chapter", 1);
                        number(ui, sort, "verse", "Verse", 1);
                    });
                    let mut range = !sort["endVerse"].is_null();
                    ui.checkbox(&mut range, "Chapter / verse range");
                    if range {
                        ui.horizontal(|ui| {
                            number(ui, sort, "endChapter", "End chapter", 1);
                            number(ui, sort, "endVerse", "End verse", 1);
                        });
                    } else {
                        sort["endChapter"] = Value::Null;
                        sort["endVerse"] = Value::Null;
                    }
                }
                "archival" => {
                    for (key, label) in [("repository", "Repository"), ("collection", "Collection"), ("shelfmark", "Shelfmark")] {
                        text(ui, sort, key, label);
                    }
                    number(ui, sort, "folio", "Folio", 0);
                    choice(ui, sort, "side", "Side", &[("recto", "Recto"), ("verso", "Verso")]);
                }
                _ => text(ui, sort, "key", "Alphabetical sort key (blank uses printed name)"),
            }
            for (key, label) in [("see", "See — alias targets"), ("seeAlso", "See also")] {
                ui.collapsing(label, |ui| {
                    let mut selected = t[key].as_array().cloned().unwrap_or_default();
                    for topic in topics.iter().filter(|x| x["id"] != t["id"]) {
                        let mut on = selected.contains(&topic["id"]);
                        if ui.checkbox(&mut on, topic["display"].as_str().unwrap_or("")).changed() {
                            if on {
                                selected.push(topic["id"].clone());
                            } else {
                                selected.retain(|id| id != &topic["id"]);
                            }
                        }
                    }
                    t[key] = json!(selected);
                });
            }
        }
        "indexMark" => {
            ui.strong(d.s("index"));
            ui.label("Bind the selected passage, or mark the caret's page.");
            let topics = fields["topics"].as_array().cloned().unwrap_or_default();
            let mut selected = fields["topic"].as_u64();
            egui::ComboBox::from_id_salt("mark-topic")
                .selected_text(
                    topics.iter().find(|t| t["id"].as_u64() == selected).and_then(|t| t["display"].as_str()).unwrap_or("Create a topic first"),
                )
                .show_ui(ui, |ui| {
                    for topic in topics {
                        ui.selectable_value(&mut selected, topic["id"].as_u64(), topic["display"].as_str().unwrap_or(""));
                    }
                });
            fields["topic"] = json!(selected);
            check(ui, &mut fields, "suppressPages", "List this occurrence without a page locator");
        }
        "indexPreview" => {
            ui.strong(d.s("index"));
            let p = fields["preview"].clone();
            ui.label(if p["stale"] == true {
                "Locations need updating; this preview has kept the last pagination."
            } else {
                "Locations match the current edition."
            });
            ui.label("Choose an occurrence and press OK to visit its page.");
            let mut selected = fields["selected"].as_u64();
            if let Some(occurrences) = p["occurrences"].as_array() {
                for o in occurrences {
                    let label = if let Some(problem) = o["problem"].as_str() {
                        format!("{} — {problem}", o["excerpt"].as_str().unwrap_or("Removed passage"))
                    } else {
                        format!("Page {} · {}", o["start"]["label"].as_str().unwrap_or("—"), o["excerpt"].as_str().unwrap_or(""))
                    };
                    ui.selectable_value(&mut selected, o["id"].as_u64(), label);
                }
            }
            fields["selected"] = json!(selected);
            ui.separator();
            ui.strong("Index text");
            if let Some(rows) = p["rows"].as_array() {
                for row in rows {
                    let text: String = row["text"]["runs"].as_array().into_iter().flatten().filter_map(|r| r["text"].as_str()).collect();
                    ui.label(text);
                }
            }
        }
        _ => {}
    });
    if let Some(fields) = fields.as_object() {
        d.fields = fields.clone();
    }
}

pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let state = app.session.doc().map_err(|e| e.to_string())?;
    if d.s("mode") != "indexPreview" && d.fields.get("targetRevision") != Some(&json!(state.revision)) {
        return Err("The publication changed while this dialog was open. Reopen it to use the current content.".into());
    }
    match d.s("mode").as_str() {
        "index" => {
            let mut aliases = serde_json::Map::new();
            for row in d.fields.get("bookAliases").and_then(Value::as_array).into_iter().flatten() {
                if let (Some(key), Some(value)) = (row["key"].as_str(), row["value"].as_str()) {
                    aliases.insert(key.into(), json!(value));
                }
            }
            app.run("publication.index.define",json!({"name":if d.s("originalName").is_empty(){d.s("name")}else{d.s("originalName")},"rename":d.s("name"),"sorting":d.s("sorting"),"bookOrder":d.s("bookOrder").lines().filter(|s|!s.trim().is_empty()).collect::<Vec<_>>(),"bookAliases":aliases,"format":d.fields.get("format")}))
        }
        "indexTopic" => {
            let mut p = d.fields.get("topic").cloned().unwrap_or(Value::Null);
            p["index"] = json!(d.s("index"));
            app.run("publication.index.topic", p)
        }
        "indexMark" => {
            if d.fields.get("targetSelection") != Some(&serde_json::to_value(&state.selection).map_err(|e| e.to_string())?) {
                return Err("The selection moved; select the intended passage and reopen Mark occurrence.".into());
            }
            app.run(
                "publication.index.mark",
                json!({"index":d.s("index"),"topic":d.fields.get("topic"),"suppressPages":d.fields.get("suppressPages")}),
            )
        }
        "indexPreview" => match d.fields.get("selected").and_then(Value::as_u64) {
            Some(id) => app.run("publication.index.goTo", json!({"index":d.s("index"),"id":id})),
            None => Ok(Value::Null),
        },
        _ => Err("unknown index operation".into()),
    }
}
