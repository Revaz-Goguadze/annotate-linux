use serde::{Deserialize, Serialize};

/// Wire commands. One NDJSON line per command, one `Response` line back.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Command {
    Toggle,
    Show,
    Hide,
    Passthrough { on: Option<bool> },
    Clear,
    Undo,
    Redo,
    Tool { name: String },
    Color { value: String },
    Width { value: String },
    Board { mode: Option<String>, opacity: Option<f64> },
    CounterReset,
    Mode { fade: Option<bool>, seconds: Option<f64> },
    Cursor { style: Option<String>, highlight: Option<bool> },
    Status,
    ReloadConfig,
    Quit,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Response {
    Ok,
    Status(StatusPayload),
    Error { message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct StatusPayload {
    pub mode: String,
    pub tool: String,
    pub color: String,
    pub width: f64,
    pub board: String,
    pub objects: usize,
    pub outputs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrip() {
        let cmds = vec![
            Command::Toggle,
            Command::Passthrough { on: Some(true) },
            Command::Tool { name: "pen".into() },
            Command::Board { mode: Some("white".into()), opacity: Some(0.85) },
            Command::Mode { fade: Some(true), seconds: Some(3.0) },
        ];
        for cmd in cmds {
            let line = serde_json::to_string(&cmd).unwrap();
            let back: Command = serde_json::from_str(&line).unwrap();
            assert_eq!(back, cmd);
        }
    }

    #[test]
    fn command_wire_format_is_kebab_tagged() {
        let line = serde_json::to_string(&Command::CounterReset).unwrap();
        assert_eq!(line, r#"{"cmd":"counter-reset"}"#);
        let line = serde_json::to_string(&Command::Tool { name: "pen".into() }).unwrap();
        assert_eq!(line, r#"{"cmd":"tool","name":"pen"}"#);
    }

    #[test]
    fn response_roundtrip() {
        let resp = Response::Status(StatusPayload {
            mode: "interactive".into(),
            tool: "pen".into(),
            color: "#ff0000".into(),
            width: 4.0,
            board: "none".into(),
            objects: 3,
            outputs: vec!["eDP-1".into()],
        });
        let line = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&line).unwrap();
        assert_eq!(back, resp);
    }
}
