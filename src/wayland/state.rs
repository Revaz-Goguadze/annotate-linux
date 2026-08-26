//! AppState: all daemon state plus the SCTK protocol handler impls.

use std::collections::HashMap;

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

use super::outputs::{OverlayOutput, output_key};
use super::scaling::ScalingState;
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
    pub scaling: ScalingState,
    pub qh: QueueHandle<AppState>,
    pub loop_signal: LoopSignal,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,

    pub config: Config,
    pub mode: Mode,
    /// Surfaces, present only while shown. Keyed by wl_output protocol id.
    overlays: HashMap<u32, OverlayOutput>,
    /// Annotations, persistent across hide/show. Keyed by output key.
    scenes: HashMap<u64, Scene>,
    /// Output the current pointer drag started on.
    active_drag: Option<u32>,

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
            scaling: ScalingState::bind(globals, qh),
            qh: qh.clone(),
            loop_signal,
            keyboard: None,
            pointer: None,
            width: config.appearance.default_width,
            config,
            mode: Mode::Hidden,
            overlays: HashMap::new(),
            scenes: HashMap::new(),
            active_drag: None,
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

    fn keyboard_interactivity(&self) -> KeyboardInteractivity {
        match self.config.general.keyboard_interactivity.as_str() {
            "on-demand" => KeyboardInteractivity::OnDemand,
            _ => KeyboardInteractivity::Exclusive,
        }
    }

    fn create_overlay_for(&mut self, output: &wl_output::WlOutput) -> Result<()> {
        let key = output_key(output);
        if self.overlays.contains_key(&key) {
            return Ok(());
        }
        let name = self.output_state.info(output).and_then(|i| i.name);
        let overlay = Overlay::create(
            &self.compositor_state,
            &self.layer_shell,
            &self.shm,
            &self.scaling,
            &self.qh,
            output,
            key,
            &self.config.general.namespace,
            self.keyboard_interactivity(),
        )?;
        self.scenes.entry(key as u64).or_default();
        self.overlays.insert(key, OverlayOutput { output: output.clone(), name, overlay });
        Ok(())
    }

    pub fn show(&mut self) -> Result<()> {
        if self.mode == Mode::Interactive {
            return Ok(());
        }
        let outputs: Vec<_> = self.output_state.outputs().collect();
        anyhow::ensure!(!outputs.is_empty(), "no outputs available");
        for output in &outputs {
            self.create_overlay_for(output)?;
        }
        self.mode = Mode::Interactive;
        log::info!("overlay shown on {} output(s)", self.overlays.len());
        Ok(())
    }

    /// Destroying the surfaces (not hiding them) guarantees the keyboard
    /// grab is released and costs the compositor nothing while hidden.
    /// Scenes persist unless auto-clear is on.
    pub fn hide(&mut self) {
        if !self.overlays.is_empty() {
            self.overlays.clear();
            log::info!("overlay hidden");
        }
        self.input.drag = crate::input::Drag::Idle;
        self.active_drag = None;
        if self.config.general.auto_clear_on_toggle {
            self.scenes.values_mut().for_each(|s| *s = Scene::new());
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

    pub fn on_preferred_scale(&mut self, key: u32, scale: f64) {
        if let Some(oo) = self.overlays.get_mut(&key) {
            oo.overlay.set_scale(scale);
        }
    }

    fn damage_all(&mut self) {
        for oo in self.overlays.values_mut() {
            oo.overlay.damage.invalidate_all();
            oo.overlay.dirty = true;
        }
    }

    fn damage_key(&mut self, key: u32) {
        if let Some(oo) = self.overlays.get_mut(&key) {
            oo.overlay.damage.invalidate_all();
            oo.overlay.dirty = true;
        }
    }

    fn record_damage(&mut self, key: u32, rects: &[Rect]) {
        if let Some(oo) = self.overlays.get_mut(&key) {
            for r in rects {
                oo.overlay.damage.record(*r);
            }
            if !rects.is_empty() {
                oo.overlay.dirty = true;
            }
        }
    }

    fn apply_drag_update(&mut self, key: u32, update: DragUpdate) {
        self.record_damage(key, &update.damage);
        if let Some(kind) = update.committed {
            let style = self.current_style();
            let scene = self.scenes.entry(key as u64).or_default();
            let id = scene.alloc_id();
            let obj = Object::new(id, kind, style);
            let at = scene.len();
            self.undo.commit(key as u64, Edit::Insert { at, obj }, scene);
        }
    }

    fn surface_key(&self, surface: &wl_surface::WlSurface) -> Option<u32> {
        self.overlays
            .iter()
            .find(|(_, oo)| oo.overlay.layer.wl_surface() == surface)
            .map(|(k, _)| *k)
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::SelectTool(tool) => {
                self.input.tool = tool;
                log::debug!("tool: {}", tool.name());
            }
            Action::Undo => {
                let scenes = &mut self.scenes;
                if let Some(key) = self.undo.undo(|k| scenes.get_mut(&k)) {
                    self.damage_key(key as u32);
                }
            }
            Action::Redo => {
                let scenes = &mut self.scenes;
                if let Some(key) = self.undo.redo(|k| scenes.get_mut(&k)) {
                    self.damage_key(key as u32);
                }
            }
            Action::Clear => {
                let keys: Vec<u64> = self.scenes.keys().copied().collect();
                let mut any = false;
                for key in keys {
                    let scene = self.scenes.get_mut(&key).expect("key just listed");
                    if let Some(edit) = Edit::clear_all(scene) {
                        self.undo.commit(key, edit, scene);
                        any = true;
                    }
                }
                if any {
                    self.damage_all();
                }
            }
            Action::Hide => self.hide(),
        }
    }

    /// Called after every event-loop dispatch: render at most one frame per
    /// output, compositor-paced via frame callbacks.
    pub fn flush_frames(&mut self) {
        let style = self.current_style();
        let preview = self.input.preview(&style);
        let debug_damage = self.debug_damage;
        for (key, oo) in &mut self.overlays {
            let o = &mut oo.overlay;
            if o.configured && o.dirty && !o.frame_pending {
                let scene = self.scenes.entry(*key as u64).or_default();
                let preview_here =
                    if self.active_drag == Some(*key) { preview.as_ref() } else { None };
                if let Err(e) = o.draw(&self.qh, scene, preview_here, debug_damage) {
                    log::error!("draw failed on output {key}: {e:#}");
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
                    objects: self.scenes.values().map(|s| s.len()).sum(),
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
        self.overlays.clear();
    }
}

impl CompositorHandler for AppState {
    /// Integer-scale fallback, only for surfaces without a fractional-scale
    /// object (never combine set_buffer_scale with a viewport destination).
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(key) = self.surface_key(surface) {
            if let Some(oo) = self.overlays.get_mut(&key) {
                if oo.overlay.fractional.is_none() {
                    surface.set_buffer_scale(factor);
                    oo.overlay.set_scale(factor as f64);
                }
            }
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(key) = self.surface_key(surface) {
            if let Some(oo) = self.overlays.get_mut(&key) {
                oo.overlay.frame_pending = false;
            }
        }
        self.flush_frames();
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        // Compositor closed one surface (its output usually went away).
        self.overlays.retain(|_, oo| &oo.overlay.layer != layer);
        if self.overlays.is_empty() {
            self.hide();
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(oo) = self.overlays.values_mut().find(|oo| &oo.overlay.layer == layer) else {
            return;
        };
        let (w, h) = configure.new_size;
        oo.overlay.set_size(w, h);
        oo.overlay.configured = true;
        oo.overlay.dirty = true;
        log::debug!("configured {w}x{h} on {:?}", oo.name);
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if self.mode != Mode::Hidden {
            if let Err(e) = self.create_overlay_for(&output) {
                log::error!("overlay on new output failed: {e:#}");
            }
        }
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let key = output_key(&output);
        if let Some(oo) = self.overlays.get_mut(&key) {
            oo.name = self.output_state.info(&output).and_then(|i| i.name);
        }
    }

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let key = output_key(&output);
        self.overlays.remove(&key);
        self.scenes.remove(&(key as u64));
        self.undo.forget_key(key as u64);
        if self.active_drag == Some(key) {
            self.active_drag = None;
            self.input.drag = crate::input::Drag::Idle;
        }
        log::info!("output {key} removed");
    }
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
        if let Some(key) = self.active_drag {
            self.apply_drag_update(key, update);
        }
    }
}

impl PointerHandler for AppState {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        use PointerEventKind::*;
        for event in events {
            let Some(surface_key) = self.surface_key(&event.surface) else { continue };
            let pos = Point::new(event.position.0, event.position.1);
            let style = self.current_style();
            let (key, update) = match event.kind {
                Press { button: BTN_LEFT, .. } => {
                    self.active_drag = Some(surface_key);
                    (surface_key, self.input.on_press(pos, &style))
                }
                Motion { .. } => {
                    let Some(key) = self.active_drag else { continue };
                    if key != surface_key {
                        continue; // drag crossed outputs; ignore foreign motion
                    }
                    (key, self.input.on_motion(pos, &style))
                }
                Release { button: BTN_LEFT, .. } => {
                    let Some(key) = self.active_drag.take() else { continue };
                    (key, self.input.on_release(pos, &style))
                }
                _ => continue,
            };
            self.apply_drag_update(key, update);
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
