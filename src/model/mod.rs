//! Pure annotation data model. No wayland, no cairo, no I/O — everything
//! here is unit-testable without a compositor.

pub mod arrow;
pub mod constraints;
pub mod edit;
pub mod geom;
pub mod object;
pub mod scene;
pub mod undo;
