//! Fractional scale + viewporter support, bound directly from
//! wayland-protocols (SCTK 0.20 only handles integer scale factors).
//!
//! Per surface: a wp_fractional_scale_v1 reports the preferred scale in
//! 1/120ths; we render the buffer at `round(logical × scale)` px and attach
//! a wp_viewport with the logical size as destination. Compositors missing
//! either global fall back to integer `set_buffer_scale`.

use wayland_client::{Connection, Dispatch, QueueHandle, globals::GlobalList};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use super::state::AppState;

pub struct ScalingState {
    pub viewporter: Option<WpViewporter>,
    pub fractional_manager: Option<WpFractionalScaleManagerV1>,
}

impl ScalingState {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<AppState>) -> Self {
        let viewporter = globals.bind::<WpViewporter, _, _>(qh, 1..=1, ()).ok();
        let fractional_manager = globals.bind::<WpFractionalScaleManagerV1, _, _>(qh, 1..=1, ()).ok();
        if viewporter.is_none() || fractional_manager.is_none() {
            log::warn!("fractional scale unavailable, falling back to integer buffer scale");
        }
        Self { viewporter, fractional_manager }
    }

    /// True when both globals are present (fractional path usable).
    pub fn fractional_capable(&self) -> bool {
        self.viewporter.is_some() && self.fractional_manager.is_some()
    }
}

/// User data: the output key this surface's scale events belong to.
pub struct FractionalScaleData(pub u32);

impl Dispatch<WpFractionalScaleV1, FractionalScaleData> for AppState {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        data: &FractionalScaleData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.on_preferred_scale(data.0, scale as f64 / 120.0);
        }
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewporter, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
