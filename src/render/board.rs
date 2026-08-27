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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::test_surface::Canvas;

    #[test]
    fn cycle_visits_every_kind_and_returns() {
        let mut k = BoardKind::default();
        assert_eq!(k, BoardKind::None);
        k = k.cycle();
        assert_eq!(k, BoardKind::White);
        k = k.cycle();
        assert_eq!(k, BoardKind::Black);
        assert_eq!(k.cycle(), BoardKind::None);
    }

    #[test]
    fn names_round_trip_and_aliases_parse() {
        for k in [BoardKind::None, BoardKind::White, BoardKind::Black] {
            assert_eq!(BoardKind::from_name(k.name()), Some(k));
        }
        assert_eq!(BoardKind::from_name("off"), Some(BoardKind::None));
        assert_eq!(BoardKind::from_name("whiteboard"), Some(BoardKind::White));
        assert_eq!(BoardKind::from_name("blackboard"), Some(BoardKind::Black));
        assert_eq!(BoardKind::from_name("grey"), None);
    }

    #[test]
    fn none_leaves_the_frame_transparent() {
        let mut c = Canvas::new(8, 8);
        c.paint(|cr| paint(cr, BoardKind::None, 1.0));
        assert_eq!(c.ink(), 0);
    }

    #[test]
    fn white_and_black_fill_the_whole_frame() {
        for kind in [BoardKind::White, BoardKind::Black] {
            let mut c = Canvas::new(8, 8);
            c.paint(|cr| paint(cr, kind, 1.0));
            assert_eq!(c.ink(), 64, "{kind:?} must cover every pixel");
            assert_eq!(c.alpha_at(4, 4), 255);
        }
    }

    #[test]
    fn opacity_clamps_to_a_visible_floor() {
        let mut c = Canvas::new(4, 4);
        c.paint(|cr| paint(cr, BoardKind::White, 0.0));
        let a = c.alpha_at(2, 2);
        assert!((24..=27).contains(&a), "0.0 opacity clamps to 0.1, got alpha {a}");

        let mut c = Canvas::new(4, 4);
        c.paint(|cr| paint(cr, BoardKind::White, 5.0));
        assert_eq!(c.alpha_at(2, 2), 255, "over-1.0 opacity clamps to opaque");
    }
}
