//! Start screen, recent documents, and community links.

use super::{Color32, Content, DesignApp, Sense, Tokens, json, semibold, vec2};

pub fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let r = ui.available_rect_before_wrap();
    ui.painter().rect_filled(r, 0.0, t.panel_darker);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(r.shrink(48.0)).layout(egui::Layout::top_down(if crate::i18n::is_rtl(&app.ui.language) {
            egui::Align::Max
        } else {
            egui::Align::Min
        })),
        |ui| {
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Welcome to DesignCraft")).font(semibold(28.0)).color(t.text_strong),
            ));
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page layout for print and screen — fast, open and scriptable."))
                    .size(15.0)
                    .color(t.text_dim),
            ));
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "  New file  ")).size(14.0).color(Color32::WHITE),
                        ))
                        .fill(t.accent_strong)
                        .corner_radius(16.0)
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("app.newDocumentDialog", json!({}));
                }
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open  ")).size(14.0)))
                            .corner_radius(16.0)
                            .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("app.openDialog", json!({}));
                }
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open sample magazine  ")).size(14.0),
                        ))
                        .corner_radius(16.0)
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("file.newSample", json!({}));
                }
            });
            ui.add_space(20.0);
            community_card(app, ui);
            ui.add_space(24.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Start a new document")).font(semibold(15.0)).color(t.text_strong),
            ));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for p in designcraft_doc::build::PRESETS.iter().take(10) {
                    let (cr, resp) = ui.allocate_exact_size(vec2(120.0, 150.0), Sense::click());
                    let hov = resp.hovered();
                    ui.painter().rect_filled(cr, 6.0, if hov { t.hover } else { t.panel });
                    let k = (80.0 / p.width.max(p.height)) as f32;
                    let pr = egui::Rect::from_center_size(cr.center() - vec2(0.0, 14.0), vec2(p.width as f32 * k, p.height as f32 * k));
                    ui.painter().rect_filled(pr, 0.0, Color32::from_gray(245));
                    ui.painter().text(egui::pos2(cr.center().x, cr.max.y - 26.0), egui::Align2::CENTER_CENTER, p.name, semibold(12.0), t.text_strong);
                    ui.painter().text(
                        egui::pos2(cr.center().x, cr.max.y - 11.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{} × {}", designcraft_geom::format_measure(p.width, p.units), designcraft_geom::format_measure(p.height, p.units)),
                        egui::FontId::proportional(10.0),
                        t.text_dim,
                    );
                    if resp.clicked() {
                        let _ = app.run("file.new", json!({"preset": p.name}));
                    }
                }
            });
        },
    );
    let _ = Content::Unassigned;
}

/// Start screen: the ArtCraft community (Discord first) and project links.
fn community_card(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(t.panel).corner_radius(10.0).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        ui.set_max_width(640.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            crate::about::paint_mark(ui, r, crate::about::BRAND);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Join the ArtCraft community")).font(semibold(14.0)).color(t.text_strong),
                ));
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Get help, share your layouts and shape what we build next."))
                        .size(12.0)
                        .color(t.text_dim),
                ));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (text, id) in [("DesignCraft page", "help.appPage"), ("GitHub", "help.github"), ("getartcraft.com", "help.website")] {
                        let url = designcraft_engine::links::get(&id[5..]).unwrap_or_default();
                        let l = ui
                            .add(egui::Label::new(egui::RichText::new(text).size(12.0).color(t.accent).underline()).sense(Sense::click()))
                            .on_hover_text(url)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if l.clicked() {
                            let _ = app.run(id, json!({}));
                        }
                        ui.add_space(8.0);
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::about::discord_button(ui, crate::i18n::tr(&app.ui.language, "Join our Discord"), vec2(190.0, 38.0)) {
                    let _ = app.run("help.discord", json!({}));
                }
            });
        });
    });
}
