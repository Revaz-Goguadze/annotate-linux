//! Modifier-driven shape constraints: Shift snaps lines/arrows to 45° and
//! shapes to square/circle; Alt expands rects/ellipses from the center.

use crate::model::geom::{Point, Rect};
use crate::model::object::ObjectKind;
use crate::input::Tool;

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

/// Resolve a drag (anchor → cur) into the shape it currently represents.
/// Freehand tools are not handled here (they accumulate points).
pub fn resolve(tool: Tool, anchor: Point, cur: Point, mods: Mods) -> Option<ObjectKind> {
    match tool {
        Tool::Line => Some(ObjectKind::Line { a: anchor, b: endpoint(anchor, cur, mods) }),
        Tool::Arrow => Some(ObjectKind::Arrow { a: anchor, b: endpoint(anchor, cur, mods) }),
        Tool::Rect => Some(ObjectKind::Rect { r: rect(anchor, cur, mods) }),
        Tool::Ellipse => Some(ObjectKind::Ellipse { r: rect(anchor, cur, mods) }),
        _ => None,
    }
}

/// Shift: snap the segment direction to the nearest 45° while keeping its length.
fn endpoint(anchor: Point, cur: Point, mods: Mods) -> Point {
    if !mods.shift {
        return cur;
    }
    let (dx, dy) = (cur.x - anchor.x, cur.y - anchor.y);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return cur;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let snapped = (dy.atan2(dx) / step).round() * step;
    Point::new(anchor.x + len * snapped.cos(), anchor.y + len * snapped.sin())
}

/// Shift: square (side = larger delta, direction preserved).
/// Alt: anchor is the center instead of a corner.
fn rect(anchor: Point, cur: Point, mods: Mods) -> Rect {
    let (mut dx, mut dy) = (cur.x - anchor.x, cur.y - anchor.y);
    if mods.shift {
        let side = dx.abs().max(dy.abs());
        dx = side * if dx < 0.0 { -1.0 } else { 1.0 };
        dy = side * if dy < 0.0 { -1.0 } else { 1.0 };
    }
    if mods.alt {
        Rect::from_corners(
            Point::new(anchor.x - dx, anchor.y - dy),
            Point::new(anchor.x + dx, anchor.y + dy),
        )
    } else {
        Rect::from_corners(anchor, Point::new(anchor.x + dx, anchor.y + dy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods { shift: false, ctrl: false, alt: false, logo: false };
    const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false, logo: false };
    const ALT: Mods = Mods { shift: false, ctrl: false, alt: true, logo: false };
    const SHIFT_ALT: Mods = Mods { shift: true, ctrl: false, alt: true, logo: false };

    #[test]
    fn line_shift_snaps_to_45() {
        let a = Point::new(0.0, 0.0);
        // 40° off-axis drag snaps to 45°, length preserved
        let cur = Point::new(10.0f64.to_radians().cos(), 0.0);
        let Some(ObjectKind::Line { b, .. }) =
            resolve(Tool::Line, a, Point::new(100.0, 84.0), SHIFT)
        else {
            panic!()
        };
        let len = (100.0f64.powi(2) + 84.0f64.powi(2)).sqrt();
        let exp = len * std::f64::consts::FRAC_1_SQRT_2;
        assert!((b.x - exp).abs() < 1e-9 && (b.y - exp).abs() < 1e-9);
        let _ = cur;
    }

    #[test]
    fn line_shift_snaps_to_horizontal() {
        let Some(ObjectKind::Line { b, .. }) =
            resolve(Tool::Line, Point::new(0.0, 0.0), Point::new(100.0, 10.0), SHIFT)
        else {
            panic!()
        };
        assert!((b.y - 0.0).abs() < 1e-9);
        assert!((b.x - (100.0f64.powi(2) + 100.0).sqrt()).abs() < 1e-9);
    }

    #[test]
    fn rect_shift_is_square() {
        let Some(ObjectKind::Rect { r }) =
            resolve(Tool::Rect, Point::new(10.0, 10.0), Point::new(50.0, 20.0), SHIFT)
        else {
            panic!()
        };
        assert_eq!(r, Rect::new(10.0, 10.0, 40.0, 40.0));
    }

    #[test]
    fn rect_shift_negative_direction_preserved() {
        let Some(ObjectKind::Rect { r }) =
            resolve(Tool::Rect, Point::new(10.0, 10.0), Point::new(-30.0, 5.0), SHIFT)
        else {
            panic!()
        };
        assert_eq!(r, Rect::new(-30.0, -30.0, 40.0, 40.0));
    }

    #[test]
    fn rect_alt_center_expands() {
        let Some(ObjectKind::Rect { r }) =
            resolve(Tool::Rect, Point::new(50.0, 50.0), Point::new(70.0, 60.0), ALT)
        else {
            panic!()
        };
        assert_eq!(r, Rect::new(30.0, 40.0, 40.0, 20.0));
    }

    #[test]
    fn ellipse_shift_alt_is_centered_circle_bbox() {
        let Some(ObjectKind::Ellipse { r }) =
            resolve(Tool::Ellipse, Point::new(50.0, 50.0), Point::new(80.0, 60.0), SHIFT_ALT)
        else {
            panic!()
        };
        assert_eq!(r, Rect::new(20.0, 20.0, 60.0, 60.0));
    }

    #[test]
    fn no_mods_plain_rect() {
        let Some(ObjectKind::Rect { r }) =
            resolve(Tool::Rect, Point::new(10.0, 20.0), Point::new(0.0, 5.0), NONE)
        else {
            panic!()
        };
        assert_eq!(r, Rect::new(0.0, 5.0, 10.0, 15.0));
    }
}
