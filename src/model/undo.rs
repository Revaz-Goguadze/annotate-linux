use crate::model::edit::Edit;
use crate::model::scene::Scene;

/// Bounded undo/redo. Stores the *inverse* of every applied edit.
pub struct UndoStack {
    done: Vec<Edit>,
    undone: Vec<Edit>,
    cap: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl UndoStack {
    pub fn new(cap: usize) -> Self {
        Self { done: Vec::new(), undone: Vec::new(), cap }
    }

    /// Apply a fresh user edit. Clears the redo stack.
    pub fn commit(&mut self, edit: Edit, scene: &mut Scene) {
        let inverse = edit.apply(scene);
        self.done.push(inverse);
        if self.done.len() > self.cap {
            self.done.remove(0);
        }
        self.undone.clear();
    }

    pub fn undo(&mut self, scene: &mut Scene) -> bool {
        match self.done.pop() {
            Some(inverse) => {
                self.undone.push(inverse.apply(scene));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, scene: &mut Scene) -> bool {
        match self.undone.pop() {
            Some(edit) => {
                self.done.push(edit.apply(scene));
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::Point;
    use crate::model::object::{Object, ObjectKind, Style};
    use crate::util::color::Rgba;

    fn style() -> Style {
        Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width: 4.0, group_alpha: 1.0 }
    }

    fn line(scene: &mut Scene, x: f64) -> Object {
        let id = scene.alloc_id();
        Object::new(id, ObjectKind::Line { a: Point::new(x, 0.0), b: Point::new(x, 10.0) }, style())
    }

    #[test]
    fn thirty_edits_undo_all_redo_all() {
        let mut scene = Scene::new();
        let mut undo = UndoStack::default();
        for i in 0..30 {
            let obj = line(&mut scene, i as f64);
            undo.commit(Edit::Insert { at: scene.len(), obj }, &mut scene);
        }
        assert_eq!(scene.len(), 30);
        let full = scene.objects.clone();

        for _ in 0..30 {
            assert!(undo.undo(&mut scene));
        }
        assert!(scene.is_empty());
        assert!(!undo.undo(&mut scene), "31st undo must be a no-op");

        for _ in 0..30 {
            assert!(undo.redo(&mut scene));
        }
        assert_eq!(scene.objects, full, "redo must restore identical objects in order");
        assert!(!undo.redo(&mut scene));
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut scene = Scene::new();
        let mut undo = UndoStack::default();
        let a = line(&mut scene, 0.0);
        undo.commit(Edit::Insert { at: 0, obj: a }, &mut scene);
        undo.undo(&mut scene);
        let b = line(&mut scene, 1.0);
        undo.commit(Edit::Insert { at: 0, obj: b }, &mut scene);
        assert!(!undo.redo(&mut scene), "redo history invalidated by new edit");
    }
}
