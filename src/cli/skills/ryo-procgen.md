---
name: ryo-procgen
description: Procedural generation framework for dungeons, caves, terrain, and more.
metadata:
  version: 1.0.0
---

# Ryo2D Procedural Generation Specialist

You help users generate procedural content using the procgen framework: dungeons, caves, terrain, point distributions, and constraint-based patterns.

**For complete API signatures, see the `ryo-api` skill.**

## Core Concept

The procgen framework provides multiple algorithms that can be used standalone or chained together:

- **BSP** - Binary Space Partitioning for structured dungeons with rooms
- **Cellular** - Cellular automata for organic caves
- **Noise** - Perlin/Simplex noise for terrain and heightmaps
- **RandomWalk** - Drunkard's walk for organic paths
- **Poisson** - Even point distribution for object placement
- **WFC** - Wave Function Collapse for constraint-based generation
- **MST** - Minimum spanning tree for connecting regions

## Setup

Procedural generation is the optional `ryo2d-procgen` crate. Add it beside `ryo2d` in the game's `Cargo.toml` (a path or version like `ryo2d`'s) and import its prelude:

```toml
[dependencies]
ryo2d-procgen = { path = "<ryo2d checkout>/crates/ryo2d-procgen" }
```

## Quick Start with ProcgenBuilder

```rust
use ryo2d::prelude::*;
use ryo2d_procgen::prelude::*;

// Generate a dungeon
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::dungeon_preset())
    .build();

let grid = result.grid();

// Convert to navigation grid for pathfinding
let nav_grid = grid.to_navigation_grid(16.0);
```

## Algorithms

### BSP Dungeons

Binary Space Partitioning creates structured dungeons with rooms and corridors:

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default()
        .with_room_size(5, 12)
        .with_split_depth(4)
        .with_corridor_width(2)
        .with_extra_connectivity(0.1))
    .build();
```

**Presets:**
- `BspConfig::dungeon_preset()` - Classic dungeon with doors
- `BspConfig::maze_preset()` - Tight maze-like passages
- `BspConfig::halls_preset()` - Open halls with wide corridors

### Cellular Automata Caves

Creates organic cave systems using neighbor-counting rules:

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .cellular(CellularConfig::default()
        .with_fill_probability(0.45)
        .with_iterations(5)
        .with_birth_threshold(5)
        .with_survival_threshold(4))
    .build();
```

**Presets:**
- `CellularConfig::cave_preset()` - Open caves with pillars
- `CellularConfig::maze_preset()` - Tighter passages
- `CellularConfig::smooth_cave_preset()` - Fewer isolated walls

### Noise-Based Terrain

Perlin/Simplex noise for terrain and heightmaps:

```rust
let result = ProcgenBuilder::new(128, 128)
    .with_seed(12345)
    .noise(NoiseConfig::default()
        .with_noise_type(NoiseType::Simplex)
        .with_scale(0.02)
        .with_octaves(6)
        .with_thresholds(vec![
            NoiseThreshold::new(-0.3, TileType::Water),
            NoiseThreshold::new(0.3, TileType::Floor),
            NoiseThreshold::new(f64::MAX, TileType::Wall),
        ]))
    .build();
```

**Presets:**
- `NoiseConfig::terrain_preset()` - Multi-biome terrain (water, sand, grass, forest, mountain, snow)
- `NoiseConfig::island_preset()` - Island with water edges
- `NoiseConfig::cave_preset()` - Binary floor/wall noise

**Heightmap generation** (`generate_heightmap` is not in the prelude; import it from `ryo2d_procgen`):
```rust
use ryo2d_procgen::generate_heightmap;

let mut rng = SeededRng::new(12345);
let heightmap = generate_heightmap(128, 128, &NoiseConfig::default(), &mut rng);
// heightmap is Vec<f32> with values 0.0-1.0
```

### Random Walk (Drunkard's Walk)

Creates organic paths and caves:

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .random_walk(RandomWalkConfig::default()
        .with_steps(2000)
        .with_brush_size(2)
        .with_walker_count(3)
        .with_momentum(0.3)
        .with_diagonal(false))
    .build();
```

### Poisson Disk Sampling

Even point distribution for spawning objects:

```rust
// Standalone function (not in builder)
let points = poisson_points(
    800.0, 600.0,
    &PoissonConfig::default().with_min_distance(50.0),
    12345
);

for (x, y) in points.iter() {
    // Spawn tree/enemy/item at (x, y)
}
```

**Variants** (`poisson_circle` and `poisson_masked` are not in the prelude; import them from `ryo2d_procgen`):
```rust
use ryo2d_procgen::{poisson_circle, poisson_masked};

let mut rng = SeededRng::new(12345);

// Points within a circle
let points = poisson_circle(center_x, center_y, radius, &config, &mut rng);

// Points with custom mask
let points = poisson_masked(width, height, &config, &mut rng, |x, y| {
    // Return true if point is valid at (x, y)
    x > 100.0 && y > 100.0
});
```

### Wave Function Collapse

Constraint-based tile pattern generation:

```rust
let result = ProcgenBuilder::new(32, 32)
    .with_seed(12345)
    .wfc(WfcConfig::simple_preset())
    .build();
