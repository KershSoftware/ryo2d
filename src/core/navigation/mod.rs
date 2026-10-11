//! Navigation and Pathfinding System
//!
//! Provides grid-based A* pathfinding for game entities.
//!
//! # Overview
//!
//! The navigation system enables entities to find and follow paths through
//! a game world, avoiding obstacles. It consists of:
//!
//! - [`NavigationGrid`] - A cell-based walkability map
//! - [`Pathfinder`] - A* algorithm with optional time-slicing
//! - [`NavAgent`] - Component for entities that need pathfinding
//! - [`NavTarget`] - Component to set navigation destinations
//!
//! # Quick Start
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! struct GameScene {
//!     nav_grid: NavigationGrid,
//!     pathfinder: Pathfinder,
//! }
//!
//! impl Scene for GameScene {
//!     fn on_enter(&mut self, ctx: &mut SceneContext) {
//!         // Create navigation grid from world colliders
//!         self.nav_grid = NavigationBuilder::new(800.0, 600.0)
//!             .with_cell_size(16.0)
//!             .build_from_world(&ctx.world);
//!
//!         // Spawn a navigating entity
//!         ctx.world.spawn((
//!             Transform::new(100.0, 100.0),
//!             NavAgent::new(100.0),  // 100 px/second
//!         ));
//!     }
//!
//!     fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
//!         // Set destination for an entity
//!         if ctx.input.just_pressed(KeyCode::Space) {
//!             ctx.world.add_component(entity, NavTarget::position(500.0, 400.0));
//!         }
//!
//!         // Handle arrival events
//!         for event in ctx.events.read::<PathCompleted>() {
//!             println!("Entity {:?} arrived!", event.entity);
//!         }
//!     }
//! }
//! ```
//!
//! # Grid Setup
//!
//! The navigation grid divides the world into cells and tracks which are walkable:
//!
//! ```ignore
//! // Manual grid creation
//! let mut grid = NavigationGrid::new(800.0, 600.0, 16.0);
//! grid.block_rect(100.0, 100.0, 200.0, 200.0);  // Block an area
//!
//! // Or build from world colliders automatically
//! let grid = NavigationBuilder::new(800.0, 600.0)
//!     .with_cell_size(16.0)
//!     .build_from_world(&ctx.world);
//! ```
//!
//! # Pathfinding
//!
//! Find paths synchronously or with time-slicing for many agents:
//!
//! ```ignore
//! // Synchronous (blocks until complete)
//! let result = pathfinder.find_path(&grid, start_x, start_y, goal_x, goal_y);
//! if let PathResult::Found(path) = result {
//!     // Use the path
//! }
//!
//! // Time-sliced (for 100+ agents)
//! let config = PathfinderConfig::default().with_time_slicing(50);
//! let mut pathfinder = Pathfinder::with_config(config);
//!
//! let (request_id, _) = pathfinder.start_pathfinding(&grid, x1, y1, x2, y2);
//!
//! // Each frame, continue processing
//! match pathfinder.continue_pathfinding(&grid, request_id) {
//!     PathResult::Found(path) => { /* done */ }
//!     PathResult::InProgress => { /* keep waiting */ }
//!     PathResult::NoPath => { /* failed */ }
//!     _ => {}
//! }
//! ```
//!
//! # Path Smoothing
//!
//! Computed paths can be smoothed for more natural movement:
//!
//! ```ignore
//! // Line-of-sight smoothing removes unnecessary waypoints
//! let agent = NavAgent::new(100.0)
//!     .with_smoothing(PathSmoothing::LineOfSight);
//!
//! // Spline smoothing creates curved paths
//! let agent = NavAgent::new(100.0)
//!     .with_smoothing(PathSmoothing::Spline);
//! ```
//!
//! # Events
//!
//! The navigation system emits events for game logic:
//!
//! | Event | When Emitted |
//! |-------|--------------|
//! | [`PathCompleted`] | Agent reached destination |
//! | [`PathFailed`] | No path found or path blocked |
//! | [`PathInvalidated`] | Grid changed, path needs recompute |
//! | [`PathStarted`] | Agent began computing a path |

mod agent;
mod events;
mod grid;
mod path;
mod pathfinder;
pub mod steering;

// Re-export public API
pub use agent::{NavAgent, NavAgentState, NavTarget};
pub use events::{PathCompleted, PathFailed, PathFailureReason, PathInvalidated, PathStarted};
pub use grid::{CellState, GridCoord, NavigationBuilder, NavigationGrid};
pub use path::{Path, PathNode, PathSmoothing};
pub use pathfinder::{PathResult, Pathfinder, PathfinderConfig, PathfindingState};
