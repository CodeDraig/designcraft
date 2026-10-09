//! Scoped font menus, family selection, and style selection.

use super::{DesignApp, json};

/// Resolved attributes at the text selection (or the selected text frames).
/// The fonts the active document sees: its own (its `Document Fonts` folder) ahead of the shared
/// ones.
pub fn fonts(app: &DesignApp) -> designcraft_fonts::ScopedFonts<'static> {
    designcraft_fonts::FontDb::global().scoped(font_scope(app))
}

/// The font menus' families in menu order: Western (and other scripts) first, then Japanese,
/// Simplified Chinese, Traditional Chinese and Korean, each alphabetical by the name shown (CJK
/// families by their native names unless Show Font Names in English).
pub fn font_menu(app: &DesignApp) -> Vec<designcraft_fonts::FamilyInfo> {
    let mut v = fonts(app).family_infos();
    designcraft_fonts::sort_for_menu(&mut v, app.session.prefs.show_font_names_in_english);
    v
}

/// The name the font menus show for `family` (its native name, see [`font_menu`]).
pub fn font_label(app: &DesignApp, menu: &[designcraft_fonts::FamilyInfo], family: &str) -> String {
    let english = app.session.prefs.show_font_names_in_english;
    menu.iter().find(|f| f.family == family).map_or(family, |f| f.label(english)).to_string()
}

/// `menu` as rows of a popup or combo box: the name shown (the English name on hover when they
/// differ), a separator between groups. Returns the family clicked.
pub fn font_menu_rows(app: &DesignApp, ui: &mut egui::Ui, menu: &[designcraft_fonts::FamilyInfo], current: &str) -> Option<String> {
    let english = app.session.prefs.show_font_names_in_english;
    let mut pick = None;
    let mut group = None;
    for f in menu {
        if group.is_some_and(|g| g != f.group) {
            ui.separator();
        }
        group = Some(f.group);
        let label = f.label(english);
        let mut resp = ui.selectable_label(f.family == current, label);
        if label != f.family {
            resp = resp.on_hover_text(&f.family);
        }
        if resp.clicked() {
            pick = Some(f.family.clone());
        }
    }
    pick
}

/// The active document's font scope (0 without a document or fonts of its own).
pub fn font_scope(app: &DesignApp) -> u32 {
    app.session.active().map_or(0, |d| d.doc.font_scope)
}

/// A five-pointed star (filled, or outlined).
fn paint_star(p: &egui::Painter, c: egui::Pos2, r: f32, filled: bool, color: egui::Color32) {
    let pt = |i: usize, rad: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        c + egui::vec2(a.cos(), a.sin()) * rad
    };
    let outline: Vec<egui::Pos2> = (0..10).map(|i| pt(i, if i % 2 == 0 { r } else { r * 0.42 })).collect();
    if filled {
        // The inner pentagon and the five points (each convex).
        let inner: Vec<egui::Pos2> = (0..5).map(|i| outline[2 * i + 1]).collect();
        p.add(egui::Shape::convex_polygon(inner, color, egui::Stroke::NONE));
        for i in 0..5 {
            p.add(egui::Shape::convex_polygon(vec![outline[(2 * i + 9) % 10], outline[2 * i], outline[2 * i + 1]], color, egui::Stroke::NONE));
        }
    } else {
        p.add(egui::Shape::closed_line(outline, egui::Stroke::new(1.0, color)));
    }
}

