//! Position and justify a shaped line, including CJK metrics, tabs and bidi order.

use super::{Align, DEFAULT_TAB, Glyph, ParaProps, PlacedGlyph, ScopedFonts, Spacing, TabAlign, breaker, shape, story};

/// Breaker parameters from the paragraph's settings.
pub(super) fn spacing_for(pp: &ParaProps, base_size: f64) -> Spacing {
    Spacing {
        justify: pp.align.is_justified(),
        word_min: pp.word_space_min,
        word_desired: pp.word_space_desired,
        word_max: pp.word_space_max,
        letter_min: pp.letter_space_min.min(pp.letter_space_desired),
        letter_desired: pp.letter_space_desired,
        letter_max: pp.letter_space_max.max(pp.letter_space_desired),
        glyph_min: pp.glyph_scale_min.min(pp.glyph_scale_desired).max(0.01),
        glyph_desired: pp.glyph_scale_desired.max(0.01),
        glyph_max: pp.glyph_scale_max.max(pp.glyph_scale_desired),
        hyphen_penalty: 50.0 + 450.0 * pp.hyph_weight,
        hyphen_limit: pp.hyph_limit,
        ragged_stretch: base_size * 2.0,
        hyph_zone: if pp.align.is_justified() { 0.0 } else { pp.hyph_zone },
        optical: pp.optical_margin,
        korean_char_breaks: pp.korean_char_breaks,
    }
}

/// Desired word spacing, letter spacing and glyph scaling apply to every line (any alignment).
pub(super) fn apply_desired_spacing(glyphs: &mut [Glyph], pp: &ParaProps) {
    for i in 0..glyphs.len() {
        let (left, right) = glyphs.split_at_mut(i + 1);
        let Some(g) = left.last_mut() else { continue };
        if pp.kinsoku_hang != designcraft_doc::cjk::KinsokuHang::None && pp.kinsoku.as_ref().is_some_and(|k| k.hanging.contains(g.ch)) {
            g.cjk_hang = g.adv;
        }
        g.ideographic_space_elastic = pp.treat_ideographic_space_as_space;
        if g.ch == '\u{3000}' && pp.treat_ideographic_space_as_space {
            g.space = g.adv;
        }
        if let Some(next) = right.first() {
            let a = g.ch;
            let b = next.ch;
            // A kinsoku set rules where CJK text may break; Korean still breaks at spaces.
            let cjk = breaker::cjk_pair(g, next, pp.korean_char_breaks);
            if g.break_after != Some(false) {
                if let Some(set) = &pp.kinsoku {
                    if !set.allows(a, b) {
                        g.break_after = Some(false);
                    } else if cjk {
                        g.break_after = Some(true);
                    }
                }
                if (pp.bunri_kinshi && a == b && matches!(a, '.' | '-' | '…' | '‥' | '—' | '―')) || (pp.rensuuji && a.is_numeric() && b.is_numeric())
                {
                    g.break_after = Some(false);
                }
            }
        }
    }
    let ws = pp.word_space_desired;
    let ls = pp.letter_space_desired;
    let gs = pp.glyph_scale_desired.max(0.01);
    if (ws - 1.0).abs() < 1e-9 && ls.abs() < 1e-9 && (gs - 1.0).abs() < 1e-9 {
        return;
    }
    for g in glyphs {
        if g.locked_advance {
            continue;
        }
        if g.ch == ' ' || (g.ch == '\u{3000}' && g.ideographic_space_elastic) {
            g.adv += g.space * (ws - 1.0);
        } else if !g.is_space() && g.adv > 0.0 {
            g.adv = g.adv * gs + ls * g.space;
            g.sx *= gs;
            g.dx *= gs;
        }
    }
}

