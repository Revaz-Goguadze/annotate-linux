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
use crate::render::board::{self, BoardKind};
use crate::render::damage::DamageTracker;
use crate::render::objects::paint_object;
use crate::render::ui::{self, UiLayout, paint::UiPaintCtx};

/// Everything one frame needs, borrowed from AppState.
pub struct FrameCtx<'a> {
    pub scene: &'a Scene,
    pub preview: Option<&'a Object>,
    pub board: BoardKind,
    pub board_opacity: f64,
    /// Present only on the output that shows the toolbar.
    pub ui: Option<(&'a UiLayout, UiPaintCtx<'a>)>,
    pub debug_damage: bool,
}

pub struct Overlay {
    pub layer: LayerSurface,
    pub pool: SlotPool,
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
        Ok(Self {
            layer,
            pool,
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
        let (buffer, canvas) = self.pool.create_buffer(bw, bh, bw * 4, wl_shm::Format::Argb8888)?;
        let slot_key = canvas.as_ptr() as usize;

        let rects = match self.damage.take(slot_key, surface_rect) {
            None => vec![surface_rect],
            Some(rs) if rs.is_empty() => {
                // Nothing owed on this slot; skip the frame entirely.
                self.dirty = false;
                return Ok(());
            }
            Some(rs) => rs,
        };

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
                    paint_object(cr, obj);
                }
            }
            if let Some(p) = ctx.preview {
                if rects.iter().any(|r| r.intersects(p.bounds)) {
                    paint_object(cr, p);
                }
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

        let surface = self.layer.wl_surface();
        if let Some(viewport) = &self.viewport {
            viewport.set_destination(self.width as i32, self.height as i32);
        }
        for r in &rects {
            // Convert logical damage to buffer px with 1px slack per side.
            let x = ((r.x * scale).floor() as i32 - 1).max(0);
            let y = ((r.y * scale).floor() as i32 - 1).max(0);
            let w = ((r.w * scale).ceil() as i32) + 2;
            let h = ((r.h * scale).ceil() as i32) + 2;
            surface.damage_buffer(x, y, w, h);
        }
        surface.frame(qh, surface.clone());
        buffer.attach_to(surface)?;
        self.layer.commit();

        self.dirty = false;
        self.frame_pending = true;
        Ok(())
    }
}
