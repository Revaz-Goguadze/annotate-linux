//! Overlay layer-surface lifecycle and per-frame drawing with per-slot
//! damage tracking and fractional-scale-aware buffers.

use anyhow::Result;
use smithay_client_toolkit::{
    compositor::CompositorState,
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{
    QueueHandle,
    protocol::{wl_output::WlOutput, wl_shm},
};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

use super::buffer::with_cairo;
use super::scaling::{FractionalScaleData, ScalingState};
use super::state::AppState;
use crate::model::geom::Rect;
use crate::model::object::Object;
use crate::model::scene::Scene;
use crate::model::fade;
use crate::render::board::{self, BoardKind};
use crate::render::cursor_fx::{self, CursorFx};
use crate::render::damage::DamageTracker;
use crate::render::objects::paint_object;
use crate::render::ui::{self, UiLayout, paint::UiPaintCtx};

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
    pub ripples: Vec<(crate::model::geom::Point, f64)>,
    /// Present only on the output that shows the toolbar.
    pub ui: Option<(&'a UiLayout, UiPaintCtx<'a>)>,
    pub debug_damage: bool,
}

pub struct Overlay {
    pub layer: LayerSurface,
    pub pool: SlotPool,
    /// ANNOTATE_PERF=1: rolling per-frame paint times (ms).
    perf: Option<Vec<f64>>,
    /// ANNOTATE_FULL_DAMAGE=1: bypass the per-slot ledger, repaint fully.
    force_full: bool,
    /// Pool byte length last frame; growth remaps the pool, which can move
    /// slot addresses and alias the per-slot ledger keys.
    pool_len: usize,
    /// Logical size from the layer configure.
    pub width: u32,
    pub height: u32,
    /// Fractional scale (preferred_scale/120) or integer fallback.
    pub scale: f64,
    pub viewport: Option<WpViewport>,
    pub fractional: Option<WpFractionalScaleV1>,
    pub configured: bool,
    pub dirty: bool,
    pub frame_pending: bool,
    pub damage: DamageTracker,
}

impl Drop for Overlay {
    fn drop(&mut self) {
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
    }
}

