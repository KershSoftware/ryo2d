//! Simple Entity-Component-System (ECS) module

mod component;
mod components;
mod entity;
mod world;

pub use component::Component;
pub use components::{RenderLayer, SpriteRenderer, Transform, Velocity};
pub use entity::Entity;
pub use world::World;
