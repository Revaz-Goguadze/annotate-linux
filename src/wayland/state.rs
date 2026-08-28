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
