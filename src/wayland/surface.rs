//! Overlay layer-surface lifecycle and per-frame drawing (M1: single
//! output, integer scale, full damage every frame).

use anyhow::Result;
use smithay_client_toolkit::{
    compositor::CompositorState,
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{QueueHandle, protocol::wl_shm};

use super::buffer::with_cairo;
use super::state::AppState;
use crate::model::geom::Point;

pub struct Overlay {
    pub layer: LayerSurface,
    pub pool: SlotPool,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    pub dirty: bool,
    pub frame_pending: bool,
}

impl Overlay {
    /// Create a full-screen overlay surface on the compositor-chosen output.
    /// Mapped after the initial empty commit + first configure.
    pub fn create(
        compositor: &CompositorState,
        layer_shell: &LayerShell,
        shm: &Shm,
        qh: &QueueHandle<AppState>,
        namespace: &str,
        keyboard_interactivity: KeyboardInteractivity,
    ) -> Result<Self> {
        let surface = compositor.create_surface(qh);
        let layer = layer_shell.create_layer_surface(qh, surface, Layer::Overlay, Some(namespace), None);
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(keyboard_interactivity);
        layer.commit();

        // Real size arrives with the first configure; start with a minimal pool.
        let pool = SlotPool::new(4096, shm)?;
        Ok(Self {
            layer,
            pool,
            width: 0,
            height: 0,
            configured: false,
            dirty: false,
            frame_pending: false,
        })
    }

    /// Render strokes and commit one frame. Full damage (M1).
    pub fn draw(
        &mut self,
        qh: &QueueHandle<AppState>,
        strokes: &[Vec<Point>],
        current: Option<&Vec<Point>>,
    ) -> Result<()> {
        let (w, h) = (self.width as i32, self.height as i32);
        if w == 0 || h == 0 {
            return Ok(());
        }
        let (buffer, canvas) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)?;

        with_cairo(canvas, w, h, |cr| {
            // Slots are recycled and never zeroed — clear with Source or
            // ghost strokes from previous frames survive.
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.paint().expect("clear");
            cr.set_operator(cairo::Operator::Over);

            // M1: hard-coded red 4px pen.
            cr.set_source_rgb(0.898, 0.224, 0.208);
            cr.set_line_width(4.0);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            for stroke in strokes.iter().chain(current) {
                paint_polyline(cr, stroke);
            }
        })?;

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w, h);
        surface.frame(qh, surface.clone());
        buffer.attach_to(surface)?;
        self.layer.commit();

        self.dirty = false;
        self.frame_pending = true;
        Ok(())
    }
}

fn paint_polyline(cr: &cairo::Context, pts: &[Point]) {
    let Some(first) = pts.first() else { return };
    cr.new_path();
    cr.move_to(first.x, first.y);
    if pts.len() == 1 {
        // A click without motion still leaves a dot.
        cr.line_to(first.x, first.y);
    }
    for p in &pts[1..] {
        cr.line_to(p.x, p.y);
    }
    cr.stroke().expect("stroke");
}
