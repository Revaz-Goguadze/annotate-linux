use clap::{Parser, Subcommand};

use crate::ipc::protocol::Command;

/// Reject NaN/inf at the CLI boundary: `serde_json` encodes a non-finite
/// f64 as `null`, which reaches the daemon as an absent optional field and
/// is silently dropped instead of rejected.
fn finite_f64(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|e| format!("{e}"))?;
    if v.is_finite() { Ok(v) } else { Err(format!("{s} is not a finite number")) }
}

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
    Width {
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// Set board background: none|white|black
    Board {
        mode: Option<String>,
        #[arg(long, value_parser = finite_f64)]
        opacity: Option<f64>,
    },
    /// Reset the counter tool sequence to 1
    CounterReset,
    /// Set annotation lifetime mode: fade|persist
    Mode {
        mode: String,
        #[arg(long, value_parser = finite_f64)]
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
    /// Save the annotations as a PNG (transparent background unless a
    /// board is active). Default: ./annotate-<timestamp>.png
    Export { path: Option<String> },
    /// Copy the annotations to the clipboard as a PNG (needs wl-copy)
    Copy,
    /// Stop the daemon
    Quit,
    /// Print shell completions to stdout
    Completions { shell: clap_complete::Shell },
}

impl Cmd {
    /// Map a client subcommand to the wire command. `Daemon` has no mapping.
    pub fn to_ipc(&self) -> Option<Command> {
        Some(match self {
            Cmd::Daemon | Cmd::Completions { .. } | Cmd::Export { .. } | Cmd::Copy => return None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Cmd {
        Cli::try_parse_from(std::iter::once("annotate-linux").chain(args.iter().copied()))
            .unwrap()
            .command
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn simple_subcommands_map_to_their_wire_command() {
        let cases = [
            (vec!["toggle"], Command::Toggle),
            (vec!["show"], Command::Show),
            (vec!["hide"], Command::Hide),
            (vec!["clear"], Command::Clear),
            (vec!["undo"], Command::Undo),
            (vec!["redo"], Command::Redo),
            (vec!["counter-reset"], Command::CounterReset),
            (vec!["status"], Command::Status),
            (vec!["reload-config"], Command::ReloadConfig),
            (vec!["quit"], Command::Quit),
        ];
        for (args, want) in cases {
            assert_eq!(parse(&args).to_ipc(), Some(want), "{args:?}");
        }
    }

    #[test]
    fn locally_handled_subcommands_have_no_wire_command() {
        for args in [
            vec!["daemon"],
            vec!["copy"],
            vec!["export"],
            vec!["export", "/tmp/a.png"],
            vec!["completions", "bash"],
        ] {
            assert_eq!(parse(&args).to_ipc(), None, "{args:?}");
        }
    }

    #[test]
    fn passthrough_state_is_optional_and_anything_but_on_means_off() {
        assert_eq!(parse(&["passthrough"]).to_ipc(), Some(Command::Passthrough { on: None }));
        assert_eq!(
            parse(&["passthrough", "on"]).to_ipc(),
            Some(Command::Passthrough { on: Some(true) })
        );
        assert_eq!(
            parse(&["passthrough", "off"]).to_ipc(),
            Some(Command::Passthrough { on: Some(false) })
        );
    }

    #[test]
    fn value_subcommands_carry_their_arguments() {
        assert_eq!(
            parse(&["tool", "arrow"]).to_ipc(),
            Some(Command::Tool { name: "arrow".into() })
        );
        assert_eq!(
            parse(&["color", "#ff00ff"]).to_ipc(),
            Some(Command::Color { value: "#ff00ff".into() })
        );
        assert_eq!(parse(&["width", "+2"]).to_ipc(), Some(Command::Width { value: "+2".into() }));
        assert_eq!(parse(&["width", "-3"]).to_ipc(), Some(Command::Width { value: "-3".into() }));
        assert_eq!(
            parse(&["board", "white", "--opacity", "0.5"]).to_ipc(),
            Some(Command::Board { mode: Some("white".into()), opacity: Some(0.5) })
        );
        assert_eq!(
            parse(&["board"]).to_ipc(),
            Some(Command::Board { mode: None, opacity: None })
        );
        assert_eq!(
            parse(&["cursor", "crosshair", "--highlight", "true"]).to_ipc(),
            Some(Command::Cursor { style: Some("crosshair".into()), highlight: Some(true) })
        );
    }

    #[test]
    fn mode_maps_fade_only_for_the_fade_keyword() {
        assert_eq!(
            parse(&["mode", "fade", "--seconds", "2.5"]).to_ipc(),
            Some(Command::Mode { fade: Some(true), seconds: Some(2.5) })
        );
        assert_eq!(
            parse(&["mode", "persist"]).to_ipc(),
            Some(Command::Mode { fade: Some(false), seconds: None })
        );
    }

    #[test]
    fn missing_or_unknown_arguments_are_rejected() {
        for args in [
            vec!["tool"],
            vec!["color"],
            vec!["nonsense"],
            vec!["completions", "fish-shell"],
            vec!["board", "white", "--opacity", "loud"],
            // Non-finite floats would serialize to JSON null and be dropped.
            vec!["board", "white", "--opacity", "inf"],
            vec!["mode", "fade", "--seconds", "inf"],
            vec!["mode", "fade", "--seconds", "NaN"],
            vec!["mode", "fade", "--seconds", "-inf"],
        ] {
            assert!(
                Cli::try_parse_from(std::iter::once("annotate-linux").chain(args.iter().copied()))
                    .is_err(),
                "{args:?} should not parse"
            );
        }
    }
}
