//! Procedural Generation Framework
//!
//! A comprehensive toolkit for procedural content generation in games.
//!
//! # Overview
//!
//! This crate provides multiple algorithms for generating game content:
//!
//! | Algorithm | Use Case |
//! |-----------|----------|
//! | [`bsp`] | Structured dungeons with rooms and corridors |
//! | [`cellular`] | Organic caves using cellular automata |
//! | [`noise`] | Terrain and heightmaps via Perlin/Simplex |
//! | [`random_walk`] | Organic paths and caves (drunkard's walk) |
//! | [`poisson`] | Even point distribution for object placement |
//! | [`wfc`] | Constraint-based pattern generation |
//! | [`mst`] | Connecting regions with minimum spanning tree |
//!
//! # Quick Start
//!
//! Use the [`ProcgenBuilder`] for a fluent API:
//!
//! ```ignore
//! use ryo2d::prelude::*;
//! use ryo2d_procgen::prelude::*;
//!
//! // Generate a dungeon with BSP
//! let result = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .bsp(BspConfig::dungeon_preset())
//!     .build();
//!
//! let grid = result.grid();
//!
//! // Convert to navigation grid for pathfinding
//! let nav_grid = grid.to_navigation_grid(16.0);
//! ```
//!
//! # Individual Algorithms
//!
//! ## BSP Dungeons
//!
//! Binary Space Partitioning creates structured dungeons with rooms:
//!
//! ```ignore
//! let result = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .bsp(BspConfig::default()
//!         .with_room_size(5, 12)
//!         .with_split_depth(4))
//!     .build();
//! ```
//!
//! ## Cellular Automata Caves
//!
//! Creates organic cave systems:
//!
//! ```ignore
//! let result = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .cellular(CellularConfig::cave_preset())
//!     .build();
//! ```
//!
//! ## Noise-Based Terrain
//!
//! Perlin/Simplex noise for height maps and terrain:
//!
//! ```ignore
//! let result = ProcgenBuilder::new(128, 128)
//!     .with_seed(12345)
//!     .noise(NoiseConfig::terrain_preset())
//!     .build();
//! ```
//!
//! ## Poisson Disk Sampling
//!
//! Even point distribution for spawning:
//!
//! ```ignore
//! use ryo2d_procgen::{poisson_points, PoissonConfig};
//!
//! let points = poisson_points(
//!     800.0, 600.0,
//!     &PoissonConfig::default().with_min_distance(50.0),
//!     12345
//! );
//!
//! for (x, y) in points.iter() {
//!     // Spawn tree at (x, y)
//! }
//! ```
//!
//! ## Wave Function Collapse
//!
//! Constraint-based generation:
//!
//! ```ignore
//! let result = ProcgenBuilder::new(32, 32)
//!     .with_seed(12345)
//!     .wfc(WfcConfig::simple_preset())
//!     .build();
//! ```
//!
//! # Chaining Algorithms
//!
//! Combine multiple algorithms in a pipeline:
//!
//! ```ignore
//! let result = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .bsp(BspConfig::default())
//!     .cellular_smooth(CellularConfig::default().with_iterations(2))
//!     .border(TileType::Wall, 2)
//!     .build();
//! ```
//!
//! # Seed Reproducibility
//!
//! All algorithms are deterministic with a given seed:
//!
//! ```ignore
//! // Same seed = same output
//! let grid1 = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .bsp(BspConfig::default())
//!     .build();
//!
//! let grid2 = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .bsp(BspConfig::default())
//!     .build();
//!
//! // grid1 and grid2 are identical
//! ```
//!
//! # Integration with Navigation
//!
//! Generated grids can be converted to navigation grids:
//!
//! ```ignore
//! let result = ProcgenBuilder::new(64, 64)
//!     .with_seed(12345)
//!     .bsp(BspConfig::default())
//!     .build();
//!
//! // Each procgen cell becomes a 16x16 pixel navigation cell
//! let nav_grid = result.grid().to_navigation_grid(16.0);
//!
//! // Now use with pathfinding
//! let path = pathfinder.find_path(&nav_grid, start_x, start_y, goal_x, goal_y);
//! ```

mod bsp;
mod builder;
mod cellular;
mod config;
mod events;
mod grid;
mod mst;
mod noise;
mod poisson;
mod random_walk;
mod wfc;

// Re-export public API
pub use bsp::BspConfig;
pub use builder::{ProcgenBuilder, poisson_points};
pub use cellular::CellularConfig;
pub use config::{GridRect, SeededRng, TileType};
pub use events::{
  GenerationCompleted, GenerationFailed, GenerationFailureReason, GenerationStarted, RegionCreated,
  RegionsConnected,
};
pub use grid::{Connection, GenerationGrid, GenerationResult, Region, RegionMetadata, RegionType};
pub use mst::{MstConfig, MstEdge};
pub use noise::{
  NoiseConfig, NoiseThreshold, NoiseType, apply_island_gradient, generate_heightmap,
};
pub use poisson::{
  PoissonConfig, PoissonResult, generate_in_circle as poisson_circle,
  generate_with_mask as poisson_masked,
};
pub use random_walk::RandomWalkConfig;
pub use wfc::{WfcConfig, WfcDirection, WfcResult, WfcTile};

/// Everything a game uses, for `use ryo2d_procgen::prelude::*;` beside
/// `ryo2d::prelude::*`.
pub mod prelude {
  pub use crate::{
    BspConfig, CellularConfig, Connection, GenerationCompleted, GenerationFailed,
    GenerationFailureReason, GenerationGrid, GenerationResult, GenerationStarted, GridRect,
    MstConfig, MstEdge, NoiseConfig, NoiseThreshold, NoiseType, PoissonConfig, PoissonResult,
    ProcgenBuilder, RandomWalkConfig, Region, RegionCreated, RegionMetadata, RegionType,
    RegionsConnected, SeededRng, TileType, WfcConfig, WfcDirection, WfcResult, WfcTile,
    poisson_points,
  };
}
