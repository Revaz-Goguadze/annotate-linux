//! Cairo path/text primitives shared by the object, cursor and UI painters.
//! All coordinates are logical px; the caller owns the source and stroke
//! settings, these only build paths (or show text at the current source).

use crate::model::geom::Rect;

pub const FONT_FAMILY: &str = "Sans";

/// Fresh full circle centered on (`cx`, `cy`).
pub fn circle(cr: &cairo::Context, cx: f64, cy: f64, r: f64) {
    cr.new_path();
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
}

/// Fresh ellipse with radii (`rx`, `ry`). The scale transform used to build
/// it is undone before returning, so a following stroke keeps a uniform
/// line width.
pub fn ellipse(cr: &cairo::Context, cx: f64, cy: f64, rx: f64, ry: f64) {
    cr.save().expect("save");
    cr.translate(cx, cy);
    cr.scale(rx, ry);
    circle(cr, 0.0, 0.0, 1.0);
    cr.restore().expect("restore");
}

/// Fresh rounded rectangle, corner radius clamped to fit.
pub fn rounded_rect(cr: &cairo::Context, r: Rect, rad: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    cr.new_path();
    cr.arc(r.x + r.w - rad, r.y + rad, rad, -FRAC_PI_2, 0.0);
    cr.arc(r.x + r.w - rad, r.y + r.h - rad, rad, 0.0, FRAC_PI_2);
    cr.arc(r.x + rad, r.y + r.h - rad, rad, FRAC_PI_2, PI);
    cr.arc(r.x + rad, r.y + rad, rad, PI, 1.5 * PI);
    cr.close_path();
}

/// Add every rect to the current path (clip regions, dashed chrome).
pub fn add_rects(cr: &cairo::Context, rects: impl IntoIterator<Item = Rect>) {
    for r in rects {
        cr.rectangle(r.x, r.y, r.w, r.h);
    }
}

/// Select the toy font at `px`. No shaping or fallback — see `render::text`.
pub fn select_font(cr: &cairo::Context, px: f64, weight: cairo::FontWeight) {
    cr.select_font_face(FONT_FAMILY, cairo::FontSlant::Normal, weight);
    cr.set_font_size(px);
}

/// Show `label` centered on (`cx`, `cy`) in the currently selected font.
pub fn centered_text(cr: &cairo::Context, cx: f64, cy: f64, label: &str) {
    let ext = cr.text_extents(label).expect("text extents");
    cr.move_to(cx - ext.width() / 2.0 - ext.x_bearing(), cy + ext.height() / 2.0);
    cr.show_text(label).expect("show_text");
}
