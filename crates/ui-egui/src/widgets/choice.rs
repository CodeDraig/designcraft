//! Icon toggles, checkboxes, and segmented choices.

use super::{BUTTON_H, FIELD_H, Rect, Response, Sense, Stroke, StrokeKind, Tokens, Ui, label_font, pos2, vec2};

/// A 21 pt icon toggle (active = `#303030` well with a 1 pt rim).
pub fn icon_toggle(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> Response {
    icon_toggle_sized(ui, icon, on, tip, vec2(23.0, 21.0))
}

pub fn icon_toggle_sized(ui: &mut Ui, icon: &str, on: bool, tip: &str, size: egui::Vec2) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    if on {
        ui.painter().rect(r, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, t.hover);
    }
    let s = (size.y - 4.0).min(17.0);
    crate::icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(s, s)), icon, if on { t.text_strong } else { t.icon });
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_ui(|ui| {
            crate::rtl::label(ui, tip);
        })
    }
}

/// An InDesign checkbox: 12 pt rounded box, white check, label.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let g = ui.painter().layout_no_wrap(label.to_string(), label_font(), t.text);
    let (r, mut resp) = ui.allocate_exact_size(vec2(18.0 + g.size().x, FIELD_H), Sense::click());
    let b = Rect::from_center_size(pos2(r.min.x + 6.5, r.center().y), vec2(12.0, 12.0));
    if *on {
        ui.painter().rect_filled(b, 2.0, t.icon);
        ui.painter().line_segment([b.min + vec2(2.5, 6.0), b.min + vec2(5.0, 8.5)], Stroke::new(1.5, t.panel));
        ui.painter().line_segment([b.min + vec2(5.0, 8.5), b.min + vec2(9.5, 3.5)], Stroke::new(1.5, t.panel));
    } else {
        ui.painter().rect_stroke(b, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    }
    ui.painter().galley(pos2(r.min.x + 18.0, r.center().y - g.size().y / 2.0), g, t.text);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp
}

/// A two-segment control ([Paragraph Styles | Character Styles]); returns the clicked index.
pub fn segmented(ui: &mut Ui, labels: &[&str], active: usize, width: f32) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, BUTTON_H), Sense::hover());
    let w = width / labels.len() as f32;
    let mut out = None;
    for (i, l) in labels.iter().enumerate() {
        let sr = Rect::from_min_size(pos2(r.min.x + w * i as f32, r.min.y), vec2(w, BUTTON_H));
        let resp = ui.interact(sr, ui.id().with(("seg", l)), Sense::click());
        if i == active {
            ui.painter().rect_filled(sr, 0.0, t.well);
        } else if resp.hovered() {
            ui.painter().rect_filled(sr, 0.0, t.hover);
        }
        crate::rtl::paint(ui.painter(), sr.center(), egui::Align2::CENTER_CENTER, l, label_font(), if i == active { t.accent } else { t.text });
        if i > 0 {
            ui.painter().line_segment([sr.left_top(), sr.left_bottom()], Stroke::new(1.0, t.field_border));
        }
        if resp.clicked() {
            out = Some(i);
        }
    }
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    out
}
