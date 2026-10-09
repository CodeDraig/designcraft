//! Window chrome: application bar (with menus), Control panel, document tabs, status bar, start screen.

mod control_bar;
pub use control_bar::control_bar;
mod navigation;
pub use navigation::{doc_tabs, status_bar};
mod start;
pub use start::start_screen;

use designcraft_doc::{Align, Content};
use designcraft_geom::Unit;
use egui::{Color32, Sense, Stroke, vec2};
use serde_json::json;

use crate::panels::{self, SelInfo};
use crate::theme::{Tokens, semibold};
use crate::widgets::{caption, measure, number};
use crate::{DesignApp, icons};

pub fn app_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let left = if app.integrated_titlebar { 78 } else { 8 };
    // 36 pt bar + 7 pt lower band (InDesign 2026), then a 1 pt border.
    let resp = egui::Panel::top("app_bar")
        .exact_size(43.0)
        .frame(egui::Frame::NONE.fill(t.app_bar).inner_margin(egui::Margin { left, right: 10, top: 0, bottom: 7 }))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let mut menus_end = full.min.x;
            let mut tools_start = full.max.x;
            ui.horizontal_centered(|ui| {
                if icons::button(ui, "home", 24.0, app.session.active().is_none(), crate::i18n::tr(&app.ui.language, "Home")).clicked() {
                    app.session_home();
                }
                ui.add_space(6.0);
                if !app.native_menu {
                    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                    ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
                    ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
                    ui.style_mut().spacing.button_padding = vec2(7.0, 3.0);
                    crate::menus::menu_bar(app, ui);
                }
                menus_end = ui.min_rect().max.x;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    // Search field.
                    let (r, _) = ui.allocate_exact_size(vec2(125.0, 18.0), Sense::click());
                    ui.painter().rect(r, 1.0, t.input, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                    icons::paint(ui.painter(), egui::Rect::from_min_size(r.min + vec2(3.0, 2.0), vec2(14.0, 14.0)), "search", t.icon);
                    crate::rtl::paint(
                        ui.painter(),
                        r.min + vec2(20.0, 9.0),
                        egui::Align2::LEFT_CENTER,
                        crate::i18n::tr(&app.ui.language, "Search"),
                        egui::FontId::proportional(11.0),
                        t.text_dim,
                    );
                    ui.add_space(8.0);
                    let current = app.ui.workspace.clone();
                    let shown = crate::i18n::workspace_name(&app.ui.language, &current);
                    ui.menu_button(crate::rtl::widget(ui, egui::RichText::new(format!("{shown} ▾")).font(semibold(11.5)).color(t.text)), |ui| {
                        let customs: Vec<String> = app.ui.custom_workspaces.iter().map(|w| w.name.clone()).collect();
                        for w in
                            ["Essentials", "Advanced", "Book", "Digital Publishing", "Interactive for PDF", "Printing and Proofing", "Typography"]
                                .into_iter()
                                .map(str::to_string)
                                .chain(customs.iter().cloned())
                        {
                            if ui.selectable_label(w == current, crate::rtl::widget(ui, crate::i18n::workspace_name(&app.ui.language, &w))).clicked()
                            {
                                let _ = app.run("window.workspace", json!({"name": w}));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui
                            .button(crate::rtl::widget(
                                ui,
                                format!("{} {}", crate::i18n::tr(&app.ui.language, "Reset"), crate::i18n::workspace_name(&app.ui.language, &current)),
                            ))
                            .clicked()
                        {
                            let _ = app.run("window.resetWorkspace", json!({}));
                            ui.close();
                        }
                        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Workspace…"))).clicked() {
                            let _ = app.run("window.newWorkspace", json!({}));
                            ui.close();
                        }
                        if !customs.is_empty() {
                            ui.menu_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Workspace")), |ui| {
                                for w in &customs {
                                    if ui.button(w).clicked() {
                                        let _ = app.run("window.deleteWorkspace", json!({"name": w}));
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                    ui.add_space(6.0);
                    if icons::button(ui, "share", 22.0, false, crate::i18n::tr(&app.ui.language, "Share")).clicked() {
                        app.status("Export a PDF, IDML or package to share — no cloud account needed.");
                    }
                    ui.add_space(8.0);
                    // Always one click away: the ArtCraft community Discord.
                    if crate::about::discord_button(ui, "Discord", vec2(78.0, 22.0)) {
                        let _ = app.run("help.discord", json!({}));
                    }
                    tools_start = ui.min_rect().min.x;
                });
            });
            // Centred title.
            let title = match app.session.active() {
                Some(d) => format!("DesignCraft — {}", crate::rtl::isolate(&d.title())),
                None => "DesignCraft".to_string(),
            };
            // Centred between the menus and the right-hand controls; left out when it won't fit.
            let galley = crate::rtl::plain(ui.ctx(), &title, egui::FontId::proportional(11.5), t.text);
            let w = galley.size().x;
            let cx = full.center().x;
            if cx - w / 2.0 > menus_end + 12.0 && cx + w / 2.0 < tools_start - 12.0 {
                ui.painter().galley(egui::pos2(cx - w / 2.0, full.min.y + 18.0 - galley.size().y / 2.0), galley, t.text);
            }
        });
    let r = resp.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - 7.0), r.max), 0.0, t.pasteboard);
    ui.painter().line_segment([egui::pos2(r.min.x, r.max.y), egui::pos2(r.max.x, r.max.y)], Stroke::new(1.0, t.border));
}

impl DesignApp {
    fn session_home(&mut self) {
        self.ui.status = "Home".into();
    }
}
