//! Tool vocabulary, default keymap, and the pointer drag state machine.
//! Pure logic — Wayland types stay in `wayland/`.

pub mod text_edit;

use crate::model::constraints::{self, Mods};
use crate::model::geom::{Point, Rect};
use crate::model::object::{Object, ObjectId, ObjectKind, Style};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tool {
    Pen,
    Highlighter,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Counter,
    Text,
    Select,
    Eraser,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Pen => "pen",
            Tool::Highlighter => "highlighter",
            Tool::Line => "line",
            Tool::Arrow => "arrow",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Counter => "counter",
            Tool::Text => "text",
            Tool::Select => "select",
            Tool::Eraser => "eraser",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "pen" => Tool::Pen,
            "highlighter" => Tool::Highlighter,
            "line" => Tool::Line,
            "arrow" => Tool::Arrow,
            "rect" | "rectangle" => Tool::Rect,
            "ellipse" | "circle" => Tool::Ellipse,
            "counter" => Tool::Counter,
            "text" => Tool::Text,
            "select" => Tool::Select,
            "eraser" => Tool::Eraser,
            _ => return None,
        })
    }
}

/// Every verb the app understands. Keymap and IPC both funnel into this.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SelectTool(Tool),
    Undo,
    Redo,
    Clear,
    Hide,
    ToggleColorPicker,
    ToggleWidthPicker,
    CycleBoard,
    CounterReset,
    Copy,
    Cut,
    Paste,
    Duplicate,
    DeleteSelection,
}

/// The in-progress pointer drag.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    Idle,
    Stroke {
        pts: Vec<Point>,
    },
    Shape {
        anchor: Point,
        cur: Point,
    },
    /// Dragging the width-slider knob (UI, not drawing).
    UiSlider,
}

/// What a pointer event changed: damage to repaint, and a finished object
/// kind on release.
#[derive(Debug, Default)]
pub struct DragUpdate {
    pub damage: Vec<Rect>,
    pub committed: Option<ObjectKind>,
}

pub struct InputState {
    pub tool: Tool,
    pub mods: Mods,
    pub drag: Drag,
}

impl Default for InputState {
    fn default() -> Self {
        Self { tool: Tool::Pen, mods: Mods::default(), drag: Drag::Idle }
    }
}

impl InputState {
    /// Start a drag. Counter and Text presses never reach this — the app
    /// layer handles them before the drag FSM.
    pub fn on_press(&mut self, pos: Point, style: &Style) -> DragUpdate {
        self.drag = match self.tool {
            Tool::Pen | Tool::Highlighter => Drag::Stroke { pts: vec![pos] },
            Tool::Counter | Tool::Text | Tool::Select | Tool::Eraser => {
                return DragUpdate::default();
            }
            _ => Drag::Shape { anchor: pos, cur: pos },
        };
        DragUpdate {
            damage: vec![Rect::new(pos.x, pos.y, 0.0, 0.0).inflate(style.width / 2.0 + 2.0)],
            committed: None,
        }
    }

    pub fn on_motion(&mut self, pos: Point, style: &Style) -> DragUpdate {
        let margin = style.width / 2.0 + 2.0;
        match &mut self.drag {
            Drag::Idle | Drag::UiSlider => DragUpdate::default(),
            Drag::Stroke { pts } => {
                let last = *pts.last().expect("stroke has at least the press point");
                pts.push(pos);
                DragUpdate {
                    damage: vec![Rect::from_corners(last, pos).inflate(margin)],
                    committed: None,
                }
            }
            Drag::Shape { anchor, cur } => {
                let (anchor, old) = (*anchor, *cur);
                *cur = pos;
                let old_bounds = preview_bounds(self.tool, anchor, old, self.mods, style);
                let new_bounds = preview_bounds(self.tool, anchor, pos, self.mods, style);
                DragUpdate { damage: vec![old_bounds.union(new_bounds)], committed: None }
            }
        }
    }

    /// Shift/Alt press or release mid-drag reshapes the live preview.
    pub fn on_mods_changed(&mut self, mods: Mods, style: &Style) -> DragUpdate {
        let old_mods = self.mods;
        self.mods = mods;
        if let Drag::Shape { anchor, cur } = self.drag {
            let old_bounds = preview_bounds(self.tool, anchor, cur, old_mods, style);
            let new_bounds = preview_bounds(self.tool, anchor, cur, mods, style);
            DragUpdate { damage: vec![old_bounds.union(new_bounds)], committed: None }
        } else {
            DragUpdate::default()
        }
    }

