use crate::model::geom::Rect;
use crate::model::object::{Object, ObjectId};

/// Z-ordered annotation objects (index == z, last drawn on top).
#[derive(Default, Debug)]
pub struct Scene {
    pub objects: Vec<Object>,
    next_id: u64,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc_id(&mut self) -> ObjectId {
        self.next_id += 1;
        ObjectId(self.next_id)
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn index_of(&self, id: ObjectId) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    /// Union of the bounds of the objects at `idxs`, for damage of a group
    /// operation. Out-of-range indices are skipped; empty input gives an
    /// empty rect.
    pub fn bounds_union(&self, idxs: impl IntoIterator<Item = usize>) -> Rect {
        idxs.into_iter()
            .filter_map(|i| self.objects.get(i))
            .fold(Rect::default(), |acc, o| acc.union(o.bounds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::Point;
    use crate::model::object::{ObjectKind, Style};
    use crate::util::color::Rgba;

    fn scene_with_rects(rects: &[Rect]) -> Scene {
        let mut scene = Scene::new();
        for r in rects {
            let id = scene.alloc_id();
            let style = Style { stroke: Rgba::new(0.0, 0.0, 0.0, 1.0), width: 0.0, group_alpha: 1.0 };
            scene.objects.push(Object::new(id, ObjectKind::Rect { r: *r }, style));
        }
        scene
    }

    #[test]
    fn bounds_union_covers_listed_objects_only() {
        let scene = scene_with_rects(&[Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(100.0, 0.0, 10.0, 10.0)]);
        let r = scene.bounds_union([0]);
        assert!(r.contains(Point::new(5.0, 5.0)));
        assert!(!r.contains(Point::new(105.0, 5.0)));
        let both = scene.bounds_union([0, 1, 9]);
        assert!(both.contains(Point::new(105.0, 5.0)));
        assert_eq!(scene.bounds_union([]), Rect::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::Rect;
    use crate::model::object::{ObjectKind, Style};
    use crate::util::color::Rgba;

    fn obj(scene: &mut Scene, at: f64) -> ObjectId {
        let id = scene.alloc_id();
        scene.objects.push(Object::new(
            id,
            ObjectKind::Rect { r: Rect::new(at, at, 10.0, 10.0) },
            Style { stroke: Rgba::new(1.0, 1.0, 1.0, 1.0), width: 2.0, group_alpha: 1.0 },
        ));
        id
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let mut scene = Scene::new();
        let ids: Vec<_> = (0..5).map(|_| scene.alloc_id()).collect();
        assert_eq!(ids, (1..=5).map(ObjectId).collect::<Vec<_>>());
        scene.objects.clear();
        assert_eq!(scene.alloc_id(), ObjectId(6), "clearing must not recycle ids");
    }

    #[test]
    fn len_and_is_empty_track_the_object_list() {
        let mut scene = Scene::new();
        assert!(scene.is_empty());
        obj(&mut scene, 0.0);
        obj(&mut scene, 20.0);
        assert!(!scene.is_empty());
        assert_eq!(scene.len(), 2);
    }

    #[test]
    fn index_of_reports_z_order_and_none_for_removed_objects() {
        let mut scene = Scene::new();
        let a = obj(&mut scene, 0.0);
        let b = obj(&mut scene, 20.0);
        assert_eq!(scene.index_of(a), Some(0));
        assert_eq!(scene.index_of(b), Some(1), "last pushed is topmost");
        scene.objects.remove(0);
        assert_eq!(scene.index_of(a), None);
        assert_eq!(scene.index_of(b), Some(0));
        assert_eq!(scene.index_of(ObjectId(999)), None);
    }

    #[test]
    fn default_scene_is_empty() {
        let scene = Scene::default();
        assert!(scene.is_empty());
        assert!(scene.index_of(ObjectId(1)).is_none());
    }
}
