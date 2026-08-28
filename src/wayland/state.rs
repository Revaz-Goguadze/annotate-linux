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
use super::surface::Overlay;
use crate::config::keys::Keymap;
use crate::config::state::RuntimeState;
use crate::config::Config;
use crate::input::{Action, Drag, DragUpdate, InputState, Tool, text_edit};
use crate::model::fade;
use crate::render::board::BoardKind;
use crate::render::cursor_fx::{CursorFx, CursorStyle};
use crate::render::frame::FrameCtx;
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

/// Parse configured palette colors, warning on (and skipping) invalid ones.
/// Caps at PALETTE_MAX so a huge config cannot grow the toolbar forever.
fn parse_palette(specs: &[String]) -> Vec<Rgba> {
    let mut out = Vec::new();
    for s in specs {
        match Rgba::parse(s) {
            Ok(c) => {
                if out.len() >= PALETTE_MAX {
                    log::warn!("config: palette truncated to {PALETTE_MAX} colors");
                    break;
                }
                out.push(c);
            }
            Err(e) => log::warn!("config: invalid palette color {s:?} skipped: {e}"),
        }
    }
    out
}

/// Index of `c` in `palette`, appending when new and under PALETTE_MAX.
fn try_adopt_color(palette: &mut Vec<Rgba>, c: Rgba) -> Option<usize> {
    if let Some(i) = palette.iter().position(|p| *p == c) {
        return Some(i);
    }
    if palette.len() >= PALETTE_MAX {
        return None;
    }
    palette.push(c);
    Some(palette.len() - 1)
}

fn cursor_style_or_default(name: &str) -> CursorStyle {
    CursorStyle::from_name(name).unwrap_or_else(|| {
        log::warn!("config: unknown cursor style {name:?}, using default");
        CursorStyle::default()
    })
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
