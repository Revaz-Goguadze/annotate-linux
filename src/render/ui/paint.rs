//! Cairo painting for the toolbar and popups. Icons are simple hand-drawn
//! glyphs, all in logical px.

use super::{UiButton, UiLayout, track_x_from_width};
use crate::input::Tool;
use crate::model::geom::Rect;
use crate::render::board::BoardKind;
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
            rounded(cr, r.inflate(-2.0), 6.0);
            cr.set_source_rgba(BTN_ACTIVE.0, BTN_ACTIVE.1, BTN_ACTIVE.2, BTN_ACTIVE.3);
            cr.fill().expect("active bg");
        }
        icon(cr, *b, *r, ctx);
    }

    if let Some((p, swatches)) = &l.color_popup {
        panel(cr, *p);
        for (i, r) in swatches.iter().enumerate() {
            let c = ctx.palette[i];
            rounded(cr, *r, 5.0);
            cr.set_source_rgba(c.r, c.g, c.b, c.a);
            cr.fill().expect("swatch");
            if i == ctx.color_idx {
                rounded(cr, r.inflate(2.0), 6.0);
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
        cr.new_path();
        cr.arc(kx, cy, kr, 0.0, std::f64::consts::TAU);
        cr.fill().expect("knob");
    }
}

fn panel(cr: &cairo::Context, r: Rect) {
    rounded(cr, r, 10.0);
    cr.set_source_rgba(BG.0, BG.1, BG.2, BG.3);
    cr.fill().expect("panel");
}

fn rounded(cr: &cairo::Context, r: Rect, rad: f64) {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    cr.new_path();
    cr.arc(r.x + r.w - rad, r.y + rad, rad, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(r.x + r.w - rad, r.y + r.h - rad, rad, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(r.x + rad, r.y + r.h - rad, rad, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(r.x + rad, r.y + rad, rad, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
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
            cr.save().unwrap();
            cr.translate(cx, cy);
            cr.scale((x1 - x0) / 2.0, (y1 - y0) / 2.0 - 2.0);
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
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
