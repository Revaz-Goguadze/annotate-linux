//! Cursor spotlight, click ripples, and drawn cursor glyphs.

use crate::model::geom::{Point, Rect};
use crate::util::color::Rgba;

pub const RIPPLE_MAX_R: f64 = 36.0;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CursorStyle {
    /// System cursor untouched, nothing drawn.
    #[default]
    Default,
    /// System cursor hidden, nothing drawn.
    None,
    Outline,
    Circle,
    Crosshair,
}

impl CursorStyle {
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "default" => CursorStyle::Default,
            "none" => CursorStyle::None,
            "outline" => CursorStyle::Outline,
            "circle" => CursorStyle::Circle,
            "crosshair" => CursorStyle::Crosshair,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            CursorStyle::Default => "default",
            CursorStyle::None => "none",
            CursorStyle::Outline => "outline",
            CursorStyle::Circle => "circle",
            CursorStyle::Crosshair => "crosshair",
        }
    }

    /// Styles that replace the system cursor with a drawn glyph.
    pub fn hides_system_cursor(self) -> bool {
        self != CursorStyle::Default
    }
}

pub struct CursorFx {
    pub pos: Point,
    pub style: CursorStyle,
    pub highlight: bool,
    pub highlight_radius: f64,
    pub color: Rgba,
}

impl CursorFx {
    /// Everything the cursor drawing can touch (for damage).
    pub fn bounds(&self) -> Rect {
        let r = if self.highlight { self.highlight_radius } else { 0.0 }.max(16.0) + 2.0;
        Rect::new(self.pos.x - r, self.pos.y - r, 2.0 * r, 2.0 * r)
    }
}

pub fn paint_cursor(cr: &cairo::Context, fx: &CursorFx) {
    let p = fx.pos;
    if fx.highlight {
        cr.set_source_rgba(1.0, 0.85, 0.2, 0.35);
        cr.new_path();
        cr.arc(p.x, p.y, fx.highlight_radius, 0.0, std::f64::consts::TAU);
        cr.fill().expect("highlight");
    }
    let c = fx.color;
    cr.set_source_rgba(c.r, c.g, c.b, 0.95);
    cr.set_line_width(2.0);
    match fx.style {
        CursorStyle::Default | CursorStyle::None => {}
        CursorStyle::Outline => {
            cr.new_path();
            cr.arc(p.x, p.y, 8.0, 0.0, std::f64::consts::TAU);
            cr.stroke().expect("outline");
        }
        CursorStyle::Circle => {
            cr.new_path();
            cr.arc(p.x, p.y, 5.0, 0.0, std::f64::consts::TAU);
            cr.fill().expect("dot");
        }
        CursorStyle::Crosshair => {
            cr.new_path();
            cr.move_to(p.x - 12.0, p.y);
            cr.line_to(p.x + 12.0, p.y);
            cr.move_to(p.x, p.y - 12.0);
            cr.line_to(p.x, p.y + 12.0);
            cr.stroke().expect("crosshair");
        }
    }
}

/// `t` in 0..1: expanding, fading ring.
pub fn paint_ripple(cr: &cairo::Context, at: Point, t: f64, color: Rgba) {
    let r = 6.0 + t * (RIPPLE_MAX_R - 6.0);
    cr.set_source_rgba(color.r, color.g, color.b, (1.0 - t) * 0.6);
    cr.set_line_width(3.0);
    cr.new_path();
    cr.arc(at.x, at.y, r, 0.0, std::f64::consts::TAU);
    cr.stroke().expect("ripple");
}

