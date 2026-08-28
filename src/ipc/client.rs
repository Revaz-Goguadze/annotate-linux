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

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    use super::*;
    use crate::ipc::protocol::StatusPayload;

    /// A one-shot server on an abstract socket: hands back the line it read and
    /// replies with `reply` (nothing at all when `None`).
    fn fake_daemon(name: &str, reply: Option<String>) -> (UnixStream, mpsc::Receiver<String>) {
        use std::os::linux::net::SocketAddrExt;
        let addr =
            std::os::unix::net::SocketAddr::from_abstract_name(format!("{name}-{}", std::process::id()))
                .expect("abstract address");
        let listener = UnixListener::bind_addr(&addr).expect("bind");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read command");
            tx.send(line).expect("report command");
            if let Some(reply) = reply {
                let mut stream = reader.into_inner();
                stream.write_all(reply.as_bytes()).expect("write reply");
            }
        });
        (UnixStream::connect_addr(&addr).expect("connect"), rx)
    }

    #[test]
    fn sends_one_json_line_and_parses_the_reply() {
        let payload = StatusPayload {
            mode: "persist".into(),
            tool: "pen".into(),
            color: "#fff".into(),
            width: 4.0,
            board: "none".into(),
            objects: 2,
            outputs: vec!["DP-1".into()],
        };
        let reply = serde_json::to_string(&Response::Status(payload.clone())).unwrap() + "\n";
        let (stream, rx) = fake_daemon("annotate-ipc-ok", Some(reply));

        let resp = send_on(stream, &Command::Tool { name: "arrow".into() }).unwrap();
        assert_eq!(resp, Response::Status(payload));

        let sent = rx.recv().unwrap();
        assert!(sent.ends_with('\n'), "commands are newline delimited: {sent:?}");
        assert_eq!(
            serde_json::from_str::<Command>(sent.trim_end()).unwrap(),
            Command::Tool { name: "arrow".into() }
        );
    }

    #[test]
    fn a_silent_daemon_is_an_error() {
        let (stream, _rx) = fake_daemon("annotate-ipc-silent", None);
        let err = send_on(stream, &Command::Status).unwrap_err().to_string();
        assert!(err.contains("without replying"), "{err}");
    }

    #[test]
    fn an_unparseable_reply_is_an_error() {
        let (stream, _rx) = fake_daemon("annotate-ipc-garbage", Some("not json\n".into()));
        let err = send_on(stream, &Command::Status).unwrap_err().to_string();
        assert!(err.contains("unparseable response"), "{err}");
    }
}
