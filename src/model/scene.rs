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
}
