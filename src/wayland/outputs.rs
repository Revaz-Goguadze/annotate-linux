//! Per-output overlay registry: each wl_output gets its own layer surface
//! and damage state. Scenes live in AppState keyed by the same output key
//! so annotations survive hide/show; no global coordinate space means
//! mixed scales never need coordinate conversion.

use wayland_client::Proxy;
use wayland_client::protocol::wl_output::WlOutput;

use super::surface::Overlay;

pub fn output_key(output: &WlOutput) -> u32 {
    output.id().protocol_id()
}

pub struct OverlayOutput {
    pub output: WlOutput,
    pub name: Option<String>,
    pub overlay: Overlay,
}