/// Em box relative to the baseline. Font ascender/descender proportions locate it;
/// the box's total extent is the em, rather than the font's optional line gap.
fn cjk_em_box(g: &Glyph) -> (f64, f64) {
    let height = g.face.units_per_em() * g.sy.abs();
    let top = -height * g.ascent / (g.ascent + g.descent).max(1e-9);
    (top, top + height)
}
fn cjk_alignment_shift(g: &Glyph, reference: &Glyph) -> f64 {
    use designcraft_doc::cjk::CharacterAlignment as A;
    let (t, b) = cjk_em_box(g);
    let (rt, rb) = cjk_em_box(reference);
    match g.character_alignment {
        A::Baseline => 0.0,
        A::EmTop => t - rt,
        A::EmCenter => (t + b - rt - rb) / 2.0,
        A::EmBottom => b - rb,
        A::IcfTop => reference.ascent - g.ascent,
        A::IcfBottom => g.descent - reference.descent,
    }
}
pub(super) fn cjk_line_reference(line: &[Glyph]) -> f64 {
    use designcraft_doc::cjk::LeadingModel as L;
    let Some(g) = line.iter().max_by(|a, b| a.size.total_cmp(&b.size)) else { return 0.0 };
    let (top, bottom) = cjk_em_box(g);
    match g.leading_model {
        L::Roman => 0.0,
        L::AkiBelow => bottom,
        L::AkiAbove => top,
        L::Center | L::CenterDown => (top + bottom) / 2.0,
    }
}

pub(super) fn line_metrics(
    line: &[Glyph],
    all: &[Glyph],
    s: usize,
    base_leading: f64,
    base_size: f64,
    db: &ScopedFonts<'_>,
    base: &designcraft_doc::CharProps,
) -> (f64, f64, f64) {
    let src: &[Glyph] = if line.is_empty() { all.get(s..(s + 1).min(all.len())).unwrap_or(&[]) } else { line };
    if src.is_empty() {
        let face = db.face(&base.font_family, &base.font_style);
        let k = base_size / face.upem;
        return (face.ascent * k, face.descent * k, base_leading);
    }
    let mut asc: f64 = 0.0;
    let mut desc: f64 = 0.0;
    let mut lead: f64 = 0.0;
    let reference = src.iter().max_by(|a, b| a.size.total_cmp(&b.size));
    for g in src {
        let shift = g.shift + reference.map_or(0.0, |r| cjk_alignment_shift(g, r));
        asc = asc.max(g.ascent + shift.max(0.0));
        desc = desc.max(g.descent - shift.min(0.0));
        lead = lead.max(g.leading);
    }
    (asc, desc, lead)
}

/// How far a line's word spacing falls outside `[min, max]` (ratios of the space width): 0 inside,
/// then 1–3 for up to 10%, 25% and beyond.
pub fn hj_severity(ratio: f64, min: f64, max: f64) -> u8 {
    let r = if ratio > max + 1e-6 {
        ratio / max.max(1e-6)
    } else if ratio < min - 1e-6 {
        min / ratio.max(1e-6)
    } else {
        return 0;
    };
    if r < 1.1 {
        1
    } else if r < 1.25 {
        2
    } else {
        3
    }
}

