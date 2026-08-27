//! AppState: all daemon state plus the SCTK protocol handler impls.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use calloop::timer::{TimeoutAction, Timer};
use calloop::{LoopHandle, LoopSignal};
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
use super::surface::{FrameCtx, Overlay};
use crate::config::keys::Keymap;
use crate::config::state::RuntimeState;
use crate::config::Config;
use crate::input::{Action, Drag, DragUpdate, InputState, Tool, text_edit};
use crate::model::fade;
use crate::render::board::BoardKind;
use crate::render::cursor_fx::{CursorFx, CursorStyle};
use crate::render::ui::{self, UiButton, UiHit, UiState, paint::UiPaintCtx};
use crate::ipc::protocol::{Command, Response, StatusPayload};
use crate::model::constraints::Mods;
use crate::model::edit::Edit;
use crate::model::geom::{Point, Rect};
use crate::model::object::{Object, ObjectKind, Style};
use crate::model::scene::Scene;
use crate::model::undo::UndoStack;
use crate::util::color::Rgba;

const BTN_LEFT: u32 = 0x110;
const WIDTH_MIN: f64 = 0.5;
const WIDTH_MAX: f64 = 20.0;
/// Upper bound on palette entries, so ad-hoc colors cannot grow it forever.
const PALETTE_MAX: usize = 64;
/// Highlighter strokes are drawn thicker than the pen at the same setting.
const HIGHLIGHTER_WIDTH_FACTOR: f64 = 3.0;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Mode {
    Hidden,
    Interactive,
    /// Always-on: annotations visible, all input passes through.
    Passthrough,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const TICK: Duration = Duration::from_millis(33);

/// An open text entry (new or double-click edit). Not yet in the scene.
struct TextDraft {
    key: u32,
    at: Point,
    s: String,
    px: f64,
    style: Style,
    /// (index, original) when editing an existing object.
    replace: Option<(usize, Object)>,
}

impl TextDraft {
    fn object(&self) -> Object {
        Object::new(
            crate::model::object::ObjectId(0),
            ObjectKind::Text { at: self.at, s: self.s.clone(), px: self.px },
            self.style,
        )
    }
}

/// Live reposition drag of existing objects (text tool single, select
/// tool multi). Scene objects mutate in place; undo records the inverse
/// batch on release.
struct ObjMove {
    key: u32,
    /// (index, original object) per moved item.
    items: Vec<(usize, Object)>,
    grab: Point,
    moved: bool,
}

const HIT_TOL: f64 = 6.0;

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
    ui: UiState,
    board: BoardKind,
    board_opacity: f64,
    /// Output under the pointer; the toolbar lives here.
    focused_output: Option<u32>,
    counter_next: u32,
    text_draft: Option<TextDraft>,
    obj_move: Option<ObjMove>,
    last_click: Option<(Instant, u32, usize)>,
    /// Selected object ids on one output (select tool).
    selection: Option<(u32, Vec<crate::model::object::ObjectId>)>,
    /// In-process clipboard for copy/cut/paste/duplicate.
    clipboard: Vec<Object>,
    /// Rubber-band in progress: (output, anchor, cursor).
    marquee: Option<(u32, Point, Point)>,
    /// Eraser sweep: (output, last sample point, removals in order).
    erase: Option<(u32, Point, Vec<(usize, Object)>)>,

    // fade mode
    fade_enabled: bool,
    fade_seconds: f64,
    fade_timer: bool,
    // cursor fx
    cursor_style: CursorStyle,
    cursor_highlight: bool,
    pointer_pos: Option<(u32, Point)>,
    /// Live click ripples: (output, center, started).
    ripples: Vec<(u32, Point, Instant)>,
    fx_timer: bool,
    pub loop_handle: LoopHandle<'static, AppState>,
    keymap: Keymap,
    /// Debounced runtime-state save armed?
    state_timer: bool,
    debug_damage: bool,
}

