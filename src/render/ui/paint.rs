//! Cairo painting for the toolbar and popups. Icons are simple hand-drawn
//! glyphs, all in logical px.

use super::{UiButton, UiLayout, track_x_from_width};
use crate::input::Tool;
use crate::model::geom::Rect;
use crate::render::board::BoardKind;
use crate::render::draw;
use crate::util::color::Rgba;

pub struct UiPaintCtx<'a> {
    pub active_tool: Tool,
    pub palette: &'a [Rgba],
    pub color_idx: usize,
    pub width: f64,
    pub board: BoardKind,
}

const BG: (f64, f64, f64, f64) = (0.13, 0.13, 0.15, 0.92);
const BTN_ACTIVE: (f64, f64, f64, f64) = (0.30, 0.42, 0.60, 1.0);
const FG: (f64, f64, f64) = (0.92, 0.92, 0.92);

pub fn paint(cr: &cairo::Context, l: &UiLayout, ctx: &UiPaintCtx) {
    panel(cr, l.toolbar);
    for (b, r) in &l.buttons {
        let active = matches!(b, UiButton::Tool(t) if *t == ctx.active_tool);
        if active {
            draw::rounded_rect(cr, r.inflate(-2.0), 6.0);
            cr.set_source_rgba(BTN_ACTIVE.0, BTN_ACTIVE.1, BTN_ACTIVE.2, BTN_ACTIVE.3);
            cr.fill().expect("active bg");
        }
        icon(cr, *b, *r, ctx);
    }

    if let Some((p, swatches)) = &l.color_popup {
        panel(cr, *p);
        for (i, r) in swatches.iter().enumerate() {
            let c = ctx.palette[i];
            draw::rounded_rect(cr, *r, 5.0);
            cr.set_source_rgba(c.r, c.g, c.b, c.a);
            cr.fill().expect("swatch");
            if i == ctx.color_idx {
                draw::rounded_rect(cr, r.inflate(2.0), 6.0);
                cr.set_source_rgb(FG.0, FG.1, FG.2);
                cr.set_line_width(2.0);
                cr.stroke().expect("swatch ring");
            }
        }
    }

    if let Some((p, track)) = &l.width_popup {
        panel(cr, *p);
        // track line
        let cy = track.y + track.h / 2.0;
        cr.set_source_rgba(FG.0, FG.1, FG.2, 0.5);
        cr.set_line_width(3.0);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.new_path();
        cr.move_to(track.x, cy);
        cr.line_to(track.x + track.w, cy);
        cr.stroke().expect("track");
        // knob sized by current width
        let kx = track_x_from_width(*track, ctx.width);
        let kr = 4.0 + ctx.width / 2.0;
        let c = ctx.palette[ctx.color_idx];
        cr.set_source_rgba(c.r, c.g, c.b, 1.0);
        draw::circle(cr, kx, cy, kr);
        cr.fill().expect("knob");
    }
}


fn panel(cr: &cairo::Context, r: Rect) {
    draw::rounded_rect(cr, r, 10.0);
    cr.set_source_rgba(BG.0, BG.1, BG.2, BG.3);
    cr.fill().expect("panel");
}