/// Position glyphs `s..e` within `[x0, x1]`; returns (glyphs, end x, word-space ratio).
///
/// Justified lines distribute the difference to the measure in priority order: word spaces up to
/// their limit, then letter spacing, then glyph scaling, and anything left over to the word spaces
/// (an H&J violation). With optical margin alignment, edge punctuation hangs outside `[x0, x1]`.
#[allow(clippy::too_many_arguments)]
pub(super) fn layout_line(
    glyphs: &[Glyph],
    s: usize,
    e: usize,
    hyphen: bool,
    x0: f64,
    x1: f64,
    tab_origin: f64,
    pp: &ParaProps,
    sp: &Spacing,
    last: bool,
    forced_mid: bool,
    left_page: bool,
    bidi_info: &unicode_bidi::BidiInfo<'_>,
) -> (Vec<PlacedGlyph>, f64, f64) {
    let mut line: Vec<Glyph> = glyphs[s..e.max(s)].to_vec();
    let reference = line.iter().max_by(|a, b| a.size.total_cmp(&b.size)).cloned();
    if let Some(reference) = reference {
        for g in &mut line {
            g.shift += cjk_alignment_shift(g, &reference);
        }
    }
    if hyphen && let Some(g) = line.last() {
        let h = shape::hyphen_after(g);
        line.push(h);
    }
    // Tabs: compute widths left to right.
    let measure = x1 - x0;
    let mut x = 0.0;
    let mut i = 0;
    let mut leaders: Vec<(usize, String)> = Vec::new();
    while i < line.len() {
        if line[i].ch == '\t' {
            let abs = x0 + x - tab_origin;
            let stop = pp.tabs.iter().find(|t| t.position > abs + 0.01).cloned();
            if let Some(t) = &stop
                && !t.leader.is_empty()
            {
                leaders.push((i, t.leader.clone()));
            }
            let (pos, align) = match &stop {
                Some(t) => (t.position, t.align),
                None => (((abs / DEFAULT_TAB).floor() + 1.0) * DEFAULT_TAB, TabAlign::Left),
            };
            // Width of the text after this tab up to the next tab / line end.
            let seg_end = line[i + 1..].iter().position(|g| g.ch == '\t').map_or(line.len(), |p| i + 1 + p);
            let seg_w: f64 = line[i + 1..seg_end].iter().map(|g| g.adv).sum();
            let target = pos + tab_origin - x0;
            let w = match align {
                TabAlign::Left => target - x,
                TabAlign::Right => target - x - seg_w,
                TabAlign::Center => target - x - seg_w / 2.0,
                TabAlign::Char => {
                    let ch = stop.as_ref().and_then(|t| t.align_on.chars().next()).unwrap_or('.');
                    let before: f64 = line[i + 1..seg_end].iter().take_while(|g| g.ch != ch).map(|g| g.adv).sum();
                    target - x - before
                }
            };
            line[i].adv = w.max(0.0);
        } else if line[i].ch == story::RIGHT_INDENT_TAB {
            let rest: f64 = line[i + 1..].iter().map(|g| g.adv).sum();
            line[i].adv = (measure - x - rest).max(0.0);
        }
        x += line[i].adv;
        i += 1;
    }
    let has_tab = line.iter().any(|g| g.ch == '\t' || g.ch == story::RIGHT_INDENT_TAB);
    // Optical margin alignment: the measure grows by the hang of the edge glyphs.
    let (mut x0, mut measure) = (x0, measure);
    if (sp.optical || pp.kinsoku_hang != designcraft_doc::cjk::KinsokuHang::None) && !has_tab {
        let first = line.iter().find(|g| g.adv > 0.0 && !g.is_space());
        let lastg = line.iter().rev().find(|g| g.adv > 0.0 && !g.is_space());
        let hl = if sp.optical { first.map_or(0.0, |g| breaker::hang(g.ch).0 * g.adv) } else { 0.0 };
        let optical_right = if sp.optical { lastg.map_or(0.0, |g| breaker::hang(g.ch).1 * g.adv) } else { 0.0 };
        let cjk_right = lastg.map_or(0.0, |g| g.cjk_hang);
        let cjk_right = if pp.kinsoku_hang == designcraft_doc::cjk::KinsokuHang::Regular {
            cjk_right.min((line.iter().map(|g| g.adv).sum::<f64>() - measure).max(0.0))
        } else {
            cjk_right
        };
        let hr = optical_right.max(cjk_right);
        x0 -= hl;
        measure += hl + hr;
    }
    let natural: f64 = line.iter().map(|g| g.adv).sum();
    let mut extra = measure - natural;
    let spaces: Vec<usize> = line.iter().enumerate().filter(|(_, g)| g.is_space() && !g.no_break).map(|(i, _)| i).collect();
    let align = match pp.align {
        Align::TowardsSpine => {
            if left_page {
                Align::Right
            } else {
                Align::Left
            }
        }
        Align::AwayFromSpine => {
            if left_page {
                Align::Left
            } else {
                Align::Right
            }
        }
        a => a,
    };
    let justify_this = align.is_justified() && (!last || align == Align::FullyJustified || forced_mid) && !has_tab;
    // A justified paragraph's last line may have been composed with shrunk spaces: shrink it too.
    let squeeze_last = align.is_justified() && !justify_this && !has_tab && extra < 0.0 && !spaces.is_empty();
    // Extra advance per glyph (word spaces and letter gaps) and horizontal scale per glyph.
    let mut add = vec![0.0; line.len()];
    let mut scale = vec![1.0; line.len()];
    let mut offset = 0.0;
    // Kashidas: in justified Arabic, the joins of words take the extra length first.
    let mut kashidas: Vec<(usize, f64)> = Vec::new();
    if justify_this && extra > 0.0 && pp.kashidas && pp.arabic_justification != "DefaultJustification" {
        let points = kashida_points(&line);
        if !points.is_empty() {
            let total = extra.min(points.iter().map(|&i| line[i].size * 1.5).sum());
            let per = total / points.len() as f64;
            kashidas = points.into_iter().map(|i| (i, per)).collect();
            extra -= total;
            for &(i, l) in &kashidas {
                add[i] += l;
            }
        }
    }
    if (justify_this || squeeze_last) && !spaces.is_empty() {
        distribute(&line, &spaces, extra, sp, &mut add, &mut scale);
    } else if justify_this && spaces.is_empty() && line.len() > 1 && !last {
        // Single word: Single Word Justification.
        match pp.single_word_justify {
            Align::FullyJustified => {
                let gaps: Vec<_> = line
                    .iter()
                    .enumerate()
                    .take(line.len() - 1)
                    .filter(|(_, g)| !g.locked_advance || g.break_after != Some(false))
                    .map(|(i, _)| i)
                    .collect();
                if !gaps.is_empty() {
                    let per = extra / gaps.len() as f64;
                    for i in gaps {
                        add[i] = per;
                    }
                }
            }
            Align::Center => offset = extra / 2.0,
            Align::Right => offset = extra,
            _ => {}
        }
    } else {
        let last_align = match align {
            Align::LeftJustified => Align::Left,
            Align::CenterJustified => Align::Center,
            Align::RightJustified => Align::Right,
            Align::FullyJustified => Align::Left,
            a => a,
        };
        offset = match last_align {
            Align::Center => extra / 2.0,
            Align::Right => extra,
            _ => 0.0,
        };
        // Overfull ragged lines are not shifted left of the slot.
        if offset < 0.0 {
            offset = 0.0;
        }
    }
    // Word-space ratio actually used (vs the space width), for H&J highlighting.
    let mut ratio_sum = 0.0;
    let mut ratio_n = 0usize;
    for &i in &spaces {
        let g = &line[i];
        if g.ch == ' ' && g.space > 0.0 {
            ratio_sum += (g.adv + add[i]) / g.space;
            ratio_n += 1;
        }
    }
    let ratio = if ratio_n > 0 { ratio_sum / ratio_n as f64 } else { 1.0 };
    let mut out = Vec::with_capacity(line.len());
    let mut x = x0 + offset;
    let bidi = bidi_info.has_rtl();
    let mut pens = Vec::with_capacity(if bidi { line.len() } else { 0 });
    for (i, g) in line.iter().enumerate() {
        if bidi {
            pens.push(x);
        }
        let mut p = place(g, x);
        if scale[i] != 1.0 {
            p.sx *= scale[i];
            p.x = x + g.dx * scale[i];
            p.adv *= scale[i];
        }
        p.adv += add[i];
        if let Some((_, l)) = kashidas.iter().find(|k| line[k.0].bidi_offset == g.bidi_offset) {
            // Move the whole cluster, including its attached marks, past the elongation.
            p.x += l;
        }
        x += p.adv;
        if let Some((_, l)) = kashidas.iter().find(|k| k.0 == i) {
            // The glyph keeps its own width; the gap before it is the tatweel's.
            p.adv -= l;
        }
        out.push(p);
    }
    if bidi {
        pens.push(x);
        reorder_visual(&line, &mut out, &pens, x0 + offset, bidi_info);
    }
    for (i, leader) in &leaders {
        if let Some((g, p)) = line.get(*i).zip(out.get(*i)) {
            tab_leader(g, leader, p.x, p.adv, tab_origin, &mut out);
        }
    }
    for (i, l) in kashidas {
        let first = (0..=i).rev().take_while(|&j| line[j].bidi_offset == line[i].bidi_offset).last().unwrap_or(i);
        let (g, at) = (&line[i], out[first].x - line[first].dx - l);
        let gid = designcraft_fonts::first_glyph(&g.face, &['\u{0640}']);
        let w = g.face.advance(gid) * g.sx;
        if gid == 0 || w <= 0.0 {
            continue;
        }
        // One tatweel stretched over the gap (overlapping its neighbours a little).
        let k = (l + 0.4) / w;
        out.push(PlacedGlyph {
            gid,
            x: at - 0.2,
            y: -g.shift,
            adv: 0.0,
            sx: g.sx * k,
            len: 0,
            generated: None,
            upright: false,
            tcy: None,
            ..place(g, at)
        });
    }
    (out, x, ratio)
}