pub fn ripple_bounds(at: Point) -> Rect {
    let r = RIPPLE_MAX_R + 3.0;
    Rect::new(at.x - r, at.y - r, 2.0 * r, 2.0 * r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::test_surface::Canvas;

    fn fx(style: CursorStyle, highlight: bool) -> CursorFx {
        CursorFx {
            pos: Point::new(60.0, 60.0),
            style,
            highlight,
            highlight_radius: 40.0,
            color: Rgba::new(1.0, 0.0, 0.0, 1.0),
        }
    }

    const ALL: [CursorStyle; 5] = [
        CursorStyle::Default,
        CursorStyle::None,
        CursorStyle::Outline,
        CursorStyle::Circle,
        CursorStyle::Crosshair,
    ];

    #[test]
    fn names_round_trip() {
        for s in ALL {
            assert_eq!(CursorStyle::from_name(s.name()), Some(s));
        }
        assert_eq!(CursorStyle::from_name("pointer"), None);
        assert_eq!(CursorStyle::default(), CursorStyle::Default);
    }

    #[test]
    fn only_the_default_style_keeps_the_system_cursor() {
        assert!(!CursorStyle::Default.hides_system_cursor());
        for s in [CursorStyle::None, CursorStyle::Outline, CursorStyle::Circle, CursorStyle::Crosshair] {
            assert!(s.hides_system_cursor(), "{s:?}");
        }
    }

    #[test]
    fn bounds_center_on_the_cursor_with_a_glyph_floor() {
        let plain = fx(CursorStyle::Crosshair, false).bounds();
        assert_eq!(plain, Rect::new(42.0, 42.0, 36.0, 36.0), "16px glyph floor + 2px margin");

        let lit = fx(CursorStyle::Crosshair, true).bounds();
        assert_eq!(lit, Rect::new(18.0, 18.0, 84.0, 84.0), "highlight radius + 2px margin");
        assert!(lit.contains(Point::new(60.0, 60.0)));
    }

    #[test]
    fn drawn_styles_ink_the_cursor_position_and_default_does_not() {
        for style in ALL {
            let f = fx(style, false);
            let mut c = Canvas::new(120, 120);
            c.paint(|cr| paint_cursor(cr, &f));
            let drawn = c.ink() > 0;
            let expected = matches!(
                style,
                CursorStyle::Outline | CursorStyle::Circle | CursorStyle::Crosshair
            );
            assert_eq!(drawn, expected, "{style:?} ink mismatch");
        }
    }

    #[test]
    fn glyph_ink_stays_inside_the_damage_bounds() {
        for style in [CursorStyle::Outline, CursorStyle::Circle, CursorStyle::Crosshair] {
            let f = fx(style, false);
            let b = f.bounds();
            let mut c = Canvas::new(120, 120);
            c.paint(|cr| paint_cursor(cr, &f));
            let total = c.ink();
            let inside = c.ink_in(b.x as i32, b.y as i32, b.w as i32, b.h as i32);
            assert_eq!(inside, total, "{style:?} inked outside {b:?}");
        }
    }

    #[test]
    fn highlight_paints_a_translucent_spotlight_under_the_glyph() {
        let f = fx(CursorStyle::None, true);
        let mut c = Canvas::new(120, 120);
        c.paint(|cr| paint_cursor(cr, &f));
        // spotlight is drawn even when no glyph is
        let a = c.alpha_at(60, 30);
        assert!(a > 0 && a < 255, "spotlight must be translucent, got {a}");
        assert_eq!(c.alpha_at(60, 5), 0, "outside the highlight radius");
    }

    #[test]
    fn ripple_grows_and_fades_with_t() {
        let at = Point::new(60.0, 60.0);
        let color = Rgba::new(0.0, 1.0, 0.0, 1.0);
        let ring = |t: f64| {
            let mut c = Canvas::new(120, 120);
            c.paint(|cr| paint_ripple(cr, at, t, color));
            (c.ink(), c.alpha_at(60, 60 - (6.0 + t * (RIPPLE_MAX_R - 6.0)) as i32))
        };
        let (early_px, early_alpha) = ring(0.1);
        let (late_px, late_alpha) = ring(0.9);
        assert!(late_px > early_px, "ring circumference grows with t");
        assert!(late_alpha < early_alpha, "ring fades out with t");
    }

    #[test]
    fn ripple_ink_stays_inside_ripple_bounds() {
        let at = Point::new(60.0, 60.0);
        let b = ripple_bounds(at);
        let mut c = Canvas::new(120, 120);
        c.paint(|cr| paint_ripple(cr, at, 1.0, Rgba::new(1.0, 1.0, 1.0, 1.0)));
        let inside = c.ink_in(
            b.x.max(0.0) as i32,
            b.y.max(0.0) as i32,
            b.w.min(120.0) as i32,
            b.h.min(120.0) as i32,
        );
        assert_eq!(inside, c.ink());
    }
}
