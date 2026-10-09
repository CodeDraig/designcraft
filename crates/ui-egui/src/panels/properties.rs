//! The Properties panel (context-sensitive, laid out as InDesign 2026's: selection-type band,
//! sections per selection state — `plan/indesign/11-observed-ui.md` §4.1) and small
//! single-purpose panels (Stroke, Character, Paragraph, Text Wrap, Align, Links, Info).

mod document;
use document::document_sections;
mod objects;
use objects::{align_row, appearance_section, quick_actions_object, swatch_menu, transform_section, wrap_section};
mod text;
use text::{
    LANGUAGES, bullets_section, character_section, paragraph_section, quick_actions_text, text_appearance_section, text_frame_section,
    text_style_section,
};
mod character;
pub use character::character_panel;
mod paragraph;
pub use paragraph::paragraph_panel;
mod stroke;
pub use stroke::stroke_panel;
mod layout;
pub use layout::{align_panel, pathfinder_panel, wrap_panel};
mod links;
pub use links::links_panel;
mod info;
pub use info::info_panel;
mod effects;
pub use effects::effects_panel;
mod preflight;
pub use preflight::preflight_panel;
mod attributes;
pub use attributes::attributes_panel;

use designcraft_doc::{Align, Arrowhead, Cap, Join, StrokeAlign, StrokeType};
use designcraft_geom::Unit;
use designcraft_geom::corners::CornerShape;
use egui::{Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::{SelInfo, sel_info, text_attrs};
use crate::theme::Tokens;
use crate::widgets::{
    self, FIELD_H, NumField, button_pair, caption, divider, dropdown, full_width, icon_toggle, link_label, measure, more_options, number,
    outline_button, rule, section, section_row, selection_band,
};
use crate::{DesignApp, icons};

fn units(app: &DesignApp) -> Unit {
    app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas)
}

/// Allocate a full-width block `h` tall.
fn block(ui: &mut Ui, h: f32) -> Rect {
    ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover()).0
}

/// A sub-rect of a block at (`x`, `y`) relative to its top-left.
fn sub(b: Rect, x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(b.min + vec2(x, y), vec2(w, h))
}

/// Run `f` in a child ui occupying `r` (left-to-right, vertically centred). The parent's cursor
/// does not move: callers allocate the whole block first with [`block`].
pub(crate) fn place<R>(ui: &mut Ui, r: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
    f(&mut child)
}

fn temp_bool(ui: &Ui, key: &str, default: bool) -> bool {
    ui.data(|d| d.get_temp::<bool>(egui::Id::new(key))).unwrap_or(default)
}

fn set_temp_bool(ui: &Ui, key: &str, v: bool) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(key), v));
}

/// Label of the current selection for the selection-type band.
fn selection_title(app: &DesignApp, info: &Option<SelInfo>) -> String {
    let Some(st) = app.session.active() else { return String::new() };
    if st.selection.text.is_some() {
        return crate::i18n::tr(&app.ui.language, "Characters").into();
    }
    match info {
        None => crate::i18n::tr(&app.ui.language, "No Selection").into(),
        Some(i) if i.count > 1 => crate::i18n::tr(&app.ui.language, "Multiple Objects").into(),
        Some(i) => match i.kind {
            "<text frame>" => crate::i18n::tr(&app.ui.language, "Text Frame").into(),
            "<group>" => crate::i18n::tr(&app.ui.language, "Group").into(),
            "<image>" => crate::i18n::tr(&app.ui.language, "Image").into(),
            "<rectangle>" => crate::i18n::tr(&app.ui.language, "Rectangle").into(),
            "<ellipse>" => crate::i18n::tr(&app.ui.language, "Ellipse").into(),
            "<polygon>" => crate::i18n::tr(&app.ui.language, "Polygon").into(),
            "<line>" => crate::i18n::tr(&app.ui.language, "Line").into(),
            _ => crate::i18n::tr(&app.ui.language, "Path").into(),
        },
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    if app.session.active().is_none() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No document open")).color(t.text_dim));
        return;
    }
    ui.spacing_mut().item_spacing = vec2(6.0, 5.0);
    let info = sel_info(app);
    let has_text = app.session.active().is_some_and(|s| s.selection.text.is_some());
    selection_band(ui, &selection_title(app, &info));
    if has_text {
        text_style_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Appearance"), true);
        text_appearance_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Character"), true);
        character_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Paragraph"), true);
        paragraph_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Bullets and Numbering"), true);
        bullets_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
        quick_actions_text(app, ui);
        return;
    }
    let Some(i) = info else {
        document_sections(app, ui);
        return;
    };
    section(ui, crate::i18n::tr(&app.ui.language, "Transform"), true);
    transform_section(app, ui, &i);
    divider(ui);
    if i.is_text && i.count == 1 {
        text_style_section(app, ui);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Appearance"), true);
    appearance_section(app, ui, &i);
    divider(ui);
    if i.is_text && i.count == 1 {
        section(ui, crate::i18n::tr(&app.ui.language, "Character"), true);
        character_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Paragraph"), true);
        paragraph_section(app, ui);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Align"), true);
    align_row(app, ui);
    divider(ui);
    if i.is_text && i.count == 1 {
        section(ui, crate::i18n::tr(&app.ui.language, "Text Frame"), true);
        text_frame_section(app, ui);
        divider(ui);
    } else {
        if i.is_graphic {
            section(ui, crate::i18n::tr(&app.ui.language, "Frame Fitting"), true);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (icon, mode, tip) in [
                    ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
                    ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
                    ("fit-content", "fitContentToFrame", "Fit Content to Frame"),
                    ("fit-frame", "fitFrameToContent", "Fit Frame to Content"),
                    ("fit-center", "centerContent", "Center Content"),
                ] {
                    if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                        let _ = app.run("object.fit", json!({"mode": mode}));
                    }
                }
            });
            divider(ui);
        }
        section(ui, crate::i18n::tr(&app.ui.language, "Text Wrap"), true);
        wrap_section(app, ui, &i);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
    quick_actions_object(app, ui, &i);
}

// ---------------------------------------------------------------- No Selection

/// Page-size presets for the Document dropdown (portrait, points).
const PAGE_PRESETS: &[(&str, f64, f64)] = &[
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("Tabloid", 792.0, 1224.0),
    ("Letter - Half", 396.0, 612.0),
    ("Legal - Half", 504.0, 612.0),
    ("A3", 841.89, 1190.55),
    ("A4", 595.276, 841.89),
    ("A5", 419.528, 595.276),
    ("B5", 498.898, 708.661),
];
