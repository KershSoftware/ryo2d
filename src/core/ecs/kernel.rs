//! Ryo2D's ECS kernel: the entity-component types the engine is built on.
//!
//! Game code reaches these through `ryo2d::ecs` and the prelude. The crate
//! that implements them is a dependency detail; nothing outside this file and
//! `Cargo.toml` names it.

pub use bevy_ecs::bundle::Bundle;
pub use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut, Mut, Ref};
pub use bevy_ecs::component::{Component, Mutable};
pub use bevy_ecs::entity::Entity;
pub use bevy_ecs::entity_disabling::Disabled;
pub use bevy_ecs::error::BevyError as SystemError;
pub(crate) use bevy_ecs::error::panic as panic_on_error;
pub(crate) use bevy_ecs::error::{ErrorContext, FallbackErrorHandler};
pub use bevy_ecs::event::{EntityEvent, Event};
pub use bevy_ecs::hierarchy::{ChildOf, Children};
pub(crate) use bevy_ecs::lifecycle::HookContext;
pub use bevy_ecs::lifecycle::RemovedComponents;
pub use bevy_ecs::message::{Message, MessageCursor, MessageReader, MessageWriter, Messages};
pub use bevy_ecs::observer::{Observer, On};
pub(crate) use bevy_ecs::query::QueryState;
pub use bevy_ecs::query::{Added, Allow, Changed, Has, Or, With, Without};
pub(crate) use bevy_ecs::system::SystemIdMarker;
// Resources live on hidden entities; `IsResource` marks them so world-wide
// entity sweeps can leave them alone.
pub use bevy_ecs::resource::{IsResource, Resource};
pub(crate) use bevy_ecs::schedule::SingleThreadedExecutor;
pub(crate) use bevy_ecs::schedule::graph::Direction;
pub(crate) use bevy_ecs::schedule::{InternedSystemSet, NodeId};
pub use bevy_ecs::schedule::{IntoScheduleConfigs, Schedule, ScheduleLabel, SystemSet};
pub use bevy_ecs::system::{
  Commands, EntityCommands, IntoSystem, Query, Res, ResMut, ScheduleSystem, System,
};
pub(crate) use bevy_ecs::world::DeferredWorld;
pub use bevy_ecs::world::{EntityWorldMut, World};
