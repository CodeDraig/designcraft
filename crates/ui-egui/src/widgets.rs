//! Small InDesign-style widgets (measured from InDesign 2026, `plan/indesign/11-observed-ui.md` §4):
//! 21 pt fields (`#454545` fill, 1 pt `#747474` border) with an optional 18 pt spinner segment on
//! the **left** of the value and an optional preset chevron segment on the right, 20 pt outline
//! buttons, underlined link labels, `•••` more-options, 21 pt icon toggles in a `#303030` well,
//! section headers and 1.5 pt section rules.

mod numeric;
pub use numeric::{NumField, NumKind, measure, number, spin_measure, spin_number};
mod swatches;
pub use swatches::{paint_chip, paint_stroke_chip, swatch_chip, swatch_colors};
mod choice;
pub use choice::{checkbox, icon_toggle, icon_toggle_sized, segmented};

use designcraft_geom::Unit;
use designcraft_geom::units::{format_measure_prec, parse_measure};
use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::theme::Tokens;

/// Field height including the 1 pt border.
pub const FIELD_H: f32 = 21.0;
/// Width of the spinner (⌃⌄) and preset (⌄) segments.
pub const SEG_W: f32 = 18.0;
/// Outline button height.
pub const BUTTON_H: f32 = 20.0;
/// UI label size (InDesign: about 10.5–11 pt).
pub const LABEL_SIZE: f32 = 11.0;

fn label_font() -> egui::FontId {
    egui::FontId::proportional(LABEL_SIZE)
}

pub fn chevron_down(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    p.line_segment([pos2(c.x - 4.0, c.y - 2.0), pos2(c.x, c.y + 2.0)], Stroke::new(1.0, col));
    p.line_segment([pos2(c.x, c.y + 2.0), pos2(c.x + 4.0, c.y - 2.0)], Stroke::new(1.0, col));
}

/// A dropdown box (field style, ⌄ segment on the right). Returns the response; open a popup on it
/// with [`egui::Popup::menu`].
pub fn dropdown(ui: &mut Ui, text: &str, width: f32) -> Response {
    dropdown_h(ui, text, width, FIELD_H)
}

pub fn dropdown_h(ui: &mut Ui, text: &str, width: f32, h: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.input });
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let seg = Rect::from_min_max(pos2(r.max.x - SEG_W, r.min.y), r.max);
    p.line_segment([seg.left_top(), seg.left_bottom()], Stroke::new(1.0, t.field_border));
    chevron_down(p, seg.center(), t.icon);
    let clip = Rect::from_min_max(r.min, pos2(seg.min.x - 2.0, r.max.y));
    p.with_clip_rect(clip).text(pos2(r.min.x + 7.0, r.center().y), egui::Align2::LEFT_CENTER, text, label_font(), t.text);
    resp
}

/// A dropdown that lists `items` and returns the picked index.
pub fn dropdown_list(ui: &mut Ui, text: &str, width: f32, items: &[String], current: Option<usize>) -> Option<usize> {
    let resp = dropdown(ui, text, width);
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(width);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                if ui.selectable_label(current == Some(i), it).clicked() {
                    out = Some(i);
                    ui.close();
                }
            }
        });
    });
    out
}

/// A 20 pt outline button (1 pt `#747474`, transparent fill, centred label).
pub fn outline_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, BUTTON_H), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, 2.0, t.well);
    } else if resp.hovered() {
        p.rect_filled(r, 2.0, t.hover);
    }
    p.rect_stroke(r, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    crate::rtl::paint(p, r.center(), egui::Align2::CENTER_CENTER, label, label_font(), t.text);
    resp
}

/// Two half-width buttons side by side (10 pt gutter); returns which was clicked.
pub fn button_pair(ui: &mut Ui, a: &str, b: &str) -> (bool, bool) {
    let w = (full_width(ui) - 10.0) / 2.0;
    let mut out = (false, false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        out.0 = outline_button(ui, a, w).clicked();
        out.1 = outline_button(ui, b, w).clicked();
    });
    out
}

