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
