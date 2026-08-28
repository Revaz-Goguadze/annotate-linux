//! ObjectKind → cairo paths. All coordinates logical px; the caller sets up
//! any scale transform.

use crate::model::arrow;
use crate::model::object::{Object, ObjectKind};
use crate::render::draw;

/// `alpha` is an extra multiplier (fade mode); 1.0 = fully opaque.
pub fn paint_object(cr: &cairo::Context, obj: &Object, alpha: f64) {
    if alpha <= 0.0 {
        return;
    }
    let s = &obj.style;
    cr.set_line_width(s.width);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    let group_alpha = s.group_alpha * alpha;
    if group_alpha < 1.0 {
        // Composite the whole object as a group so a self-crossing
        // highlighter stroke doesn't double-darken at intersections; the
        // same path applies fade alpha per object.
        cr.push_group();
        cr.set_source_rgba(s.stroke.r, s.stroke.g, s.stroke.b, s.stroke.a);
        paint_kind(cr, obj);
        cr.pop_group_to_source().expect("pop group");
        cr.paint_with_alpha(group_alpha).expect("paint group");
    } else {
        cr.set_source_rgba(s.stroke.r, s.stroke.g, s.stroke.b, s.stroke.a);
        paint_kind(cr, obj);
    }
}

fn paint_kind(cr: &cairo::Context, obj: &Object) {
    match &obj.kind {
        ObjectKind::Freehand { pts } => {
            polyline(cr, pts);
            cr.stroke().expect("stroke");
        }
        ObjectKind::Line { a, b } => {
            cr.new_path();
            cr.move_to(a.x, a.y);
            cr.line_to(b.x, b.y);
            cr.stroke().expect("stroke");
        }
        ObjectKind::Arrow { a, b } => {
            let end = arrow::shaft_end(*a, *b, obj.style.width);
            cr.new_path();
            cr.move_to(a.x, a.y);
            cr.line_to(end.x, end.y);
            cr.stroke().expect("stroke");
            let [tip, l, r] = arrow::head_points(*a, *b, obj.style.width);
            cr.new_path();
            cr.move_to(tip.x, tip.y);
            cr.line_to(l.x, l.y);
            cr.line_to(r.x, r.y);
            cr.close_path();
            cr.fill().expect("fill");
        }
        ObjectKind::Rect { r } => {
            cr.new_path();
            cr.rectangle(r.x, r.y, r.w, r.h);
            cr.stroke().expect("stroke");
        }
        ObjectKind::Counter { at, n, r } => {
            crate::render::text::paint_counter(cr, *at, *n, *r);
        }
        ObjectKind::Text { at, s, px } => {
            crate::render::text::paint_text(cr, *at, s, *px);
        }
        ObjectKind::Ellipse { r } => {
            if r.w <= 0.0 || r.h <= 0.0 {
                return;
            }
            draw::ellipse(cr, r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0);
            // stroke outside the scaled transform so line width stays uniform
            cr.stroke().expect("stroke");
        }
    }
}