    pub fn on_release(&mut self, pos: Point, style: &Style) -> DragUpdate {
        let margin = style.width / 2.0 + 2.0;
        match std::mem::take(&mut self.drag) {
            Drag::Idle | Drag::UiSlider => DragUpdate::default(),
            Drag::Stroke { mut pts } => {
                let last = *pts.last().expect("nonempty");
                pts.push(pos);
                DragUpdate {
                    damage: vec![Rect::from_corners(last, pos).inflate(margin)],
                    committed: Some(ObjectKind::Freehand { pts }),
                }
            }
            Drag::Shape { anchor, .. } => {
                let kind = constraints::resolve(self.tool, anchor, pos, self.mods)
                    .expect("shape tools resolve");
                let bounds = preview_bounds(self.tool, anchor, pos, self.mods, style);
                DragUpdate { damage: vec![bounds], committed: Some(kind) }
            }
        }
    }

    /// The object being dragged right now, for rendering on top of the scene.
    pub fn preview(&self, style: &Style) -> Option<Object> {
        let kind = match &self.drag {
            Drag::Idle | Drag::UiSlider => return None,
            Drag::Stroke { pts } => ObjectKind::Freehand { pts: pts.clone() },
            Drag::Shape { anchor, cur } => constraints::resolve(self.tool, *anchor, *cur, self.mods)?,
        };
        Some(Object::new(ObjectId(0), kind, *style))
    }
}

