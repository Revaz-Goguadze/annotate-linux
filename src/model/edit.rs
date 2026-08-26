use crate::model::object::Object;
use crate::model::scene::Scene;

/// A reversible scene mutation. `apply` performs it and returns its exact
/// inverse, so undo and redo share one code path.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Insert { at: usize, obj: Object },
    Remove { at: usize },
    Replace { at: usize, obj: Object },
    Batch(Vec<Edit>),
}

impl Edit {
    /// Build an undoable "remove everything" batch.
    pub fn clear_all(scene: &Scene) -> Option<Edit> {
        if scene.is_empty() {
            return None;
        }
        // Remove from the top down so indices stay valid while applying.
        Some(Edit::Batch((0..scene.len()).rev().map(|at| Edit::Remove { at }).collect()))
    }

    pub fn apply(self, scene: &mut Scene) -> Edit {
        match self {
            Edit::Insert { at, obj } => {
                scene.objects.insert(at, obj);
                Edit::Remove { at }
            }
            Edit::Remove { at } => {
                let obj = scene.objects.remove(at);
                Edit::Insert { at, obj }
            }
            Edit::Replace { at, obj } => {
                let old = std::mem::replace(&mut scene.objects[at], obj);
                Edit::Replace { at, obj: old }
            }
            Edit::Batch(edits) => {
                let mut inverses: Vec<Edit> = edits.into_iter().map(|e| e.apply(scene)).collect();
                inverses.reverse();
                Edit::Batch(inverses)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::Point;
    use crate::model::object::{ObjectKind, Style};
    use crate::util::color::Rgba;

    fn obj(scene: &mut Scene, x: f64) -> Object {
        let id = scene.alloc_id();
        Object::new(
            id,
            ObjectKind::Line { a: Point::new(x, 0.0), b: Point::new(x + 10.0, 10.0) },
            Style { stroke: Rgba::new(1.0, 0.0, 0.0, 1.0), width: 4.0, group_alpha: 1.0 },
        )
    }

    #[test]
    fn insert_inverse_removes() {
        let mut s = Scene::new();
        let o = obj(&mut s, 0.0);
        let inv = Edit::Insert { at: 0, obj: o.clone() }.apply(&mut s);
        assert_eq!(s.len(), 1);
        let inv2 = inv.apply(&mut s);
        assert_eq!(s.len(), 0);
        assert_eq!(inv2, Edit::Insert { at: 0, obj: o });
    }

    #[test]
    fn batch_inverse_restores_exactly() {
        let mut s = Scene::new();
        for i in 0..3 {
            let o = obj(&mut s, i as f64);
            Edit::Insert { at: s.len(), obj: o }.apply(&mut s);
        }
        let before = s.objects.clone();
        let clear = Edit::clear_all(&s).unwrap();
        let inv = clear.apply(&mut s);
        assert!(s.is_empty());
        inv.apply(&mut s);
        assert_eq!(s.objects, before);
    }
}
