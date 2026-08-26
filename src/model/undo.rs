use crate::model::edit::Edit;
use crate::model::scene::Scene;

/// Bounded undo/redo over multiple scenes (one per output). Every entry is
/// tagged with an opaque scene key; the stack stores the *inverse* of each
/// applied edit.
pub struct UndoStack {
    done: Vec<(u64, Edit)>,
    undone: Vec<(u64, Edit)>,
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

    /// Apply a fresh user edit to `scene` (which lives under `key`).
    /// Clears the redo stack.
    pub fn commit(&mut self, key: u64, edit: Edit, scene: &mut Scene) {
        let inverse = edit.apply(scene);
        self.done.push((key, inverse));
        if self.done.len() > self.cap {
            self.done.remove(0);
        }
        self.undone.clear();
    }

    /// Undo the most recent edit. `resolve` maps the entry's key to its
    /// scene; keys must stay resolvable (purge with `forget_key` when an
    /// output disappears). Returns the affected key.
    pub fn undo<'a>(&mut self, resolve: impl FnOnce(u64) -> Option<&'a mut Scene>) -> Option<u64> {
        let (key, inverse) = self.done.pop()?;
        let scene = resolve(key)?;
        self.undone.push((key, inverse.apply(scene)));
        Some(key)
    }

    pub fn redo<'a>(&mut self, resolve: impl FnOnce(u64) -> Option<&'a mut Scene>) -> Option<u64> {
        let (key, edit) = self.undone.pop()?;
        let scene = resolve(key)?;
        self.done.push((key, edit.apply(scene)));
        Some(key)
    }

    /// Drop every entry touching `key` (its output is gone).
    pub fn forget_key(&mut self, key: u64) {
        self.done.retain(|(k, _)| *k != key);
        self.undone.retain(|(k, _)| *k != key);
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
            undo.commit(1, Edit::Insert { at: scene.len(), obj }, &mut scene);
        }
        assert_eq!(scene.len(), 30);
        let full = scene.objects.clone();

        for _ in 0..30 {
            assert_eq!(undo.undo(|_| Some(&mut scene)), Some(1));
        }
        assert!(scene.is_empty());
        assert_eq!(undo.undo(|_| Some(&mut scene)), None, "31st undo is a no-op");

        for _ in 0..30 {
            assert_eq!(undo.redo(|_| Some(&mut scene)), Some(1));
        }
        assert_eq!(scene.objects, full, "redo restores identical objects in order");
        assert_eq!(undo.redo(|_| Some(&mut scene)), None);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut scene = Scene::new();
        let mut undo = UndoStack::default();
        let a = line(&mut scene, 0.0);
        undo.commit(1, Edit::Insert { at: 0, obj: a }, &mut scene);
        undo.undo(|_| Some(&mut scene));
        let b = line(&mut scene, 1.0);
        undo.commit(1, Edit::Insert { at: 0, obj: b }, &mut scene);
        assert_eq!(undo.redo(|_| Some(&mut scene)), None);
    }

    #[test]
    fn per_key_edits_undo_into_their_own_scene() {
        let mut s1 = Scene::new();
        let mut s2 = Scene::new();
        let mut undo = UndoStack::default();
        let o1 = line(&mut s1, 0.0);
        undo.commit(1, Edit::Insert { at: 0, obj: o1 }, &mut s1);
        let o2 = line(&mut s2, 5.0);
        undo.commit(2, Edit::Insert { at: 0, obj: o2 }, &mut s2);

        // top of stack is key 2 → undo touches s2 only
        let key = undo.undo(|k| Some(if k == 1 { &mut s1 } else { &mut s2 }));
        assert_eq!(key, Some(2));
        assert_eq!((s1.len(), s2.len()), (1, 0));
    }

    #[test]
    fn forget_key_purges_entries() {
        let mut s1 = Scene::new();
        let mut undo = UndoStack::default();
        let o = line(&mut s1, 0.0);
        undo.commit(7, Edit::Insert { at: 0, obj: o }, &mut s1);
        undo.forget_key(7);
        assert_eq!(undo.undo(|_| Some(&mut s1)), None);
        assert_eq!(s1.len(), 1, "forget does not mutate the scene");
    }
}