/// Width of a full-width row in the Properties panel (214 pt in a 253 pt panel).
pub fn full_width(ui: &Ui) -> f32 {
    (ui.available_width() - 8.0).max(60.0)
}

/// An underlined link label (`Stroke`, `Corner`, `Opacity`) that opens the full panel.
pub fn link_label(ui: &mut Ui, text: &str, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, FIELD_H), Sense::click());
    let g = crate::rtl::plain(ui.ctx(), text, label_font(), t.text);
    let pos = pos2(r.min.x, r.center().y - g.size().y / 2.0);
    let w = g.size().x;
    ui.painter().galley(pos, g, t.text);
    let y = r.center().y + 6.5;
    ui.painter().line_segment([pos2(r.min.x, y), pos2(r.min.x + w, y)], Stroke::new(1.0, if resp.hovered() { t.text_strong } else { t.text_dim }));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `•••` more-options, right-aligned on its own row at the bottom of a section.
pub fn more_options(ui: &mut Ui, language: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    let r = Rect::from_min_max(pos2(row.max.x - 26.0, row.min.y), pos2(row.max.x - 4.0, row.max.y));
    let resp = ui.interact(r, ui.id().with(("more", row.min.y as i32)), Sense::click());
    for i in 0..3 {
        ui.painter().circle_filled(pos2(r.min.x + 4.0 + i as f32 * 7.0, r.center().y), 1.9, if resp.hovered() { t.text_strong } else { t.icon });
    }
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, "More Options"));
    })
}

/// A small dim label (field captions like `X:` or `W:`).
pub fn caption(ui: &mut Ui, s: &str) {
    let t = Tokens::get(ui.ctx());
    crate::rtl::label(ui, egui::RichText::new(s).size(LABEL_SIZE).color(t.text));
}

/// A right-aligned caption of a fixed width (`W:` before a spinner).
pub fn caption_w(ui: &mut Ui, s: &str, w: f32) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::hover());
    crate::rtl::paint(ui.painter(), pos2(r.max.x - 2.0, r.center().y), egui::Align2::RIGHT_CENTER, s, egui::FontId::proportional(11.5), t.text);
}

/// The selection-type line at the top of the Properties panel ("No Selection", "Rectangle", …):
/// a 33 pt band followed by a section rule.
pub fn selection_band(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::hover());
    crate::rtl::paint(ui.painter(), pos2(r.min.x, r.center().y + 1.0), egui::Align2::LEFT_CENTER, title, crate::theme::semibold(11.5), t.text_strong);
    rule(ui);
}

/// Panel section header (InDesign 2026: regular-weight white title, no disclosure chevron; a
/// 1.5 pt rule separates sections). Returns whether to show the section body (always true).
pub fn section(ui: &mut Ui, title: &str, _default_open: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.add_space(7.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    crate::rtl::paint(ui.painter(), r.left_center(), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(11.5), t.text_strong);
    ui.add_space(3.0);
    true
}

/// A section header whose controls sit on the same row (Rulers & Grids, Guides, Flex Layout):
/// returns the remaining rect to the right of the title.
pub fn section_row(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.set_min_height(23.0);
        let (r, _) = ui.allocate_exact_size(vec2(100.0, 23.0), Sense::hover());
        crate::rtl::paint(ui.painter(), r.left_center(), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(11.5), t.text_strong);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            add(ui)
        });
    });
    ui.add_space(6.0);
}

