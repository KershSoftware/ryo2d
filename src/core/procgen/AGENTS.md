# AGENTS.md - Procedural Generation Module

## Overview

The `procgen` module provides a comprehensive toolkit for procedural content generation. It includes multiple algorithms that can be used individually or chained together in pipelines.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public API exports and module documentation |
| `config.rs` | `SeededRng`, `TileType`, `GridRect` - foundational types |
| `grid.rs` | `GenerationGrid`, `Region`, `Connection` - core data structures |
| `events.rs` | Generation events for monitoring |
| `bsp.rs` | Binary Space Partitioning dungeons |
| `cellular.rs` | Cellular automata caves |
| `noise.rs` | Perlin/Simplex noise terrain |
| `random_walk.rs` | Drunkard's walk corridors |
| `poisson.rs` | Poisson disk sampling for placement |
| `wfc.rs` | Wave Function Collapse patterns |
| `mst.rs` | Minimum Spanning Tree connectivity |
| `builder.rs` | `ProcgenBuilder` fluent API |

## Algorithm Selection Guide

| Algorithm | Best For |
|-----------|----------|
| **BSP** | Structured dungeons with distinct rooms |
| **Cellular** | Organic caves, natural terrain |
| **Noise** | Height maps, terrain, biomes |
| **Random Walk** | Organic corridors, winding paths |
| **Poisson** | Object placement (trees, enemies) |
| **WFC** | Tile-based patterns with rules |
| **MST** | Connecting existing regions |

## Core Types

### SeededRng

Wrapper around `rand::StdRng` for reproducible generation:

```rust
let mut rng = SeededRng::new(12345);
let value: f32 = rng.random();
rng.reset(); // Restart sequence
```

### TileType

Enum representing tile states:

```rust
enum TileType {
    Empty,      // Void, not rendered
    Floor,      // Walkable
    Wall,       // Solid
    Door,       // Special
    Water,      // Difficult terrain
    Custom(u32), // Game-specific
}
```

### GenerationGrid

2D grid of tiles with region tracking:

```rust
let mut grid = GenerationGrid::new(64, 64);
grid.set(10, 10, TileType::Floor);
grid.fill_rect(5, 5, 10, 10, TileType::Floor);

// Track regions
let id = grid.add_region(bounds, RegionType::Room);

// Convert to navigation
let nav_grid = grid.to_navigation_grid(16.0);
```

### Region

Represents a room, cave, or area:

```rust
struct Region {
    id: u32,
    bounds: GridRect,
    center: (i32, i32),
    region_type: RegionType,
    metadata: RegionMetadata,
}
```

## Builder API

The `ProcgenBuilder` provides a fluent API for generation pipelines:

```rust
let result = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::dungeon_preset())
    .cellular_smooth(CellularConfig::default().with_iterations(2))
    .border(TileType::Wall, 2)
    .build();
```

### Available Steps

| Method | Description |
|--------|-------------|
| `bsp(config)` | Generate BSP dungeon |
| `cellular(config)` | Generate cellular cave |
| `cellular_smooth(config)` | Apply cellular smoothing |
| `noise(config)` | Generate noise terrain |
| `random_walk(config)` | Generate/add random walk |
| `wfc(config)` | Generate WFC pattern |
| `connect_mst(config)` | Connect regions with MST |
| `fill(tile)` | Fill grid with tile |
| `border(tile, thickness)` | Add border |
| `transform(fn)` | Custom transformation |

## Algorithm Details

### BSP (Binary Space Partitioning)

Creates dungeons by recursively dividing space:

```rust
BspConfig::default()
    .with_room_size(5, 12)      // min, max room size
    .with_split_depth(4)        // recursion depth
    .with_corridor_width(2)     // corridor width
    .with_extra_connectivity(0.1) // loop probability
```

**Presets:**
- `BspConfig::dungeon_preset()` - Classic dungeon
- `BspConfig::maze_preset()` - Tight corridors
- `BspConfig::halls_preset()` - Large open rooms

### Cellular Automata

Creates caves using neighbor-counting rules:

