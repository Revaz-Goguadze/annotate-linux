use crate::model::arrow;
use crate::model::geom::{Point, Rect};
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
}

impl Object {
    pub fn new(id: ObjectId, kind: ObjectKind, style: Style) -> Self {
        let bounds = bounds_of(&kind, &style);
        Self { id, kind, style, bounds }
    }

    pub fn recompute_bounds(&mut self) {
        self.bounds = bounds_of(&self.kind, &self.style);
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
    };
    // Half the stroke width hangs outside the path on each side, +2 px AA.
    geo.inflate(style.width / 2.0 + 2.0)
}
