//! Publication navigation and chapter operations. Mutations use the same commands as agents.
use crate::{DesignApp, theme::Tokens};
use designcraft_doc::{ComponentId, publication::ComponentRole};
use serde_json::json;

fn run(app: &mut DesignApp, command: &str, params: serde_json::Value) {
    if let Err(error) = app.run(command, params) {
        app.status(error);
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(view) = app.session.active().and_then(|s| s.doc.publication.clone()) else {
        ui.label("A publication holds chapters and editions in one file.");
        if ui.button("New publication…").clicked() {
            run(app, "app.publicationDialog", json!({}));
        }
        if ui.add_enabled(app.session.active().is_some(), egui::Button::new("Convert current document")).clicked() {
            run(app, "publication.convert", json!({}));
        }
        if ui.button("Convert a Book…").clicked() {
            run(app, "app.publicationBook", json!({}));
        }
        return;
    };
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(&view.root.title).strong());
    let name = view.root.editions.get(&view.edition).map_or("", |e| e.name.as_str());
    egui::ComboBox::from_id_salt("publication_edition").selected_text(name).show_ui(ui, |ui| {
        for e in view.root.editions.values() {
            if ui.selectable_label(e.id == view.edition, &e.name).clicked() {
                run(app, "publication.activate", json!({"edition":e.id}));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Add chapter").clicked() {
            run(app, "publication.addComponent", json!({"title":format!("Chapter {}",view.root.components.len()+1)}));
        }
        if ui.button("Import…").clicked() {
            run(app, "app.publicationImport", json!({}));
        }
        if ui.button("New edition…").clicked() {
            run(app, "app.publicationDialog", json!({"mode":"edition"}));
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in [("editionSettings", "Edition settings…"), ("export", "Export edition…")] {
            if ui.button(label).clicked() {
                run(app, "app.publicationDialog", json!({"mode":mode}));
            }
        }
    });
    ui.separator();
    ui.collapsing("Review and proofs", |ui| {
        ui.label(format!("Tracking: {} · {}", if view.root.review.tracking { "on" } else { "off" }, view.root.review.author()));
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in
                [("review", "Review changes…"), ("comment", "Comment…"), ("proof", "Capture / approve proof…"), ("proofCompare", "Compare proofs…")]
            {
                if ui.button(label).clicked() {
                    run(app, "app.publicationDialog", json!({"mode":mode}));
                }
            }
            if ui.button("Compare manuscript…").clicked() {
                run(app, "app.publicationManuscript", json!({}));
            }
        });
    });
    if ui.button("Layout work and verification…").clicked() {
        run(app, "app.publicationDialog", json!({"mode":"layout"}));
    }
    // Iterative traversal avoids unbounded recursion for deeply nested imported structures.
    let mut pending: Vec<_> = view.root.roots.iter().rev().map(|id| (*id, 0usize)).collect();
    while let Some((id, depth)) = pending.pop() {
        let Some(c) = view.root.components.get(&id) else { continue };
        pending.extend(c.children.iter().rev().map(|id| (*id, depth + 1)));
        ui.push_id(id.0, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(depth.min(8) as f32 * 10.0);
                let response = ui.selectable_label(id == view.component, &c.title);
                if response.clicked() {
                    run(app, "publication.activate", json!({"component":id}));
                }
                response.context_menu(|ui| {
                    if ui.button("Properties…").clicked() {
                        run(app, "app.publicationDialog", json!({"mode":"component","component":id}));
                        ui.close();
                    }
                    let parent = view.root.components.values().find(|p| p.children.contains(&id)).map(|p| p.id);
                    let siblings = parent.and_then(|id| view.root.components.get(&id)).map_or(&view.root.roots, |c| &c.children);
                    let position = siblings.iter().position(|c| *c == id).unwrap_or(0);
                    if ui.add_enabled(position > 0, egui::Button::new("Move earlier")).clicked() {
                        move_component(app, id, parent, position.saturating_sub(1));
                        ui.close();
                    }
                    if ui.add_enabled(position + 1 < siblings.len(), egui::Button::new("Move later")).clicked() {
                        move_component(app, id, parent, position + 1);
                        ui.close();
                    }
                });
            });
            let role = match c.role {
                ComponentRole::FrontMatter => "Front matter",
                ComponentRole::Chapter => "Chapter",
                ComponentRole::BackMatter => "Back matter",
            };
            let pages = view.root.editions.get(&view.edition).and_then(|e| e.components.get(&id)).map_or(0, |l| l.page_count());
            ui.label(egui::RichText::new(format!("{role} · {pages} pages")).small().color(t.text_dim));
        });
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in [("settings", "Metadata…"), ("styles", "Synchronize styles…"), ("numbering", "Numbering…"), ("mark", "Mark passage…")]
        {
            if ui.button(label).clicked() {
                run(app, "app.publicationDialog", json!({"mode":mode}));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in
            [("reference", "Reference…"), ("generated", "Generated section…"), ("template", "Templates…"), ("scan", "Register destinations…")]
        {
            if ui.button(label).clicked() {
                run(app, "app.publicationDialog", json!({"mode":mode}));
            }
        }
    });
    if let Some(edition) = view.root.editions.get(&view.edition) {
        let refs = &edition.references;
        let stale = (!view.root.references.fields.is_empty() || !view.root.references.sections.is_empty() || !view.root.indexes.is_empty())
            && refs.source_revision != Some(view.root.revision);
        ui.label(if stale { "Generated content: update needed" } else { "Generated content: current" });
        ui.horizontal(|ui| {
            let mut deferred = refs.mode == designcraft_doc::publication::references::UpdateMode::Deferred;
            if ui.checkbox(&mut deferred, "Defer updates").changed() {
                run(app, "publication.updateMode", json!({"mode":if deferred {"deferred"}else{"live"}}));
            }
            if ui.button("Update now").clicked() {
                run(app, "publication.update", json!({}));
            }
        });
        for message in &refs.diagnostics {
            ui.label(egui::RichText::new(message).color(t.text_strong));
        }
        ui.collapsing("Resolved destinations", |ui| {
            if let Ok(index) = designcraft_doc::publication::references::SemanticIndex::build(&view.root) {
                for entity in index.entries {
                    let page = refs.locations.get(&entity.id).map(|l| l.label.as_str()).unwrap_or("unplaced");
                    if ui.button(format!("{} · {page}", entity.title)).clicked() {
                        run(app, "publication.reference.goTo", json!({"object":entity.id}));
                    }
                }
            }
        });
    }
    ui.separator();
    if ui.button("New index…").clicked() {
        run(app, "app.publicationDialog", json!({"mode":"index"}));
    }
    for index in view.root.indexes.values() {
        ui.collapsing(format!("Index: {}", index.name), |ui| {
            ui.label(format!("{} topics · {} occurrences", index.topics.len(), index.occurrences.len()));
            ui.horizontal_wrapped(|ui| {
                for (mode, label) in
                    [("index", "Settings…"), ("indexTopic", "New topic…"), ("indexMark", "Mark occurrence…"), ("indexPreview", "Preview…")]
                {
                    if ui.button(label).clicked() {
                        run(app, "app.publicationDialog", json!({"mode":mode,"index":index.name}));
                    }
                }
                if ui.button("Generate / update").clicked() {
                    run(app, "publication.index.generate", json!({"index":index.name}));
                }
            });
            ui.collapsing("Topics", |ui| {
                for topic in index.topics.values() {
                    if ui.button(&topic.display).clicked() {
                        run(app, "app.publicationDialog", json!({"mode":"indexTopic","index":index.name,"id":topic.id}));
                    }
                }
            });
            if index.destination.is_some() && ui.button("Detach editable output").clicked() {
                run(app, "publication.index.detach", json!({"index":index.name}));
            }
        });
    }
    ui.collapsing("Notes and critical readings", |ui| {
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                ("noteStream", "New stream…"),
                ("noteInsert", "Insert note…"),
                ("notes", "Browse notes…"),
                ("apparatus", "Witnesses…"),
                ("apparatusRecover", "Recover readings…"),
            ] {
                if ui.button(label).clicked() {
                    run(app, "app.publicationDialog", json!({"mode":mode,"stream":if mode=="noteStream"{Some("New stream")}else{None}}));
                }
            }
        });
        for name in view.root.note_streams.keys() {
            ui.horizontal_wrapped(|ui| {
                ui.label(name);
                if ui.button("Numbering / style…").clicked() {
                    run(app, "app.publicationDialog", json!({"mode":"noteStream","stream":name}));
                }
                if ui.button("Placement…").clicked() {
                    run(app, "app.publicationDialog", json!({"mode":"notePolicy","stream":name}));
                }
            });
        }
    });
    ui.collapsing("Reference definitions", |ui| {
        for f in view.root.references.fields.values() {
            if ui.button(format!("Field {} · {}", f.id, f.template)).clicked() {
                run(app, "app.publicationDialog", json!({"mode":"reference","id":f.id}));
            }
        }
        for name in view.root.references.templates.keys() {
            if ui.button(format!("Template: {name}")).clicked() {
                run(app, "app.publicationDialog", json!({"mode":"template","name":name}));
            }
        }
        for name in view.root.references.sections.keys() {
            if ui.button(format!("Section: {name}")).clicked() {
                run(app, "app.publicationDialog", json!({"mode":"generated","name":name}));
            }
        }
    });
    for name in view.root.numbering.keys() {
        if ui.button(format!("Sequence: {name}…")).clicked() {
            run(app, "app.publicationDialog", json!({"mode":"numbering","name":name}));
        }
        let can_insert = app.session.active().is_some_and(|s| s.selection.text.is_some());
        match view.root.numbers(name) {
            Ok(numbers) => {
                ui.collapsing(format!("Numbers in {name}"), |ui| {
                    for number in numbers.iter().filter(|n| n.object.is_some() || n.component == view.component) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(if number.resolved {
                                format!("{} {}", number.label, number.title)
                            } else {
                                format!("Unresolved: {}", number.title)
                            });
                            if ui.add_enabled(can_insert && number.resolved, egui::Button::new("Insert")).clicked() {
                                run(app, "publication.numbering.insert", json!({"name":name,"object":number.object}));
                            }
                        });
                    }
                });
            }
            Err(error) => {
                ui.label(egui::RichText::new(format!("Numbering error: {error}")).color(t.text_strong));
            }
        }
    }
    ui.collapsing("Insert shared value", |ui| {
        let can_insert = app.session.active().is_some_and(|s| s.selection.text.is_some());
        for (metadata, map) in [(true, &view.root.metadata), (false, &view.root.values)] {
            for (key, value) in map {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{key}: {value}"));
                    if ui.add_enabled(can_insert, egui::Button::new("Insert")).clicked() {
                        run(app, "publication.value.insert", json!({"key":key,"metadata":metadata}));
                    }
                });
            }
        }
    });
    ui.separator();
    ui.label(egui::RichText::new("Text edits are shared by every edition. Save keeps the whole publication.").small().color(t.text_dim));
}

fn move_component(app: &mut DesignApp, id: ComponentId, parent: Option<ComponentId>, at: usize) {
    run(app, "publication.moveComponent", json!({"component":id,"parent":parent,"at":at}));
}