fn icon(cr: &cairo::Context, b: UiButton, r: Rect, ctx: &UiPaintCtx) {
    let (x0, y0) = (r.x + 10.0, r.y + 10.0);
    let (x1, y1) = (r.x + r.w - 10.0, r.y + r.h - 10.0);
    cr.set_source_rgb(FG.0, FG.1, FG.2);
    cr.set_line_width(2.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.new_path();
    match b {
        UiButton::Tool(Tool::Pen) => {
            cr.move_to(x0, y1);
            cr.line_to(x1, y0);
            cr.stroke().unwrap();
        }
        UiButton::Tool(Tool::Highlighter) => {
            cr.set_line_width(8.0);
            cr.set_source_rgba(FG.0, FG.1, FG.2, 0.5);
            cr.move_to(x0, y1);
            cr.line_to(x1, y0);
            cr.stroke().unwrap();
        }
        UiButton::Tool(Tool::Line) => {
            cr.move_to(x0, (y0 + y1) / 2.0);
            cr.line_to(x1, (y0 + y1) / 2.0);
            cr.stroke().unwrap();
        }
        UiButton::Tool(Tool::Arrow) => {
            let cy = (y0 + y1) / 2.0;
            cr.move_to(x0, cy);
            cr.line_to(x1, cy);
            cr.move_to(x1 - 6.0, cy - 5.0);
            cr.line_to(x1, cy);
            cr.line_to(x1 - 6.0, cy + 5.0);
            cr.stroke().unwrap();
        }
        UiButton::Tool(Tool::Rect) => {
            cr.rectangle(x0, y0 + 3.0, x1 - x0, y1 - y0 - 6.0);
            cr.stroke().unwrap();
        }
        UiButton::Tool(Tool::Ellipse) => {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            draw::ellipse(cr, cx, cy, (x1 - x0) / 2.0, (y1 - y0) / 2.0 - 2.0);
            cr.stroke().unwrap();
        }
        UiButton::Tool(Tool::Counter) => {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cr.arc(cx, cy, (x1 - x0) / 2.0, 0.0, std::f64::consts::TAU);
            cr.stroke().unwrap();
            draw::select_font(cr, 12.0, cairo::FontWeight::Bold);
            draw::centered_text(cr, cx, cy, "1");
        }
        UiButton::Tool(Tool::Text) => {
            draw::select_font(cr, 16.0, cairo::FontWeight::Bold);
            draw::centered_text(cr, (x0 + x1) / 2.0, (y0 + y1) / 2.0, "T");
        }
        UiButton::Tool(Tool::Select) => {
            // cursor-arrow glyph
            cr.move_to(x0 + 2.0, y0);
            cr.line_to(x0 + 2.0, y1 - 2.0);
            cr.line_to(x0 + 7.0, y1 - 7.0);
            cr.line_to(x0 + 11.0, y1);
            cr.line_to(x0 + 14.0, y1 - 3.0);
            cr.line_to(x0 + 9.0, y1 - 10.0);
            cr.line_to(x1 - 4.0, y1 - 10.0);
            cr.close_path();
            cr.fill().unwrap();
        }
        UiButton::Tool(Tool::Eraser) => {
            cr.save().unwrap();
            cr.translate((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cr.rotate(-0.6);
            cr.rectangle(-9.0, -6.0, 18.0, 12.0);
            cr.restore().unwrap();
            cr.stroke().unwrap();
        }
        UiButton::ColorSwatch => {
            let c = ctx.palette[ctx.color_idx];
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cr.set_source_rgba(c.r, c.g, c.b, c.a);
            cr.arc(cx, cy, (x1 - x0) / 2.0, 0.0, std::f64::consts::TAU);
            cr.fill().unwrap();
            cr.set_source_rgba(FG.0, FG.1, FG.2, 0.8);
            cr.arc(cx, cy, (x1 - x0) / 2.0, 0.0, std::f64::consts::TAU);
            cr.stroke().unwrap();
        }
        UiButton::WidthIndicator => {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            cr.arc(cx, cy, (ctx.width / 2.0).clamp(1.5, (x1 - x0) / 2.0), 0.0, std::f64::consts::TAU);
            cr.fill().unwrap();
        }
        UiButton::Board => {
            match ctx.board {
                BoardKind::None => cr.set_source_rgba(FG.0, FG.1, FG.2, 0.35),
                BoardKind::White => cr.set_source_rgb(1.0, 1.0, 1.0),
                BoardKind::Black => cr.set_source_rgb(0.05, 0.05, 0.05),
            }
            cr.rectangle(x0, y0, x1 - x0, y1 - y0);
            cr.fill().unwrap();
            cr.set_source_rgba(FG.0, FG.1, FG.2, 0.9);
            cr.rectangle(x0, y0, x1 - x0, y1 - y0);
            cr.stroke().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::Point;
    use crate::render::test_surface::Canvas;
    use crate::render::ui::{UiState, layout, ui_region, width_from_track_x};

    const W: i32 = 900;
    const H: i32 = 400;
    const SURFACE: Rect = Rect { x: 0.0, y: 0.0, w: W as f64, h: H as f64 };

    fn palette() -> Vec<Rgba> {
        vec![
            Rgba::new(1.0, 0.0, 0.0, 1.0),
            Rgba::new(0.0, 1.0, 0.0, 1.0),
            Rgba::new(0.0, 0.0, 1.0, 1.0),
            Rgba::new(1.0, 1.0, 0.0, 1.0),
        ]
    }

    fn ctx<'a>(palette: &'a [Rgba], tool: Tool, width: f64, board: BoardKind) -> UiPaintCtx<'a> {
        UiPaintCtx { active_tool: tool, palette, color_idx: 2, width, board }
    }

    #[test]
    fn toolbar_ink_is_confined_to_the_damage_region() {
        let l = layout(SURFACE, 4, &UiState::default());
        let p = palette();
        let mut c = Canvas::new(W, H);
        c.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Pen, 6.0, BoardKind::None)));
        let region = ui_region(&l);
        let inside = c.ink_in(
            region.x as i32,
            region.y as i32,
            region.w.ceil() as i32,
            region.h.ceil() as i32,
        );
        assert!(inside > 0);
        assert_eq!(inside, c.ink(), "painted outside ui_region {region:?}");
    }

    #[test]
    fn every_tool_icon_draws_inside_its_button() {
        let l = layout(SURFACE, 4, &UiState::default());
        let p = palette();
        for (b, r) in &l.buttons {
            let mut c = Canvas::new(W, H);
            let one = UiLayout {
                toolbar: *r,
                buttons: vec![(*b, *r)],
                color_popup: None,
                width_popup: None,
            };
            c.paint(|cr| paint(cr, &one, &ctx(&p, Tool::Pen, 6.0, BoardKind::White)));
            let inside = c.ink_in(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
            assert!(inside > 0, "{b:?} icon drew nothing");
            assert_eq!(inside, c.ink(), "{b:?} icon leaked outside its button");
        }
    }

    #[test]
    fn active_tool_button_is_highlighted() {
        let p = palette();
        let l = layout(SURFACE, 4, &UiState::default());
        let (_, pen_rect) = l.buttons.iter().find(|(b, _)| *b == UiButton::Tool(Tool::Pen)).unwrap();
        // sample a corner of the button, away from the icon glyph itself
        let (sx, sy) = ((pen_rect.x + 6.0) as i32, (pen_rect.y + 6.0) as i32);

        let mut active = Canvas::new(W, H);
        active.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Pen, 6.0, BoardKind::None)));
        let mut inactive = Canvas::new(W, H);
        inactive.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Eraser, 6.0, BoardKind::None)));

        assert_ne!(
            active.rgba_at(sx, sy),
            inactive.rgba_at(sx, sy),
            "active tool must be visually distinct"
        );
        assert_eq!(active.alpha_at(sx, sy), 255, "active background is opaque");
    }

    #[test]
    fn board_button_reflects_the_active_board() {
        let p = palette();
        let l = layout(SURFACE, 4, &UiState::default());
        let (_, r) = l.buttons.iter().find(|(b, _)| *b == UiButton::Board).unwrap();
        let (sx, sy) = ((r.x + r.w / 2.0) as i32, (r.y + r.h / 2.0) as i32);
        let sample = |board: BoardKind| {
            let mut c = Canvas::new(W, H);
            c.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Pen, 6.0, board)));
            c.rgba_at(sx, sy)
        };
        let (wr, wg, wb, _) = sample(BoardKind::White);
        assert!(wr > 240 && wg > 240 && wb > 240, "whiteboard swatch is white");
        let (br, bg, bb, _) = sample(BoardKind::Black);
        assert!(br < 30 && bg < 30 && bb < 30, "blackboard swatch is near-black");
        assert_ne!(sample(BoardKind::None), sample(BoardKind::White));
    }

    #[test]
    fn open_color_popup_paints_each_swatch_in_its_palette_color() {
        let p = palette();
        let ui = UiState { color_picker_open: true, ..Default::default() };
        let l = layout(SURFACE, p.len(), &ui);
        let mut c = Canvas::new(W, H);
        c.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Pen, 6.0, BoardKind::None)));

        let (_, swatches) = l.color_popup.as_ref().unwrap();
        for (i, r) in swatches.iter().enumerate() {
            let (sx, sy) = ((r.x + r.w / 2.0) as i32, (r.y + r.h / 2.0) as i32);
            let (pr, pg, pb, a) = c.rgba_at(sx, sy);
            let want = p[i];
            assert_eq!(a, 255);
            assert_eq!(
                (pr / 8, pg / 8, pb / 8),
                (
                    (want.r * 255.0) as u8 / 8,
                    (want.g * 255.0) as u8 / 8,
                    (want.b * 255.0) as u8 / 8
                ),
                "swatch {i} color mismatch"
            );
        }
    }

    #[test]
    fn width_popup_knob_tracks_the_current_width() {
        let p = palette();
        let ui = UiState { width_picker_open: true, ..Default::default() };
        let l = layout(SURFACE, p.len(), &ui);
        let (_, track) = l.width_popup.unwrap();
        let cy = (track.y + track.h / 2.0) as i32;

        for width in [1.0, 10.0, 20.0] {
            let mut c = Canvas::new(W, H);
            c.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Pen, width, BoardKind::None)));
            let kx = track_x_from_width(track, width);
            let knob = c.rgba_at(kx as i32, cy);
            // the knob is drawn in the selected palette color, opaque
            let want = p[2];
            assert_eq!(knob.3, 255, "knob at width {width} is opaque");
            assert_eq!(
                (knob.0 / 16, knob.1 / 16, knob.2 / 16),
                (
                    (want.r * 255.0) as u8 / 16,
                    (want.g * 255.0) as u8 / 16,
                    (want.b * 255.0) as u8 / 16
                ),
                "knob at width {width} is not the selected color"
            );
            assert!((width_from_track_x(track, kx) - width).abs() < 0.01);
        }
    }

    #[test]
    fn popups_paint_above_the_toolbar() {
        let p = palette();
        let ui = UiState { color_picker_open: true, width_picker_open: true };
        let l = layout(SURFACE, p.len(), &ui);
        let mut c = Canvas::new(W, H);
        c.paint(|cr| paint(cr, &l, &ctx(&p, Tool::Text, 6.0, BoardKind::Black)));
        let (panel, _) = l.color_popup.as_ref().unwrap();
        assert!(panel.y + panel.h <= l.toolbar.y);
        assert!(c.ink_in(panel.x as i32, panel.y as i32, panel.w as i32, panel.h as i32) > 0);
        assert!(!ui_region(&l).contains(Point::new(0.0, 0.0)));
    }
}
