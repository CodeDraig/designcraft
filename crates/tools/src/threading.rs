//! Shared port geometry and the Selection tool's loaded-text gesture.
use crate::{Action, Cursor, Overlay, PointerEvent, PointerKind, ToolContext, rect_json, spread_json};
use designcraft_doc::{Item, ItemId};
use designcraft_geom::{Affine, Point, Rect, Vec2};
use serde_json::json;

/// Canvas-space ports, with offsets measured in screen pixels along the transformed frame edge.
pub fn port_positions(item: &Item, parent: Affine, zoom: f64) -> (Point, Point) {
    let m = parent * item.xf;
    let edge = m * Point::new(0.0, 1.0) - m * Point::ORIGIN;
    let edge = if edge.hypot() > 1e-9 { edge / edge.hypot() } else { Vec2::new(0.0, 1.0) };
    let unit = edge / zoom.max(1e-6);
    let r = item.inner_bounds();
    (m * Point::new(r.x0, r.y0) + unit * 14.5, m * Point::new(r.x1, r.y1) - unit * 12.0)
}

fn port_at(cx: &ToolContext, p: Point) -> Option<ItemId> {
    cx.selection.items.iter().find_map(|id| {
        let it = cx.doc.item(*id)?;
        let story = cx.doc.story(it.text_frame()?.story)?;
        if story.frames.last() != Some(id) {
            return None;
        }
        let (_, out) = port_positions(it, cx.item_canvas_xf(*id)?, cx.zoom);
        let delta = p - out;
        (delta.x.abs() <= cx.tol(6.0) && delta.y.abs() <= cx.tol(6.0)).then_some(*id)
    })
}

/// Locked visible frames remain hit targets so they report an error instead of looking like blank canvas.
fn target_at(cx: &ToolContext, p: Point) -> Option<ItemId> {
    let (spread, point) = cx.layout.spread_at(p)?;
    let mut items: Vec<_> = cx.doc.spread(spread)?.items.iter().collect();
    items.sort_by_key(|it| cx.doc.layer_rank(it.layer));
    items
        .into_iter()
        .rev()
        .find(|it| !it.hidden && cx.doc.layer(it.layer).is_some_and(|l| l.visible) && designcraft_doc::edit_hit(it, point, cx.tol(4.0)))
        .map(|it| it.id)
}

#[derive(Default)]
pub(crate) struct Threading {
    from: Option<ItemId>,
    start: Option<Point>,
    current: Option<Point>,
}
impl Threading {
    pub fn busy(&self) -> bool {
        self.from.is_some()
    }
    pub fn cancel(&mut self) -> bool {
        let active = self.busy();
        *self = Self::default();
        active
    }
    pub fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Option<Vec<Action>> {
        let Some(from) = self.from else {
            if ev.kind == PointerKind::Down
                && let Some(from) = port_at(cx, ev.pos)
            {
                if cx.doc.validate_thread(from, None).is_err() {
                    return Some(vec![Action::Exec("frame.thread".into(), json!({"from": from.0, "to": from.0}))]);
                }
                self.from = Some(from);
                return Some(vec![]);
            }
            return None;
        };
        if cx.doc.item(from).is_none() {
            self.cancel();
            return Some(vec![]);
        }
        match ev.kind {
            PointerKind::Down => {
                if let Some(to) = target_at(cx, ev.pos) {
                    if cx.doc.validate_thread(from, Some(to)).is_ok() {
                        self.cancel();
                    }
                    return Some(vec![Action::Exec("frame.thread".into(), json!({"from": from.0, "to": to.0}))]);
                }
                self.start = Some(ev.pos);
                self.current = Some(ev.pos);
            }
            PointerKind::Drag => {
                if self.start.is_some() {
                    self.current = Some(ev.pos);
                }
            }
            PointerKind::Up => {
                if let Some(start) = self.start.take() {
                    self.current = None;
                    if (ev.pos - start).hypot() >= cx.tol(3.0)
                        && let Some((spread, a)) = cx.layout.spread_at(start)
                    {
                        let b = cx.layout.to_spread(spread, ev.pos);
                        let r = Rect::from_points(a, b);
                        if r.width() >= 0.5 && r.height() >= 0.5 {
                            self.cancel();
                            return Some(vec![Action::Exec(
                                "frame.thread".into(),
                                json!({"from": from.0, "spread": spread_json(spread), "rect": rect_json(r)}),
                            )]);
                        }
                    }
                }
            }
            _ => {}
        }
        Some(vec![])
    }
    pub fn cursor(&self, cx: &ToolContext, p: Point) -> Option<Cursor> {
        if let Some(from) = self.from {
            let forbidden = target_at(cx, p).is_some_and(|to| cx.doc.validate_thread(from, Some(to)).is_err());
            Some(if forbidden { Cursor::NotAllowed } else { Cursor::LoadedText })
        } else {
            port_at(cx, p).map(|_| Cursor::LoadedText)
        }
    }
    pub fn overlays(&self) -> Vec<Overlay> {
        self.start.zip(self.current).map(|(a, b)| vec![Overlay::Marquee(Rect::from_points(a, b))]).unwrap_or_default()
    }
}