/// The 1.5 pt section rule, full panel width.
pub fn rule(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.5), Sense::hover());
    let clip = ui.clip_rect();
    ui.painter().with_clip_rect(clip.expand2(vec2(12.0, 0.0))).rect_filled(
        Rect::from_min_max(pos2(r.min.x - 12.0, r.min.y), pos2(r.max.x + 12.0, r.max.y)),
        0.0,
        t.section_divider,
    );
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(8.0);
    rule(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &egui::Context, events: Vec<egui::Event>, mods: egui::Modifiers, f: &mut dyn FnMut(&mut Ui)) {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 200.0))),
            events: std::iter::once(egui::Event::ModifiersChanged(mods)).chain(events).collect(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| f(ui));
        out.textures_delta.clear();
    }

    fn click(p: egui::Pos2, mods: egui::Modifiers) -> Vec<Vec<egui::Event>> {
        vec![
            vec![egui::Event::PointerMoved(p)],
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: mods }],
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: mods }],
        ]
    }

    /// Drive a spinner field through frames; returns every committed value and the field rect.
    fn drive(frames: Vec<(Vec<egui::Event>, egui::Modifiers)>, value: f64) -> (Vec<f64>, Rect) {
        let ctx = egui::Context::default();
        let mut got = vec![];
        let mut rect = Rect::NOTHING;
        let mut all = vec![(vec![], egui::Modifiers::NONE)];
        all.extend(frames);
        for (ev, m) in all {
            run(&ctx, ev, m, &mut |ui| {
                rect = ui.available_rect_before_wrap();
                if let Some(v) = NumField::measure("w", Some(value), Unit::Points).width(80.0).spinner().show(ui) {
                    got.push(v);
                }
            });
        }
        (got, rect)
    }

    #[test]
    fn spinner_up_down_and_shift() {
        let (_, r) = drive(vec![], 10.0);
        let up = r.min + vec2(9.0, 5.0);
        let down = r.min + vec2(9.0, 16.0);
        let (got, _) = drive(click(up, egui::Modifiers::NONE).into_iter().map(|e| (e, egui::Modifiers::NONE)).collect(), 10.0);
        assert_eq!(got, vec![11.0]);
        let (got, _) = drive(click(down, egui::Modifiers::NONE).into_iter().map(|e| (e, egui::Modifiers::NONE)).collect(), 10.0);
        assert_eq!(got, vec![9.0]);
        let (got, _) = drive(click(up, egui::Modifiers::SHIFT).into_iter().map(|e| (e, egui::Modifiers::SHIFT)).collect(), 10.0);
        assert_eq!(got, vec![20.0]);
    }

    #[test]
    fn arrow_keys_step_and_typing_parses_arithmetic() {
        let (_, r) = drive(vec![], 10.0);
        let text = r.min + vec2(50.0, 10.0);
        let mut frames: Vec<(Vec<egui::Event>, egui::Modifiers)> =
            click(text, egui::Modifiers::NONE).into_iter().map(|e| (e, egui::Modifiers::NONE)).collect();
        let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        frames.push((vec![key(egui::Key::ArrowUp)], egui::Modifiers::NONE));
        let (got, _) = drive(frames.clone(), 10.0);
        assert_eq!(got, vec![11.0]);
        // Select all, type an expression, press Enter.
        frames.truncate(3);
        frames.push((
            vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
            egui::Modifiers::COMMAND,
        ));
        frames.push((vec![egui::Event::Text("6+6".into())], egui::Modifiers::NONE));
        frames.push((vec![key(egui::Key::Enter)], egui::Modifiers::NONE));
        let (got, _) = drive(frames, 10.0);
        assert_eq!(got, vec![12.0]);
    }

    #[test]
    fn number_kind_formats_and_parses() {
        let k = NumKind::Number { suffix: "%", decimals: 0 };
        assert_eq!(k.format(100.0), "100%");
        assert_eq!(k.parse("50%"), Some(50.0));
        assert_eq!(k.parse("10*3"), Some(30.0));
        let m = NumKind::Measure(Unit::Picas);
        assert_eq!(m.parse("1p0+0p6"), Some(18.0));
        assert_eq!(m.step(), 1.0);
    }
}
