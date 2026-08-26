//! Whiteboard / blackboard backdrop.

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum BoardKind {
    #[default]
    None,
    White,
    Black,
}

impl BoardKind {
    pub fn cycle(self) -> Self {
        match self {
            BoardKind::None => BoardKind::White,
            BoardKind::White => BoardKind::Black,
            BoardKind::Black => BoardKind::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BoardKind::None => "none",
            BoardKind::White => "white",
            BoardKind::Black => "black",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "none" | "off" => BoardKind::None,
            "white" | "whiteboard" => BoardKind::White,
            "black" | "blackboard" => BoardKind::Black,
            _ => return None,
        })
    }
}

/// Fill the (already clipped) frame with the board color. Runs after the
/// transparent Source-clear, before objects.
pub fn paint(cr: &cairo::Context, kind: BoardKind, opacity: f64) {
    let v = match kind {
        BoardKind::None => return,
        BoardKind::White => 1.0,
        BoardKind::Black => 0.08,
    };
    cr.set_source_rgba(v, v, v, opacity.clamp(0.1, 1.0));
    cr.paint().expect("board fill");
}