/// Arabic letters that join the following letter (dual-joining).
fn joins_next(c: char) -> bool {
    matches!(c as u32,
        0x0626 | 0x0628 | 0x062A..=0x062E | 0x0633..=0x063F | 0x0641..=0x0647 | 0x0649 | 0x064A | 0x066E | 0x066F
        | 0x0678..=0x0687 | 0x069A..=0x06BF | 0x06C1 | 0x06CC | 0x06CE | 0x06D0 | 0x06D1 | 0x06FA..=0x06FC)
}

/// Arabic letters that join the preceding one (dual- or right-joining).
fn joins_prev(c: char) -> bool {
    joins_next(c)
        || matches!(c as u32,
            0x0622..=0x0625 | 0x0627 | 0x0629 | 0x062F..=0x0632 | 0x0648 | 0x0671..=0x0673 | 0x0675..=0x0677 | 0x0688..=0x0699
            | 0x06C0 | 0x06C3..=0x06CB | 0x06CD | 0x06CF | 0x06D2 | 0x06D3)
}

/// Where kashidas go on a line: one join per word, after a seen or sad when there is one, else the
/// word's last join (indices of the glyph before the join).
fn kashida_points(line: &[Glyph]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut best: Option<(bool, usize)> = None;
    for (i, g) in line.iter().enumerate() {
        if g.is_space() {
            if let Some((_, k)) = best.take() {
                out.push(k);
            }
            continue;
        }
        // Only consider boundaries between complete clusters. Marks with len=0
        // are part of their base cluster, not word boundaries or insertion sites.
        let Some(n) = line.get(i + 1) else { continue };
        if n.bidi_offset == g.bidi_offset || !g.allow_kashidas || !n.allow_kashidas || !n.safe_tatweel_before {
            continue;
        }
        if g.face.id() != n.face.id() || g.face.glyph_for('\u{0640}') == 0 {
            continue;
        }
        if !joins_next(g.ch) || !joins_prev(n.ch) || (g.ch == '\u{0644}' && matches!(n.ch, '\u{0622}' | '\u{0623}' | '\u{0625}' | '\u{0627}')) {
            continue;
        }
        let seen = matches!(g.ch as u32, 0x0633..=0x0636);
        if best.is_none_or(|(s, _)| !s || seen) {
            best = Some((seen, i));
        }
    }
    out.extend(best.map(|b| b.1));
    out
}

