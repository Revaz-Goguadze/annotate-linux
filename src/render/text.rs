//! Text + counter rendering via cairo's toy font API. v1 limitation
//! (documented): no shaping or font fallback — non-Latin text may render
//! as boxes. Pango is the planned upgrade path.

use crate::model::geom::Point;
use crate::model::object::{Object, ObjectKind};

pub const FONT_FAMILY: &str = "Sans";

fn select_font(cr: &cairo::Context, px: f64) {
    cr.select_font_face(FONT_FAMILY, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(px);
}

/// Paint a Text object. `at` is the top-left corner.
pub fn paint_text(cr: &cairo::Context, at: Point, s: &str, px: f64) {
    select_font(cr, px);
    cr.move_to(at.x + 2.0, at.y + px);
    cr.show_text(s).expect("show_text");
}

/// Paint a Counter badge: filled circle + centered number in white.
pub fn paint_counter(cr: &cairo::Context, at: Point, n: u32, r: f64) {
    cr.new_path();
    cr.arc(at.x, at.y, r, 0.0, std::f64::consts::TAU);
    cr.fill().expect("badge");
    let label = n.to_string();
    let px = r * 1.1;
    select_font(cr, px);
    let ext = cr.text_extents(&label).expect("extents");
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.move_to(at.x - ext.width() / 2.0 - ext.x_bearing(), at.y + ext.height() / 2.0);
    cr.show_text(&label).expect("badge label");
}

/// Caret after the last character of a Text draft.
pub fn paint_caret(cr: &cairo::Context, obj: &Object) {
    let ObjectKind::Text { at, s, px } = &obj.kind else { return };
    select_font(cr, *px);
    let advance = cr.text_extents(s).map(|e| e.x_advance()).unwrap_or(0.0);
    let x = at.x + 2.0 + advance + 1.0;
    let c = obj.style.stroke;
    cr.set_source_rgba(c.r, c.g, c.b, 0.9);
    cr.set_line_width(2.0);
    cr.new_path();
    cr.move_to(x, at.y + 2.0);
    cr.line_to(x, at.y + px * 1.2);
    cr.stroke().expect("caret");
}
