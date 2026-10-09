//! Character formatting to OpenType feature selection.

use super::{Capitalization, CharProps, Feature, Kerning, Position, feature};

pub(super) fn features_for(p: &CharProps) -> Vec<Feature> {
    let mut v = Vec::new();
    if !matches!(p.kerning, Kerning::Metrics | Kerning::Optical) {
        v.extend(feature("-kern"));
    }
    if !p.ligatures || p.tracking.abs() > 1e-9 {
        v.extend(feature("-liga"));
        v.extend(feature("-clig"));
    }
    if p.capitalization == Capitalization::AllCaps {
        v.extend(feature("case"));
    }
    if matches!(p.capitalization, Capitalization::SmallCaps | Capitalization::OpenTypeAllSmallCaps) {
        v.extend(feature("smcp"));
    }
    if p.capitalization == Capitalization::OpenTypeAllSmallCaps {
        v.extend(feature("c2sc"));
    }
    match p.position {
        Position::OtSuperscript => v.extend(feature("sups")),
        Position::OtSubscript => v.extend(feature("subs")),
        Position::OtNumerator => v.extend(feature("numr")),
        Position::OtDenominator => v.extend(feature("dnom")),
        _ => {}
    }
    let form = match p.glyph_form.as_str() {
        "ExpertForm" => "expt",
        "TraditionalForm" => "trad",
        "SimplifiedForm" => "smpl",
        "JIS78Form" => "jp78",
        "JIS83Form" => "jp83",
        "JIS90Form" => "jp90",
        "JIS04Form" => "jp04",
        _ => "",
    };
    if !form.is_empty() {
        v.extend(feature(form));
    }
    for f in &p.otf_features {
        v.extend(feature(f));
    }
    let positional = match p.positional_form.as_str() {
        "Initial" => Some("init"),
        "Medial" => Some("medi"),
        "Final" => Some("fina"),
        "Isolated" => Some("isol"),
        _ => None,
    };
    if let Some(tag) = positional {
        for form in ["isol", "init", "medi", "fina"] {
            v.extend(feature(&format!("{}{}", if form == tag { "" } else { "-" }, form)));
        }
    }
    v
}
