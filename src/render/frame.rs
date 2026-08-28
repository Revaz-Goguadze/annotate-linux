//! One frame's worth of paintable state, and the single paint sequence that
//! renders it. Every consumer goes through here — the incremental overlay
//! frame, the ANNOTATE_CHECK reference render, and PNG export — so they
//! cannot drift apart.

use crate::model::fade;
use crate::model::geom::{Point, Rect};
use crate::model::object::Object;
use crate::model::scene::Scene;
use crate::render::board::{self, BoardKind};
use crate::render::cursor_fx::{self, CursorFx};
use crate::render::objects::paint_object;
use crate::render::ui::{self, UiLayout, paint::UiPaintCtx};
use crate::util::color::Rgba;

const RIPPLE_COLOR: Rgba = Rgba::new(1.0, 0.85, 0.2, 1.0);
const CHROME_COLOR: Rgba = Rgba::new(0.45, 0.65, 0.95, 0.95);
const CHROME_DASH: [f64; 2] = [6.0, 4.0];

/// Everything one frame needs, borrowed from AppState.
pub struct FrameCtx<'a> {
    pub scene: &'a Scene,
    pub preview: Option<&'a Object>,
    pub board: BoardKind,
    pub board_opacity: f64,
    /// Draw an end-of-text caret on the preview (open text draft).
    pub caret: bool,
    /// Rubber-band rectangle in progress on this output.
    pub marquee: Option<Rect>,
    /// Bounds of selected objects on this output (dashed highlight).
    pub selection: Vec<Rect>,
    /// Fade-mode hold seconds; None = persist (full alpha).
    pub fade_hold: Option<f64>,
    pub now: std::time::Instant,
    /// Cursor glyph/spotlight on this output.
    pub cursor: Option<CursorFx>,
    /// Click ripples on this output: (center, progress 0..1).
    pub ripples: Vec<(Point, f64)>,
    /// Present only on the output that shows the toolbar.
    pub ui: Option<(&'a UiLayout, UiPaintCtx<'a>)>,
    pub debug_damage: bool,
}

impl<'a> FrameCtx<'a> {
    /// A still frame: board and annotations only, no live drag, cursor or
    /// UI chrome (PNG export).
    pub fn still(scene: &'a Scene, board: BoardKind, board_opacity: f64) -> Self {
        Self {
            scene,
            preview: None,
            board,
            board_opacity,
            caret: false,
            marquee: None,
            selection: Vec::new(),
            fade_hold: None,
            now: std::time::Instant::now(),
            cursor: None,
            ripples: Vec::new(),
            ui: None,
            debug_damage: false,
        }
    }

    /// Paint the frame in logical px, in z order. `clip` is the damage the
    /// caller already clipped the context to, used to skip work outside it;
    /// `None` paints everything.
    pub fn paint(&self, cr: &cairo::Context, clip: Option<&[Rect]>) {
        let visible = |bounds: Rect| match clip {
            Some(rects) => rects.iter().any(|r| r.intersects(bounds)),
            None => true,
        };

        board::paint(cr, self.board, self.board_opacity);

        for obj in &self.scene.objects {
            if visible(obj.bounds) {
                paint_object(cr, obj, self.alpha_of(obj));
            }
        }
        if let Some(p) = self.preview
            && visible(p.bounds)
        {
            paint_object(cr, p, 1.0);
            if self.caret {
                crate::render::text::paint_caret(cr, p);
            }
        }

        for (at, t) in &self.ripples {
            if visible(cursor_fx::ripple_bounds(*at)) {
                cursor_fx::paint_ripple(cr, *at, *t, RIPPLE_COLOR);
            }
        }
        if let Some(fx) = &self.cursor
            && visible(fx.bounds())
        {
            cursor_fx::paint_cursor(cr, fx);
        }

        self.paint_chrome(cr);

        if let Some((layout, paint_ctx)) = &self.ui
            && visible(ui::ui_region(layout))
        {
            ui::paint::paint(cr, layout, paint_ctx);
        }
    }

    /// Fade-mode alpha for `obj`; 1.0 in persist mode.
    fn alpha_of(&self, obj: &Object) -> f64 {
        match self.fade_hold {
            Some(hold) => fade::alpha(self.now.duration_since(obj.born).as_secs_f64(), hold),
            None => 1.0,
        }
    }

    /// Dashed chrome: selection highlights + marquee band.
    fn paint_chrome(&self, cr: &cairo::Context) {
        if self.selection.is_empty() && self.marquee.is_none() {
            return;
        }
        let c = CHROME_COLOR;
        cr.set_source_rgba(c.r, c.g, c.b, c.a);
        cr.set_line_width(1.5);
        cr.set_dash(&CHROME_DASH, 0.0);
        crate::render::draw::add_rects(cr, self.selection.iter().copied().chain(self.marquee));
        cr.stroke().expect("dashed chrome");
        cr.set_dash(&[], 0.0);
    }
}

/// Clear the (already clipped) frame to transparent. Shm slots are recycled
/// and never zeroed, so this must use Source, not Over.
pub fn clear(cr: &cairo::Context) {
    cr.set_operator(cairo::Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.paint().expect("clear");
    cr.set_operator(cairo::Operator::Over);
}
