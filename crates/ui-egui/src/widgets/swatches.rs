//! Swatch colour resolution and paint previews.

use super::{Color32, Response, Sense, Stroke, StrokeKind, Tokens, Ui, vec2};

/// A swatch chip (colour square, [None] slash, gradient ramp).
pub fn swatch_chip(ui: &mut Ui, size: f32, color: Option<Color32>, gradient: Option<(Color32, Color32)>) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_chip(ui.painter(), r, color, gradient);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    resp
}

pub fn paint_chip(p: &egui::Painter, r: egui::Rect, color: Option<Color32>, gradient: Option<(Color32, Color32)>) {
    match (color, gradient) {
        (_, Some((a, b))) => {
            let n = 12;
            for i in 0..n {
                let f = i as f32 / (n - 1) as f32;
                let c = Color32::from_rgb(
                    (a.r() as f32 * (1.0 - f) + b.r() as f32 * f) as u8,
                    (a.g() as f32 * (1.0 - f) + b.g() as f32 * f) as u8,
                    (a.b() as f32 * (1.0 - f) + b.b() as f32 * f) as u8,
                );
                let x0 = r.min.x + r.width() * i as f32 / n as f32;
                p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, r.min.y), egui::pos2(x0 + r.width() / n as f32 + 0.5, r.max.y)), 0.0, c);
            }
        }
        (Some(c), None) => {
            p.rect_filled(r, 0.0, c);
        }
        (None, None) => {
            p.rect_filled(r, 0.0, Color32::WHITE);
            p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5, Color32::from_rgb(230, 30, 30)));
        }
    }
}

/// A stroke chip: a ring of the colour around a dark centre (None = white with a red slash).
pub fn paint_stroke_chip(p: &egui::Painter, r: egui::Rect, color: Option<Color32>, inner: Color32) {
    match color {
        Some(c) => {
            p.rect_filled(r, 0.0, c);
            p.rect_filled(r.shrink(r.width() * 0.3), 0.0, inner);
        }
        None => {
            paint_chip(p, r, None, None);
            p.rect_stroke(r.shrink(r.width() * 0.3), 0.0, Stroke::new(1.0, Color32::from_gray(40)), StrokeKind::Middle);
        }
    }
}

/// Resolve a swatch to a display colour (and gradient end colours).
pub fn swatch_colors(doc: &designcraft_doc::Document, name: &str, tint: f32) -> (Option<Color32>, Option<(Color32, Color32)>) {
    let conv = |c: designcraft_color::Color| {
        let [r, g, b, _] = c.to_rgba8(1.0);
        Color32::from_rgb(r, g, b)
    };
    if let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, name)
        && let (Some(a), Some(b)) = (g.stops.first(), g.stops.last())
    {
        return (None, Some((conv(a.color), conv(b.color))));
    }
    (doc.resolve_color(name, tint).map(conv), None)
}