/// The Font menu: search, favourites (★, Show Favorites Only), and each family's name shown in
/// that family.
pub fn font_family_picker(app: &mut DesignApp, ui: &mut egui::Ui, current: &str, width: f32) {
    let (fonts, scope) = (fonts(app), font_scope(app));
    let menu = font_menu(app);
    let english = app.session.prefs.show_font_names_in_english;
    let shown_current = font_label(app, &menu, current);
    let mut favs = app.session.prefs.favorite_fonts.clone();
    let mut pick = None;
    let mut favs_changed = false;
    let state_id = egui::Id::new("font_menu_state");
    let (mut query, mut only_favs): (String, bool) = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::ComboBox::from_id_salt("font_family")
        .selected_text(if current.is_empty() { "—" } else { &shown_current })
        .width(width)
        .height(420.0)
        // The search field, Favorites and the stars keep the menu open; picking a font closes it.
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(300.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search fonts")))
                        .desired_width(200.0),
                );
                ui.toggle_value(&mut only_favs, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Favorites"))).on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show Favorites Only"));
                });
            });
            let q = query.to_lowercase();
            let ppp = ui.ctx().pixels_per_point();
            let shown: Vec<&designcraft_fonts::FamilyInfo> =
                menu.iter().filter(|f| f.matches(&q) && (!only_favs || favs.contains(&f.family))).collect();
            let mut group = None;
            for info in shown {
                // A separator between the Western fonts and each CJK language's.
                if group.is_some_and(|g| g != info.group) {
                    ui.separator();
                }
                group = Some(info.group);
                let f = &info.family;
                let label = info.label(english);
                let (row, mut resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
                if !ui.is_rect_visible(row) {
                    continue;
                }
                if label != f {
                    resp = resp.on_hover_text(f);
                }
                if f == current {
                    ui.painter().rect_filled(row, 0.0, t.row_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(row, 0.0, t.hover);
                }
                // Favourite star.
                let star = egui::Rect::from_min_size(row.min + egui::vec2(2.0, 4.0), egui::vec2(16.0, 16.0));
                let fav = favs.contains(f);
                let sr = ui.interact(star, ui.id().with(("fav", f)), egui::Sense::click());
                paint_star(ui.painter(), star.center(), 6.5, fav, if fav { t.accent } else { t.text_dim });
                if sr.clicked() {
                    if fav {
                        favs.retain(|x| x != f);
                    } else {
                        favs.push(f.clone());
                    }
                    favs_changed = true;
                }
                ui.painter().text(row.min + egui::vec2(22.0, 12.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), t.text);
                // The name in its own face (rendered once per family and scale).
                let key = egui::Id::new(("font_preview", f, scope, (ppp * 100.0) as u32, t.text.to_array()));
                let tex: Option<egui::TextureHandle> = ui.data(|d| d.get_temp(key));
                let tex = tex.unwrap_or_else(|| {
                    let img = designcraft_render::glyphs::text_line(&fonts, f, "Regular", "Sample", (18.0 * ppp) as u32, t.text.to_array());
                    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
                    let h = ui.ctx().load_texture(format!("font_preview_{f}"), ci, egui::TextureOptions::LINEAR);
                    ui.data_mut(|d| d.insert_temp(key, h.clone()));
                    h
                });
                let size = tex.size_vec2() / ppp;
                let at = egui::pos2(row.max.x - size.x - 4.0, row.center().y - size.y / 2.0);
                ui.painter().image(
                    tex.id(),
                    egui::Rect::from_min_size(at, size),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                if resp.clicked() {
                    pick = Some(f.clone());
                    ui.close();
                }
            }
        });
    ui.data_mut(|d| d.insert_temp(state_id, (query, only_favs)));
    if favs_changed {
        let _ = app.run("prefs.set", json!({ "favoriteFonts": favs }));
    }
    if let Some(f) = pick {
        let styles = fonts.styles(&f);
        let style = if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() };
        let _ = app.run("type.char", json!({"attrs": {"fontFamily": f, "fontStyle": style}}));
    }
}

pub fn font_style_picker(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, current: &str, width: f32) {
    let styles = fonts(app).styles(family);
    let mut pick = None;
    egui::ComboBox::from_id_salt("font_style").selected_text(if current.is_empty() { "—" } else { current }).width(width).show_ui(ui, |ui| {
        for s in &styles {
            if ui.selectable_label(s == current, s).clicked() {
                pick = Some(s.clone());
            }
        }
    });
    if let Some(s) = pick {
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": s}}));
    }
}
