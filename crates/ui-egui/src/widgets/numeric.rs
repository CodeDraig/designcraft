//! Numeric and unit-aware text fields with spin controls.

use super::{
    Color32, FIELD_H, Rect, SEG_W, Sense, Stroke, StrokeKind, Tokens, Ui, Unit, chevron_down, format_measure_prec, label_font, parse_measure, pos2,
    vec2,
};

/// What a numeric field holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumKind<'a> {
    /// A length in points shown in `Unit`; accepts any unit and arithmetic.
    Measure(Unit),
    /// A plain number with a display suffix (`%`, `°`, ` pt`).
    Number { suffix: &'a str, decimals: usize },
}

impl NumKind<'_> {
    pub fn format(&self, v: f64) -> String {
        match *self {
            NumKind::Measure(u) => format_measure_prec(v, u, 3),
            NumKind::Number { suffix, decimals } => {
                let s = format!("{v:.decimals$}");
                let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
                let s = if s == "-0" { "0".to_string() } else { s };
                format!("{s}{suffix}")
            }
        }
    }
    pub fn parse(&self, s: &str) -> Option<f64> {
        match *self {
            NumKind::Measure(u) => parse_measure(s, u).ok(),
            NumKind::Number { .. } => {
                // Numbers accept the same arithmetic as measures (`12+3`, `50%*2`): evaluate as points.
                let cleaned: String =
                    s.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | '*' | '/' | '(' | ')' | ' ')).collect();
                parse_measure(cleaned.trim(), Unit::Points).ok().or_else(|| cleaned.trim().parse().ok())
            }
        }
    }
    /// One spinner click (points for measures).
    pub fn step(&self) -> f64 {
        match *self {
            NumKind::Measure(u) => match u {
                Unit::Points | Unit::Picas | Unit::Ciceros | Unit::Pixels | Unit::Agates => 1.0,
                Unit::Inches | Unit::InchesDecimal => 72.0 / 16.0,
                Unit::Millimeters | Unit::Centimeters | Unit::Q | Unit::Ha => Unit::Millimeters.to_pt(1.0),
            },
            NumKind::Number { .. } => 1.0,
        }
    }
}

/// A numeric field. Build with [`NumField::measure`] / [`NumField::number`], then [`NumField::show`].
pub struct NumField<'a> {
    id: &'a str,
    value: Option<f64>,
    kind: NumKind<'a>,
    width: f32,
    spinner: bool,
    presets: &'a [f64],
    step: Option<f64>,
    range: (f64, f64),
    enabled: bool,
    parens: bool,
}

