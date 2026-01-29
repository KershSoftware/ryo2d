//! Simple Entity-Component-System (ECS) module

mod component;
mod components;
mod entity;
mod world;

pub use component::Component;
pub use components::{Mat4, RenderLayer, SpriteRenderer, Transform, Vec2, Vec3, Vec4, Velocity};
pub use entity::Entity;
pub use world::World;
