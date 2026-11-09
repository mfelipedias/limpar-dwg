//! Entity modification tools for exploding, mirroring, ROTATE/ROTATE3D,
//! transforming, and translating geometry.
//!
//! The operations are exposed through [`Entity`](super::Entity) and
//! [`EntityType`](super::EntityType) methods.

pub mod explode;
pub mod mirror;
pub mod mirror3d;
pub mod rotate;
pub mod rotate3d;
pub mod transform;
pub mod translate;
