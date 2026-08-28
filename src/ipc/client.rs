use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::protocol::{Command, Response};
use super::socket_path;

const AUTOSTART_WAIT: Duration = Duration::from_millis(1500);

/// Send one command to the running daemon, return its response.
/// Fails with a clear message when no daemon is listening.
pub fn send(cmd: &Command) -> Result<Response> {
    let path = socket_path()?;
    let stream = UnixStream::connect(&path).with_context(|| {
        format!(
            "no daemon running (connect to {} failed) — start it with `annotate-linux daemon`",
            path.display()
        )
    })?;
    send_on(stream, cmd)
}

/// Like [`send`], but spawns a detached daemon on connect failure and
/// retries once after it binds the socket. Only a failed connect means
/// "no daemon" — errors from a live daemon (bad write, unparseable
/// response) propagate instead of spawning a duplicate.
pub fn send_or_autostart(cmd: &Command) -> Result<Response> {
    let path = socket_path()?;
    match UnixStream::connect(&path) {
        Ok(stream) => return send_on(stream, cmd),
        Err(e) => log::debug!("connect to {} failed ({e}), autostarting the daemon", path.display()),
    }
    spawn_daemon().context("autostarting the daemon")?;
    let deadline = Instant::now() + AUTOSTART_WAIT;
    while Instant::now() < deadline {
        if let Ok(stream) = UnixStream::connect(&path) {
            return send_on(stream, cmd);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("daemon did not come up within {AUTOSTART_WAIT:?}");
}

/// Start `annotate-linux daemon` in its own session so it survives the
/// caller (and the compositor bind's exec) exiting.
fn spawn_daemon() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().context("resolving own binary path")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            rustix::process::setsid().map_err(std::io::Error::from)?;
            Ok(())
        });
    }
    cmd.spawn().context("spawning the daemon")?;
    Ok(())
}

fn send_on(mut stream: UnixStream, cmd: &Command) -> Result<Response> {
    let mut line = serde_json::to_string(cmd)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut reply = String::new();
    reader.read_line(&mut reply).context("daemon closed the connection without replying")?;
    if reply.is_empty() {
        bail!("daemon closed the connection without replying");
    }
    let resp: Response = serde_json::from_str(reply.trim_end()).context("daemon sent an unparseable response")?;
    Ok(resp)
}
