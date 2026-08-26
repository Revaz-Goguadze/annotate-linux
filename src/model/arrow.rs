//! Arrowhead geometry: pure function from shaft endpoints + stroke width to
//! the filled head triangle.

use crate::model::geom::Point;

/// Barb half-angle ~28°.
const BARB_RAD: f64 = 0.49;
/// Head length scales with stroke width…
const WIDTH_FACTOR: f64 = 4.5;
/// …but never dominates a short arrow.
const MAX_SHAFT_FRACTION: f64 = 0.35;
const MIN_LEN: f64 = 6.0;

/// `[tip, left_barb, right_barb]` of the head triangle at `b`, pointing a→b.
/// Zero-length arrows get a degenerate head at `b`.
pub fn head_points(a: Point, b: Point, width: f64) -> [Point; 3] {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let shaft = dx.hypot(dy);
    if shaft == 0.0 {
        return [b, b, b];
    }
    let len = (WIDTH_FACTOR * width).max(MIN_LEN).min(MAX_SHAFT_FRACTION * shaft);
    let angle = dy.atan2(dx);
    let barb = |da: f64| {
        Point::new(
            b.x - len * (angle + da).cos(),
            b.y - len * (angle + da).sin(),
        )
    };
    [b, barb(BARB_RAD), barb(-BARB_RAD)]
}

/// Where the shaft should stop so it doesn't poke through the head tip.
pub fn shaft_end(a: Point, b: Point, width: f64) -> Point {
    let [_, l, r] = head_points(a, b, width);
    Point::new((l.x + r.x) / 2.0, (l.y + r.y) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_arrow_head_is_symmetric() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(100.0, 0.0);
        let [tip, l, r] = head_points(a, b, 4.0);
        assert_eq!(tip, b);
        // barbs behind the tip, mirrored across the shaft
        assert!(l.x < b.x && r.x < b.x);
        assert!((l.y + r.y).abs() < 1e-9);
        assert!((l.x - r.x).abs() < 1e-9);
        // width 4 → head len = 18, barb x = 100 - 18*cos(0.49)
        assert!((l.x - (100.0 - 18.0 * (0.49f64).cos())).abs() < 1e-9);
    }

    #[test]
    fn short_arrow_head_capped_to_shaft_fraction() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(10.0, 0.0);
        let [_, l, _] = head_points(a, b, 4.0);
        // cap: 0.35 * 10 = 3.5 (not 18)
        assert!((l.x - (10.0 - 3.5 * (0.49f64).cos())).abs() < 1e-9);
    }

    #[test]
    fn zero_length_is_degenerate() {
        let p = Point::new(5.0, 5.0);
        assert_eq!(head_points(p, p, 4.0), [p, p, p]);
    }
}
