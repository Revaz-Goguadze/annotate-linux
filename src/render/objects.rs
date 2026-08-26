//! ObjectKind → cairo paths. All coordinates logical px; the caller sets up
//! any scale transform.

use crate::model::arrow;
use crate::model::object::{Object, ObjectKind};

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
            cr.save().expect("save");
            cr.translate(r.x + r.w / 2.0, r.y + r.h / 2.0);
            cr.scale(r.w / 2.0, r.h / 2.0);
            cr.new_path();
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            cr.restore().expect("restore");
            // stroke after restore so line width stays uniform
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
