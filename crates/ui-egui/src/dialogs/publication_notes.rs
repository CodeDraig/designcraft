//! Note controls share the command surface used by headless clients.
use super::{Dialog, Value, json};
use crate::DesignApp;
use designcraft_doc::publication::annotations::Stream;

pub(super) fn open(app: &mut DesignApp, p: &Value, f: &mut Value) -> Result<(), String> {
    let state = app.session.doc().map_err(|e| e.to_string())?;
    let v = state.doc.publication.clone().ok_or("No publication is open.")?;
    f["targetRevision"] = json!(state.revision);
    f["targetSelection"] = json!(state.selection);
    f["paraStyles"] = json!(state.doc.styles.paragraph.iter().map(|s| &s.name).collect::<Vec<_>>());
    f["charStyles"] = json!(state.doc.styles.character.iter().map(|s| &s.name).collect::<Vec<_>>());
    f["streams"] = json!(v.root.note_streams.keys().collect::<Vec<_>>());
    let name = p["stream"].as_str().unwrap_or_else(|| v.root.note_streams.keys().next().map_or("Notes", String::as_str));
    f["stream"] = json!(name);
    match f["mode"].as_str().unwrap_or("") {
        "noteStream" => {
            f["existingStream"] = json!(v.root.note_streams.contains_key(name));
            f["settings"] = json!(v.root.note_streams.get(name).cloned().unwrap_or_else(|| Stream {
                name: name.into(),
                heading: name.into(),
                ..Default::default()
            }))
        }
        "notePolicy" => f["settings"] = json!(v.root.note_policy(v.edition, name)),
        "noteInsert" => {
            if state.selection.text.is_none() {
                return Err("Place the caret where the note belongs first.".into());
            }
            f["text"] = json!("");
        }
        "notes" => {
            f["report"] = app.run("publication.notes.list", json!({}))?;
            f["action"] = json!("note");
        }
        "apparatus" => {
            f["report"] = app.run("publication.apparatus.list", json!({}))?;
            f["witness"] = f["report"]["witness"].as_str().map_or(json!(""), |w| json!(w));
        }
        "apparatusRecover" => {
            f["report"] = app.run("publication.apparatus.recover", json!({}))?;
            f["apply"] = json!(false);
        }
        _ => return Err("Unknown note dialog.".into()),
    }
    Ok(())
}
fn text(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str) {
    ui.label(label);
    let mut s = v[key].as_str().unwrap_or("").to_owned();
    ui.add(egui::TextEdit::singleline(&mut s).desired_width(360.));
    v[key] = json!(s);
}
fn choice(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, items: &[(&str, &str)]) {
    let mut s = v[key].as_str().unwrap_or("").to_owned();
    egui::ComboBox::from_id_salt((label, key))
        .selected_text(format!("{label}: {}", items.iter().find(|(id, _)| *id == s).map_or(s.as_str(), |(_, name)| *name)))
        .show_ui(ui, |ui| {
            for (id, name) in items {
                ui.selectable_value(&mut s, (*id).into(), *name);
            }
        });
    v[key] = json!(s);
}
fn number(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, range: std::ops::RangeInclusive<f64>) {
    let mut n = v[key].as_f64().unwrap_or(1.);
    ui.add(egui::DragValue::new(&mut n).range(range).prefix(format!("{label}: ")));
    v[key] = if matches!(key, "startAt" | "columns") { json!(n as u64) } else { json!(n) };
}
fn check(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str) {
    let mut on = v[key] == true;
    ui.checkbox(&mut on, label);
    v[key] = json!(on);
}
fn styles(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, list: &[Value]) {
    let choices: Vec<_> = list.iter().filter_map(Value::as_str).map(|s| (s, s)).collect();
    choice(ui, v, key, label, &choices);
}
const PLACEMENTS: &[(&str, &str)] = &[
    ("footnote", "Footnotes"),
    ("componentEnd", "Chapter endnotes"),
    ("publicationEnd", "Publication endnotes"),
    ("sidenote", "Sidenotes"),
    ("omit", "Omit in this edition"),
];

pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    let mut f = Value::Object(d.fields.clone());
    let mode = d.s("mode");
    egui::ScrollArea::vertical().id_salt("publication-notes").max_height(540.).show(ui, |ui| match mode.as_str() {
        "noteStream" => {
            let para = f["paraStyles"].as_array().cloned().unwrap_or_default();
            let chars = f["charStyles"].as_array().cloned().unwrap_or_default();
            let v = &mut f["settings"];
            ui.add_enabled_ui(d.fields.get("existingStream") != Some(&json!(true)), |ui| text(ui, v, "name", "Stream name"));
            choice(
                ui,
                v,
                "style",
                "Numbering",
                &[
                    ("arabic", "1, 2, 3"),
                    ("lowerRoman", "i, ii, iii"),
                    ("upperRoman", "I, II, III"),
                    ("lowerLetters", "a, b, c"),
                    ("upperLetters", "A, B, C"),
                    ("symbols", "Symbols"),
                ],
            );
            number(ui, v, "startAt", "Start at", 1. ..=1_000_000.);
            choice(
                ui,
                v,
                "restart",
                "Restart",
                &[
                    ("publication", "Whole publication"),
                    ("component", "Each chapter"),
                    ("page", "Each page"),
                    ("spread", "Each spread"),
                    ("section", "Each page section"),
                ],
            );
            ui.horizontal(|ui| {
                text(ui, v, "prefix", "Prefix");
                text(ui, v, "suffix", "Suffix");
            });
            choice(
                ui,
                v,
                "affixIn",
                "Use prefix / suffix",
                &[("none", "Neither"), ("reference", "Callout"), ("text", "Note text"), ("both", "Both")],
            );
            choice(ui, v, "refPosition", "Callout position", &[("superscript", "Superscript"), ("normal", "Normal"), ("subscript", "Subscript")]);
            styles(ui, v, "refCharStyle", "Callout character style", &chars);
            check(ui, v, "preserveParagraphStyles", "Keep each note’s existing paragraph styles");
            styles(ui, v, "paraStyle", "Note paragraph style", &para);
            text(ui, v, "heading", "Endnote heading");
            styles(ui, v, "headingStyle", "Heading paragraph style", &para);
            text(ui, v, "separator", "Space or punctuation after number");
            choice(ui, v, "defaultPlacement", "Default placement", PLACEMENTS);
        }
        "notePolicy" => {
            ui.strong(format!("{} — current edition", d.s("stream")));
            let v = &mut f["settings"];
            choice(ui, v, "placement", "Placement", PLACEMENTS);
            ui.horizontal(|ui| {
                number(ui, v, "columns", "Note columns", 1. ..=12.);
                number(ui, v, "gutter", "Gutter (pt)", 0. ..=1000.);
            });
            check(ui, v, "runIn", "Run short notes together in one paragraph");
            text(ui, v, "betweenNotes", "Separator between run-in notes");
            number(ui, v, "maxFraction", "Maximum page fraction", 0.05..=0.9);
            ui.horizontal(|ui| {
                number(ui, v, "spaceBefore", "Before area (pt)", 0. ..=1000.);
                number(ui, v, "spaceBetween", "Between notes (pt)", 0. ..=1000.);
            });
            if v["placement"] == "sidenote" {
                number(ui, v, "sideWidth", "Sidenote width (pt)", 12. ..=2000.);
                number(ui, v, "sideGap", "Gap (pt)", 0. ..=1000.);
                choice(ui, v, "side", "Side", &[("outer", "Outer"), ("left", "Left"), ("right", "Right")]);
                ui.label("Sidenotes use a lane inside the text frame. Overflow is reported before applying the policy.");
            }
        }
        "noteInsert" => {
            let list = f["streams"].as_array().cloned().unwrap_or_default();
            styles(ui, &mut f, "stream", "Stream", &list);
            ui.label("Note text");
            let mut text = f["text"].as_str().unwrap_or("").to_owned();
            ui.add(egui::TextEdit::multiline(&mut text).desired_width(440.).desired_rows(8));
            f["text"] = json!(text);
        }
        "notes" => {
            ui.label("Select a note, then choose what to open.");
            choice(ui, &mut f, "action", "Open", &[("note", "Printed note"), ("callout", "Callout"), ("edit", "Note editor")]);
            let notes = f["report"]["notes"].as_array().cloned().unwrap_or_default();
            let mut selected = f["selected"].as_u64();
            for n in notes {
                let text = n["text"]["text"].as_str().unwrap_or("");
                let summary = designcraft_doc::xref::clean(text).chars().take(90).collect::<String>();
                ui.selectable_value(
                    &mut selected,
                    n["id"].as_u64(),
                    format!("{} {} · {}", n["stream"].as_str().unwrap_or(""), n["label"].as_str().unwrap_or("?"), summary),
                );
            }
            f["selected"] = json!(selected);
        }
        "apparatus" => {
            ui.label("Choose the witness displayed by this edition. Edited passages remain as edited.");
            let entries = f["report"]["entries"].as_array().cloned().unwrap_or_default();
            let mut witnesses = std::collections::BTreeSet::new();
            for e in &entries {
                for r in e["readings"].as_array().into_iter().flatten() {
                    for w in r["witnesses"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                        witnesses.insert(w.to_owned());
                    }
                }
            }
            let mut choices = vec![("", "Imported reading")];
            choices.extend(witnesses.iter().map(|s| (s.as_str(), s.as_str())));
            choice(ui, &mut f, "witness", "Witness", &choices);
            for e in entries {
                ui.collapsing(format!("{} · {}", e["key"].as_str().unwrap_or("Apparatus"), e["display"].as_str().unwrap_or("")), |ui| {
                    for r in e["readings"].as_array().into_iter().flatten() {
                        let labels = r["witnessLabels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
                        ui.label(format!(
                            "{} [{}]: {}",
                            if r["lemma"] == true { "Lemma" } else { "Reading" },
                            labels,
                            designcraft_doc::xref::clean(r["text"]["text"].as_str().unwrap_or(""))
                        ));
                    }
                });
            }
        }
        "apparatusRecover" => {
            ui.label("Recover reading structure from the preserved TEI for this chapter. Existing passage and note text stay intact.");
            for c in f["report"]["candidates"].as_array().into_iter().flatten() {
                ui.label(format!(
                    "{}: {}",
                    c["key"].as_str().unwrap_or("Apparatus"),
                    c["problem"].as_str().unwrap_or("Matched; ready to attach readings")
                ));
            }
            check(ui, &mut f, "apply", "Attach the unambiguous matches when I press OK");
        }
        _ => {}
    });
    if let Some(fields) = f.as_object() {
        d.fields = fields.clone();
    }
}

pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let state = app.session.doc().map_err(|e| e.to_string())?;
    if d.fields.get("targetRevision").and_then(Value::as_u64) != Some(state.revision) {
        return Err("The publication changed; reopen these controls to review the current values.".into());
    }
    match d.s("mode").as_str() {
        "noteStream" => app.run("publication.notes.stream", d.fields.get("settings").cloned().unwrap_or(Value::Null)),
        "notePolicy" => {
            let mut p = d.fields.get("settings").cloned().unwrap_or(Value::Null);
            p["stream"] = json!(d.s("stream"));
            app.run("publication.notes.policy", p)
        }
        "noteInsert" => {
            if d.fields.get("targetSelection") != Some(&json!(state.selection)) {
                return Err("The caret moved; reopen Insert Note at the intended position.".into());
            }
            app.run("publication.note.insert", json!({"stream":d.s("stream"),"text":d.s("text")}))
        }
        "notes" => {
            let Some(id) = d.fields.get("selected").and_then(Value::as_u64) else { return Ok(Value::Null) };
            if d.s("action") == "edit" {
                app.run("app.storyEditor", json!({"note":id}))
            } else {
                app.run("publication.note.goTo", json!({"id":id,"target":d.s("action")}))
            }
        }
        "apparatus" => app.run("publication.apparatus.witness", json!({"witness":if d.s("witness").is_empty(){None}else{Some(d.s("witness"))}})),
        "apparatusRecover" => app.run("publication.apparatus.recover", json!({"apply":d.fields.get("apply")})),
        _ => Err("Unknown note operation.".into()),
    }
}
