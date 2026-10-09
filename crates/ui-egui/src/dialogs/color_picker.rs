//! Colour picker controls.

use super::{Dialog, json};

/// Color Picker: saturation/brightness field and hue slider, with RGB, CMYK and hex fields.
pub(super) fn color_picker(ui: &mut egui::Ui, d: &mut Dialog) {
    let hex = d.s("hex");
    let mut c = designcraft_color::Color::from_hex(&hex).map(|c| c.to_rgb()).map_or(egui::Color32::BLACK, |[r, g, b]| {
        egui::Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8)
    });
    let before = c;
    ui.horizontal_top(|ui| {
        egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
        ui.vertical(|ui| {
            let mut rgb = [c.r() as f32, c.g() as f32, c.b() as f32];
            egui::Grid::new("cp_rgb").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                for (k, l) in ["R", "G", "B"].iter().enumerate() {
                    ui.label(*l);
                    ui.add(egui::DragValue::new(&mut rgb[k]).range(0.0..=255.0));
                    ui.end_row();
                }
            });
            c = egui::Color32::from_rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
            let col = designcraft_color::Color::rgb8(c.r(), c.g(), c.b());
            let [cc, m, y, k] = col.to_cmyk();
            ui.add_space(6.0);
            ui.label(format!("C {:.0}%  M {:.0}%  Y {:.0}%  K {:.0}%", cc * 100.0, m * 100.0, y * 100.0, k * 100.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("#");
                let mut h = format!("{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
                if ui.add(egui::TextEdit::singleline(&mut h).desired_width(70.0)).changed()
                    && let Some(n) = designcraft_color::Color::from_hex(&h)
                {
                    let [r, g, b] = n.to_rgb();
                    c = egui::Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8);
                }
            });
        });
    });
    if c != before {
        d.fields.insert("hex".into(), json!(format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())));
    }
}