```rust
CellularConfig::default()
    .with_fill_probability(0.45)  // Initial wall density
    .with_iterations(5)           // Simulation steps
    .with_birth_threshold(5)      // Walls needed to become wall
    .with_survival_threshold(4)   // Walls needed to stay wall
    .with_keep_largest_only(true) // Remove isolated caves
```

**Presets:**
- `CellularConfig::cave_preset()` - Open caves
- `CellularConfig::maze_preset()` - Tight passages
- `CellularConfig::smooth_cave_preset()` - Smooth walls

### Noise

Perlin/Simplex noise for terrain:

```rust
NoiseConfig::default()
    .with_noise_type(NoiseType::Simplex)
    .with_scale(0.05)        // Zoom level
    .with_octaves(4)         // Detail layers
    .with_persistence(0.5)   // Octave amplitude decay
    .with_thresholds(vec![
        NoiseThreshold::new(-0.2, TileType::Water),
        NoiseThreshold::new(0.3, TileType::Floor),
        NoiseThreshold::new(f64::MAX, TileType::Wall),
    ])
```

**Presets:**
- `NoiseConfig::island_preset()` - Island with water edges
- `NoiseConfig::cave_preset()` - Binary floor/wall
- `NoiseConfig::terrain_preset()` - Multi-biome

### Random Walk

Drunkard's walk for organic paths:

```rust
RandomWalkConfig::default()
    .with_start(32, 32)      // Starting position
    .with_steps(1000)        // Number of steps
    .with_brush_size(2)      // Carving brush
    .with_momentum(0.3)      // Direction persistence
    .with_walker_count(3)    // Multiple walkers
```

### Poisson Disk Sampling

Even point distribution:

```rust
let points = poisson_points(
    800.0, 600.0,  // Area size
    &PoissonConfig::default()
        .with_min_distance(50.0),  // Minimum spacing
    12345  // Seed
);

for (x, y) in points.iter() {
    // Place object at (x, y)
}
```

### Wave Function Collapse

Constraint-based generation:

```rust
let floor = WfcTile::new(0, TileType::Floor)
    .with_weight(2.0)
    .allow_self_adjacent()
    .allow_neighbor(WfcDirection::Up, 1);

let config = WfcConfig::new()
    .with_tile(floor)
    .with_tile(wall)
    .with_backtracking(true);
```

**Presets:**
- `WfcConfig::simple_preset()` - Basic floor/wall
- `WfcConfig::dungeon_preset()` - Rooms with transitions

### MST Connectivity

Connect regions with minimum spanning tree:

```rust
MstConfig::default()
    .with_extra_connectivity(0.2)  // Add loops
```

## Integration with Navigation

Convert procgen grids to pathfinding grids:

```rust
let procgen = ProcgenBuilder::new(64, 64)
    .with_seed(12345)
    .bsp(BspConfig::default())
    .build();

// cell_size: each procgen cell = 16 world units
let nav_grid = procgen.grid().to_navigation_grid(16.0);

// Use with pathfinding
let path = pathfinder.find_path(&nav_grid, x1, y1, x2, y2);
```

## Events

Events emitted during generation:

| Event | When |
|-------|------|
| `GenerationStarted` | Algorithm begins |
| `GenerationCompleted` | Algorithm finishes successfully |
| `GenerationFailed` | Algorithm fails (contradiction, timeout) |
| `RegionCreated` | New room/area created |
| `RegionsConnected` | Two regions linked |

## Best Practices

1. **Use presets as starting points** - Tune from there
2. **Chain algorithms** - BSP + cellular smoothing
3. **Always use seeds** - For reproducibility during development
4. **Convert to navigation early** - Verify pathfinding works
5. **Test edge cases** - Small grids, extreme configs

## Notes for AI Agents

1. **Seed reproducibility** - Same seed = same output
2. **Config validation** - Configs have sensible defaults
3. **Memory usage** - Grid is O(width * height)
4. **Performance** - Most algorithms are O(n) where n = cell count
5. **WFC can fail** - Always check `WfcResult`