/// Bidi: put a laid-out line (glyphs in text order, `pens` their pen positions and then the end
/// position) into visual order from `start`, cluster by cluster (Unicode Bidirectional
/// Algorithm, rule L2).
fn reorder_visual(line: &[Glyph], out: &mut [PlacedGlyph], pens: &[f64], start: f64, info: &unicode_bidi::BidiInfo<'_>) {
    use unicode_bidi::{BidiInfo, Level};
    // Clusters: runs of glyphs from the same source character.
    let mut units: Vec<std::ops::Range<usize>> = Vec::new();
    for i in 0..line.len() {
        match units.last_mut() {
            Some(u) if line[u.start].bidi_offset == line[i].bidi_offset && line[u.start].byte == line[i].byte => u.end = i + 1,
            _ => units.push(i..i + 1),
        }
    }
    let Some(first) = line.first() else { return };
    let Some(para) = info.paragraphs.iter().find(|p| p.range.contains(&first.bidi_offset)) else { return };
    let end = line.iter().map(|g| g.bidi_end).max().unwrap_or(first.bidi_end).min(para.range.end);
    let resolved = info.reordered_levels(para, first.bidi_offset..end);
    let levels: Vec<Level> = units.iter().map(|u| resolved.get(line[u.start].bidi_offset).copied().unwrap_or(para.level)).collect();
    let order = BidiInfo::reorder_visual(&levels);
    let width = |u: &std::ops::Range<usize>| pens[u.end] - pens[u.start];
    let mut cur = start;
    let mut shifts = vec![0.0; line.len()];
    for &k in &order {
        let u = &units[k];
        for i in u.clone() {
            shifts[i] = cur - pens[u.start];
            out[i].rtl = levels[k].is_rtl();
        }
        cur += width(u);
    }
    for (p, d) in out.iter_mut().zip(shifts) {
        p.x += d;
    }
}

/// Fill a tab's gap with repeats of its leader, on a grid shared by all lines (so dots align).
fn tab_leader(tab: &Glyph, leader: &str, x: f64, w: f64, origin: f64, out: &mut Vec<PlacedGlyph>) {
    let glyphs: Vec<(u32, f64)> = leader
        .chars()
        .map(|c| {
            let gid = designcraft_fonts::first_glyph(&tab.face, &[c]);
            (gid, tab.face.advance(gid) * tab.sx)
        })
        .collect();
    let pw: f64 = glyphs.iter().map(|g| g.1).sum();
    if pw <= 0.01 || w < pw {
        return;
    }
    let mut at = origin + ((x - origin) / pw).ceil() * pw;
    let end = x + w;
    let mut n = 0;
    while at + pw <= end + 0.01 && n < 2000 {
        for &(gid, adv) in &glyphs {
            out.push(PlacedGlyph {
                face: tab.face,
                gid,
                x: at,
                y: -tab.shift + tab.dy,
                adv: 0.0,
                sx: tab.sx,
                sy: tab.sy,
                style: tab.style,
                byte: tab.byte,
                len: 0,
                generated: None,
                visible: true,
                upright: false,
                tcy: None,
                rtl: false,
            });
            at += adv;
        }
        n += 1;
    }
}