impl<'a> NumField<'a> {
    pub fn measure(id: &'a str, value: Option<f64>, unit: Unit) -> Self {
        Self::new(id, value, NumKind::Measure(unit))
    }
    pub fn number(id: &'a str, value: Option<f64>, suffix: &'a str, decimals: usize) -> Self {
        Self::new(id, value, NumKind::Number { suffix, decimals })
    }
    fn new(id: &'a str, value: Option<f64>, kind: NumKind<'a>) -> Self {
        NumField { id, value, kind, width: 72.0, spinner: false, presets: &[], step: None, range: (f64::MIN, f64::MAX), enabled: true, parens: false }
    }
    /// Total width (spinner and preset segments included).
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    /// Show the ⌃⌄ spinner segment on the left.
    pub fn spinner(mut self) -> Self {
        self.spinner = true;
        self
    }
    /// Show a ⌄ preset segment on the right listing `presets`.
    pub fn presets(mut self, p: &'a [f64]) -> Self {
        self.presets = p;
        self
    }
    pub fn step(mut self, s: f64) -> Self {
        self.step = Some(s);
        self
    }
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = (min, max);
        self
    }
    /// Show the value in parentheses (an automatic value, like auto leading "(14.4 pt)").
    pub fn parens(mut self, on: bool) -> Self {
        self.parens = on;
        self
    }
    fn fmt(&self, v: f64) -> String {
        if self.parens { format!("({})", self.kind.format(v)) } else { self.kind.format(v) }
    }
    fn parse(&self, s: &str) -> Option<f64> {
        self.kind.parse(s.trim().trim_start_matches('(').trim_end_matches(')'))
    }
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// Draw the field; returns the new value when the user commits an edit (Enter, focus loss,
    /// a spinner click, ↑/↓ while focused (Shift = ×10) or a preset).
    pub fn show(self, ui: &mut Ui) -> Option<f64> {
        let t = Tokens::get(ui.ctx());
        let key = ui.id().with(self.id);
        let (rect, _) = ui.allocate_exact_size(vec2(self.width, FIELD_H), Sense::hover());
        let step = self.step.unwrap_or_else(|| self.kind.step());
        let clamp = |v: f64| v.clamp(self.range.0, self.range.1);
        let mut out: Option<f64> = None;
        let mut text_rect = rect;
        let focused_id = key.with("te");
        let focused = ui.memory(|m| m.has_focus(focused_id));
        let (bg, fg, border) =
            if focused { (Color32::WHITE, Color32::from_gray(20), Color32::from_rgb(0x45, 0xa0, 0xf5)) } else { (t.input, t.text, t.field_border) };
        let fg = if self.enabled { fg } else { t.text_disabled };
        // Spinner segment.
        if self.spinner {
            let seg = Rect::from_min_size(rect.min, vec2(SEG_W, FIELD_H));
            text_rect.min.x = seg.max.x;
            ui.painter().rect_filled(seg, 0.0, t.input);
            let up = Rect::from_min_max(seg.min, pos2(seg.max.x, seg.center().y));
            let down = Rect::from_min_max(pos2(seg.min.x, seg.center().y), seg.max);
            for (r, dir) in [(up, 1.0), (down, -1.0)] {
                let resp = ui.interact(r, key.with(("spin", dir as i32)), if self.enabled { Sense::click() } else { Sense::hover() });
                if resp.hovered() && self.enabled {
                    ui.painter().rect_filled(r.shrink(1.0), 0.0, t.hover);
                }
                let c = r.center();
                let (a, b) = if dir > 0.0 { (2.5, -1.5) } else { (-2.5, 1.5) };
                ui.painter().line_segment(
                    [pos2(c.x - 4.0, c.y + a * 0.6), pos2(c.x, c.y + b)],
                    Stroke::new(1.0, if self.enabled { t.icon } else { t.text_disabled }),
                );
                ui.painter().line_segment(
                    [pos2(c.x, c.y + b), pos2(c.x + 4.0, c.y + a * 0.6)],
                    Stroke::new(1.0, if self.enabled { t.icon } else { t.text_disabled }),
                );
                if resp.clicked() {
                    let mul = if ui.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
                    out = Some(clamp(self.value.unwrap_or(0.0) + dir * step * mul));
                }
            }
        }
        // Preset segment.
        let mut preset_resp = None;
        if !self.presets.is_empty() {
            let seg = Rect::from_min_max(pos2(rect.max.x - SEG_W, rect.min.y), rect.max);
            text_rect.max.x = seg.min.x;
            let resp = ui.interact(seg, key.with("presets"), Sense::click());
            ui.painter().rect_filled(seg, 0.0, if resp.hovered() { t.hover } else { t.input });
            chevron_down(ui.painter(), seg.center(), t.icon);
            preset_resp = Some(resp);
        }
        // Value.
        ui.painter().rect_filled(text_rect, 0.0, bg);
        let mut buf: String = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| self.value.map(|v| self.fmt(v)).unwrap_or_default());
        let te = egui::TextEdit::singleline(&mut buf)
            .id(focused_id)
            .frame(egui::Frame::NONE)
            .font(label_font())
            .text_color(fg)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center);
        let r = ui
            .add_enabled_ui(self.enabled, |ui| ui.put(Rect::from_min_max(text_rect.min + vec2(5.0, 1.0), text_rect.max - vec2(1.0, 1.0)), te))
            .inner;
        if r.has_focus() {
            // ↑/↓ step the value while typing.
            let (up, down, shift) = ui.input_mut(|i| {
                let s = i.modifiers.shift;
                let up = i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp) || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                let down = i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown) || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                (up, down, s)
            });
            if up || down {
                let base = self.parse(&buf).or(self.value).unwrap_or(0.0);
                let v = clamp(base + if up { step } else { -step } * if shift { 10.0 } else { 1.0 });
                buf = self.fmt(v);
                out = Some(v);
            }
            ui.data_mut(|d| d.insert_temp(key, buf.clone()));
        }
        if r.lost_focus() {
            if let Some(v) = self.parse(&buf).map(clamp)
                && self.value.is_none_or(|old| (old - v).abs() > 1e-9)
            {
                out = Some(v);
            }
            ui.data_mut(|d| d.remove::<String>(key));
        } else if !r.has_focus() {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        if out.is_some() && !r.has_focus() {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        // Frame: outer border plus segment rules.
        let p = ui.painter();
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
        if self.spinner {
            p.line_segment([pos2(rect.min.x + SEG_W, rect.min.y), pos2(rect.min.x + SEG_W, rect.max.y)], Stroke::new(1.0, t.field_border));
        }
        if !self.presets.is_empty() {
            p.line_segment([pos2(rect.max.x - SEG_W, rect.min.y), pos2(rect.max.x - SEG_W, rect.max.y)], Stroke::new(1.0, t.field_border));
        }
        if let Some(resp) = preset_resp {
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(self.width);
                for v in self.presets {
                    if ui.selectable_label(self.value.is_some_and(|c| (c - v).abs() < 1e-9), self.kind.format(*v)).clicked() {
                        out = Some(*v);
                        ui.close();
                    }
                }
            });
        }
        out
    }
}

/// A plain measurement field (no spinner), e.g. Transform X/Y/W/H.
pub fn measure(ui: &mut Ui, id: &str, value: Option<f64>, unit: Unit, width: f32) -> Option<f64> {
    NumField::measure(id, value, unit).width(width).show(ui)
}

/// A measurement field with the spinner segment.
pub fn spin_measure(ui: &mut Ui, id: &str, value: Option<f64>, unit: Unit, width: f32) -> Option<f64> {
    NumField::measure(id, value, unit).width(width).spinner().show(ui)
}

/// A plain number field (percent, degrees, counts).
pub fn number(ui: &mut Ui, id: &str, value: Option<f64>, suffix: &str, width: f32, decimals: usize) -> Option<f64> {
    NumField::number(id, value, suffix, decimals).width(width).show(ui)
}

/// A number field with the spinner segment.
pub fn spin_number(ui: &mut Ui, id: &str, value: Option<f64>, suffix: &str, width: f32, decimals: usize) -> Option<f64> {
    NumField::number(id, value, suffix, decimals).width(width).spinner().show(ui)
}