```

**Custom tile rules:**
```rust
const FLOOR: u32 = 0;
const WALL: u32 = 1;

let config = WfcConfig::new()
    .with_tile(
        WfcTile::new(FLOOR, TileType::Floor)
            .with_weight(2.0)
            .allow_self_adjacent()
            .allow_all_directions(WALL)
    )
    .with_tile(
        WfcTile::new(WALL, TileType::Wall)
            .with_weight(1.0)
            .allow_self_adjacent()
            .allow_all_directions(FLOOR)
    )
    .with_backtracking(true);
```

Collapse grows outward from one cell, which suits connected structures. `.with_seed_spacing(10)` starts from about one random cell per 10x10 area instead, so several regions grow and meet (`simple_preset` does this).

## Chaining Algorithms

Combine multiple algorithms in a pipeline:

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default())
    .cellular_smooth(CellularConfig::default().with_iterations(2))
    .border(TileType::Wall, 2)
    .build();
```

**Available pipeline steps:**
- `.bsp(config)` - Generate BSP dungeon
- `.cellular(config)` - Generate cellular cave
- `.cellular_smooth(config)` - Smooth existing grid with cellular rules
- `.noise(config)` - Generate noise terrain
- `.random_walk(config)` - Generate/apply random walk
- `.wfc(config)` - Generate WFC pattern
- `.connect_mst(config)` - Connect regions with MST
- `.fill(tile)` - Fill entire grid with tile
- `.border(tile, thickness)` - Add border around grid
- `.transform(fn)` - Custom transformation

## Core Types

### TileType

```rust
pub enum TileType {
    Empty,
    Floor,
    Wall,
    Door,
    Water,
    Custom(u32),  // For game-specific tiles
}
```

### GenerationGrid

```rust
let grid = result.grid();

// Access tiles
let tile = grid.get(x, y);
grid.set(x, y, TileType::Floor);

// Iterate
for (x, y, tile) in grid.iter() { ... }

// Count tiles
let floor_count = grid.count_type(TileType::Floor);

// Get regions (rooms, caves)
for region in grid.regions() {
    let (cx, cy) = region.center;
    let bounds = region.bounds;
}

// Convert to navigation grid
let nav_grid = grid.to_navigation_grid(cell_size);
```

### SeededRng

Deterministic random number generator:

```rust
let mut rng = SeededRng::new(12345);

// Random values
let f: f32 = rng.random();
let i = rng.gen_range(0..100);
let b = rng.gen_bool(0.5);

// Derive child RNG (for parallel generation)
let mut child_rng = rng.derive();

// Get/reset seed
let seed = rng.seed();
rng.reset();  // Return to initial state

// Random seed from system entropy
let mut rng = SeededRng::from_entropy();
```

## Seed Reproducibility

All algorithms are deterministic - same seed produces same output:

```rust
// These produce identical results
let grid1 = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default())
    .build();

let grid2 = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default())
    .build();
```

## Integration with Navigation

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default())
    .build();

// Each procgen cell becomes a 16x16 pixel navigation cell
let nav_grid = result.grid().to_navigation_grid(16.0);

// Use with pathfinding
let path = pathfinder.find_path(&nav_grid, start_x, start_y, goal_x, goal_y);
```

## Common Patterns

### Dungeon with Connected Rooms

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::dungeon_preset())
    .connect_mst(MstConfig::default().with_extra_connectivity(0.15))
    .build();
```

### Cave System

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .cellular(CellularConfig::cave_preset())
    .build();

// Single connected cave (keep_largest_only is true by default)
```

### Island Terrain

```rust
use ryo2d_procgen::apply_island_gradient;

let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .noise(NoiseConfig::island_preset())
    .build();
let mut grid = result.into_grid();
apply_island_gradient(&mut grid, 0.3);  // Water at edges
```

### Spawn Points with Minimum Distance

```rust
let spawn_points = poisson_points(
    level_width, level_height,
    &PoissonConfig::default().with_min_distance(100.0),
    seed
);

for (x, y) in spawn_points.iter() {
    spawn_enemy(x, y);
}
```

### Custom Tile Transformation

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .noise(NoiseConfig::terrain_preset())
    .transform(|x, y, tile| {
        // Add rivers at certain positions
        if x % 20 == 0 && tile == TileType::Floor {
            TileType::Water
        } else {
            tile
        }
    })
    .build();
```

## GenerationResult

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default())
    .build();

// Access result data
let grid = result.grid();           // &GenerationGrid
let grid = result.into_grid();      // GenerationGrid (consumes result)
let seed = result.seed;             // u64
let algorithm = &result.algorithm;  // String
let duration = result.duration_ms;  // u64
```

## Performance Tips

1. **Use appropriate grid sizes** - 64x64 to 256x256 is typical
2. **Limit WFC iterations** - Set `max_iterations` to prevent long runs, and check `result.wfc_result()`: `Some(WfcResult::IterationLimit)` or `Some(WfcResult::Contradiction)` means the grid is only partly collapsed (regenerate with another seed)
3. **Cellular iterations** - 4-7 iterations usually sufficient
4. **Poisson min_distance** - Larger distance = fewer points = faster
5. **BSP split_depth** - Higher depth = more rooms = slower
