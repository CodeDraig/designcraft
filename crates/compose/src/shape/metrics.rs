//! Effective type size, font metrics, and control glyphs.

use super::{Arc, CharProps, FaceRef, FontFace, Glyph, Leading, Position};

pub(super) fn metrics(face: &FontFace, p: &CharProps, auto_leading: TypeEnv) -> (f64, f64, f64, f64, f64, f64, f64, f64) {
    let (size, shift) = effective_size(p, &auto_leading.adv);
    let k = size / face.upem;
    let vs = p.v_scale;
    let leading = match p.leading {
        Leading::Auto => p.size * auto_leading.auto_leading,
        Leading::Points(v) => v,
    };
    (size, k, face.ascent * k * vs, face.descent * k * vs, leading, face.cap_height * k * vs, face.x_height * k * vs, shift)
}

/// Auto leading and the document's Advanced Type settings, for shaping.
#[derive(Clone, Copy, Debug)]
pub struct TypeEnv {
    pub auto_leading: f64,
    pub adv: designcraft_doc::AdvancedType,
    /// Characters the font lacks are drawn from fallback fonts (else as its .notdef glyph).
    pub glyph_fallback: bool,
}

/// Synthesised super/subscript: size and shift from Advanced Type (percent of the font size).
pub(super) fn effective_size(p: &CharProps, adv: &designcraft_doc::AdvancedType) -> (f64, f64) {
    match p.position {
        Position::Superscript => (p.size * adv.superscript_size / 100.0, p.baseline_shift + p.size * adv.superscript_position / 100.0),
        Position::Subscript => (p.size * adv.subscript_size / 100.0, p.baseline_shift - p.size * adv.subscript_position / 100.0),
        _ => (p.size, p.baseline_shift),
    }
}

pub(super) fn control_glyph(face: &Arc<FontFace>, p: &CharProps, auto_leading: TypeEnv, style: u32, byte: usize, ch: char) -> Glyph {
    let (size, k, ascent, descent, leading, cap, xh, shift) = metrics(face, p, auto_leading);
    Glyph {
        face: FaceRef::of(face),
        gid: face.glyph_for(' '),
        byte,
        len: ch.len_utf8(),
        ch,
        adv: 0.0,
        dx: 0.0,
        dy: 0.0,
        sx: k * p.h_scale,
        sy: k * p.v_scale,
        shift,
        ascent,
        descent,
        leading,
        cap,
        xh,
        size,
        style,
        no_break: p.no_break,
        locked_advance: p.jidori > 0,
        tcy: p.tate_chu_yoko.then_some([0.0; 3]),
        tcy_offset: [p.tate_chu_yoko_x_offset, p.tate_chu_yoko_y_offset],
        space: face.advance(face.glyph_for(' ')) * k * p.h_scale,
        break_after: None,
        cjk_hang: 0.0,
        ideographic_space_elastic: false,
        character_alignment: p.character_alignment,
        leading_model: p.leading_model,
        character_direction: p.character_direction,
        display_char: ch,
        bidi_offset: 0,
        bidi_end: 0,
        allow_kashidas: p.allow_kashidas,
        safe_tatweel_before: false,
        shaping_rtl: false,
        lang: designcraft_doc::language_tag(&p.language),
        upright: false,
    }
}
