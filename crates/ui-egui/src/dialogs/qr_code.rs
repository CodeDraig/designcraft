//! qrCode dialog controls.

use super::{DesignApp, Dialog, json, text_field};

pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let cur = d.s("type");
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Type:"));
        let label = |v: &str| match v {
            "text" => crate::i18n::tr(&app.ui.language, "Plain Text"),
            "sms" => crate::i18n::tr(&app.ui.language, "Text Message"),
            "email" => crate::i18n::tr(&app.ui.language, "Email"),
            "vcard" => crate::i18n::tr(&app.ui.language, "Business Card"),
            _ => crate::i18n::tr(&app.ui.language, "Web Hyperlink"),
        };
        egui::ComboBox::from_id_salt("qr_type").selected_text(crate::rtl::widget(ui, label(&cur))).width(160.0).show_ui(ui, |ui| {
            for v in ["url", "text", "sms", "email", "vcard"] {
                if ui.selectable_label(cur == v, crate::rtl::widget(ui, label(v))).clicked() {
                    d.fields.insert("type".into(), json!(v));
                }
            }
        });
    });
    let fields: &[(&str, &str)] = match cur.as_str() {
        "sms" => &[("number", "Cell Number:"), ("message", "Message:")],
        "email" => &[("to", "Email Address:"), ("subject", "Subject:"), ("body", "Message:")],
        "vcard" => &[("name", "Name:"), ("org", "Organization:"), ("phone", "Phone:"), ("email", "Email:"), ("url", "URL:")],
        "text" => &[("content", "Text:")],
        _ => &[("content", "URL:")],
    };
    egui::Grid::new("qr_fields").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (k, l) in fields {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
            text_field(ui, d, k, 260.0);
            ui.end_row();
        }
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Color:"));
        text_field(ui, d, "color", 160.0);
        ui.end_row();
    });
}
