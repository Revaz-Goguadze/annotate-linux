use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use annotate_linux::cli::{Cli, Cmd};
use annotate_linux::ipc;

/// Unix seconds, for unique generated filenames.
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Absolute output path for `export` (daemon has a different cwd).
fn export_target(path: Option<&str>) -> Result<PathBuf> {
    let p = match path {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(format!("annotate-{}.png", timestamp())),
    };
    Ok(if p.is_absolute() { p } else { std::env::current_dir()?.join(p) })
}

fn request_export(target: &std::path::Path) -> Result<()> {
    let cmd = ipc::protocol::Command::Export { path: target.display().to_string() };
    match ipc::client::send(&cmd)? {
        ipc::protocol::Response::Ok => Ok(()),
        ipc::protocol::Response::Error { message } => anyhow::bail!("{message}"),
        other => anyhow::bail!("unexpected response: {other:?}"),
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    match &cli.command {
        Cmd::Daemon => annotate_linux::wayland::run_daemon(),
        Cmd::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(*shell, &mut Cli::command(), "annotate-linux", &mut std::io::stdout());
            Ok(())
        }
        Cmd::Export { path } => {
            let target = export_target(path.as_deref())?;
            request_export(&target)?;
            println!("{}", target.display());
            Ok(())
        }
        Cmd::Copy => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            // Owner-only runtime dir, not /tmp: the intermediate PNG holds
            // screen content, and a predictable /tmp path is plantable.
            let tmp = annotate_linux::util::xdg::runtime_dir()?
                .join(format!("copy-{}-{ts}.png", std::process::id()));
            request_export(&tmp)?;
            let file = std::fs::File::open(&tmp)?;
            let status = std::process::Command::new("wl-copy")
                .args(["--type", "image/png"])
                .stdin(file)
                .status()
                .map_err(|e| anyhow::anyhow!("running wl-copy (is wl-clipboard installed?): {e}"))?;
            let _ = std::fs::remove_file(&tmp);
            anyhow::ensure!(status.success(), "wl-copy failed");
            println!("annotations copied to clipboard as image/png");
            Ok(())
        }
        cmd => {
            let command = cmd.to_ipc().expect("non-daemon command maps to IPC");
            // Status and Quit report on a missing daemon instead of starting one.
            let result = match &command {
                ipc::protocol::Command::Status | ipc::protocol::Command::Quit => {
                    ipc::client::send(&command)
                }
                _ => ipc::client::send_or_autostart(&command),
            };
            match result {
                Ok(ipc::protocol::Response::Ok) => Ok(()),
                Ok(ipc::protocol::Response::Status(s)) => {
                    println!("{}", serde_json::to_string_pretty(&s)?);
                    Ok(())
                }
                Ok(ipc::protocol::Response::Error { message }) => {
                    eprintln!("error: {message}");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("error: {e:#}");
                    std::process::exit(1);
                }
            }
        }
    }
}
