//! Overlay layer-surface lifecycle and per-frame drawing with per-slot
//! damage tracking. Single output + integer scale until M3.

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
use crate::model::geom::Rect;
use crate::model::object::Object;
use crate::model::scene::Scene;
use crate::render::damage::DamageTracker;
use crate::render::objects::paint_object;

pub struct Overlay {
    pub layer: LayerSurface,
    pub pool: SlotPool,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    pub dirty: bool,
    pub frame_pending: bool,
    pub damage: DamageTracker,
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
            damage: DamageTracker::new(),
        })
    }

    pub fn surface_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width as f64, self.height as f64)
    }

    /// Render one frame and commit it. Repaints only this slot's pending
    /// damage; `None` from the tracker means full repaint.
    pub fn draw(
        &mut self,
        qh: &QueueHandle<AppState>,
        scene: &Scene,
        preview: Option<&Object>,
        debug_damage: bool,
    ) -> Result<()> {
        let (w, h) = (self.width as i32, self.height as i32);
        if w == 0 || h == 0 {
            return Ok(());
        }
        let surface_rect = self.surface_rect();
        let (buffer, canvas) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)?;
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

        with_cairo(canvas, w, h, |cr| {
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

            for obj in &scene.objects {
                if rects.iter().any(|r| r.intersects(obj.bounds)) {
                    paint_object(cr, obj);
                }
            }
            if let Some(p) = preview {
                if rects.iter().any(|r| r.intersects(p.bounds)) {
                    paint_object(cr, p);
                }
            }

            if debug_damage {
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.9);
                cr.set_line_width(1.0);
                for r in &rects {
                    cr.rectangle(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0);
                }
                cr.stroke().expect("debug rects");
            }
        })?;

        let surface = self.layer.wl_surface();
        for r in &rects {
            // +1 px slack against float rounding.
            let x = (r.x.floor() as i32 - 1).max(0);
            let y = (r.y.floor() as i32 - 1).max(0);
            surface.damage_buffer(x, y, (r.w.ceil() as i32) + 2, (r.h.ceil() as i32) + 2);
        }
        surface.frame(qh, surface.clone());
        buffer.attach_to(surface)?;
        self.layer.commit();

        self.dirty = false;
        self.frame_pending = true;
        Ok(())
    }
}
