//! IPC server: a nonblocking UnixListener as a calloop source. Clients are
//! one-shot (one command line in, one response line out), handled inline
//! with a short read timeout so a stalled client cannot wedge the loop.

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use calloop::LoopHandle;
use calloop::generic::Generic;
use calloop::{Interest, Mode, PostAction};

use super::protocol::{Command, Response};
use super::socket_path;
use crate::wayland::state::AppState;

const READ_TIMEOUT: Duration = Duration::from_millis(200);

/// Removes the socket file when the daemon exits.
pub struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn setup(handle: &LoopHandle<AppState>) -> Result<SocketGuard> {
    let path = socket_path()?;
    let dir = path.parent().expect("socket path has a parent");
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;

    // Stale-socket reclaim: a live daemon answers the connect; a dead one
    // leaves a refused/orphaned socket file we can safely unlink.
    match UnixStream::connect(&path) {
        Ok(_) => bail!("another daemon is already running on {}", path.display()),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(_) => {
            fs::remove_file(&path).with_context(|| format!("removing stale socket {}", path.display()))?;
        }
    }

    let listener = UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    listener.set_nonblocking(true)?;

    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, app: &mut AppState| {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => handle_client(stream, app),
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => {
                            log::error!("ipc accept failed: {e}");
                            break;
                        }
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("inserting ipc source: {e}"))?;

    Ok(SocketGuard { path })
}

fn handle_client(stream: UnixStream, app: &mut AppState) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let response = match reader.read_line(&mut line) {
        Ok(0) => return,
        Ok(_) => match serde_json::from_str::<Command>(line.trim_end()) {
            Ok(cmd) => {
                log::debug!("ipc: {cmd:?}");
                app.handle_command(cmd)
            }
            Err(e) => Response::Error { message: format!("bad command: {e}") },
        },
        Err(e) => {
            log::warn!("ipc read failed: {e}");
            return;
        }
    };

    let mut out = match serde_json::to_string(&response) {
        Ok(s) => s,
        Err(e) => {
            log::error!("ipc response serialize failed: {e}");
            return;
        }
    };
    out.push('\n');
    let mut stream = reader.into_inner();
    if let Err(e) = stream.write_all(out.as_bytes()) {
        log::warn!("ipc write failed: {e}");
    }
}
