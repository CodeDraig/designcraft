//! Proof colors, transparency blend spaces and separation previews.

use super::{Rect, Rendered, vello_cpu};

/// sRGB → working CMYK through a 17³ table built once per colour settings (trilinear).
fn cmyk_lut(rgb: [f32; 3]) -> [f32; 4] {
    const N: usize = 17;
    type Lut = (designcraft_color::cms::ColorSettings, std::sync::Arc<Vec<[f32; 4]>>);
    static LUT: std::sync::Mutex<Option<Lut>> = std::sync::Mutex::new(None);
    let settings = designcraft_color::cms::active_settings();
    let table = {
        let mut g = LUT.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_ref() {
            Some((s, t)) if *s == settings => t.clone(),
            _ => {
                let cms = designcraft_color::cms::active();
                let step = |i: usize| i as f32 / (N - 1) as f32;
                let t: Vec<[f32; 4]> = (0..N * N * N)
                    .map(|i| {
                        cms.srgb_to_cmyk([step(i / (N * N)), step(i / N % N), step(i % N)], designcraft_color::cms::Intent::RelativeColorimetric)
                    })
                    .collect();
                let t = std::sync::Arc::new(t);
                *g = Some((settings, t.clone()));
                t
            }
        }
    };
    let f = rgb.map(|v| v.clamp(0.0, 1.0) * (N - 1) as f32);
    let i = f.map(|v| (v as usize).min(N - 2));
    let d = [f[0] - i[0] as f32, f[1] - i[1] as f32, f[2] - i[2] as f32];
    let mut out = [0.0f32; 4];
    for (corner, w) in (0..8).map(|c| {
        let (a, b, cc) = (c >> 2 & 1, c >> 1 & 1, c & 1);
        let w = (if a == 1 { d[0] } else { 1.0 - d[0] }) * (if b == 1 { d[1] } else { 1.0 - d[1] }) * (if cc == 1 { d[2] } else { 1.0 - d[2] });
        ((i[0] + a) * N * N + (i[1] + b) * N + i[2] + cc, w)
    }) {
        for k in 0..4 {
            out[k] += table[corner][k] * w;
        }
    }
    out
}

/// An image's ink coverage on one process plate, as grey (cached per image and plate).
pub(super) fn plate_pixmap(pm: &std::sync::Arc<vello_cpu::Pixmap>, plate: u8) -> std::sync::Arc<vello_cpu::Pixmap> {
    type Cache = std::collections::HashMap<(usize, u8), std::sync::Arc<vello_cpu::Pixmap>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (std::sync::Arc::as_ptr(pm) as usize, plate);
    if let Some(v) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&key).cloned()) {
        return v;
    }
    let data: Vec<vello_cpu::color::PremulRgba8> = pm
        .data()
        .iter()
        .map(|p| {
            if p.a == 0 {
                return *p;
            }
            let k = 255.0 / p.a as f32;
            let rgb = [p.r, p.g, p.b].map(|v| (v as f32 * k / 255.0).min(1.0));
            let cmyk = cmyk_lut(rgb);
            let v = ((1.0 - cmyk[plate.min(3) as usize].clamp(0.0, 1.0)) * p.a as f32).round() as u8;
            vello_cpu::color::PremulRgba8 { r: v, g: v, b: v, a: p.a }
        })
        .collect();
    let out = std::sync::Arc::new(vello_cpu::Pixmap::from_parts(data, pm.width(), pm.height()));
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = c.get_or_insert_with(Default::default);
    if map.len() > 32 {
        map.clear();
    }
    map.insert(key, out.clone());
    out
}

/// Separations Preview on a rendered image: `plate` 0–3 shows that process plate (C, M, Y, K) as
/// ink density in grey; with an `ink_limit` (0–4, total ink) areas over it are shown in red.
/// Pixels are separated with a plain GCR (black = 1 − max(r, g, b)), so it's a preview.
/// View › Proof Colors: simulate the proof target (a press, another RGB space, colour-vision
/// deficiencies) through the active colour settings' proof table.
pub fn proof_view(img: &mut Rendered, setup: &designcraft_color::cms::ProofSetup) {
    let lut = designcraft_color::cms::active().proof_lut(setup);
    for px in img.pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 0 {
            continue;
        }
        // Premultiplied → straight, proof, back.
        let k = 255.0 / a as f32;
        let rgb = [px[0], px[1], px[2]].map(|v| ((v as f32 * k).round()).min(255.0) as u8);
        let out = lut.apply8(rgb);
        for i in 0..3 {
            px[i] = ((out[i] as u32 * a as u32 + 127) / 255) as u8;
        }
    }
}

/// A CMYK transparency blend space on screen: the pixels of `area` (device space) through the
/// working CMYK profile and back, as a spread composited in that space looks.
pub(super) fn blend_space_view(img: &mut Rendered, area: Rect) {
    use designcraft_color::cms::{ProofSetup, ProofTarget};
    let lut = designcraft_color::cms::active().proof_lut(&ProofSetup { target: ProofTarget::WorkingCmyk, ..Default::default() });
    let (w, h) = (img.width as i64, img.height as i64);
    let (x0, y0) = ((area.x0.floor() as i64).clamp(0, w), (area.y0.floor() as i64).clamp(0, h));
    let (x1, y1) = ((area.x1.ceil() as i64).clamp(0, w), (area.y1.ceil() as i64).clamp(0, h));
    for y in y0..y1 {
        let row = &mut img.pixels[((y * w + x0) * 4) as usize..((y * w + x1) * 4) as usize];
        for px in row.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a == 0 {
                continue;
            }
            let k = 255.0 / a as f32;
            let rgb = [px[0], px[1], px[2]].map(|v| ((v as f32 * k).round()).min(255.0) as u8);
            let out = lut.apply8(rgb);
            for i in 0..3 {
                px[i] = ((out[i] as u32 * a as u32 + 127) / 255) as u8;
            }
        }
    }
}

pub fn separation_view(img: &mut Rendered, plate: Option<u8>, ink_limit: Option<f32>) {
    for px in img.pixels.as_chunks_mut::<4>().0 {
        let a = px[3] as f32 / 255.0;
        // Composite over paper white, then separate.
        let ch = |v: u8| (v as f32 / 255.0) + (1.0 - a);
        let (r, g, b) = (ch(px[0]).min(1.0), ch(px[1]).min(1.0), ch(px[2]).min(1.0));
        let k = 1.0 - r.max(g).max(b);
        let inks =
            if k >= 0.999 { [0.0, 0.0, 0.0, 1.0] } else { [(1.0 - r - k) / (1.0 - k), (1.0 - g - k) / (1.0 - k), (1.0 - b - k) / (1.0 - k), k] };
        let mut out = match plate {
            Some(p) => {
                let v = ((1.0 - inks[p.min(3) as usize].clamp(0.0, 1.0)) * 255.0).round() as u8;
                [v, v, v, 255]
            }
            None => [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255],
        };
        if let Some(limit) = ink_limit
            && inks.iter().sum::<f32>() > limit
        {
            out = [230, 40, 40, 255];
        }
        px.copy_from_slice(&out);
    }
}
