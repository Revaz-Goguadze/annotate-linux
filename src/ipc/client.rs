use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result, bail};

use super::protocol::{Command, Response};
use super::socket_path;

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
