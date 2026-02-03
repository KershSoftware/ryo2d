//! Built-in engine events
//!
//! These events are emitted by engine systems or intended for game code to emit.
//!
//! ## Automatically Emitted Events
//!
//! The following events are automatically emitted by the engine each frame:
//!
//! - [`CollisionStarted`] - Physics detected two entities began colliding
//! - [`CollisionStopped`] - Physics detected two entities stopped colliding
//!
//! ## Game-Emitted Events
//!
//! The following events are intended for game code to emit when using
//! networking or responding to asset changes:
//!
//! - [`ClientConnected`] - Emit when `ListenServer` reports a new client
//! - [`ClientDisconnected`] - Emit when `ListenServer` reports a client left
//! - [`ConnectedToServer`] - Emit when `NetworkClient` successfully connects
//! - [`DisconnectedFromServer`] - Emit when `NetworkClient` loses connection
//! - [`AssetReloaded`] - Emit when handling hot-reloaded assets
//! - [`AssetLoadFailed`] - Emit when an asset fails to load

use std::path::PathBuf;

use crate::core::ecs::Entity;
use crate::core::network::ClientId;

// ============================================================================
// Physics Events
// ============================================================================

/// Emitted when two entities start colliding.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<CollisionStarted>() {
///         if event.involves(self.player) {
///             let other = event.other(self.player);
///             println!("Player hit {:?}", other);
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollisionStarted {
  /// First entity in the collision pair
  pub entity_a: Entity,
  /// Second entity in the collision pair
  pub entity_b: Entity,
}

impl CollisionStarted {
  /// Check if this collision involves a specific entity
  pub fn involves(&self, entity: Entity) -> bool {
    self.entity_a == entity || self.entity_b == entity
  }

  /// Get the other entity in a collision (given one of them)
  ///
  /// Returns `None` if the given entity is not part of this collision.
  pub fn other(&self, entity: Entity) -> Option<Entity> {
    if self.entity_a == entity {
      Some(self.entity_b)
    } else if self.entity_b == entity {
      Some(self.entity_a)
    } else {
      None
    }
  }
}

/// Emitted when two entities stop colliding.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<CollisionStopped>() {
///         if event.involves(self.player) {
///             println!("Player left collision");
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollisionStopped {
  /// First entity in the collision pair
  pub entity_a: Entity,
  /// Second entity in the collision pair
  pub entity_b: Entity,
}

impl CollisionStopped {
  /// Check if this collision involves a specific entity
  pub fn involves(&self, entity: Entity) -> bool {
    self.entity_a == entity || self.entity_b == entity
  }

  /// Get the other entity in a collision (given one of them)
  ///
  /// Returns `None` if the given entity is not part of this collision.
  pub fn other(&self, entity: Entity) -> Option<Entity> {
    if self.entity_a == entity {
      Some(self.entity_b)
    } else if self.entity_b == entity {
      Some(self.entity_a)
    } else {
      None
    }
  }
}

// ============================================================================
// Network Events
// ============================================================================

/// Emitted when a client connects to the server (host only).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<ClientConnected>() {
///         println!("Client {:?} connected!", event.client_id);
///         // Spawn player for new client, send world state, etc.
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConnected {
  /// The unique identifier for the connected client
  pub client_id: ClientId,
}

/// Emitted when a client disconnects from the server (host only).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<ClientDisconnected>() {
///         println!("Client {:?} disconnected", event.client_id);
///         // Remove player entity, clean up state, etc.
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientDisconnected {
  /// The unique identifier for the disconnected client
  pub client_id: ClientId,
}

/// Emitted when the local client connects to a server (client only).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<ConnectedToServer>() {
///         println!("Connected! My client ID: {:?}", event.client_id);
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedToServer {
  /// The client ID assigned by the server
  pub client_id: ClientId,
}

/// Emitted when the local client disconnects from the server (client only).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for _ in ctx.events.read::<DisconnectedFromServer>() {
///         println!("Lost connection to server");
///         // Return to menu, show error, etc.
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectedFromServer {
  /// Optional reason for disconnection
  pub reason: DisconnectReason,
}

