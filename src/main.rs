use anyhow::Result;
use clap::Parser;

use annotate_linux::cli::{Cli, Cmd};
use annotate_linux::ipc;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    match &cli.command {
        Cmd::Daemon => {
            anyhow::bail!("daemon not implemented yet (milestone M1)");
        }
        cmd => {
            let command = cmd.to_ipc().expect("non-daemon command maps to IPC");
            match ipc::client::send(&command) {
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
