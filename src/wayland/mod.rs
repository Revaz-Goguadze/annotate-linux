//! Daemon entry: Wayland connection + calloop event loop + IPC socket.

pub mod buffer;
pub mod outputs;
pub mod scaling;
pub mod state;
pub mod surface;

use anyhow::{Context, Result};
use calloop::EventLoop;
use calloop::signals::{Signal, Signals};
use calloop_wayland_source::WaylandSource;
use wayland_client::{Connection, globals::registry_queue_init};

use crate::config::Config;
use crate::ipc;
use state::AppState;

pub fn run_daemon() -> Result<()> {
    buffer::assert_pixel_format_compatible();
    let config = Config::load().context("loading config.toml")?;

    let conn = Connection::connect_to_env().context("connecting to the Wayland display")?;
    let (globals, event_queue) = registry_queue_init::<AppState>(&conn)?;
    let qh = event_queue.handle();

    let mut event_loop: EventLoop<'static, AppState> = EventLoop::try_new()?;
    let loop_handle = event_loop.handle();

    WaylandSource::new(conn, event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| anyhow::anyhow!("inserting wayland source: {e}"))?;

    let signals = Signals::new(&[Signal::SIGINT, Signal::SIGTERM])?;
    loop_handle
        .insert_source(signals, |_, _, app: &mut AppState| {
            log::info!("signal received, shutting down");
            app.teardown();
            app.loop_signal.stop();
        })
        .map_err(|e| anyhow::anyhow!("inserting signal source: {e}"))?;

    let _socket_guard = ipc::server::setup(&loop_handle).context("setting up the IPC socket")?;

    let mut app = AppState::new(&globals, &qh, event_loop.get_signal(), loop_handle.clone(), config)?;
    log::info!("daemon running (socket: {})", ipc::socket_path()?.display());

    event_loop
        .run(None, &mut app, |app| app.flush_frames())
        .map_err(|e| anyhow::anyhow!("event loop error: {e}"))?;

    // Surfaces must be gone before the connection drops so the compositor
    // releases any keyboard grab deterministically.
    app.teardown();
    Ok(())
}
