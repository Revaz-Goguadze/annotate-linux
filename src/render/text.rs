//! Text + counter rendering via cairo's toy font API. v1 limitation
//! (documented): no shaping or font fallback — non-Latin text may render
//! as boxes. Pango is the planned upgrade path.

use crate::model::geom::Point;
use crate::model::object::{Object, ObjectKind};
use crate::render::draw;

fn select_font(cr: &cairo::Context, px: f64) {
    draw::select_font(cr, px, cairo::FontWeight::Normal);
}

/// Paint a Text object. `at` is the top-left corner.
pub fn paint_text(cr: &cairo::Context, at: Point, s: &str, px: f64) {
    select_font(cr, px);
    cr.move_to(at.x + 2.0, at.y + px);
    cr.show_text(s).expect("show_text");
}

/// Paint a Counter badge: filled circle + centered number in white.
pub fn paint_counter(cr: &cairo::Context, at: Point, n: u32, r: f64) {
    draw::circle(cr, at.x, at.y, r);
    cr.fill().expect("badge");
    select_font(cr, r * 1.1);
    cr.set_source_rgb(1.0, 1.0, 1.0);
    draw::centered_text(cr, at.x, at.y, &n.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::Rect;
    use crate::model::object::{ObjectId, Style};
    use crate::render::test_surface::Canvas;
    use crate::util::color::Rgba;

    fn text_obj(s: &str, px: f64) -> Object {
        Object::new(
            ObjectId(1),
            ObjectKind::Text { at: Point::new(10.0, 10.0), s: s.into(), px },
            Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width: 2.0, group_alpha: 1.0 },
        )
    }

    #[test]
    fn text_inks_below_and_right_of_its_anchor() {
        let mut c = Canvas::new(200, 80);
        c.paint(|cr| {
            cr.set_source_rgb(1.0, 0.0, 0.0);
            paint_text(cr, Point::new(20.0, 10.0), "Hello", 24.0);
        });
        assert!(c.ink() > 0, "toy font drew nothing");
        // baseline sits at at.y + px, glyphs start just right of at.x
        assert_eq!(c.ink_in(0, 0, 20, 80), 0, "ink left of the anchor");
        assert_eq!(c.ink_in(0, 0, 200, 10), 0, "ink above the anchor");
    }

    #[test]
    fn empty_text_draws_nothing() {
        let mut c = Canvas::new(60, 40);
        c.paint(|cr| {
            cr.set_source_rgb(1.0, 0.0, 0.0);
            paint_text(cr, Point::new(5.0, 5.0), "", 20.0);
        });
        assert_eq!(c.ink(), 0);
    }

    #[test]
    fn bigger_font_size_inks_more() {
        let ink = |px: f64| {
            let mut c = Canvas::new(300, 120);
            c.paint(|cr| {
                cr.set_source_rgb(1.0, 0.0, 0.0);
                paint_text(cr, Point::new(5.0, 5.0), "annotate", px);
            });
            c.ink()
        };
        assert!(ink(40.0) > ink(12.0));
    }

    #[test]
    fn counter_badge_fills_its_circle_and_labels_it_in_white() {
        let mut c = Canvas::new(100, 100);
        c.paint(|cr| {
            cr.set_source_rgb(1.0, 0.0, 0.0);
            paint_counter(cr, Point::new(50.0, 50.0), 12, 20.0);
        });
        assert_eq!(c.alpha_at(50, 32), 255, "inside the badge");
        assert_eq!(c.alpha_at(50, 20), 0, "outside the badge radius");
        // the badge covers roughly pi*r^2 px, so most of its bbox stays clear
        let filled = c.ink_in(30, 30, 40, 40);
        assert!(filled > 1000 && filled <= 1600, "unexpected badge coverage {filled}");
    }

    #[test]
    fn caret_is_drawn_past_the_last_character() {
        let obj = text_obj("abc", 24.0);
        let mut c = Canvas::new(200, 80);
        c.paint(|cr| paint_caret(cr, &obj));
        assert!(c.ink() > 0, "caret drew nothing");
        // caret sits right of the text advance, never left of the anchor
        assert_eq!(c.ink_in(0, 0, 12, 80), 0);

        let empty = text_obj("", 24.0);
        let mut c2 = Canvas::new(200, 80);
        c2.paint(|cr| paint_caret(cr, &empty));
        assert!(c2.ink() > 0, "an empty draft still shows a caret");
    }

    #[test]
    fn caret_ignores_non_text_objects() {
        let rect = Object::new(
            ObjectId(2),
            ObjectKind::Rect { r: Rect::new(10.0, 10.0, 40.0, 40.0) },
            Style { stroke: Rgba::new(1.0, 1.0, 1.0, 1.0), width: 2.0, group_alpha: 1.0 },
        );
        let mut c = Canvas::new(80, 80);
        c.paint(|cr| paint_caret(cr, &rect));
        assert_eq!(c.ink(), 0);
    }
}
