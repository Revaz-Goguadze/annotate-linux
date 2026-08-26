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
use crate::input::{Action, DragUpdate, InputState, Tool, keymap};
use crate::ipc::protocol::{Command, Response, StatusPayload};
use crate::model::constraints::Mods;
use crate::model::edit::Edit;
use crate::model::geom::{Point, Rect};
use crate::model::object::{Object, Style};
use crate::model::scene::Scene;
use crate::model::undo::UndoStack;
use crate::util::color::Rgba;

const BTN_LEFT: u32 = 0x110;
const WIDTH_MIN: f64 = 0.5;
const WIDTH_MAX: f64 = 20.0;
/// Highlighter strokes are drawn thicker than the pen at the same setting.
const HIGHLIGHTER_WIDTH_FACTOR: f64 = 3.0;

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

    scene: Scene,
    undo: UndoStack,
    input: InputState,
    palette: Vec<Rgba>,
    color_idx: usize,
    width: f64,
    debug_damage: bool,
}

impl AppState {
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<AppState>,
        loop_signal: LoopSignal,
        config: Config,
    ) -> Result<Self> {
        let palette: Vec<Rgba> = config
            .appearance
            .palette
            .iter()
            .filter_map(|s| Rgba::parse(s).ok())
            .collect();
        let palette = if palette.is_empty() { vec![Rgba::new(0.9, 0.2, 0.2, 1.0)] } else { palette };
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
            width: config.appearance.default_width,
            config,
            mode: Mode::Hidden,
            overlay: None,
            scene: Scene::new(),
            undo: UndoStack::default(),
            input: InputState::default(),
            palette,
            color_idx: 0,
            debug_damage: std::env::var("ANNOTATE_DEBUG_DAMAGE").is_ok_and(|v| v == "1"),
        })
    }

    fn current_style(&self) -> Style {
        let highlighter = self.input.tool == Tool::Highlighter;
        Style {
            stroke: self.palette[self.color_idx],
            width: if highlighter { self.width * HIGHLIGHTER_WIDTH_FACTOR } else { self.width },
            group_alpha: if highlighter { self.config.appearance.highlighter_alpha } else { 1.0 },
        }
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
        self.input.drag = crate::input::Drag::Idle;
        if self.config.general.auto_clear_on_toggle {
            self.scene = Scene::new();
            self.undo.clear();
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

    fn damage_all(&mut self) {
        if let Some(overlay) = &mut self.overlay {
            overlay.damage.invalidate_all();
            overlay.dirty = true;
        }
    }

    fn record_damage(&mut self, rects: &[Rect]) {
        if let Some(overlay) = &mut self.overlay {
            for r in rects {
                overlay.damage.record(*r);
            }
            if !rects.is_empty() {
                overlay.dirty = true;
            }
        }
    }

    fn apply_drag_update(&mut self, update: DragUpdate) {
        self.record_damage(&update.damage);
        if let Some(kind) = update.committed {
            let style = self.current_style();
            let id = self.scene.alloc_id();
            let obj = Object::new(id, kind, style);
            let at = self.scene.len();
            self.undo.commit(Edit::Insert { at, obj }, &mut self.scene);
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::SelectTool(tool) => {
                self.input.tool = tool;
                log::debug!("tool: {}", tool.name());
            }
            Action::Undo => {
                if self.undo.undo(&mut self.scene) {
                    self.damage_all();
                }
            }
            Action::Redo => {
                if self.undo.redo(&mut self.scene) {
                    self.damage_all();
                }
            }
            Action::Clear => {
                if let Some(edit) = Edit::clear_all(&self.scene) {
                    self.undo.commit(edit, &mut self.scene);
                    self.damage_all();
                }
            }
            Action::Hide => self.hide(),
        }
    }

    /// Called after every event-loop dispatch: render at most one frame,
    /// compositor-paced via frame callbacks.
    pub fn flush_frames(&mut self) {
        let preview = self
            .overlay
            .as_ref()
            .and_then(|o| (o.configured && o.dirty && !o.frame_pending).then_some(()))
            .and_then(|()| self.input.preview(&self.current_style()));
        let debug_damage = self.debug_damage;
        let scene = &self.scene;
        if let Some(overlay) = &mut self.overlay {
            if overlay.configured && overlay.dirty && !overlay.frame_pending {
                if let Err(e) = overlay.draw(&self.qh, scene, preview.as_ref(), debug_damage) {
                    log::error!("draw failed: {e:#}");
                }
            }
        }
    }

    fn set_color(&mut self, value: &str) -> Result<()> {
        if value == "next" {
            self.color_idx = (self.color_idx + 1) % self.palette.len();
        } else if value == "prev" {
            self.color_idx = (self.color_idx + self.palette.len() - 1) % self.palette.len();
        } else if let Ok(idx) = value.parse::<usize>() {
            anyhow::ensure!(idx < self.palette.len(), "palette index {idx} out of range");
            self.color_idx = idx;
        } else {
            let rgba = Rgba::parse(value)?;
            // ad-hoc colors are appended so the index stays meaningful
            self.palette.push(rgba);
            self.color_idx = self.palette.len() - 1;
        }
        Ok(())
    }

    fn set_width(&mut self, value: &str) -> Result<()> {
        let new = if let Some(delta) = value.strip_prefix('+') {
            self.width + delta.parse::<f64>()?
        } else if value.starts_with('-') {
            self.width + value.parse::<f64>()?
        } else {
            value.parse::<f64>()?
        };
        self.width = new.clamp(WIDTH_MIN, WIDTH_MAX);
        Ok(())
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
                self.dispatch(Action::Clear);
                Ok(())
            }
            Command::Undo => {
                self.dispatch(Action::Undo);
                Ok(())
            }
            Command::Redo => {
                self.dispatch(Action::Redo);
                Ok(())
            }
            Command::Tool { name } => match Tool::from_name(&name) {
                Some(tool) => {
                    self.dispatch(Action::SelectTool(tool));
                    Ok(())
                }
                None => Err(anyhow::anyhow!("unknown tool {name:?}")),
            },
            Command::Color { value } => self.set_color(&value),
            Command::Width { value } => self.set_width(&value),
            Command::Status => {
                return Response::Status(StatusPayload {
                    mode: match self.mode {
                        Mode::Hidden => "hidden".into(),
                        Mode::Interactive => "interactive".into(),
                    },
                    tool: self.input.tool.name().into(),
                    color: self.palette[self.color_idx].to_hex(),
                    width: self.width,
                    objects: self.scene.len(),
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
        // M2: integer scale 1 everywhere; fractional scale lands in M3.
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
        if w > 0 && h > 0 && (w != overlay.width || h != overlay.height) {
            overlay.width = w;
            overlay.height = h;
            overlay.damage.invalidate_all();
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
        if let Some(action) = keymap::action_for(event.keysym, self.input.mods) {
            self.dispatch(action);
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
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        let mods = Mods {
            shift: modifiers.shift,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            logo: modifiers.logo,
        };
        let style = self.current_style();
        let update = self.input.on_mods_changed(mods, &style);
        self.apply_drag_update(update);
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
            let style = self.current_style();
            let update = match event.kind {
                Press { button: BTN_LEFT, .. } => self.input.on_press(pos, &style),
                Motion { .. } => self.input.on_motion(pos, &style),
                Release { button: BTN_LEFT, .. } => self.input.on_release(pos, &style),
                _ => continue,
            };
            self.apply_drag_update(update);
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