/// Share `extra` points among word spaces, letter gaps and glyph scaling (see [`layout_line`]).
fn distribute(line: &[Glyph], spaces: &[usize], extra: f64, sp: &Spacing, add: &mut [f64], scale: &mut [f64]) {
    let stretch = extra >= 0.0;
    let word: Vec<f64> = spaces
        .iter()
        .map(|&i| {
            let g = &line[i];
            if g.ch != ' ' && !(g.ch == '\u{3000}' && g.ideographic_space_elastic) {
                0.0
            } else if stretch {
                g.space * (sp.word_max - sp.word_desired).max(0.0)
            } else {
                g.space * (sp.word_desired - sp.word_min).max(0.0)
            }
        })
        .collect();
    // Letter gaps: between visible glyphs (not after the line's last glyph).
    let last_box = line.iter().rposition(|g| !g.is_space() && g.adv > 0.0).unwrap_or(0);
    let is_box = |i: usize, g: &Glyph| i < last_box && !g.is_space() && g.adv > 0.0 && (!g.locked_advance || g.break_after != Some(false));
    let letter: Vec<f64> = line
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if !is_box(i, g) {
                0.0
            } else if stretch {
                g.space * (sp.letter_max - sp.letter_desired).max(0.0)
            } else {
                g.space * (sp.letter_desired - sp.letter_min).max(0.0)
            }
        })
        .collect();
    let glyph: Vec<f64> = line
        .iter()
        .map(|g| {
            if g.is_space() || g.adv <= 0.0 || g.locked_advance {
                0.0
            } else {
                let natural = g.adv / sp.glyph_desired.max(0.01);
                natural * if stretch { (sp.glyph_max - sp.glyph_desired).max(0.0) } else { (sp.glyph_desired - sp.glyph_min).max(0.0) }
            }
        })
        .collect();
    let (yw, yl, yg) = (word.iter().sum::<f64>(), letter.iter().sum::<f64>(), glyph.iter().sum::<f64>());
    let mut rem = extra.abs();
    let tw = rem.min(yw);
    rem -= tw;
    let tl = rem.min(yl);
    rem -= tl;
    let tg = rem.min(yg);
    rem -= tg;
    let sign = if stretch { 1.0 } else { -1.0 };
    for (k, &i) in spaces.iter().enumerate() {
        let share = if yw > 1e-9 { tw * word[k] / yw } else { 0.0 };
        // Left over beyond every limit: shared equally by the word spaces.
        add[i] += sign * (share + rem / spaces.len() as f64);
    }
    if yl > 1e-9 && tl > 0.0 {
        for (i, l) in letter.iter().enumerate() {
            add[i] += sign * tl * l / yl;
        }
    }
    if yg > 1e-9 && tg > 0.0 {
        for (i, gcap) in glyph.iter().enumerate() {
            if *gcap > 0.0 {
                let w = line[i].adv;
                scale[i] = (w + sign * tg * gcap / yg) / w;
            }
        }
    }
}

fn place(g: &Glyph, x: f64) -> PlacedGlyph {
    let visible = !(g.ch == '\t'
        || g.ch == '\n'
        || breaker::is_forced(g.ch)
        || g.ch == story::INDENT_HERE
        || g.ch == story::RIGHT_INDENT_TAB
        || g.ch == story::TABLE_ANCHOR
        || g.ch == designcraft_doc::ANCHOR_MARK
        || g.ch == shape::SOFT_HYPHEN
        || g.ch == shape::HIDDEN);
    PlacedGlyph {
        face: g.face,
        gid: g.gid,
        x: x + g.dx,
        y: -g.shift + g.dy,
        adv: g.adv,
        sx: g.sx,
        sy: g.sy,
        style: g.style,
        byte: g.byte,
        len: g.len,
        generated: g.generated.clone(),
        visible,
        upright: g.upright,
        tcy: g.tcy,
        rtl: false,
    }
}
