use anyhow::Result;
use clap::Parser;

use annotate_linux::cli::{Cli, Cmd};
use annotate_linux::ipc;

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
