//! In-overlay UI: bottom-center toolbar, color picker and width slider
//! popups. Layout and hit-testing are pure (unit-testable); painting is
//! plain cairo. No GTK.

pub mod paint;

use crate::input::Tool;
use crate::model::geom::{Point, Rect};

pub const BUTTON: f64 = 40.0;
pub const PAD: f64 = 8.0;
pub const SWATCH: f64 = 28.0;
const TRACK_W: f64 = 240.0;

pub const TOOLS: [Tool; 6] = [Tool::Pen, Tool::Highlighter, Tool::Line, Tool::Arrow, Tool::Rect, Tool::Ellipse];

#[derive(Default, Debug)]
pub struct UiState {
    pub color_picker_open: bool,
    pub width_picker_open: bool,
}

impl UiState {
    pub fn any_popup_open(&self) -> bool {
        self.color_picker_open || self.width_picker_open
    }

    pub fn close_popups(&mut self) {
        self.color_picker_open = false;
        self.width_picker_open = false;
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum UiButton {
    Tool(Tool),
    ColorSwatch,
    WidthIndicator,
    Board,
}

#[derive(Debug)]
pub struct UiLayout {
    pub toolbar: Rect,
    pub buttons: Vec<(UiButton, Rect)>,
    /// (panel, swatch rects) when the color picker is open
    pub color_popup: Option<(Rect, Vec<Rect>)>,
    /// (panel, slider track) when the width picker is open
    pub width_popup: Option<(Rect, Rect)>,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum UiHit {
    Button(UiButton),
    Color(usize),
    WidthTrack(f64),
    /// Inside UI chrome but not on a control: swallow the click.
    Chrome,
}

pub fn layout(surface: Rect, palette_len: usize, ui: &UiState) -> UiLayout {
    let n_buttons = TOOLS.len() + 3; // + color, width, board
    let bar_w = n_buttons as f64 * BUTTON + (n_buttons + 1) as f64 * PAD;
    let bar_h = BUTTON + 2.0 * PAD;
    let toolbar = Rect::new(
        (surface.w - bar_w) / 2.0,
        surface.h - bar_h - PAD * 2.0,
        bar_w,
        bar_h,
    );

    let mut buttons = Vec::with_capacity(n_buttons);
    let mut x = toolbar.x + PAD;
    for t in TOOLS {
        buttons.push((UiButton::Tool(t), Rect::new(x, toolbar.y + PAD, BUTTON, BUTTON)));
        x += BUTTON + PAD;
    }
    for b in [UiButton::ColorSwatch, UiButton::WidthIndicator, UiButton::Board] {
        buttons.push((b, Rect::new(x, toolbar.y + PAD, BUTTON, BUTTON)));
        x += BUTTON + PAD;
    }

    let color_popup = ui.color_picker_open.then(|| {
        let cols = 8usize.min(palette_len.max(1));
        let rows = palette_len.div_ceil(cols);
        let pw = cols as f64 * SWATCH + (cols + 1) as f64 * PAD;
        let ph = rows as f64 * SWATCH + (rows + 1) as f64 * PAD;
        let panel = Rect::new(
            (surface.w - pw) / 2.0,
            toolbar.y - ph - PAD,
            pw,
            ph,
        );
        let swatches = (0..palette_len)
            .map(|i| {
                let (col, row) = (i % cols, i / cols);
                Rect::new(
                    panel.x + PAD + col as f64 * (SWATCH + PAD),
                    panel.y + PAD + row as f64 * (SWATCH + PAD),
                    SWATCH,
                    SWATCH,
                )
            })
            .collect();
        (panel, swatches)
    });

    let width_popup = ui.width_picker_open.then(|| {
        let pw = TRACK_W + 2.0 * PAD * 2.0;
        let ph = BUTTON + 2.0 * PAD;
        let panel = Rect::new((surface.w - pw) / 2.0, toolbar.y - ph - PAD, pw, ph);
        let track = Rect::new(panel.x + PAD * 2.0, panel.y + PAD, TRACK_W, BUTTON);
        (panel, track)
    });

    UiLayout { toolbar, buttons, color_popup, width_popup }
}

/// Union of every visible UI rect (for damage).
pub fn ui_region(l: &UiLayout) -> Rect {
    let mut r = l.toolbar;
    if let Some((panel, _)) = &l.color_popup {
        r = r.union(*panel);
    }
    if let Some((panel, _)) = &l.width_popup {
        r = r.union(*panel);
    }
    r.inflate(2.0)
}

pub fn hit(l: &UiLayout, p: Point) -> Option<UiHit> {
    if let Some((panel, swatches)) = &l.color_popup {
        if let Some(i) = swatches.iter().position(|r| r.contains(p)) {
            return Some(UiHit::Color(i));
        }
        if panel.contains(p) {
            return Some(UiHit::Chrome);
        }
    }
    if let Some((panel, track)) = &l.width_popup {
        if track.contains(p) {
            return Some(UiHit::WidthTrack(width_from_track_x(*track, p.x)));
        }
        if panel.contains(p) {
            return Some(UiHit::Chrome);
        }
    }
    if l.toolbar.contains(p) {
        for (b, r) in &l.buttons {
            if r.contains(p) {
                return Some(UiHit::Button(*b));
            }
        }
        return Some(UiHit::Chrome);
    }
    None
}

pub fn width_from_track_x(track: Rect, x: f64) -> f64 {
    let t = ((x - track.x) / track.w).clamp(0.0, 1.0);
    ((0.5 + t * 19.5) * 100.0).round() / 100.0
}

pub fn track_x_from_width(track: Rect, width: f64) -> f64 {
    track.x + ((width - 0.5) / 19.5).clamp(0.0, 1.0) * track.w
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: Rect = Rect { x: 0.0, y: 0.0, w: 1600.0, h: 1000.0 };

    #[test]
    fn toolbar_bottom_centered_and_buttons_inside() {
        let l = layout(SURFACE, 8, &UiState::default());
        assert!((l.toolbar.x + l.toolbar.w / 2.0 - 800.0).abs() < 1e-9);
        assert!(l.toolbar.y + l.toolbar.h < SURFACE.h);
        assert_eq!(l.buttons.len(), 9);
        for (_, r) in &l.buttons {
            assert!(l.toolbar.contains(Point::new(r.x, r.y)));
            assert!(l.toolbar.contains(Point::new(r.x + r.w, r.y + r.h)));
        }
        assert!(l.color_popup.is_none() && l.width_popup.is_none());
    }

    #[test]
    fn hit_button_and_chrome_and_miss() {
        let l = layout(SURFACE, 8, &UiState::default());
        let (b, r) = &l.buttons[0];
        assert_eq!(
            hit(&l, Point::new(r.x + 5.0, r.y + 5.0)),
            Some(UiHit::Button(*b))
        );
        // between buttons: chrome, swallowed
        assert_eq!(hit(&l, Point::new(l.toolbar.x + 2.0, l.toolbar.y + 2.0)), Some(UiHit::Chrome));
        // far away: pass through to drawing
        assert_eq!(hit(&l, Point::new(100.0, 100.0)), None);
    }

    #[test]
    fn color_popup_swatches_hit() {
        let ui = UiState { color_picker_open: true, ..Default::default() };
        let l = layout(SURFACE, 8, &ui);
        let (_, swatches) = l.color_popup.as_ref().unwrap();
        assert_eq!(swatches.len(), 8);
        let s3 = swatches[3];
        assert_eq!(hit(&l, Point::new(s3.x + 1.0, s3.y + 1.0)), Some(UiHit::Color(3)));
    }

    #[test]
    fn width_track_maps_ends_exactly() {
        let ui = UiState { width_picker_open: true, ..Default::default() };
        let l = layout(SURFACE, 8, &ui);
        let (_, track) = l.width_popup.unwrap();
        assert_eq!(width_from_track_x(track, track.x - 50.0), 0.5);
        assert_eq!(width_from_track_x(track, track.x + track.w + 50.0), 20.0);
        let mid = width_from_track_x(track, track.x + track.w / 2.0);
        assert!((mid - 10.25).abs() < 0.01);
        // round-trip
        let x = track_x_from_width(track, 10.25);
        assert!((width_from_track_x(track, x) - 10.25).abs() < 0.01);
    }
}