impl Overlay {
    /// Create a full-screen overlay surface on `output`. Mapped after the
    /// initial empty commit + first configure.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        compositor: &CompositorState,
        layer_shell: &LayerShell,
        shm: &Shm,
        scaling: &ScalingState,
        qh: &QueueHandle<AppState>,
        output: &WlOutput,
        output_key: u32,
        namespace: &str,
        keyboard_interactivity: KeyboardInteractivity,
    ) -> Result<Self> {
        let surface = compositor.create_surface(qh);
        let layer =
            layer_shell.create_layer_surface(qh, surface, Layer::Overlay, Some(namespace), Some(output));
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(keyboard_interactivity);

        // Fractional path: viewport + fractional-scale object per surface.
        // Never combine set_buffer_scale with a viewport destination.
        let (viewport, fractional) = if scaling.fractional_capable() {
            let wl_surface = layer.wl_surface();
            let viewport =
                scaling.viewporter.as_ref().map(|vp| vp.get_viewport(wl_surface, qh, ()));
            let fractional = scaling
                .fractional_manager
                .as_ref()
                .map(|m| m.get_fractional_scale(wl_surface, qh, FractionalScaleData(output_key)));
            (viewport, fractional)
        } else {
            (None, None)
        };

        layer.commit();

        let pool = SlotPool::new(4096, shm)?;
        let perf = std::env::var("ANNOTATE_PERF").is_ok_and(|v| v == "1").then(Vec::new);
        let force_full = std::env::var("ANNOTATE_FULL_DAMAGE").is_ok_and(|v| v == "1");
        Ok(Self {
            force_full,
            layer,
            pool,
            perf,
            pool_len: 0,
            width: 0,
            height: 0,
            scale: 1.0,
            viewport,
            fractional,
            configured: false,
            dirty: false,
            frame_pending: false,
            damage: DamageTracker::new(),
        })
    }

    pub fn surface_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width as f64, self.height as f64)
    }

    fn buffer_size(&self) -> (i32, i32) {
        (
            (self.width as f64 * self.scale).round() as i32,
            (self.height as f64 * self.scale).round() as i32,
        )
    }

    pub fn set_scale(&mut self, scale: f64) {
        if (scale - self.scale).abs() > 1e-6 {
            log::debug!("scale -> {scale}");
            self.scale = scale;
            self.damage.invalidate_all();
            self.dirty = true;
        }
    }

    /// Click-through: empty input region + no keyboard. `ki` restores the
    /// configured interactivity when turning passthrough off. The wl_region
    /// is copied server-side at set time, so dropping it after is safe.
    pub fn set_passthrough(
        &self,
        compositor: &CompositorState,
        on: bool,
        ki: KeyboardInteractivity,
    ) -> Result<()> {
        let surface = self.layer.wl_surface();
        if on {
            let region = smithay_client_toolkit::compositor::Region::new(compositor)?;
            surface.set_input_region(Some(region.wl_region()));
            self.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        } else {
            surface.set_input_region(None);
            self.layer.set_keyboard_interactivity(ki);
        }
        surface.commit();
        Ok(())
    }

    /// Apply a new logical size from the layer configure.
    pub fn set_size(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 && (w != self.width || h != self.height) {
            self.width = w;
            self.height = h;
            self.damage.invalidate_all();
        }
    }

    /// Render one frame and commit it. Repaints only this slot's pending
    /// damage; `None` from the tracker means full repaint.
    pub fn draw(&mut self, qh: &QueueHandle<AppState>, ctx: &FrameCtx<'_>) -> Result<()> {
        let (bw, bh) = self.buffer_size();
        if bw == 0 || bh == 0 {
            return Ok(());
        }
        let scale = self.scale;
        let surface_rect = self.surface_rect();
        // Pool growth (inside create_buffer) remaps the shm mapping: old
        // slot addresses die and can alias new slots. Checking the length
        // from BEFORE this frame's create_buffer catches last frame's
        // growth; the grown slot itself is unknown-key → full repaint.
        if self.pool.len() != self.pool_len {
            self.pool_len = self.pool.len();
            self.damage.invalidate_all();
        }
        let (buffer, canvas) = self.pool.create_buffer(bw, bh, bw * 4, wl_shm::Format::Argb8888)?;
        let slot_key = canvas.as_ptr() as usize;

        let taken = self.damage.take(slot_key, surface_rect);
        log::trace!("draw slot={slot_key:#x} owed={taken:?}");
        let rects = match taken {
            _ if self.force_full => vec![surface_rect],
            None => vec![surface_rect],
            Some(rs) if rs.is_empty() => {
                // Nothing owed on this slot; skip the frame entirely.
                self.dirty = false;
                return Ok(());
            }
            Some(rs) => rs,
        };
        // Snap clip rects outward to whole device pixels: a fractional clip
        // edge (any non-integer scale) leaves partially-covered pixels that
        // neither the Source-clear nor the repaint fully own — visible as
        // 1px ghost seams around every damage rect on opaque boards.
        let rects: Vec<Rect> = rects
            .iter()
            .map(|r| {
                let x0 = (r.x * scale).floor() / scale;
                let y0 = (r.y * scale).floor() / scale;
                let x1 = ((r.x + r.w) * scale).ceil() / scale;
                let y1 = ((r.y + r.h) * scale).ceil() / scale;
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            })
            .collect();

        let t0 = std::time::Instant::now();
        with_cairo(canvas, bw, bh, |cr| {
            // All drawing below happens in logical px.
            cr.scale(scale, scale);
            for r in &rects {
                cr.rectangle(r.x, r.y, r.w, r.h);
            }
            cr.clip();

            // Slots are recycled and never zeroed — clear with Source or
            // ghost strokes from previous frames survive.
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.paint().expect("clear");
            cr.set_operator(cairo::Operator::Over);

            board::paint(cr, ctx.board, ctx.board_opacity);

            for obj in &ctx.scene.objects {
                if rects.iter().any(|r| r.intersects(obj.bounds)) {
                    let alpha = match ctx.fade_hold {
                        Some(hold) => {
                            fade::alpha(ctx.now.duration_since(obj.born).as_secs_f64(), hold)
                        }
                        None => 1.0,
                    };
                    paint_object(cr, obj, alpha);
                }
            }
            if let Some(p) = ctx.preview {
                if rects.iter().any(|r| r.intersects(p.bounds)) {
                    paint_object(cr, p, 1.0);
                    if ctx.caret {
                        crate::render::text::paint_caret(cr, p);
                    }
                }
            }

            for (at, t) in &ctx.ripples {
                if rects.iter().any(|r| r.intersects(cursor_fx::ripple_bounds(*at))) {
                    cursor_fx::paint_ripple(cr, *at, *t, crate::util::color::Rgba::new(1.0, 0.85, 0.2, 1.0));
                }
            }
            if let Some(fx) = &ctx.cursor {
                if rects.iter().any(|r| r.intersects(fx.bounds())) {
                    cursor_fx::paint_cursor(cr, fx);
                }
            }

            // dashed chrome: selection highlights + marquee band
            if !ctx.selection.is_empty() || ctx.marquee.is_some() {
                cr.set_source_rgba(0.45, 0.65, 0.95, 0.95);
                cr.set_line_width(1.5);
                cr.set_dash(&[6.0, 4.0], 0.0);
                for r in &ctx.selection {
                    cr.rectangle(r.x, r.y, r.w, r.h);
                }
                if let Some(m) = &ctx.marquee {
                    cr.rectangle(m.x, m.y, m.w, m.h);
                }
                cr.stroke().expect("dashed chrome");
                cr.set_dash(&[], 0.0);
            }

            if let Some((layout, paint_ctx)) = &ctx.ui {
                if rects.iter().any(|r| r.intersects(ui::ui_region(layout))) {
                    ui::paint::paint(cr, layout, paint_ctx);
                }
            }

            if ctx.debug_damage {
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.9);
                cr.set_line_width(1.0 / scale);
                for r in &rects {
                    cr.rectangle(r.x, r.y, r.w, r.h);
                }
                cr.stroke().expect("debug rects");
            }
        })?;

        if let Some(samples) = &mut self.perf {
            samples.push(t0.elapsed().as_secs_f64() * 1000.0);
            if samples.len() >= 100 {
                samples.sort_by(|a, b| a.total_cmp(b));
                log::info!(
                    "perf: paint median {:.2} ms, p90 {:.2} ms over {} frames",
                    samples[samples.len() / 2],
                    samples[samples.len() * 9 / 10],
                    samples.len()
                );
                samples.clear();
            }
        }

        let surface = self.layer.wl_surface();
        if let Some(viewport) = &self.viewport {
            viewport.set_destination(self.width as i32, self.height as i32);
            // Surface-local (logical) damage: the compositor's buffer→surface
            // transform of damage_buffer at fractional scale drops regions on
            // some compositors, leaving stale on-screen fragments. Logical
            // damage is unambiguous under a viewport.
            for r in &rects {
                let x = (r.x.floor() as i32 - 1).max(0);
                let y = (r.y.floor() as i32 - 1).max(0);
                surface.damage(x, y, (r.w.ceil() as i32) + 2, (r.h.ceil() as i32) + 2);
            }
        } else {
            for r in &rects {
                // Integer-scale path: buffer-pixel damage with 1px slack.
                let x = ((r.x * scale).floor() as i32 - 1).max(0);
                let y = ((r.y * scale).floor() as i32 - 1).max(0);
                let w = ((r.w * scale).ceil() as i32) + 2;
                let h = ((r.h * scale).ceil() as i32) + 2;
                surface.damage_buffer(x, y, w, h);
            }
        }
        surface.frame(qh, surface.clone());
        buffer.attach_to(surface)?;
        self.layer.commit();

        self.dirty = false;
        self.frame_pending = true;
        Ok(())
    }
}
