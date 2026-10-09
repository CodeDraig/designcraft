//! Menu bar presentation and menu visibility.

use super::{DesignApp, Item, activate, checked, menu_enabled, menu_tree, shortcut_of, shortcut_text};

/// The menu bar contents (inside the app bar; macOS uses the native menu instead).
pub fn menu_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    let mut menus = menu_tree();
    if crate::i18n::is_rtl(&lang) {
        menus.reverse();
    }
    for (menu, entries) in menus {
        ui.menu_button(crate::rtl::widget(ui, crate::i18n::tr(&lang, menu)), |ui| {
            if crate::i18n::is_rtl(&lang) {
                ui.set_max_width(320.0);
            }
            ui.with_layout(egui::Layout::top_down(if crate::i18n::is_rtl(&lang) { egui::Align::Max } else { egui::Align::Min }), |ui| {
                ui.set_min_width(240.0);
                let hidden = menu_items(app, ui, &entries, menu);
                if hidden > 0 && !app.ui.show_full_menus {
                    ui.separator();
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show All Menu Items"))).clicked() {
                        app.ui.show_full_menus = true;
                    }
                }
            });
        });
    }
}

/// The key a menu item is hidden by (Edit › Menus).
pub fn menu_key(menu: &str, label: &str) -> String {
    format!("{menu}/{label}")
}

/// Draws a menu's items; returns how many were hidden.
fn menu_items(app: &mut DesignApp, ui: &mut egui::Ui, items: &[Item], path: &str) -> usize {
    let mut hidden = 0;
    for it in items {
        match it {
            Item::Sep => {
                ui.separator();
            }
            Item::Sub(name, children) => {
                let sub = format!("{path}/{name}");
                let shown = crate::i18n::tr(&app.ui.language, name).to_owned();
                ui.menu_button(crate::rtl::widget(ui, shown), |ui| {
                    hidden += menu_items(app, ui, children, &sub);
                });
            }
            Item::Cmd { label, .. } if !app.ui.show_full_menus && app.ui.hidden_menu_items.contains(&menu_key(path, label)) => hidden += 1,
            Item::Cmd { label, id, params, shortcut: _ } => {
                let label = crate::i18n::tr(&app.ui.language, label);
                let text = match checked(app, id, params) {
                    Some(true) => format!("✓ {label}"),
                    Some(false) => format!("   {label}"),
                    None => label.to_owned(),
                };
                let mut b = egui::Button::new(crate::rtl::widget(ui, text));
                if let Some(sc) = shortcut_of(app, id) {
                    b = b.shortcut_text(shortcut_text(&sc));
                }
                if ui.add_enabled(menu_enabled(app, id), b).clicked() {
                    activate(app, id, params);
                    ui.close();
                }
            }
        }
    }
    hidden
}
