use crate::model::arrow;
use crate::model::geom::{Point, Rect, seg_dist};
use crate::util::color::Rgba;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ObjectId(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectKind {
    /// Pen and highlighter strokes (highlighter differs only in Style).
    Freehand { pts: Vec<Point> },
    Line { a: Point, b: Point },
    Arrow { a: Point, b: Point },
    Rect { r: Rect },
    Ellipse { r: Rect },
    /// Numbered badge; `r` is the circle radius.
    Counter { at: Point, n: u32, r: f64 },
    /// Single-line text; `at` is the top-left corner, `px` the font size.
    Text { at: Point, s: String, px: f64 },
}

impl ObjectKind {
    /// Translate by (dx, dy). Used by move/reposition drags.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        let mv = |p: &mut Point| {
            p.x += dx;
            p.y += dy;
        };
        match self {
            ObjectKind::Freehand { pts } => pts.iter_mut().for_each(mv),
            ObjectKind::Line { a, b } | ObjectKind::Arrow { a, b } => {
                mv(a);
                mv(b);
            }
            ObjectKind::Rect { r } | ObjectKind::Ellipse { r } => {
                r.x += dx;
                r.y += dy;
            }
            ObjectKind::Counter { at, .. } | ObjectKind::Text { at, .. } => mv(at),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Style {
    pub stroke: Rgba,
    pub width: f64,
    /// 1.0 for pen; highlighter strokes composite the whole object at this
    /// alpha so self-crossings don't double-darken.
    pub group_alpha: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub id: ObjectId,
    pub kind: ObjectKind,
    pub style: Style,
    /// Cached extent incl. stroke width + arrowhead + AA margin.
    pub bounds: Rect,
    /// Creation time (fade-mode clock).
    pub born: std::time::Instant,
}

impl Object {
    pub fn new(id: ObjectId, kind: ObjectKind, style: Style) -> Self {
        let bounds = bounds_of(&kind, &style);
        Self { id, kind, style, bounds, born: std::time::Instant::now() }
    }

    pub fn recompute_bounds(&mut self) {
        self.bounds = bounds_of(&self.kind, &self.style);
    }

    /// Precise-enough hit test for select/erase. `tol` is extra slack on
    /// top of the stroke width so thin objects stay clickable.
    pub fn hit_test(&self, p: Point, tol: f64) -> bool {
        if !self.bounds.inflate(tol).contains(p) {
            return false;
        }
        let reach = (self.style.width / 2.0).max(tol);
        match &self.kind {
            ObjectKind::Freehand { pts } => match pts.len() {
                0 => false,
                1 => p.dist(pts[0]) <= reach,
                _ => pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= reach),
            },
            ObjectKind::Line { a, b } | ObjectKind::Arrow { a, b } => seg_dist(p, *a, *b) <= reach,
            ObjectKind::Rect { r } => {
                // near the border, not the hollow inside
                let outer = r.inflate(reach);
                let inner = r.inflate(-reach);
                outer.contains(p) && (inner.is_empty() || !inner.contains(p))
            }
            ObjectKind::Ellipse { r } => {
                if r.w <= 0.0 || r.h <= 0.0 {
                    return false;
                }
                let (a, b) = (r.w / 2.0, r.h / 2.0);
                let (cx, cy) = (r.x + a, r.y + b);
                let v = (((p.x - cx) / a).powi(2) + ((p.y - cy) / b).powi(2)).sqrt();
                ((v - 1.0) * a.min(b)).abs() <= reach
            }
            // solid objects: anywhere inside counts
            ObjectKind::Counter { at, r, .. } => p.dist(*at) <= r + tol,
            ObjectKind::Text { .. } => self.bounds.inflate(tol).contains(p),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(width: f64) -> Style {
        Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width, group_alpha: 1.0 }
    }

    #[test]
    fn thin_line_clickable_with_tolerance() {
        let o = Object::new(
            ObjectId(1),
            ObjectKind::Line { a: Point::new(0.0, 0.0), b: Point::new(100.0, 0.0) },
            style(1.0),
        );
        assert!(o.hit_test(Point::new(50.0, 5.0), 6.0), "within 6px slack");
        assert!(!o.hit_test(Point::new(50.0, 12.0), 6.0));
    }

    #[test]
    fn rect_border_hits_inside_misses() {
        let o = Object::new(
            ObjectId(1),
            ObjectKind::Rect { r: Rect::new(10.0, 10.0, 100.0, 100.0) },
            style(4.0),
        );
        assert!(o.hit_test(Point::new(10.0, 60.0), 6.0), "left border");
        assert!(!o.hit_test(Point::new(60.0, 60.0), 6.0), "hollow center");
    }

    #[test]
    fn ellipse_curve_hits_center_misses() {
        let o = Object::new(
            ObjectId(1),
            ObjectKind::Ellipse { r: Rect::new(0.0, 0.0, 100.0, 60.0) },
            style(4.0),
        );
        assert!(o.hit_test(Point::new(50.0, 0.0), 6.0), "top of curve");
        assert!(!o.hit_test(Point::new(50.0, 30.0), 6.0), "center");
    }

    #[test]
    fn text_and_counter_hit_inside() {
        let t = Object::new(
            ObjectId(1),
            ObjectKind::Text { at: Point::new(0.0, 0.0), s: "hi".into(), px: 24.0 },
            style(2.0),
        );
        assert!(t.hit_test(Point::new(10.0, 10.0), 6.0));
        let c = Object::new(
            ObjectId(2),
            ObjectKind::Counter { at: Point::new(50.0, 50.0), n: 1, r: 16.0 },
            style(2.0),
        );
        assert!(c.hit_test(Point::new(55.0, 55.0), 6.0));
        assert!(!c.hit_test(Point::new(80.0, 50.0), 6.0));
    }

    #[test]
    fn translate_moves_bounds() {
        let mut o = Object::new(
            ObjectId(1),
            ObjectKind::Rect { r: Rect::new(0.0, 0.0, 10.0, 10.0) },
            style(2.0),
        );
        let before = o.bounds;
        o.kind.translate(5.0, 7.0);
        o.recompute_bounds();
        assert_eq!(o.bounds.x, before.x + 5.0);
        assert_eq!(o.bounds.y, before.y + 7.0);
    }
}

fn bounds_of(kind: &ObjectKind, style: &Style) -> Rect {
    let geo = match kind {
        ObjectKind::Freehand { pts } => Rect::from_points(pts),
        ObjectKind::Line { a, b } => Rect::from_corners(*a, *b),
        ObjectKind::Arrow { a, b } => {
            let mut r = Rect::from_corners(*a, *b);
            for p in arrow::head_points(*a, *b, style.width) {
                r = r.union(Rect::new(p.x, p.y, 0.0, 0.0));
            }
            r
        }
        ObjectKind::Rect { r } | ObjectKind::Ellipse { r } => *r,
        ObjectKind::Counter { at, r, .. } => Rect::new(at.x - r, at.y - r, 2.0 * r, 2.0 * r),
        ObjectKind::Text { at, s, px } => {
            // Toy-font estimate: generous per-char advance so damage and
            // hit-testing stay conservative without a cairo measurement.
            let w = (s.chars().count() as f64).max(1.0) * px * 0.65 + 4.0;
            Rect::new(at.x, at.y, w, px * 1.4)
        }
    };
    // Half the stroke width hangs outside the path on each side, +2 px AA.
    geo.inflate(style.width / 2.0 + 2.0)
}
