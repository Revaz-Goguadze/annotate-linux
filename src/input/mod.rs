//! Tool vocabulary, default keymap, and the pointer drag state machine.
//! Pure logic — Wayland types stay in `wayland/`.

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
    pub fn on_press(&mut self, pos: Point, style: &Style) -> DragUpdate {
        self.drag = match self.tool {
            Tool::Pen | Tool::Highlighter => Drag::Stroke { pts: vec![pos] },
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

/// Default keybindings (config-table override lands in M8).
pub mod keymap {
    use super::{Action, Tool};
    use crate::model::constraints::Mods;
    use smithay_client_toolkit::seat::keyboard::Keysym;

    pub fn action_for(keysym: Keysym, mods: Mods) -> Option<Action> {
        if mods.ctrl {
            return match keysym {
                Keysym::z if mods.shift => Some(Action::Redo),
                Keysym::Z => Some(Action::Redo),
                Keysym::z => Some(Action::Undo),
                _ => None,
            };
        }
        Some(match keysym {
            Keysym::Escape => Action::Hide,
            Keysym::p => Action::SelectTool(Tool::Pen),
            Keysym::h => Action::SelectTool(Tool::Highlighter),
            Keysym::l => Action::SelectTool(Tool::Line),
            Keysym::a => Action::SelectTool(Tool::Arrow),
            Keysym::r => Action::SelectTool(Tool::Rect),
            Keysym::e => Action::SelectTool(Tool::Ellipse),
            Keysym::c => Action::ToggleColorPicker,
            Keysym::w => Action::ToggleWidthPicker,
            Keysym::b => Action::CycleBoard,
            Keysym::Delete => Action::Clear,
            _ => return None,
        })
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
}
