//! Liquid panel controls.

use super::{DesignApp, Tokens, json};

/// Liquid Layout panel: the current page's rule, the selection's pins (object-based), the page's
/// liquid guides, and Create Alternate Layout.
pub fn liquid(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(page) = crate::canvas::current_page(app) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No document.")).size(11.0).color(t.text_dim));
        return;
    };
    let Some(d) = app.session.active().map(|d| d.doc.clone()) else { return };
    let Some((si, pi)) = d.page_loc(page) else { return };
    let pg = d.spreads[si].pages[pi].clone();
    use designcraft_doc::LiquidRule as L;
    crate::rtl::label(ui, egui::RichText::new(format!("{} {}", crate::i18n::tr(&app.ui.language, "Page"), d.page_name(page))).strong());
    let mut rule = pg.liquid;
    egui::ComboBox::from_id_salt("liquid_rule")
        .selected_text(crate::rtl::widget(
            ui,
            crate::i18n::tr(
                &app.ui.language,
                match rule {
                    L::Off => "Off",
                    L::Scale => "Scale",
                    L::ReCenter => "Re-center",
                    L::GuideBased => "Guide-based",
                    L::ObjectBased => "Object-based",
                },
            ),
        ))
        .show_ui(ui, |ui| {
            for (r, label) in
                [(L::Off, "Off"), (L::Scale, "Scale"), (L::ReCenter, "Re-center"), (L::GuideBased, "Guide-based"), (L::ObjectBased, "Object-based")]
            {
                ui.selectable_value(&mut rule, r, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label)));
            }
        });
    if rule != pg.liquid {
        let _ = app.run("liquid.pageRule", json!({"rule": rule, "pages": [page + 1]}));
    }
    if rule == L::ObjectBased
        && let Some(id) = app.session.active().and_then(|s| s.selection.items.first().copied())
        && let Some(it) = d.item(id)
    {
        ui.separator();
        let l = it.liquid.unwrap_or(designcraft_doc::ObjectLiquid { resize_width: true, resize_height: true, ..Default::default() });
        let mut v = [l.resize_width, l.resize_height, l.pin_top, l.pin_bottom, l.pin_left, l.pin_right];
        let keys = ["resizeWidth", "resizeHeight", "pinTop", "pinBottom", "pinLeft", "pinRight"];
        let labels = ["Resize width", "Resize height", "Pin top", "Pin bottom", "Pin left", "Pin right"];
        ui.horizontal_wrapped(|ui| {
            for ((on, key), label) in v.iter_mut().zip(keys).zip(labels) {
                if ui.checkbox(on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
                    let _ = app.run("liquid.object", json!({key: *on}));
                }
            }
        });
    }
    if !pg.guides.is_empty() {
        ui.separator();
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Liquid guides")).size(11.0).color(t.text_dim));
        for (gi, g) in pg.guides.iter().enumerate() {
            let mut on = g.liquid;
            let label = format!("{:?} at {:.1}", g.orientation, g.position);
            if ui.checkbox(&mut on, label).changed() {
                let _ = app.run("guide.liquid", json!({"spread": si, "page": pi, "index": gi, "on": on}));
            }
        }
    }
    ui.separator();
    let key = egui::Id::new("alt_layout");
    let (mut name, mut w, mut h): (String, f64, f64) = ui.data(|x| x.get_temp(key)).unwrap_or(("Tablet H".into(), 1024.0, 768.0));
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Create Alternate Layout")).size(11.0).color(t.text_dim));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(90.0));
        ui.add(egui::DragValue::new(&mut w).range(1.0..=15552.0).suffix(" pt"));
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "×"));
        ui.add(egui::DragValue::new(&mut h).range(1.0..=15552.0).suffix(" pt"));
    });
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Create"))).clicked()
        && let Err(e) = app.run("layout.createAlternate", json!({"name": name, "width": w, "height": h}))
    {
        app.status(format!("Alternate layout: {e}"));
    }
    ui.data_mut(|x| x.insert_temp(key, (name, w, h)));
}