fn preview_bounds(tool: Tool, anchor: Point, cur: Point, mods: Mods, style: &Style) -> Rect {
    match constraints::resolve(tool, anchor, cur, mods) {
        Some(kind) => Object::new(ObjectId(0), kind, *style).bounds,
        None => Rect::from_corners(anchor, cur).inflate(style.width / 2.0 + 2.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::color::Rgba;

    fn style() -> Style {
        Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width: 4.0, group_alpha: 1.0 }
    }

    #[test]
    fn stroke_drag_commits_freehand_with_all_points() {
        let mut input = InputState::default();
        let s = style();
        input.on_press(Point::new(0.0, 0.0), &s);
        input.on_motion(Point::new(5.0, 5.0), &s);
        let up = input.on_release(Point::new(10.0, 0.0), &s);
        let Some(ObjectKind::Freehand { pts }) = up.committed else { panic!() };
        assert_eq!(pts.len(), 3);
        assert_eq!(input.drag, Drag::Idle);
    }

    #[test]
    fn shape_drag_damage_covers_old_and_new_preview() {
        let mut input = InputState { tool: Tool::Rect, ..Default::default() };
        let s = style();
        input.on_press(Point::new(10.0, 10.0), &s);
        input.on_motion(Point::new(100.0, 50.0), &s);
        let up = input.on_motion(Point::new(20.0, 20.0), &s);
        // shrinking drag must still damage the previously painted larger rect
        let d = up.damage[0];
        assert!(d.contains(Point::new(100.0, 50.0)));
        assert!(d.contains(Point::new(20.0, 20.0)));
    }

    #[test]
    fn motion_without_press_is_noop() {
        let mut input = InputState::default();
        let up = input.on_motion(Point::new(5.0, 5.0), &style());
        assert!(up.damage.is_empty() && up.committed.is_none());
    }

    const ALL_TOOLS: [Tool; 10] = [
        Tool::Pen,
        Tool::Highlighter,
        Tool::Line,
        Tool::Arrow,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Counter,
        Tool::Text,
        Tool::Select,
        Tool::Eraser,
    ];

    #[test]
    fn tool_names_roundtrip_and_aliases_resolve() {
        for tool in ALL_TOOLS {
            assert_eq!(Tool::from_name(tool.name()), Some(tool), "{}", tool.name());
        }
        assert_eq!(Tool::from_name("rectangle"), Some(Tool::Rect));
        assert_eq!(Tool::from_name("circle"), Some(Tool::Ellipse));
        assert_eq!(Tool::from_name("Pen"), None, "names are case sensitive");
        assert_eq!(Tool::from_name(""), None);
    }

    #[test]
    fn app_level_tools_never_enter_the_drag_fsm() {
        for tool in [Tool::Counter, Tool::Text, Tool::Select, Tool::Eraser] {
            let mut input = InputState { tool, ..Default::default() };
            let up = input.on_press(Point::new(3.0, 4.0), &style());
            assert!(up.damage.is_empty() && up.committed.is_none(), "{}", tool.name());
            assert_eq!(input.drag, Drag::Idle);
            assert!(input.preview(&style()).is_none());
        }
    }

    #[test]
    fn press_damages_the_pen_tip_and_starts_a_stroke() {
        let mut input = InputState::default();
        let up = input.on_press(Point::new(20.0, 20.0), &style());
        assert_eq!(input.drag, Drag::Stroke { pts: vec![Point::new(20.0, 20.0)] });
        let d = up.damage[0];
        assert!(d.contains(Point::new(20.0, 20.0)));
        assert_eq!((d.w, d.h), (8.0, 8.0), "width/2 + 2 on every side");
    }

    #[test]
    fn shape_press_seeds_the_anchor_and_release_resolves_the_kind() {
        let mut input = InputState { tool: Tool::Ellipse, ..Default::default() };
        let s = style();
        input.on_press(Point::new(10.0, 10.0), &s);
        assert_eq!(
            input.drag,
            Drag::Shape { anchor: Point::new(10.0, 10.0), cur: Point::new(10.0, 10.0) }
        );
        let up = input.on_release(Point::new(50.0, 30.0), &s);
        assert!(matches!(up.committed, Some(ObjectKind::Ellipse { .. })));
        assert_eq!(input.drag, Drag::Idle, "release always clears the drag");
        assert!(up.damage[0].contains(Point::new(50.0, 30.0)));
    }

    #[test]
    fn shift_mid_drag_damages_both_the_free_and_constrained_preview() {
        let mut input = InputState { tool: Tool::Rect, ..Default::default() };
        let s = style();
        input.on_press(Point::new(0.0, 0.0), &s);
        input.on_motion(Point::new(80.0, 20.0), &s);

        let up = input.on_mods_changed(Mods { shift: true, ..Default::default() }, &s);
        let free = input.preview(&s).expect("a square preview");
        assert_eq!(free.bounds.w, free.bounds.h, "shift squares the rect");
        assert!(up.damage[0].contains(Point::new(80.0, 20.0)), "old wide preview is repainted");
        assert!(up.damage[0].contains(Point::new(80.0, 80.0)), "new tall preview is repainted");
    }

    #[test]
    fn mods_changed_outside_a_shape_drag_is_a_noop() {
        let mut input = InputState::default();
        let mods = Mods { shift: true, ..Default::default() };
        assert!(input.on_mods_changed(mods, &style()).damage.is_empty());
        assert_eq!(input.mods, mods, "the state is still recorded");

        input.on_press(Point::new(0.0, 0.0), &style());
        assert!(input.on_mods_changed(Mods::default(), &style()).damage.is_empty());
    }

    #[test]
    fn stroke_preview_tracks_the_points_so_far() {
        let mut input = InputState { tool: Tool::Highlighter, ..Default::default() };
        let s = style();
        input.on_press(Point::new(0.0, 0.0), &s);
        input.on_motion(Point::new(10.0, 10.0), &s);
        let Some(ObjectKind::Freehand { pts }) = input.preview(&s).map(|o| o.kind) else {
            panic!("expected a freehand preview")
        };
        assert_eq!(pts, vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)]);
    }

    #[test]
    fn ui_slider_drag_never_draws() {
        let mut input = InputState { drag: Drag::UiSlider, ..Default::default() };
        let s = style();
        assert!(input.preview(&s).is_none());
        assert!(input.on_motion(Point::new(5.0, 5.0), &s).damage.is_empty());
        assert!(input.on_mods_changed(Mods::default(), &s).damage.is_empty());
        let up = input.on_release(Point::new(5.0, 5.0), &s);
        assert!(up.damage.is_empty() && up.committed.is_none());
        assert_eq!(input.drag, Drag::Idle);
    }

    #[test]
    fn release_without_a_drag_commits_nothing() {
        let mut input = InputState::default();
        let up = input.on_release(Point::new(1.0, 1.0), &style());
        assert!(up.damage.is_empty() && up.committed.is_none());
    }
}
