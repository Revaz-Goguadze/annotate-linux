pub mod client;
pub mod protocol;
pub mod server;

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Socket path keyed by Wayland display so nested/multi-session setups
/// never cross-talk: `$XDG_RUNTIME_DIR/annotate-linux/$WAYLAND_DISPLAY.sock`.
pub fn socket_path() -> Result<PathBuf> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let display = std::env::var("WAYLAND_DISPLAY").context("WAYLAND_DISPLAY is not set (not a Wayland session?)")?;
    Ok(PathBuf::from(runtime).join("annotate-linux").join(format!("{display}.sock")))
}
