//! AppState: all daemon state plus the SCTK protocol handler impls.

use anyhow::Result;
use calloop::LoopSignal;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{KeyboardInteractivity, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::GlobalList,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

use super::surface::Overlay;
use crate::config::Config;
use crate::ipc::protocol::{Command, Response, StatusPayload};
use crate::model::geom::Point;

const BTN_LEFT: u32 = 0x110;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Mode {
    Hidden,
    Interactive,
}

pub struct AppState {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub compositor_state: CompositorState,
    pub layer_shell: LayerShell,
    pub shm: Shm,
    pub qh: QueueHandle<AppState>,
    pub loop_signal: LoopSignal,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,

    pub config: Config,
    pub mode: Mode,
    pub overlay: Option<Overlay>,

    // M1 scene: finished strokes + the one being drawn. Replaced by
    // model::Scene in M2.
    strokes: Vec<Vec<Point>>,
    current: Option<Vec<Point>>,
}

impl AppState {
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<AppState>,
        loop_signal: LoopSignal,
        config: Config,
    ) -> Result<Self> {
        Ok(Self {
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state: OutputState::new(globals, qh),
            compositor_state: CompositorState::bind(globals, qh)
                .map_err(|e| anyhow::anyhow!("wl_compositor unavailable: {e}"))?,
            layer_shell: LayerShell::bind(globals, qh).map_err(|e| {
                anyhow::anyhow!("zwlr_layer_shell_v1 unavailable (compositor without wlr-layer-shell?): {e}")
            })?,
            shm: Shm::bind(globals, qh).map_err(|e| anyhow::anyhow!("wl_shm unavailable: {e}"))?,
            qh: qh.clone(),
            loop_signal,
            keyboard: None,
            pointer: None,
            config,
            mode: Mode::Hidden,
            overlay: None,
            strokes: Vec::new(),
            current: None,
        })
    }

    pub fn show(&mut self) -> Result<()> {
        if self.overlay.is_some() {
            return Ok(());
        }
        let ki = match self.config.general.keyboard_interactivity.as_str() {
            "on-demand" => KeyboardInteractivity::OnDemand,
            _ => KeyboardInteractivity::Exclusive,
        };
        let overlay = Overlay::create(
            &self.compositor_state,
            &self.layer_shell,
            &self.shm,
            &self.qh,
            &self.config.general.namespace,
            ki,
        )?;
        self.overlay = Some(overlay);
        self.mode = Mode::Interactive;
        log::info!("overlay shown");
        Ok(())
    }

    /// Destroying the surface (not hiding it) guarantees the keyboard grab
    /// is released and costs the compositor nothing while hidden.
    pub fn hide(&mut self) {
        if self.overlay.take().is_some() {
            log::info!("overlay hidden");
        }
        self.current = None;
        if self.config.general.auto_clear_on_toggle {
            self.strokes.clear();
        }
        self.mode = Mode::Hidden;
    }

    pub fn toggle(&mut self) -> Result<()> {
        match self.mode {
            Mode::Hidden => self.show(),
            Mode::Interactive => {
                self.hide();
                Ok(())
            }
        }
    }

    fn mark_dirty(&mut self) {
        if let Some(overlay) = &mut self.overlay {
            overlay.dirty = true;
        }
    }

    /// Called after every event-loop dispatch: render at most one frame,
    /// compositor-paced via frame callbacks.
    pub fn flush_frames(&mut self) {
        let Some(overlay) = &mut self.overlay else { return };
        if overlay.configured && overlay.dirty && !overlay.frame_pending {
            if let Err(e) = overlay.draw(&self.qh, &self.strokes, self.current.as_ref()) {
                log::error!("draw failed: {e:#}");
            }
        }
    }

    pub fn handle_command(&mut self, cmd: Command) -> Response {
        let result = match cmd {
            Command::Toggle => self.toggle(),
            Command::Show => self.show(),
            Command::Hide => {
                self.hide();
                Ok(())
            }
            Command::Clear => {
                self.strokes.clear();
                self.current = None;
                self.mark_dirty();
                Ok(())
            }
            Command::Status => {
                return Response::Status(StatusPayload {
                    mode: match self.mode {
                        Mode::Hidden => "hidden".into(),
                        Mode::Interactive => "interactive".into(),
                    },
                    tool: "pen".into(),
                    color: "#e53935".into(),
                    width: 4.0,
                    objects: self.strokes.len(),
                    outputs: self
                        .output_state
                        .outputs()
                        .filter_map(|o| self.output_state.info(&o).and_then(|i| i.name))
                        .collect(),
                });
            }
            Command::Quit => {
                self.teardown();
                self.loop_signal.stop();
                Ok(())
            }
            other => {
                return Response::Error {
                    message: format!("not implemented yet (planned milestone): {other:?}"),
                };
            }
        };
        match result {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: format!("{e:#}") },
        }
    }

    pub fn teardown(&mut self) {
        self.overlay = None;
    }
}

impl CompositorHandler for AppState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {
        // M1: integer scale 1 everywhere; fractional scale lands in M3.
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if let Some(overlay) = &mut self.overlay {
            overlay.frame_pending = false;
        }
        self.flush_frames();
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        // Compositor closed our surface (output gone, etc.).
        self.hide();
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(overlay) = &mut self.overlay else { return };
        let (w, h) = configure.new_size;
        if w > 0 && h > 0 {
            overlay.width = w;
            overlay.height = h;
        }
        overlay.configured = true;
        overlay.dirty = true;
        log::debug!("configured {w}x{h}");
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for AppState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        // Esc is the hard-wired escape hatch: always hides, not rebindable.
        if event.keysym == Keysym::Escape {
            self.hide();
        }
    }

    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl PointerHandler for AppState {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        use PointerEventKind::*;
        let Some(overlay) = &self.overlay else { return };
        let our_surface = overlay.layer.wl_surface().clone();
        for event in events {
            if event.surface != our_surface {
                continue;
            }
            let pos = Point::new(event.position.0, event.position.1);
            match event.kind {
                Press { button: BTN_LEFT, .. } => {
                    self.current = Some(vec![pos]);
                    self.mark_dirty();
                }
                Motion { .. } => {
                    if let Some(stroke) = &mut self.current {
                        stroke.push(pos);
                        self.mark_dirty();
                    }
                }
                Release { button: BTN_LEFT, .. } => {
                    if let Some(stroke) = self.current.take() {
                        self.strokes.push(stroke);
                        self.mark_dirty();
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(AppState);
delegate_output!(AppState);
delegate_shm!(AppState);
delegate_seat!(AppState);
delegate_keyboard!(AppState);
delegate_pointer!(AppState);
delegate_layer!(AppState);
delegate_registry!(AppState);
