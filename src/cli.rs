use clap::{Parser, Subcommand};

use crate::ipc::protocol::Command;

#[derive(Parser, Debug)]
#[command(name = "annotate-linux", version, about = "Screen annotation overlay for wlr-layer-shell compositors")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run the long-lived overlay daemon (owns the Wayland connection)
    Daemon,
    /// Toggle the interactive overlay on/off
    Toggle,
    /// Show the interactive overlay
    Show,
    /// Hide the overlay
    Hide,
    /// Toggle or set click-through (always-on) mode: on|off, toggles when omitted
    Passthrough { state: Option<String> },
    /// Remove all annotations (undoable)
    Clear,
    Undo,
    Redo,
    /// Select a tool: pen|highlighter|line|arrow|rect|ellipse|counter|text|select|eraser
    Tool { name: String },
    /// Set stroke color: #rrggbb, next, prev, or palette index
    Color { value: String },
    /// Set stroke width: absolute (e.g. 8), or relative (+1, -1)
    Width { value: String },
    /// Set board background: none|white|black
    Board {
        mode: Option<String>,
        #[arg(long)]
        opacity: Option<f64>,
    },
    /// Reset the counter tool sequence to 1
    CounterReset,
    /// Set annotation lifetime mode: fade|persist
    Mode {
        mode: String,
        #[arg(long)]
        seconds: Option<f64>,
    },
    /// Set cursor style: none|outline|circle|crosshair
    Cursor {
        style: Option<String>,
        #[arg(long)]
        highlight: Option<bool>,
    },
    /// Print daemon status as JSON
    Status,
    /// Re-read config.toml in the running daemon
    ReloadConfig,
    /// Stop the daemon
    Quit,
    /// Print shell completions to stdout
    Completions { shell: clap_complete::Shell },
}

impl Cmd {
    /// Map a client subcommand to the wire command. `Daemon` has no mapping.
    pub fn to_ipc(&self) -> Option<Command> {
        Some(match self {
            Cmd::Daemon | Cmd::Completions { .. } => return None,
            Cmd::Toggle => Command::Toggle,
            Cmd::Show => Command::Show,
            Cmd::Hide => Command::Hide,
            Cmd::Passthrough { state } => Command::Passthrough {
                on: state.as_deref().map(|s| s == "on"),
            },
            Cmd::Clear => Command::Clear,
            Cmd::Undo => Command::Undo,
            Cmd::Redo => Command::Redo,
            Cmd::Tool { name } => Command::Tool { name: name.clone() },
            Cmd::Color { value } => Command::Color { value: value.clone() },
            Cmd::Width { value } => Command::Width { value: value.clone() },
            Cmd::Board { mode, opacity } => Command::Board {
                mode: mode.clone(),
                opacity: *opacity,
            },
            Cmd::CounterReset => Command::CounterReset,
            Cmd::Mode { mode, seconds } => Command::Mode {
                fade: Some(mode == "fade"),
                seconds: *seconds,
            },
            Cmd::Cursor { style, highlight } => Command::Cursor {
                style: style.clone(),
                highlight: *highlight,
            },
            Cmd::Status => Command::Status,
            Cmd::ReloadConfig => Command::ReloadConfig,
            Cmd::Quit => Command::Quit,
        })
    }
}