impl AppState {
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<AppState>,
        loop_signal: LoopSignal,
        loop_handle: LoopHandle<'static, AppState>,
        config: Config,
    ) -> Result<Self> {
        let palette: Vec<Rgba> = config
            .appearance
            .palette
            .iter()
            .filter_map(|s| Rgba::parse(s).ok())
            .collect();
        let palette = if palette.is_empty() { vec![Rgba::new(0.9, 0.2, 0.2, 1.0)] } else { palette };
        let board_opacity = config.appearance.board_opacity.clamp(0.1, 1.0);
        let keymap = Keymap::with_overrides(&config.keys)?;
        let cursor_style = CursorStyle::from_name(&config.cursor.style).unwrap_or_default();
        let cursor_highlight = config.cursor.highlight;

        // Restore last session's tool/color/width/board/fade.
        let saved = RuntimeState::load();
        // Restoring a non-drawing tool reads as "drawing is broken" after a
        // restart — those start back on the pen.
        let tool = match Tool::from_name(&saved.tool) {
            Some(Tool::Eraser) | Some(Tool::Select) | None => Tool::Pen,
            Some(t) => t,
        };
        let mut palette = palette;
        let color_idx = if saved.color.is_empty() {
            0
        } else if let Ok(c) = Rgba::parse(&saved.color) {
            palette.iter().position(|p| *p == c).unwrap_or_else(|| {
                palette.push(c);
                palette.len() - 1
            })
        } else {
            0
        };
        let width = if saved.width > 0.0 {
            saved.width.clamp(WIDTH_MIN, WIDTH_MAX)
        } else {
            config.appearance.default_width
        };
        let board = BoardKind::from_name(&saved.board).unwrap_or(BoardKind::None);
        let fade_enabled = saved.fade || config.general.fade_default;
        let fade_seconds = config.general.fade_seconds;
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
            width,
            config,
            mode: Mode::Hidden,
            overlays: HashMap::new(),
            scenes: HashMap::new(),
            active_drag: None,
            undo: UndoStack::default(),
            input: InputState { tool, ..Default::default() },
            palette,
            color_idx,
            ui: UiState::default(),
            board,
            board_opacity,
            focused_output: None,
            counter_next: 1,
            text_draft: None,
            obj_move: None,
            last_click: None,
            selection: None,
            clipboard: Vec::new(),
            marquee: None,
            erase: None,
            fade_enabled,
            fade_seconds,
            fade_timer: false,
            cursor_style,
            cursor_highlight,
            pointer_pos: None,
            ripples: Vec::new(),
            fx_timer: false,
            loop_handle,
            keymap,
            state_timer: false,
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
        self.commit_text_draft();
        self.obj_move = None;
        self.last_click = None;
        self.selection = None;
        self.marquee = None;
        self.erase = None;
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
            Mode::Interactive | Mode::Passthrough => {
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
        log::trace!("record key={key} rects={rects:?}");
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
        log::trace!("drag={:?} damage={:?}", std::mem::discriminant(&self.input.drag), update.damage);
        self.record_damage(key, &update.damage);
        if let Some(kind) = update.committed {
            let style = self.current_style();
            let scene = self.scenes.entry(key as u64).or_default();
            let id = scene.alloc_id();
            let obj = Object::new(id, kind, style);
            let at = scene.len();
            self.undo.commit(key as u64, Edit::Insert { at, obj }, scene);
            self.ensure_fade_timer();
        }
    }

    fn surface_key(&self, surface: &wl_surface::WlSurface) -> Option<u32> {
        self.overlays
            .iter()
            .find(|(_, oo)| oo.overlay.layer.wl_surface() == surface)
            .map(|(k, _)| *k)
    }

    /// The output showing the toolbar: the one under the pointer, else any.
    fn ui_output_key(&self) -> Option<u32> {
        self.focused_output
            .filter(|k| self.overlays.contains_key(k))
            .or_else(|| self.overlays.keys().next().copied())
    }

    fn ui_layout_on(&self, key: u32) -> Option<ui::UiLayout> {
        let oo = self.overlays.get(&key)?;
        Some(ui::layout(oo.overlay.surface_rect(), self.palette.len(), &self.ui))
    }

    /// Damage the maximal UI region (toolbar + both popups) on the UI output,
    /// covering opens, closes, and indicator changes in one conservative rect.
    fn damage_ui(&mut self) {
        let Some(key) = self.ui_output_key() else { return };
        let Some(oo) = self.overlays.get_mut(&key) else { return };
        let all_open = UiState { color_picker_open: true, width_picker_open: true };
        let layout = ui::layout(oo.overlay.surface_rect(), self.palette.len(), &all_open);
        oo.overlay.damage.record(ui::ui_region(&layout));
        oo.overlay.dirty = true;
    }

    fn set_board(&mut self, kind: BoardKind) {
        if self.board != kind {
            self.board = kind;
            self.mark_state_dirty();
            self.damage_all();
        }
    }

    /// Drop in-flight pointer interactions that captured scene indices —
    /// any scene mutation from outside the drag (undo/redo/clear/paste via
    /// IPC or keyboard, fade gc) invalidates them; recording their inverses
    /// afterwards would corrupt or panic on undo.
    fn abort_index_interactions(&mut self) {
        self.obj_move = None;
        self.marquee = None;
        self.erase = None;
    }

    /// Debounced (750 ms) atomic save of tool/color/width/board/fade.
    fn mark_state_dirty(&mut self) {
        if self.state_timer {
            return;
        }
        self.state_timer = true;
        let _ = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_millis(750)),
            |_, _, state: &mut AppState| {
                state.state_timer = false;
                let snapshot = RuntimeState {
                    tool: state.input.tool.name().into(),
                    color: state.palette[state.color_idx].to_hex(),
                    width: state.width,
                    board: state.board.name().into(),
                    fade: state.fade_enabled,
                };
                if let Err(e) = snapshot.save() {
                    log::warn!("state save failed: {e:#}");
                }
                TimeoutAction::Drop
            },
        );
    }

    /// Re-read config.toml and apply what can change at runtime.
    fn reload_config(&mut self) -> Result<()> {
        let config = Config::load()?;
        let keymap = Keymap::with_overrides(&config.keys)?;
        let palette: Vec<Rgba> =
            config.appearance.palette.iter().filter_map(|s| Rgba::parse(s).ok()).collect();
        if !palette.is_empty() {
            self.palette = palette;
            self.color_idx = self.color_idx.min(self.palette.len() - 1);
        }
        self.keymap = keymap;
        self.board_opacity = config.appearance.board_opacity.clamp(0.1, 1.0);
        self.fade_seconds = config.general.fade_seconds;
        self.cursor_style = CursorStyle::from_name(&config.cursor.style).unwrap_or_default();
        self.cursor_highlight = config.cursor.highlight;
        self.config = config;
        self.damage_all();
        log::info!("config reloaded");
        Ok(())
    }

    /// Arm the ~30 Hz fade tick if fade mode is on and anything exists.
    fn ensure_fade_timer(&mut self) {
        if self.fade_timer || !self.fade_enabled {
            return;
        }
        if self.scenes.values().all(|s| s.is_empty()) {
            return;
        }
        self.fade_timer = true;
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(TICK), |_, _, state: &mut AppState| {
                state.on_fade_tick()
            });
    }

    fn on_fade_tick(&mut self) -> TimeoutAction {
        if !self.fade_enabled {
            self.fade_timer = false;
            return TimeoutAction::Drop;
        }
        let now = Instant::now();
        let hold = self.fade_seconds;
        let keys: Vec<u64> = self.scenes.keys().copied().collect();
        let mut any_objects = false;
        for k in keys {
            let scene = self.scenes.get_mut(&k).expect("listed key");
            let mut damage: Vec<Rect> = Vec::new();
            let mut gc: Vec<usize> = Vec::new();
            for (i, o) in scene.objects.iter().enumerate() {
                let age = now.duration_since(o.born).as_secs_f64();
                if fade::is_fading(age, hold) {
                    damage.push(o.bounds);
                }
                if fade::gc_due(age, hold) {
                    gc.push(i);
                }
            }
            if !gc.is_empty() {
                for &i in gc.iter().rev() {
                    let o = scene.objects.remove(i);
                    damage.push(o.bounds);
                }
                self.abort_index_interactions();
                // Index-based undo entries are invalid after gc; dropping the
                // history is what guarantees undo never resurrects a faded
                // ghost.
                log::debug!("fade gc: removed {} object(s) on key {k}, dropping undo history", gc.len());
                self.undo.forget_key(k);
                if let Some((sk, _)) = &self.selection {
                    if *sk as u64 == k {
                        self.selection = None;
                    }
                }
            }
            any_objects |= !self.scenes[&k].objects.is_empty();
            if !damage.is_empty() {
                self.record_damage(k as u32, &damage);
            }
        }
        if any_objects {
            TimeoutAction::ToDuration(TICK)
        } else {
            self.fade_timer = false;
            TimeoutAction::Drop
        }
    }

    fn ensure_fx_timer(&mut self) {
        if self.fx_timer || self.ripples.is_empty() {
            return;
        }
        self.fx_timer = true;
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(TICK), |_, _, state: &mut AppState| {
                state.on_fx_tick()
            });
    }

    fn on_fx_tick(&mut self) -> TimeoutAction {
        let ttl = Duration::from_millis(self.config.cursor.ripple_ms.max(50));
        let now = Instant::now();
        let expired: Vec<_> = self
            .ripples
            .iter()
            .filter(|(_, _, t0)| now.duration_since(*t0) >= ttl)
            .cloned()
            .collect();
        self.ripples.retain(|(_, _, t0)| now.duration_since(*t0) < ttl);
        let damages: Vec<(u32, Rect)> = self
            .ripples
            .iter()
            .map(|(k, at, _)| (*k, crate::render::cursor_fx::ripple_bounds(*at)))
            .chain(expired.iter().map(|(k, at, _)| (*k, crate::render::cursor_fx::ripple_bounds(*at))))
            .collect();
        for (k, r) in damages {
            self.record_damage(k, &[r]);
        }
        if self.ripples.is_empty() {
            self.fx_timer = false;
            TimeoutAction::Drop
        } else {
            TimeoutAction::ToDuration(TICK)
        }
    }

    /// Enter/leave click-through mode. Recovery from passthrough is IPC
    /// only (`annotate-linux passthrough off`) — the surface takes no input.
    pub fn set_passthrough(&mut self, on: bool) -> Result<()> {
        if on {
            if self.mode == Mode::Hidden {
                self.show()?;
            }
            self.commit_text_draft();
            self.obj_move = None;
            self.marquee = None;
            self.erase = None;
            self.input.drag = Drag::Idle;
            self.active_drag = None;
            for oo in self.overlays.values() {
                oo.overlay.set_passthrough(&self.compositor_state, true, KeyboardInteractivity::None)?;
            }
            self.mode = Mode::Passthrough;
            log::info!("passthrough on");
        } else if self.mode == Mode::Passthrough {
            let ki = self.keyboard_interactivity();
            for oo in self.overlays.values() {
                oo.overlay.set_passthrough(&self.compositor_state, false, ki)?;
            }
            self.mode = Mode::Interactive;
            log::info!("passthrough off");
        }
        Ok(())
    }

    fn cursor_fx_active(&self) -> bool {
        self.cursor_highlight || self.cursor_style.hides_system_cursor()
    }

    fn cursor_fx_for(&self, key: u32) -> Option<CursorFx> {
        let (pk, pos) = self.pointer_pos?;
        if pk != key || !self.cursor_fx_active() {
            return None;
        }
        Some(CursorFx {
            pos,
            style: self.cursor_style,
            highlight: self.cursor_highlight,
            highlight_radius: self.config.cursor.highlight_radius,
            color: self.palette[self.color_idx],
        })
    }

    fn damage_cursor(&mut self, key: u32, old: Option<Point>, new: Option<Point>) {
        if !self.cursor_fx_active() {
            return;
        }
        let r = self.config.cursor.highlight_radius.max(16.0) + 4.0;
        let mut damage = Rect::default();
        for p in [old, new].into_iter().flatten() {
            damage = damage.union(Rect::new(p.x - r, p.y - r, 2.0 * r, 2.0 * r));
        }
        if !damage.is_empty() {
            self.record_damage(key, &[damage]);
        }
    }

    fn text_style(&self) -> Style {
        Style { stroke: self.palette[self.color_idx], width: 2.0, group_alpha: 1.0 }
    }

    fn topmost_text_at(scene: &Scene, pos: Point) -> Option<usize> {
        scene
            .objects
            .iter()
            .rposition(|o| matches!(o.kind, ObjectKind::Text { .. }) && o.bounds.contains(pos))
    }

    /// Commit (or discard, when empty and new) the open text draft.
    fn commit_text_draft(&mut self) {
        let Some(mut d) = self.text_draft.take() else { return };
        let key64 = d.key as u64;
        let obj_bounds = d.object().bounds;
        let scene = self.scenes.entry(key64).or_default();
        match d.replace.take() {
            Some((idx, orig)) => {
                let idx = idx.min(scene.len());
                if d.s.is_empty() {
                    // Edited down to nothing: object stays removed.
                    self.undo.record_applied(key64, Edit::Insert { at: idx, obj: orig });
                } else {
                    let mut obj = d.object();
                    obj.id = orig.id;
                    scene.objects.insert(idx, obj);
                    self.undo.record_applied(key64, Edit::Replace { at: idx, obj: orig });
                }
            }
            None => {
                if !d.s.is_empty() {
                    let mut obj = d.object();
                    obj.id = scene.alloc_id();
                    let at = scene.len();
                    self.undo.commit(key64, Edit::Insert { at, obj }, scene);
                }
            }
        }
        self.record_damage(d.key, &[obj_bounds]);
        self.ensure_fade_timer();
    }

    /// Keystroke routed into the open draft. Returns true when consumed.
    fn handle_text_key(&mut self, event: &KeyEvent) -> bool {
        if self.text_draft.is_none() {
            return false;
        }
        let old_bounds = self.text_draft.as_ref().map(|d| d.object().bounds);
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter | Keysym::Escape => {
                self.commit_text_draft();
                return true;
            }
            Keysym::BackSpace => {
                let ctrl = self.input.mods.ctrl;
                if let Some(d) = &mut self.text_draft {
                    if ctrl {
                        text_edit::backspace_word(&mut d.s);
                    } else {
                        text_edit::backspace(&mut d.s);
                    }
                }
            }
            _ => {
                let Some(u) = event.utf8.as_deref().filter(|u| !u.is_empty()) else { return true };
                if let Some(d) = &mut self.text_draft {
                    text_edit::push_str(&mut d.s, u);
                }
            }
        }
        if let (Some(old), Some(d)) = (old_bounds, self.text_draft.as_ref()) {
            let (key, damage) = (d.key, old.union(d.object().bounds));
            self.record_damage(key, &[damage]);
        }
        true
    }

    /// Key-repeat events (from SCTK's calloop repeat source).
    pub fn on_repeat_key(&mut self, event: KeyEvent) {
        self.handle_text_key(&event);
    }

    /// Press with the Text tool: new draft, move-drag, or double-click edit.
    fn handle_text_press(&mut self, key: u32, pos: Point) {
        self.commit_text_draft();
        let scene = self.scenes.entry(key as u64).or_default();
        if let Some(idx) = Self::topmost_text_at(scene, pos) {
            let now = Instant::now();
            let dbl = self
                .last_click
                .is_some_and(|(t, k, i)| k == key && i == idx && now.duration_since(t) < DOUBLE_CLICK);
            self.last_click = Some((now, key, idx));
            if dbl {
                // Second click: lift the object out of the scene into a draft.
                let orig = scene.objects.remove(idx);
                let ObjectKind::Text { at, s, px } = orig.kind.clone() else { return };
                let bounds = orig.bounds;
                self.text_draft =
                    Some(TextDraft { key, at, s, px, style: orig.style, replace: Some((idx, orig)) });
                self.record_damage(key, &[bounds]);
            } else {
                let orig = scene.objects[idx].clone();
                self.obj_move =
                    Some(ObjMove { key, items: vec![(idx, orig)], grab: pos, moved: false });
            }
        } else {
            self.last_click = None;
            let d = TextDraft {
                key,
                at: pos,
                s: String::new(),
                px: self.config.appearance.text_px,
                style: self.text_style(),
                replace: None,
            };
            let bounds = d.object().bounds;
            self.text_draft = Some(d);
            self.record_damage(key, &[bounds]);
        }
    }

    fn handle_counter_press(&mut self, key: u32, pos: Point) {
        let n = self.counter_next;
        self.counter_next += 1;
        let style = self.text_style();
        let r = self.config.appearance.counter_radius;
        let scene = self.scenes.entry(key as u64).or_default();
        let id = scene.alloc_id();
        let obj = Object::new(id, ObjectKind::Counter { at: pos, n, r }, style);
        let bounds = obj.bounds;
        let at = scene.len();
        self.undo.commit(key as u64, Edit::Insert { at, obj }, scene);
        self.record_damage(key, &[bounds]);
        self.ensure_fade_timer();
    }

    fn handle_move_motion(&mut self, surface_key: u32, pos: Point) -> bool {
        let Some(mv) = &mut self.obj_move else { return false };
        if mv.key != surface_key {
            return true;
        }
        mv.moved = true;
        let (dx, dy) = (pos.x - mv.grab.x, pos.y - mv.grab.y);
        let key = mv.key;
        let mut damage = Rect::default();
        let items = mv.items.clone();
        let scene = self.scenes.entry(key as u64).or_default();
        for (idx, orig) in items {
            let mut moved = orig;
            moved.kind.translate(dx, dy);
            moved.recompute_bounds();
            let Some(slot) = scene.objects.get_mut(idx) else { continue };
            damage = damage.union(slot.bounds).union(moved.bounds);
            *slot = moved;
        }
        self.record_damage(key, &[damage]);
        true
    }

    /// Finish a move drag: one batch inverse, so a single undo restores
    /// every moved object at once.
    fn handle_move_release(&mut self) -> bool {
        let Some(mv) = self.obj_move.take() else { return false };
        if mv.moved {
            let inverses: Vec<Edit> =
                mv.items.into_iter().map(|(at, obj)| Edit::Replace { at, obj }).collect();
            self.undo.record_applied(mv.key as u64, Edit::Batch(inverses));
        }
        true
    }

    fn selected_indices(&self, key: u32) -> Vec<usize> {
        let Some((sel_key, ids)) = &self.selection else { return Vec::new() };
        if *sel_key != key {
            return Vec::new();
        }
        let Some(scene) = self.scenes.get(&(key as u64)) else { return Vec::new() };
        let mut idxs: Vec<usize> = ids.iter().filter_map(|id| scene.index_of(*id)).collect();
        idxs.sort_unstable();
        idxs
    }

    fn selection_damage(&mut self) {
        let Some((key, _)) = self.selection.clone() else { return };
        let idxs = self.selected_indices(key);
        let Some(scene) = self.scenes.get(&(key as u64)) else { return };
        let mut r = Rect::default();
        for i in idxs {
            r = r.union(scene.objects[i].bounds);
        }
        self.record_damage(key, &[r.inflate(4.0)]);
    }

    fn handle_select_press(&mut self, key: u32, pos: Point) {
        self.selection_damage(); // old highlight area repaints
        let scene = self.scenes.entry(key as u64).or_default();
        if let Some(idx) = scene.objects.iter().rposition(|o| o.hit_test(pos, HIT_TOL)) {
            let id = scene.objects[idx].id;
            let already_selected = self
                .selection
                .as_ref()
                .is_some_and(|(k, ids)| *k == key && ids.contains(&id));
            if !already_selected {
                self.selection = Some((key, vec![id]));
            }
            let items: Vec<(usize, Object)> = self
                .selected_indices(key)
                .into_iter()
                .map(|i| (i, self.scenes[&(key as u64)].objects[i].clone()))
                .collect();
            self.obj_move = Some(ObjMove { key, items, grab: pos, moved: false });
            self.selection_damage();
        } else {
            self.selection = None;
            self.marquee = Some((key, pos, pos));
        }
    }

    fn handle_marquee_motion(&mut self, surface_key: u32, pos: Point) -> bool {
        let Some((key, anchor, cur)) = &mut self.marquee else { return false };
        if *key != surface_key {
            return true;
        }
        let old = Rect::from_corners(*anchor, *cur);
        *cur = pos;
        let new = Rect::from_corners(*anchor, pos);
        let (key, damage) = (*key, old.union(new).inflate(2.0));
        self.record_damage(key, &[damage]);
        true
    }

    /// Marquee release: select every object whose bounds intersect the band.
    fn handle_marquee_release(&mut self) -> bool {
        let Some((key, anchor, cur)) = self.marquee.take() else { return false };
        let band = Rect::from_corners(anchor, cur);
        let scene = self.scenes.entry(key as u64).or_default();
        let ids: Vec<_> = scene
            .objects
            .iter()
            .filter(|o| band.intersects(o.bounds))
            .map(|o| o.id)
            .collect();
        self.selection = (!ids.is_empty()).then_some((key, ids));
        self.record_damage(key, &[band.inflate(6.0)]);
        self.selection_damage();
        true
    }

    fn erase_at(&mut self, key: u32, pos: Point) {
        // Remove everything under the point, topmost first.
        loop {
            let scene = self.scenes.entry(key as u64).or_default();
            let Some(idx) = scene.objects.iter().rposition(|o| o.hit_test(pos, HIT_TOL)) else {
                return;
            };
            let obj = scene.objects.remove(idx);
            let bounds = obj.bounds;
            if let Some((_, _, removed)) = &mut self.erase {
                removed.push((idx, obj));
            }
            self.record_damage(key, &[bounds]);
        }
    }

    /// Sample the sweep densely between motion events so a fast (or warped)
    /// pointer can't jump over objects.
    fn erase_along(&mut self, key: u32, to: Point) {
        let Some((_, last, _)) = &mut self.erase else { return };
        let from = *last;
        *last = to;
        let dist = from.dist(to);
        let steps = (dist / (HIT_TOL * 0.8)).ceil().max(1.0) as usize;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let p = Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            self.erase_at(key, p);
        }
    }

    fn handle_erase_release(&mut self) -> bool {
        let Some((key, _, removed)) = self.erase.take() else { return false };
        if !removed.is_empty() {
            // Inverse of sequential removals: re-insert in reverse order.
            let inverses: Vec<Edit> = removed
                .into_iter()
                .rev()
                .map(|(at, obj)| Edit::Insert { at, obj })
                .collect();
            self.undo.record_applied(key as u64, Edit::Batch(inverses));
        }
        true
    }

    /// Delete the current selection as one undoable batch.
    fn delete_selection(&mut self) {
        self.abort_index_interactions();
        let Some((key, _)) = self.selection.clone() else { return };
        let idxs = self.selected_indices(key);
        if idxs.is_empty() {
            self.selection = None;
            return;
        }
        let scene = self.scenes.entry(key as u64).or_default();
        let mut damage = Rect::default();
        for &i in &idxs {
            damage = damage.union(scene.objects[i].bounds);
        }
        let edit = Edit::Batch(idxs.iter().rev().map(|&at| Edit::Remove { at }).collect());
        self.undo.commit(key as u64, edit, scene);
        self.selection = None;
        self.record_damage(key, &[damage.inflate(4.0)]);
    }

    fn selected_objects(&self) -> Vec<Object> {
        let Some((key, _)) = self.selection.clone() else { return Vec::new() };
        let idxs = self.selected_indices(key);
        let Some(scene) = self.scenes.get(&(key as u64)) else { return Vec::new() };
        idxs.into_iter().map(|i| scene.objects[i].clone()).collect()
    }

    /// Paste clones (offset) onto the selection's output or the focused one.
    fn paste_objects(&mut self, objs: Vec<Object>) {
        if objs.is_empty() {
            return;
        }
        self.abort_index_interactions();
        let Some(key) = self
            .selection
            .as_ref()
            .map(|(k, _)| *k)
            .or(self.focused_output)
            .or_else(|| self.overlays.keys().next().copied())
        else {
            return;
        };
        let scene = self.scenes.entry(key as u64).or_default();
        let mut edits = Vec::new();
        let mut new_ids = Vec::new();
        let mut damage = Rect::default();
        let base = scene.len();
        for (i, mut obj) in objs.into_iter().enumerate() {
            obj.kind.translate(16.0, 16.0);
            obj.id = scene.alloc_id();
            obj.recompute_bounds();
            damage = damage.union(obj.bounds);
            new_ids.push(obj.id);
            edits.push(Edit::Insert { at: base + i, obj });
        }
        self.undo.commit(key as u64, Edit::Batch(edits), self.scenes.entry(key as u64).or_default());
        self.selection = Some((key, new_ids));
        self.record_damage(key, &[damage.inflate(4.0)]);
        self.selection_damage();
        self.ensure_fade_timer();
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::SelectTool(tool) => {
                if tool != Tool::Text {
                    self.commit_text_draft();
                }
                if tool != Tool::Select && self.selection.is_some() {
                    self.selection_damage();
                    self.selection = None;
                }
                self.input.tool = tool;
                self.mark_state_dirty();
                log::debug!("tool: {}", tool.name());
            }
            Action::Undo => {
                self.abort_index_interactions();
                let scenes = &mut self.scenes;
                match self.undo.undo(|k| scenes.get_mut(&k)) {
                    Some(key) => {
                        log::debug!("undo applied on key {key}");
                        self.damage_key(key as u32);
                    }
                    None => log::debug!("undo: empty stack"),
                }
            }
            Action::Redo => {
                self.abort_index_interactions();
                let scenes = &mut self.scenes;
                if let Some(key) = self.undo.redo(|k| scenes.get_mut(&k)) {
                    self.damage_key(key as u32);
                }
            }
            Action::Clear => {
                self.abort_index_interactions();
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
            Action::Hide => {
                // Esc closes an open picker before it hides the overlay.
                if self.ui.any_popup_open() {
                    self.ui.close_popups();
                    self.damage_ui();
                } else {
                    self.hide();
                }
            }
            Action::ToggleColorPicker => {
                self.ui.color_picker_open = !self.ui.color_picker_open;
                self.ui.width_picker_open = false;
                self.damage_ui();
            }
            Action::ToggleWidthPicker => {
                self.ui.width_picker_open = !self.ui.width_picker_open;
                self.ui.color_picker_open = false;
                self.damage_ui();
            }
            Action::CycleBoard => {
                let next = self.board.cycle();
                self.set_board(next);
                self.damage_ui();
            }
            Action::CounterReset => {
                self.counter_next = 1;
            }
            Action::Copy => {
                let objs = self.selected_objects();
                if !objs.is_empty() {
                    self.clipboard = objs;
                }
            }
            Action::Cut => {
                let objs = self.selected_objects();
                if !objs.is_empty() {
                    self.clipboard = objs;
                    self.delete_selection();
                }
            }
            Action::Paste => {
                let objs = self.clipboard.clone();
                self.paste_objects(objs);
            }
            Action::Duplicate => {
                let objs = self.selected_objects();
                self.paste_objects(objs);
            }
            Action::DeleteSelection => self.delete_selection(),
        }
    }

    /// Returns true when the press was consumed by UI chrome.
    fn handle_ui_press(&mut self, surface_key: u32, pos: Point) -> bool {
        if Some(surface_key) != self.ui_output_key() {
            // Toolbar lives elsewhere; a click here only closes popups.
            if self.ui.any_popup_open() {
                self.ui.close_popups();
                self.damage_ui();
            }
            return false;
        }
        let Some(layout) = self.ui_layout_on(surface_key) else { return false };
        match ui::hit(&layout, pos) {
            Some(UiHit::Button(UiButton::Tool(t))) => {
                self.dispatch(Action::SelectTool(t));
                self.damage_ui();
            }
            Some(UiHit::Button(UiButton::ColorSwatch)) => self.dispatch(Action::ToggleColorPicker),
            Some(UiHit::Button(UiButton::WidthIndicator)) => self.dispatch(Action::ToggleWidthPicker),
            Some(UiHit::Button(UiButton::Board)) => self.dispatch(Action::CycleBoard),
            Some(UiHit::Color(i)) => {
                self.color_idx = i;
                self.ui.close_popups();
                self.damage_ui();
            }
            Some(UiHit::WidthTrack(w)) => {
                self.width = w;
                self.input.drag = Drag::UiSlider;
                self.damage_ui();
            }
            Some(UiHit::Chrome) => {}
            None => {
                if self.ui.any_popup_open() {
                    // Click outside an open popup closes it and is swallowed.
                    self.ui.close_popups();
                    self.damage_ui();
                    return true;
                }
                return false;
            }
        }
        true
    }

    fn adjust_width(&mut self, delta: f64) {
        self.width = (self.width + delta).clamp(WIDTH_MIN, WIDTH_MAX);
        self.damage_ui();
    }

    /// Called after every event-loop dispatch: render at most one frame per
    /// output, compositor-paced via frame callbacks.
    pub fn flush_frames(&mut self) {
        let style = self.current_style();
        // The open text draft previews (with caret) on its own output and
        // outranks any drag preview.
        let draft_preview: Option<(u32, Object)> =
            self.text_draft.as_ref().map(|d| (d.key, d.object()));
        let preview = self.input.preview(&style);
        let debug_damage = self.debug_damage;
        let ui_key = self.ui_output_key();
        let ui_layouts: HashMap<u32, ui::UiLayout> = ui_key
            .and_then(|k| self.ui_layout_on(k).map(|l| (k, l)))
            .into_iter()
            .collect();
        let now = Instant::now();
        let fade_hold = self.fade_enabled.then_some(self.fade_seconds);
        let ripple_ttl = self.config.cursor.ripple_ms.max(50) as f64 / 1000.0;
        let all_ripples: Vec<(u32, Point, f64)> = self
            .ripples
            .iter()
            .map(|(k, at, t0)| (*k, *at, (now.duration_since(*t0).as_secs_f64() / ripple_ttl).min(1.0)))
            .collect();
        let cursor_fx: Option<(u32, CursorFx)> =
            self.pointer_pos.and_then(|(k, _)| self.cursor_fx_for(k).map(|f| (k, f)));
        for (key, oo) in &mut self.overlays {
            let o = &mut oo.overlay;
            if o.configured && o.dirty && !o.frame_pending {
                let scene = self.scenes.entry(*key as u64).or_default();
                let (preview_here, caret) = match &draft_preview {
                    Some((dk, obj)) if dk == key => (Some(obj), true),
                    _ if self.active_drag == Some(*key) => (preview.as_ref(), false),
                    _ => (None, false),
                };
                let marquee = self
                    .marquee
                    .filter(|(mk, _, _)| mk == key)
                    .map(|(_, a, b)| Rect::from_corners(a, b));
                let ripples: Vec<(Point, f64)> = all_ripples
                    .iter()
                    .filter(|(rk, _, _)| rk == key)
                    .map(|(_, at, t)| (*at, *t))
                    .collect();
                let cursor = match &cursor_fx {
                    Some((ck, fx)) if ck == key => Some(CursorFx { ..*fx }),
                    _ => None,
                };
                let selection: Vec<Rect> = match &self.selection {
                    Some((sk, ids)) if sk == key => ids
                        .iter()
                        .filter_map(|id| scene.index_of(*id).map(|i| scene.objects[i].bounds))
                        .collect(),
                    _ => Vec::new(),
                };
                let ctx = FrameCtx {
                    scene,
                    preview: preview_here,
                    caret,
                    marquee,
                    selection,
                    fade_hold,
                    now,
                    cursor,
                    ripples,
                    board: self.board,
                    board_opacity: self.board_opacity,
                    ui: ui_layouts.get(key).map(|layout| {
                        (
                            layout,
                            UiPaintCtx {
                                active_tool: self.input.tool,
                                palette: &self.palette,
                                color_idx: self.color_idx,
                                width: self.width,
                                board: self.board,
                            },
                        )
                    }),
                    debug_damage,
                };
                if let Err(e) = o.draw(&self.qh, &ctx) {
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
            match self.palette.iter().position(|c| *c == rgba) {
                Some(idx) => self.color_idx = idx,
                None => {
                    anyhow::ensure!(
                        self.palette.len() < PALETTE_MAX,
                        "palette is full ({PALETTE_MAX} colors); pick one by index"
                    );
                    self.palette.push(rgba);
                    self.color_idx = self.palette.len() - 1;
                }
            }
        }
        self.mark_state_dirty();
        self.damage_ui();
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
        anyhow::ensure!(new.is_finite(), "width must be a finite number");
        self.width = new.clamp(WIDTH_MIN, WIDTH_MAX);
        self.mark_state_dirty();
        self.damage_ui();
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
            Command::Passthrough { on } => {
                let target = on.unwrap_or(self.mode != Mode::Passthrough);
                self.set_passthrough(target)
            }
            Command::Mode { fade, seconds } => (|| {
                if let Some(s) = seconds {
                    anyhow::ensure!(s.is_finite() && s > 0.0, "seconds must be a finite positive number");
                    self.fade_seconds = s;
                }
                if let Some(f) = fade {
                    self.fade_enabled = f;
                    if f {
                        self.ensure_fade_timer();
                    }
                    self.mark_state_dirty();
                    self.damage_all();
                }
                Ok(())
            })(),
            Command::Cursor { style, highlight } => (|| {
                if let Some(s) = style {
                    self.cursor_style = CursorStyle::from_name(&s)
                        .ok_or_else(|| anyhow::anyhow!("unknown cursor style {s:?}"))?;
                }
                if let Some(h) = highlight {
                    self.cursor_highlight = h;
                }
                self.damage_all();
                Ok(())
            })(),
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
            Command::CounterReset => {
                self.dispatch(Action::CounterReset);
                Ok(())
            }
            Command::Board { mode, opacity } => (|| {
                if let Some(o) = opacity {
                    anyhow::ensure!((0.1..=1.0).contains(&o), "opacity must be 0.1..=1.0");
                    self.board_opacity = o;
                    self.damage_all();
                }
                if let Some(m) = mode {
                    let kind = BoardKind::from_name(&m)
                        .ok_or_else(|| anyhow::anyhow!("unknown board mode {m:?}"))?;
                    self.set_board(kind);
                }
                Ok(())
            })(),
            Command::Status => {
                return Response::Status(StatusPayload {
                    mode: match self.mode {
                        Mode::Hidden => "hidden".into(),
                        Mode::Interactive => "interactive".into(),
                        Mode::Passthrough => "passthrough".into(),
                    },
                    tool: self.input.tool.name().into(),
                    color: self.palette[self.color_idx].to_hex(),
                    width: self.width,
                    board: self.board.name().into(),
                    objects: self.scenes.values().map(|s| s.len()).sum(),
                    outputs: self
                        .output_state
                        .outputs()
                        .filter_map(|o| self.output_state.info(&o).and_then(|i| i.name))
                        .collect(),
                });
            }
            Command::ReloadConfig => self.reload_config(),
            Command::Export { path } => self.export_scene(&path),
            Command::Quit => {
                self.teardown();
                self.loop_signal.stop();
                Ok(())
            }
        };
        match result {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: format!("{e:#}") },
        }
    }

    /// Render one output's annotations to a PNG: the focused output when it
    /// has content, else the first output that does.
    fn export_scene(&self, path: &str) -> Result<()> {
        let with_content: Vec<u32> = self
            .scenes
            .iter()
            .filter(|(_, s)| !s.is_empty())
            .map(|(k, _)| *k as u32)
            .collect();
        let key = with_content
            .iter()
            .copied()
            .find(|k| Some(*k) == self.focused_output)
            .or_else(|| with_content.first().copied())
            .or(self.focused_output)
            .or_else(|| self.overlays.keys().next().copied())
            .ok_or_else(|| anyhow::anyhow!("nothing to export (no annotations, no overlay)"))?;

        let (logical, scale) = if let Some(oo) = self.overlays.get(&key).filter(|oo| oo.overlay.width > 0) {
            ((oo.overlay.width, oo.overlay.height), oo.overlay.scale)
        } else {
            let output = self
                .output_state
                .outputs()
                .find(|o| output_key(o) == key)
                .ok_or_else(|| anyhow::anyhow!("output {key} is gone"))?;
            let info = self
                .output_state
                .info(&output)
                .ok_or_else(|| anyhow::anyhow!("no info for output {key}"))?;
            let (w, h) = info
                .logical_size
                .ok_or_else(|| anyhow::anyhow!("no logical size for output {key}"))?;
            ((w as u32, h as u32), info.scale_factor as f64)
        };

        let empty = Scene::new();
        let scene = self.scenes.get(&(key as u64)).unwrap_or(&empty);
        crate::render::export::export_png(
            std::path::Path::new(path),
            logical,
            scale,
            self.board,
            self.board_opacity,
            scene,
        )?;
        log::info!("exported {} object(s) to {path}", scene.len());
        Ok(())
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
            self.keyboard = self
                .seat_state
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.loop_handle.clone(),
                    Box::new(|state, _kbd, event| state.on_repeat_key(event)),
                )
                .ok();
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
        // An open text draft eats the keyboard before the keymap.
        if self.handle_text_key(&event) {
            return;
        }
        if let Some(action) = self.keymap.lookup(event.keysym, event.raw_code, self.input.mods) {
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
                Enter { serial } => {
                    self.focused_output = Some(surface_key);
                    if self.cursor_style.hides_system_cursor() {
                        if let Some(p) = &self.pointer {
                            p.set_cursor(serial, None, 0, 0);
                        }
                    }
                    self.pointer_pos = Some((surface_key, pos));
                    self.damage_cursor(surface_key, None, Some(pos));
                    continue;
                }
                Leave { .. } => {
                    if let Some((k, p)) = self.pointer_pos.take() {
                        self.damage_cursor(k, Some(p), None);
                    }
                    continue;
                }
                Press { button: BTN_LEFT, .. } => {
                    log::trace!("press tool={:?} pos={pos:?} width={}", self.input.tool, self.width);
                    if self.config.cursor.ripple {
                        self.ripples.push((surface_key, pos, Instant::now()));
                        self.record_damage(surface_key, &[crate::render::cursor_fx::ripple_bounds(pos)]);
                        self.ensure_fx_timer();
                    }
                    if self.handle_ui_press(surface_key, pos) {
                        continue;
                    }
                    match self.input.tool {
                        Tool::Text => {
                            self.handle_text_press(surface_key, pos);
                            continue;
                        }
                        Tool::Counter => {
                            self.handle_counter_press(surface_key, pos);
                            continue;
                        }
                        Tool::Select => {
                            self.handle_select_press(surface_key, pos);
                            continue;
                        }
                        Tool::Eraser => {
                            self.erase = Some((surface_key, pos, Vec::new()));
                            self.erase_at(surface_key, pos);
                            continue;
                        }
                        _ => {}
                    }
                    self.active_drag = Some(surface_key);
                    (surface_key, self.input.on_press(pos, &style))
                }
                Motion { .. } => {
                    let old = self.pointer_pos.replace((surface_key, pos)).map(|(_, p)| p);
                    self.damage_cursor(surface_key, old, Some(pos));
                    if self.handle_move_motion(surface_key, pos) {
                        continue;
                    }
                    if self.handle_marquee_motion(surface_key, pos) {
                        continue;
                    }
                    if let Some((ek, _, _)) = self.erase {
                        if ek == surface_key {
                            self.erase_along(surface_key, pos);
                        }
                        continue;
                    }
                    if self.input.drag == Drag::UiSlider {
                        if let Some(layout) = self.ui_layout_on(surface_key) {
                            if let Some((_, track)) = layout.width_popup {
                                self.width = ui::width_from_track_x(track, pos.x);
                                self.damage_ui();
                            }
                        }
                        continue;
                    }
                    let Some(key) = self.active_drag else { continue };
                    if key != surface_key {
                        continue; // drag crossed outputs; ignore foreign motion
                    }
                    (key, self.input.on_motion(pos, &style))
                }
                Release { button: BTN_LEFT, .. } => {
                    // One full repaint per drag end: partial damage keeps
                    // in-drag rendering cheap, and this bounds the lifetime
                    // of any missed preview pixels to the drag itself.
                    self.damage_key(surface_key);
                    if self.handle_move_release() {
                        continue;
                    }
                    if self.handle_marquee_release() {
                        continue;
                    }
                    if self.handle_erase_release() {
                        continue;
                    }
                    if self.input.drag == Drag::UiSlider {
                        self.input.drag = Drag::Idle;
                        continue;
                    }
                    let Some(key) = self.active_drag.take() else { continue };
                    (key, self.input.on_release(pos, &style))
                }
                Axis { vertical, .. } => {
                    // Ctrl+scroll adjusts the stroke width anywhere.
                    if self.input.mods.ctrl && vertical.absolute.abs() > 0.0 {
                        let step = if vertical.absolute < 0.0 { 0.5 } else { -0.5 };
                        self.adjust_width(step);
                    }
                    continue;
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