fn polyline(cr: &cairo::Context, pts: &[crate::model::geom::Point]) {
    let Some(first) = pts.first() else { return };
    cr.new_path();
    cr.move_to(first.x, first.y);
    if pts.len() == 1 {
        cr.line_to(first.x, first.y);
    }
    for p in &pts[1..] {
        cr.line_to(p.x, p.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::{Point, Rect};
    use crate::model::object::{ObjectId, Style};
    use crate::render::test_surface::Canvas;
    use crate::util::color::Rgba;

    fn style(width: f64, group_alpha: f64) -> Style {
        Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width, group_alpha }
    }

    fn obj(kind: ObjectKind, width: f64) -> Object {
        Object::new(ObjectId(1), kind, style(width, 1.0))
    }

    /// Paint one object on a 100x100 canvas and report the inked pixels.
    fn ink_of(kind: ObjectKind, width: f64, alpha: f64) -> usize {
        let o = obj(kind, width);
        let mut c = Canvas::new(100, 100);
        c.paint(|cr| paint_object(cr, &o, alpha));
        c.ink()
    }

    #[test]
    fn every_kind_puts_ink_inside_its_own_bounds() {
        let kinds = [
            ObjectKind::Freehand { pts: vec![Point::new(10.0, 10.0), Point::new(80.0, 60.0)] },
            ObjectKind::Line { a: Point::new(10.0, 10.0), b: Point::new(80.0, 80.0) },
            ObjectKind::Arrow { a: Point::new(10.0, 50.0), b: Point::new(80.0, 50.0) },
            ObjectKind::Rect { r: Rect::new(20.0, 20.0, 50.0, 40.0) },
            ObjectKind::Ellipse { r: Rect::new(20.0, 20.0, 50.0, 40.0) },
            ObjectKind::Counter { at: Point::new(50.0, 50.0), n: 7, r: 16.0 },
            ObjectKind::Text { at: Point::new(10.0, 30.0), s: "hi".into(), px: 24.0 },
        ];
        for kind in kinds {
            let o = obj(kind.clone(), 4.0);
            let mut c = Canvas::new(100, 100);
            c.paint(|cr| paint_object(cr, &o, 1.0));
            let total = c.ink();
            assert!(total > 0, "{kind:?} drew nothing");
            let b = o.bounds;
            let inside = c.ink_in(
                b.x.floor().max(0.0) as i32,
                b.y.floor().max(0.0) as i32,
                b.w.ceil() as i32 + 1,
                b.h.ceil() as i32 + 1,
            );
            assert_eq!(inside, total, "{kind:?} inked pixels outside its bounds {b:?}");
        }
    }

    #[test]
    fn nonpositive_alpha_skips_painting() {
        let kind = ObjectKind::Line { a: Point::new(10.0, 10.0), b: Point::new(80.0, 80.0) };
        assert_eq!(ink_of(kind.clone(), 4.0, 0.0), 0);
        assert_eq!(ink_of(kind.clone(), 4.0, -1.0), 0);
        assert!(ink_of(kind, 4.0, 1.0) > 0);
    }

    #[test]
    fn fade_alpha_only_dims_the_stroke() {
        let kind = ObjectKind::Line { a: Point::new(10.0, 50.0), b: Point::new(90.0, 50.0) };
        let o = obj(kind, 6.0);

        let mut opaque = Canvas::new(100, 100);
        opaque.paint(|cr| paint_object(cr, &o, 1.0));
        let mut faded = Canvas::new(100, 100);
        faded.paint(|cr| paint_object(cr, &o, 0.25));

        assert!(faded.alpha_at(50, 50) > 0);
        assert!(
            faded.alpha_at(50, 50) < opaque.alpha_at(50, 50),
            "fade alpha must reduce coverage"
        );
    }

    #[test]
    fn highlighter_self_crossing_does_not_double_darken() {
        // Group compositing means the crossing point is no more opaque than
        // a single pass over the same stroke.
        let o = Object::new(
            ObjectId(1),
            ObjectKind::Freehand {
                pts: vec![
                    Point::new(20.0, 20.0),
                    Point::new(80.0, 80.0),
                    Point::new(80.0, 20.0),
                    Point::new(20.0, 80.0),
                ],
            },
            style(12.0, 0.4),
        );
        let mut c = Canvas::new(100, 100);
        c.paint(|cr| paint_object(cr, &o, 1.0));
        let crossing = c.alpha_at(50, 50);
        let single_pass = c.alpha_at(30, 30);
        assert!(crossing > 0 && single_pass > 0);
        assert!(
            crossing <= single_pass + 2,
            "crossing {crossing} darker than a single pass {single_pass}"
        );
    }

    #[test]
    fn degenerate_shapes_are_no_ops_not_panics() {
        assert_eq!(ink_of(ObjectKind::Freehand { pts: vec![] }, 4.0, 1.0), 0);
        assert_eq!(
            ink_of(ObjectKind::Ellipse { r: Rect::new(20.0, 20.0, 0.0, 30.0) }, 4.0, 1.0),
            0
        );
        assert_eq!(
            ink_of(ObjectKind::Ellipse { r: Rect::new(20.0, 20.0, 30.0, -5.0) }, 4.0, 1.0),
            0
        );
    }

    #[test]
    fn single_point_freehand_draws_a_dot() {
        let kind = ObjectKind::Freehand { pts: vec![Point::new(50.0, 50.0)] };
        let o = obj(kind, 10.0);
        let mut c = Canvas::new(100, 100);
        c.paint(|cr| paint_object(cr, &o, 1.0));
        assert!(c.ink() > 0, "a tap must leave a round cap dot");
        assert!(c.alpha_at(50, 50) > 0);
        assert_eq!(c.alpha_at(90, 90), 0);
    }
}
