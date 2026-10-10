use super::*;

/// Every fragment of a canonical annotation, including continuation pages and nested cells.
pub fn annotation_fragments(cs: &ComposedStory, id: u64) -> Vec<(usize, Point, Arc<ComposedStory>)> {
    fn walk(ft: &FrameText, id: u64, fi: usize, origin: Point, out: &mut Vec<(usize, Point, Arc<ComposedStory>)>, depth: usize) {
        if depth > 32 {
            return;
        }
        for n in &ft.notes {
            let at = origin + n.origin.to_vec2();
            if n.annotation == Some(id) {
                out.push((fi, at, n.text.clone()));
            }
            for f in &n.text.frames {
                walk(f, id, fi, at, out, depth + 1);
            }
        }
        for t in &ft.tables {
            for c in &t.cells {
                for f in &c.text.frames {
                    walk(f, id, fi, origin + c.origin.to_vec2(), out, depth + 1);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (fi, ft) in cs.frames.iter().enumerate() {
        walk(ft, id, fi, Point::ZERO, &mut out, 0);
    }
    out
}

pub fn hit_annotation(cs: &ComposedStory, fi: usize, p: Point) -> Option<(u64, usize)> {
    fn walk(ft: &FrameText, p: Point, depth: usize) -> Option<(u64, usize)> {
        if depth > 32 {
            return None;
        }
        for n in &ft.notes {
            if !n.rect.contains(p) {
                continue;
            }
            let local = p - n.origin.to_vec2();
            for f in &n.text.frames {
                if let Some(hit) = walk(f, local, depth + 1) {
                    return Some(hit);
                }
            }
            if let Some(id) = n.annotation {
                return Some((id, crate::hit(&n.text, 0, local).unwrap_or(n.source_range.start)));
            }
        }
        for t in &ft.tables {
            for c in &t.cells {
                if c.rect.contains(p) {
                    for f in &c.text.frames {
                        if let Some(hit) = walk(f, p - c.origin.to_vec2(), depth + 1) {
                            return Some(hit);
                        }
                    }
                }
            }
        }
        None
    }
    walk(cs.frames.get(fi)?, p, 0)
}
