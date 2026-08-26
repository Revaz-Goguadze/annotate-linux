//! Logical-pixel geometry primitives. All coordinates are f64 logical px
//! (the same space pointer events arrive in; cairo scales to buffer px).

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn dist(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// Normalized rect from two arbitrary corners (drag anchor + cursor).
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (a.x - b.x).abs(),
            h: (a.y - b.y).abs(),
        }
    }

    pub fn from_points(pts: &[Point]) -> Self {
        let mut it = pts.iter();
        let Some(first) = it.next() else { return Rect::default() };
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in it {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Grow by `m` on every side (negative shrinks; clamps at empty).
    pub fn inflate(self, m: f64) -> Rect {
        let w = (self.w + 2.0 * m).max(0.0);
        let h = (self.h + 2.0 * m).max(0.0);
        Rect::new(self.x - m, self.y - m, w, h)
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.x + self.w && p.y >= self.y && p.y <= self.y + self.h
    }

    pub fn intersects(self, other: Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    pub fn area(self) -> f64 {
        if self.is_empty() { 0.0 } else { self.w * self.h }
    }
}

/// Distance from point `p` to segment `ab`. Hit-testing backbone for
/// lines, arrows, and freehand polylines.
pub fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_corners_normalizes() {
        let r = Rect::from_corners(Point::new(10.0, 20.0), Point::new(2.0, 5.0));
        assert_eq!(r, Rect::new(2.0, 5.0, 8.0, 15.0));
    }

    #[test]
    fn union_exact() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.union(b), Rect::new(0.0, 0.0, 15.0, 15.0));
        // union with empty is identity
        assert_eq!(a.union(Rect::default()), a);
        assert_eq!(Rect::default().union(b), b);
    }

    #[test]
    fn inflate_exact() {
        let r = Rect::new(10.0, 10.0, 4.0, 4.0).inflate(3.0);
        assert_eq!(r, Rect::new(7.0, 7.0, 10.0, 10.0));
        // deflate past empty clamps
        assert!(Rect::new(0.0, 0.0, 4.0, 4.0).inflate(-3.0).is_empty());
    }

    #[test]
    fn intersects_and_contains() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.intersects(Rect::new(9.0, 9.0, 5.0, 5.0)));
        assert!(!a.intersects(Rect::new(11.0, 0.0, 5.0, 5.0)));
        assert!(a.contains(Point::new(10.0, 10.0)));
        assert!(!a.contains(Point::new(10.1, 10.0)));
    }

    #[test]
    fn seg_dist_exact() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(10.0, 0.0);
        assert_eq!(seg_dist(Point::new(5.0, 3.0), a, b), 3.0);
        // beyond the endpoint: distance to endpoint, not the infinite line
        assert_eq!(seg_dist(Point::new(13.0, 4.0), a, b), 5.0);
        // degenerate zero-length segment
        assert_eq!(seg_dist(Point::new(3.0, 4.0), a, a), 5.0);
    }

    #[test]
    fn bbox_of_points() {
        let pts = [Point::new(3.0, 7.0), Point::new(-1.0, 2.0), Point::new(4.0, 5.0)];
        assert_eq!(Rect::from_points(&pts), Rect::new(-1.0, 2.0, 5.0, 5.0));
        assert_eq!(Rect::from_points(&[]), Rect::default());
    }
}