/// Reason for disconnection from server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisconnectReason {
  /// Unknown or unspecified reason
  #[default]
  Unknown,
  /// Server closed the connection
  ServerClosed,
  /// Connection timed out
  Timeout,
  /// Kicked by server
  Kicked,
}

// ============================================================================
// Asset Events
// ============================================================================

/// Emitted when an asset is hot-reloaded during development.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<AssetReloaded>() {
///         println!("Asset reloaded: {:?}", event.path);
///         // Refresh any cached data derived from this asset
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetReloaded {
  /// Path to the reloaded asset (relative to assets directory)
  pub path: PathBuf,
}

/// Emitted when an asset fails to load or reload.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<AssetLoadFailed>() {
///         eprintln!("Failed to load {}: {}", event.path.display(), event.error);
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetLoadFailed {
  /// Path to the asset that failed to load
  pub path: PathBuf,
  /// Error message describing the failure
  pub error: String,
}

// ============================================================================
// Tween Events
// ============================================================================

/// Emitted when a tween starts playing (after any delay).
///
/// This event is emitted by the engine when a [`Tween`](crate::core::ecs::Tween)
/// component begins its animation. Use this to react to tween starts across
/// multiple systems without direct coupling.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<TweenStarted>() {
///         println!("Tween started on entity {:?}", event.entity);
///         // Play a sound, trigger particles, etc.
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweenStarted {
  /// The entity whose tween started
  pub entity: Entity,
}

/// Emitted when a tween completes an iteration.
///
/// This event is emitted by the engine when a [`Tween`](crate::core::ecs::Tween)
/// finishes. For looping tweens (`Loop`, `PingPong`), this fires each cycle.
/// For `Times(n)`, it fires after each iteration including the final one.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<TweenCompleted>() {
///         if event.entity == self.door {
///             // Door finished opening animation
///             self.door_open = true;
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweenCompleted {
  /// The entity whose tween completed
  pub entity: Entity,
  /// Whether the tween has completely finished (not just one iteration)
  ///
  /// - `true` for `TweenLoop::Once` or final iteration of `TweenLoop::Times(n)`
  /// - `false` for `Loop`, `PingPong`, or non-final iterations of `Times(n)`
  pub finished: bool,
}

// ============================================================================
// Knockback Events
// ============================================================================

/// Emitted when knockback starts on an entity.
///
/// This event is emitted by the engine when a [`Knockback`](crate::core::ecs::Knockback)
/// component becomes active (either newly added or reapplied).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<KnockbackStarted>() {
///         // Play hit sound, spawn impact particles, etc.
///         self.play_sound("hit.wav");
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnockbackStarted {
  /// The entity that started being knocked back
  pub entity: Entity,
}

/// Emitted when knockback finishes on an entity.
///
/// This event is emitted by the engine when a [`Knockback`](crate::core::ecs::Knockback)
/// component's strength decays to zero.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<KnockbackFinished>() {
///         // Entity can move freely again
///         if event.entity == self.player {
///             self.player_stunned = false;
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnockbackFinished {
  /// The entity that finished being knocked back
  pub entity: Entity,
}

// ============================================================================
// Squash/Stretch Events
// ============================================================================

/// Emitted when a squash/stretch effect starts on an entity.
///
/// This event is emitted by the engine when a [`SquashStretch`](crate::core::ecs::SquashStretch)
/// component becomes active (either newly added or re-triggered).
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<SquashStretchStarted>() {
///         // Play impact sound
///         self.play_sound("impact.wav");
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SquashStretchStarted {
  /// The entity that started squash/stretch effect
  pub entity: Entity,
}

/// Emitted when a squash/stretch effect finishes on an entity.
///
/// This event is emitted by the engine when a [`SquashStretch`](crate::core::ecs::SquashStretch)
/// component's strength decays to zero and the entity returns to base scale.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<SquashStretchFinished>() {
///         // Entity has returned to normal scale
///         println!("Entity {:?} recovered from impact", event.entity);
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SquashStretchFinished {
  /// The entity that finished squash/stretch effect
  pub entity: Entity,
}
