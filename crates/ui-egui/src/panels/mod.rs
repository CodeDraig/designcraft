//! Panels and shared panel helpers.

mod selection;
pub use selection::{SelInfo, preflight_errors, sel_info, sel_tint, text_attrs};
mod font_picker;
pub use font_picker::{font_family_picker, font_label, font_menu, font_menu_rows, font_scope, font_style_picker, fonts};

pub mod conditions;
pub mod datamerge;
pub mod glyphs;
pub mod interactive;
pub mod layers;
pub mod library;
pub mod notes;
pub mod pages;
pub mod properties;
pub mod styles;
pub mod swatches;
pub mod table;

use designcraft_doc::{Content, StrokeAlign, WrapMode};
use designcraft_geom::Rect;
use egui::vec2;
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;

/// A swatch dropdown showing a chip and name; `on_pick` gets the chosen swatch name.
pub fn swatch_picker(app: &mut DesignApp, ui: &mut egui::Ui, id: &str, current: Option<String>, on_pick: impl FnOnce(&mut DesignApp, String)) {
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let cur = current.unwrap_or_default();
    let (c, g) = crate::widgets::swatch_colors(&doc, &cur, 1.0);
    let mut picked = None;
    ui.push_id(id, |ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(30.0, 18.0), egui::Sense::click());
        crate::widgets::paint_chip(
            ui.painter(),
            egui::Rect::from_min_size(r.min, vec2(18.0, 18.0)),
            if cur.is_empty() { Some(egui::Color32::GRAY) } else { c },
            g,
        );
        crate::icons::paint(
            ui.painter(),
            egui::Rect::from_min_size(r.min + vec2(18.0, 3.0), vec2(12.0, 12.0)),
            "chevron-down",
            Tokens::get(ui.ctx()).icon,
        );
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(200.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for sw in &doc.swatches {
                    let (c, g) = crate::widgets::swatch_colors(&doc, &sw.name, 1.0);
                    let row = ui.horizontal(|ui| {
                        let (cr, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                        crate::widgets::paint_chip(ui.painter(), cr, c, g);
                        ui.add(egui::Button::new(&sw.name).frame(false).selected(sw.name == cur))
                    });
                    if row.inner.clicked() {
                        picked = Some(sw.name.clone());
                        ui.close();
                    }
                }
            });
        });
    });
    if let Some(p) = picked {
        on_pick(app, p);
    }
}

pub fn para_style_picker(app: &mut DesignApp, ui: &mut egui::Ui, current: &str, overridden: bool, width: f32) {
    let names: Vec<String> = app.session.active().map(|d| d.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
    let mut pick = None;
    let shown = crate::i18n::style_name(&app.ui.language, current);
    let label = if overridden { format!("{shown}+") } else { shown.to_string() };
    egui::ComboBox::from_id_salt("para_style").selected_text(crate::rtl::widget(ui, label)).width(width).show_ui(ui, |ui| {
        for n in names.iter().filter(|n| *n != designcraft_doc::NO_PARA_STYLE) {
            if ui.selectable_label(n == current, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                pick = Some(n.clone());
            }
        }
    });
    if let Some(n) = pick {
        let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": false}));
    }
}

#[cfg(test)]
mod font_menu_tests {
    use designcraft_fonts::FontGroup;
    use designcraft_fonts::testing::{font_with, with_names};

    use super::*;

    #[test]
    fn cjk_fonts_follow_the_western_ones_under_their_native_names() {
        const FAMILY: &str = "DC UI Test Mincho";
        let font = font_with(FAMILY, &['a']).unwrap();
        let font = with_names(&font, &[(3, 1, 0x409, 1, FAMILY), (3, 1, 0x409, 2, "Regular"), (3, 1, 0x0411, 1, "UIテスト明朝")]).unwrap();
        designcraft_fonts::FontDb::global().add_font(font);
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let menu = font_menu(&app);
        let at = menu.iter().position(|f| f.family == FAMILY).unwrap();
        assert_eq!(menu[at].group, FontGroup::Japanese);
        assert!(menu[..at].iter().any(|f| f.family == designcraft_fonts::DEFAULT_FAMILY), "Western fonts first");
        assert!(menu.windows(2).all(|w| w[0].group <= w[1].group), "grouped");
        assert_eq!(font_label(&app, &menu, FAMILY), "UIテスト明朝");
        assert_eq!(font_label(&app, &menu, designcraft_fonts::DEFAULT_FAMILY), designcraft_fonts::DEFAULT_FAMILY);
        app.run("prefs.set", json!({"showFontNamesInEnglish": true})).unwrap();
        assert_eq!(font_label(&app, &font_menu(&app), FAMILY), FAMILY);
    }
}
