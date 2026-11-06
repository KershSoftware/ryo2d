# Floors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tile map layers belong to floors on every grid. Characters draw on and behind raised ground, collide only with their floor, climb stairs between floors, and find paths that cross them.

**Architecture:**
- **Ground space.** A layer above the ground floor stands in ground space: its offset less its floor's height. Everything it owns (colliders, shadow chains, objects, class tiles) is therefore in ground space, and the tile drawer adds the height back.
- **`Floor`.** A public `Floor` component (in `ecs`, so physics, lighting and render can read it) puts an entity on a floor.
- **Drawing.** A tile map system resolves each floored entity's drawn lift from the map under it and writes a crate-internal `DrawLift` on the entity and its descendants. The draw sites add it.
- **Collision.** Physics filters floors with a Rapier hook over spare collider user-data bits. Each higher floor's walkable footprint is traced as its own grid, whose hollow outline is the edge.
- **Stairs.** They are tile runs. `Climber` entities change floor halfway along a run, and their lift eases along it.
- **Paths.** Navigation builds one grid per floor and links them at stair cells, searched by one A* core.

**Tech Stack:** Rust; bevy_ecs 0.20 as `ryo2d::ecs`; Rapier2d `=0.36.1` / parry 0.31.1; wgpu; Tiled 1.12.2 (headless) for fixtures and demo maps.

**Spec:** `docs/specs/2026-10-31-tilemap-shapes-design.md`:
- stage 6 (floors);
- goal 4 and the calls at :120-131 and :157;
- the tests at :264-276;
- **"Floors: details settled at planning (2026-11-06, my calls)"**, which settles what those leave open. Read that section first: this plan argues from it.

**Code maps** (read-only surveys of the code this plan touches, with file:line for every claim), in the scratchpad `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`:
- `floors-map-tilemap.md`;
- `floors-map-physics-render.md`;
- `floors-map-lighting-nav.md`.

The line numbers below come from them and drift as tasks land; find code by name.

## Global Constraints

- **Dependencies:** no new ones. bevy_ecs 0.20, rapier2d `=0.36.1`.
- **Library code:** no `unwrap`/`expect`/`panic` in library code.
- **Style:**
  - LF line endings.
  - Comments normal and minimal, matching the surrounding code's density and naming.
- **Breaking changes are allowed** (owner's rule). Struct literals of `LayerInfo`, `TileData` and `PathNode` change.
- **A map with no layer above the ground floor behaves exactly as before:** the same entities, transforms, z, draw order, colliders, shadows and navigation.
  - The tilemap bench's `static_frame` and `edit` guards keep passing on every grid.
  - `tests/tilemap_alloc.rs` keeps a lit static frame, and a warm frame with moving floored entities, at zero allocations.
- **Public API this plan adds**, all exported from the prelude:
  - `Floor`, `Climber`, `FloorChanged`, `MapFloors`, `MapFloor`, `FloorNavigation`, `StairLink`;
  - the fields and methods each task's Interfaces block names.

  Nothing else becomes public. `DrawLift`, stair runs and the hook's bit layout are crate-internal.
- **Naming:** "floor" already means the surface underfoot in physics (`CharacterState::floor`, `on_floor`, `floor_normal`, `test_util::floor`, `floor_map`).
  - Don't rename those.
  - In docs, say "storey" wherever "floor" could be read either way.
  - LDtk's world depths are not floors.
- **Skills:**
  - Bump each skill once on the branch, by a minor version, in the first task that changes it:
    - ryo-physics 3.7.0 → 3.8.0;
    - ryo-api 2.22.0 → 2.23.0;
    - ryo-character 1.4.0 → 1.5.0;
    - ryo-tilemap 1.7.0 → 1.8.0;
    - ryo-lighting 2.3.0 → 2.4.0;
    - ryo-messages 2.6.0 → 2.7.0;
    - ryo-game-dev 2.8.0 → 2.9.0;
    - ryo-ecs-internals 2.2.0 → 2.3.0.
  - Their Rust examples compile (`tests/skill-examples`).
  - No stage or task labels in user-facing text.
- **Tiled:**
  - Headless only (`tiled --evaluate`): never open its window.
  - Loader fixtures are saved by Tiled 1.12.2 through `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js`, with its renders beside them.
  - Demo maps go through the demo's `make-grid-maps` binary.
- **Measuring:**
  - Release timings run on a quiet machine, with `uptime` recorded.
  - A failing-first run of a memory guard allocates a few hundred MB at most.
  - Logs go under the scratchpad, never /tmp.
- **Commits:**
  - Use each task's commit step.
  - Stage files by name; never `git add -A`.
  - No Co-Authored-By or Claude-Session trailers.
  - Never mention AGENTS.md (say "module notes").

## Review Focus

1. **A map with floors that is moved, turned or scaled at run time.** Its lifted tiles, the lift of the entities on it, and picking all follow the map's transform, because a floor's height is a vector in map space. Covered by Task 3's `picking_through_floors_follows_a_moved_map` and Task 4's `a_turned_map_lifts_its_floor_entities_along_its_own_up`.
2. **A storey made in Tiled as a group layer**, with `floor` and the offset set on the group. Its children are on that floor, at that height. Covered by Task 2's `a_group_layer_s_floor_and_offset_pass_to_its_children`.
3. **A hot reload that moves a layer to another floor or changes its offset.** The layer respawns in its new ground space, and the floor's edge and stairs rebuild. Covered by Task 2's `a_reload_that_moves_a_layer_to_another_floor_respawns_it_in_ground_space` and Task 5's `a_reload_rebuilds_a_floor_s_edge`.
4. **A streamed or infinite map with floors.** The edge follows chunks as they land and unload. Covered by Task 5's `a_streamed_floor_s_edge_follows_its_chunks`.
5. **A character whose sprite and torch are child entities.** Both draw lifted, and the torch's shadows come from its floor's occluders. Covered by Task 4's `a_floor_entity_s_children_draw_lifted_with_it` and Task 6's `a_held_light_on_a_floor_casts_that_floor_s_shadows_drawn_lifted`.

---

### Task 1: The floor filter in physics

**Files:**
- Create: `src/core/ecs/floor.rs`. Re-export it from `src/core/ecs/mod.rs` and the prelude (`src/lib.rs`).
- Create: `src/core/physics/hooks.rs`. `EngineHooks` replaces `OneWayHooks` (`one_way.rs:51-72`, passed at `world.rs:1368`). `one_way.rs` keeps its direction encoding.
- Modify: `src/core/physics/world.rs`:
  - `collider_builder` :700-728;
  - `apply_material` :386-418;
  - `ColliderEntry` :162-173;
  - `sync_collider_with`'s early return :788.
- Modify: `src/core/physics/sync.rs`:
  - `Resync` :79-87, adding `Changed<Floor>`;
  - `forget_removed` :247-290, which reads removed `Floor`.
- Modify: `src/core/physics/character.rs`:
  - the character's own floor read at :421-432;
  - the `solid` predicate :312-326;
  - `coincident_nudge` :742-771.
- Modify: `src/core/physics/query.rs` (`QueryFilter` :17-56, `with_queries` :101-117).
- Modify: `src/core/physics/tiles.rs`:
  - `TileColliders` :655-698, with `with_floor` beside `with_layer` :1156;
  - `spawn` :1431 and `spawn_pack` :1599, whose parts take the grid's `Floor`;
  - `TileRecord` :2408-2437.
- Modify: `src/core/gameplay/zones.rs` (`Respawn` :44, `apply_checkpoints` :270, `apply_kill_zones` :216).
- Docs:
  - `src/core/physics/AGENTS.md`: the layout table, Sync Rules :53-64, One-Way Colliders :86-94 (now the hooks), Characters :103-117, Tile Colliders :119-149;
  - ryo-physics 3.8.0: Layers and masks (floors are apart from the 32 bits), Triggers, Queries (`on_floor`), Pitfalls;
  - ryo-api 2.23.0: a `Floor` row, plus the `Collider`, `QueryFilter` and `TileColliders` rows;
  - ryo-character 1.5.0: the controller's collision line :37, respawns across floors.

**Interfaces:**
- Produces:

```rust
// src/core/ecs/floor.rs (prelude)
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Floor { On(u8), Every }
impl Default for Floor { /* On(0) */ }
impl Floor {
    pub const GROUND: Floor = Floor::On(0);
    /// On(a) meets On(b) when a == b; Every meets every floor.
    pub fn meets(self, other: Floor) -> bool;
    pub fn number(self) -> Option<u8>; // None for Every
}
// physics
impl QueryFilter { pub fn on_floor(self, floor: Floor) -> Self } // default: every floor
pub struct TileColliders { pub floor: Floor, /* .. */ }          // default GROUND
impl TileColliders { pub fn with_floor(self, floor: Floor) -> Self }
```

- Crate-internal, in `hooks.rs`:
  - `pub(crate) fn floor_bits(Floor) -> u128` and `pub(crate) fn floor_of(user_data: u128) -> Floor`. The layout is bits 65..=72 for the floor number and bit 73 for every floor. `GROUND` encodes as 0, so one-way data is unchanged.
  - `pub(crate) fn floor_hooks(Floor) -> ActiveHooks`: `FILTER_CONTACT_PAIRS | FILTER_INTERSECTION_PAIR` for `On(n ≥ 1)`, and empty for `GROUND` and `Every`.

**Rules the tests pin:**
- **A collider's floor** is its own entity's `Floor`, else its owning body's (`owning_body`, `sync.rs:174`), else `GROUND`. A change to either entity's `Floor`, or its removal, resyncs the colliders it reaches.
- **The hook flags** are `one_way::hooks(dir) | floor_hooks(floor)`, set at insert and on change.
- **On a floor change:**
  - write `user_data` and the hooks;
  - force Rapier to filter the pair again, by setting the collision groups to another value and back. Hook and user-data writes alone flag no change (`floors-map-physics-render.md` §2).
- **The character sweep** reads its own floor from its collider's `user_data`, and `solid` rejects colliders whose floor does not meet it. `coincident_nudge` skips those pairs too.
- **A `Respawn`** records the floor of the checkpoint it last touched: the checkpoint entity's `Floor`, else `GROUND`. A respawn writes it to the entity's `Floor` when the entity has one, or when the recorded floor isn't `GROUND`.

- [ ] **Step 1: Write the failing tests**
  - In `floor.rs`: `floors_meet_their_own_number_and_every_floor_meets_all`.
  - In `hooks.rs`: `only_colliders_off_the_ground_floor_and_not_on_every_floor_get_floor_hooks`. It asserts `floor_hooks(Floor::GROUND)` and `floor_hooks(Floor::Every)` are empty, and that a one-way collider on `On(1)` gets `MODIFY_SOLVER_CONTACTS | FILTER_CONTACT_PAIRS | FILTER_INTERSECTION_PAIR`. It also checks that `floor_of(floor_bits(f)) == f` for `On(0)`, `On(1)`, `On(255)` and `Every`.
  - In `sync.rs` tests (`test_util::{world, run, floor, ball}`):
    - `colliders_on_different_floors_never_touch`: a ball on `On(1)` falls through a ground slab within 120 steps, and the same ball on `GROUND` rests on it.
    - `a_collider_on_every_floor_meets_all_of_them`: it rests on a slab of `On(2)`.
    - `a_body_s_floor_applies_to_its_child_colliders`.
    - `a_trigger_on_another_floor_sees_nothing`: there is no `TriggerEntered` until the trigger is `Every`.
    - `changing_a_floor_refilters_a_resting_pair`: let the ball sleep on the slab, set `On(1)`, and it falls. Removing `Floor` puts it back on the ground floor.
  - In `character.rs` tests:
    - `a_top_down_character_walks_through_a_wall_on_another_floor`: on `On(1)` it passes a ground wall, and on `GROUND` it is stopped.
    - `a_platformer_stands_only_on_its_own_floor`.
  - In `query.rs` tests: `queries_see_every_floor_unless_filtered`. A raycast hits an `On(1)` collider by default; `on_floor(Floor::GROUND)` misses it; `on_floor(Floor::On(1))` hits it; an `Every` collider is hit under any filter.
  - In `tiles.rs` tests: `a_tile_grid_s_floor_goes_on_every_part`. With `with_floor(On(2))`, every `TilePart` carries `Floor::On(2)`, and changing the floor rebuilds the parts with the new one.
  - In `zones.rs` tests: `a_respawn_returns_to_the_floor_of_its_last_checkpoint`.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib physics:: gameplay:: ecs::floor`

  Expected: compile errors (no `Floor`), then FAIL once it exists.

- [ ] **Step 3: Implement the rules above** in the files listed. `Floor` is a component and also a value. Read it with `Option<&Floor>` in the `colliders` query (`sync.rs:500`) and through `owning_body`, and store the resolved floor in `ColliderEntry` for the early-return compare.

- [ ] **Step 4: Run the tests**

  Run: `cargo test -p ryo2d --lib physics:: gameplay:: ecs::` and `cargo test --test integration physics`

  Expected: all PASS, including every existing one-way, layer/mask and character test.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: a floor filter in physics keeps colliders on different floors apart"
  ```

---

### Task 2: Floors in tile maps: loading, ground space and `MapFloors`

**Files:**
- Modify: `src/core/tilemap/map.rs`. `LayerInfo` :51-90 gains `floor: u8`; `LayerInfo::new` :92-116 sets 0.
- Modify: `src/core/tilemap/tileset.rs`. `TileData` :66-96 gains `stairs: Option<u8>`.
- Modify: `src/core/tilemap/tiled/build.rs`:
  - `Builder::layer` :850-976 reads `floor`. Add `floor: Option<u8>` to `Inherit` :868-881, so a group's floor passes to children that have none.
  - `tile_data` :332-347 reads `stairs`.
  - Refusals: an `Err` naming the layer, or the tileset and tile id, when the value isn't a whole number from 0 to 255. `Properties::int` accepts whole floats, `property.rs:51`.
- Create: `src/core/tilemap/floors.rs`, holding `MapFloors`, the lift rule and `pub(crate) fn ground_offset(info: &LayerInfo, floors: Option<&MapFloors>) -> Vec2`.
- Modify: `src/core/tilemap/spawn.rs`:
  - `spawn_layer` :1058-1091 places the layer at `ground_offset`;
  - `land` :1309-1394 inserts `MapFloors` on the map when any layer is above the ground floor.
- Modify: `src/core/tilemap/reload.rs`:
  - `LayerShape::Tiles` :78-87 also compares `floor` and the ground offset;
  - an object layer's kept entity takes `ground_offset` :395-399;
  - `MapFloors` is replaced.
- Modify: `src/core/tilemap/view.rs`, adding `floors` and `lift`.
- Fixtures, added to `make-fixtures.js` and saved by Tiled with renders:
  - `maps/floors.tmx` and `maps/floors.tmj`. An isometric 8×8 map with:
    - `ground` (floor 0), holding two cells of a tile with `stairs: 1`;
    - a group `upper` with `floor: 1` and `offsety: -32`, holding a tile layer `upper-ground` and an object layer with one point object;
    - a tile layer `roof` with `floor: 2` and `offsety: -64`.
  - `maps/bad-floor.tmj` (`floor: "two"`) and `maps/floor-300.tmj` (`floor: 300`).
- Docs:
  - tilemap module notes: `map.rs` :26, `spawn.rs` :27-28, reserved properties :87, spawned maps :88;
  - ryo-tilemap 1.8.0: §3's reserved table (`floor`), §4 tile data (`stairs`), §2 what lands (`MapFloors`, layers above the ground in ground space);
  - ryo-api rows: `LayerInfo`, `TileData`, `MapFloors`, `MapsView::floors` and `lift`.

**Interfaces:**
- Consumes: `Floor` (Task 1).
- Produces:

```rust
pub struct LayerInfo { pub floor: u8, /* .. */ }
pub struct TileData { pub stairs: Option<u8>, /* .. */ }
/// On a map's MapRoot entity when some layer is above the ground floor.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MapFloors { /* .. */ }
impl MapFloors {
    /// The floor's height as drawn, in map pixels: GROUND gives Some(ZERO);
    /// a floor no layer is on, or Every, gives None.
    pub fn lift(&self, floor: Floor) -> Option<Vec2>;
    pub fn floors(&self) -> impl Iterator<Item = Floor> + '_; // ascending, ground first
    pub fn highest(&self) -> Floor;
}
impl MapsView<'_> {
    pub fn floors(&self, map: Entity) -> Option<&MapFloors>;
    /// The floor's height as a world vector, through the map's transform.
    pub fn lift(&self, map: Entity, floor: Floor) -> Option<Vec2>;
}
```

**The lift rule:**
- Floor `f ≥ 1` has the height `Vec2::new(0.0, offset.y)` of its bottom layer (the first in draw order with `floor == f`).
- `ground_offset` is `info.offset - lift(f)` for layers above the ground floor, and `info.offset` otherwise.

- [ ] **Step 1: Write the failing tests**
  - In `tiled/mod.rs`:
    - `a_layer_s_floor_and_a_tile_s_stairs_load`: over both fixture formats, `ground` is 0, the `upper` children are 1, `roof` is 2, and the stairs tile reads `Some(1)`.
    - `a_group_layer_s_floor_and_offset_pass_to_its_children` (Review Focus 2): a child with its own `floor` keeps it.
    - `a_floor_that_is_not_a_whole_number_from_0_to_255_refuses_the_map`: both bad fixtures, with the message naming the layer and `floor`. Add a tileset whose tile has `stairs: -1`, refused with the tileset and tile id named.
  - In `spawn` tests:
    - `a_higher_floor_s_layers_stand_in_ground_space`: the `upper-ground` layer's `Transform` is its offset less (0, -32); the point object's `GlobalTransform` is at its Tiled position in ground space; `MapFloors::lift(Floor::On(1)) == Some(Vec2::new(0.0, -32.0))`.
    - `a_floor_s_height_is_its_bottom_layer_s_vertical_offset`: two floor-1 layers at -32 and -40 give a lift of -32 and a second layer at (0, -8).
    - `a_map_without_floors_lands_as_before`: there is no `MapFloors`, and every layer `Transform` equals its offset, a vertical one on the ground floor included.
    - `a_code_built_map_has_floors_too`: a `TileMap` built in code with `LayerInfo.floor = 1`.
  - In `reload` tests: `a_reload_that_moves_a_layer_to_another_floor_respawns_it_in_ground_space` (Review Focus 3).
  - In `view` tests: `a_floor_s_world_lift_follows_the_map_s_transform`, with a map turned 90°.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib tilemap::`

  Expected: FAIL (no field `floor`, no `MapFloors`).

- [ ] **Step 3: Implement** the fields, loader, lift rule, landing, reload and `MapsView` methods above.

- [ ] **Step 4: Run the tests**

  Run: `cargo test -p ryo2d --lib tilemap::` and `cargo test --test integration tilemap`

  Expected: PASS, with every existing loader and reload test green.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: tile map layers belong to floors, and a higher floor's layers stand in ground space"
  ```

---

### Task 3: Drawing floors: lifted tiles, one z, ties by floor, and picking through floors

**Files:**
- Modify: `src/core/render/tiles.rs`:
  - `TileDrawer::collect` :794-799: the z rule, and `affine *= Affine2::from_translation(lift)` for a layer above the ground floor, beside the sub-layer offset;
  - `View::chunk` :1183-1190: rows sort through the ground affine;
  - `Objects::draw` :1572-1640: drawn lifted, sorted at the ground feet.
- Modify: `src/core/render/entities.rs`. `DrawOrder` :57-95 gains `floor`; update the `cmp` and the constructors' callers.
- Modify: `src/core/tilemap/view.rs` and `edit.rs`, adding `y_sort_z` and `floor_cell_at_world`.
- Docs:
  - render module notes: Draw Order :110-117, tile chunks :87-88;
  - tilemap module notes: the render step :9, `view.rs` :55;
  - ryo-tilemap §6 (picking rows: `grid()` is ground space; picking through floors);
  - ryo-tilemap §10 (one z on a map with floors; characters at `y_sort_z`; make raised ground a y-sorted layer so characters interleave with it).

**Interfaces:**
- Consumes: `MapFloors`, `LayerInfo::floor` and `ground_offset` (Task 2).
- Produces:

```rust
// render (crate): order by z, then plain before y-sorted, then sort_y, then floor (lower first), then today's ties
pub(crate) struct DrawOrder { pub z: i32, pub sort_y: Option<f32>, pub floor: u8 }
impl MapsView<'_> {
    /// The z a map's y-sorted layers draw at, for characters to sort with them:
    /// on a map with floors every y-sorted layer's (the first's), else the
    /// first y-sorted layer's. None when no layer is y-sorted.
    pub fn y_sort_z(&self, map: Entity) -> Option<i32>;
    /// The floor and ground cell drawn under a world point: floors from the
    /// highest down, each through its lift and its bottom tile layer's grid;
    /// a higher floor answers only where one of its tile layers on that grid
    /// holds a tile; the ground floor answers anywhere.
    pub fn floor_cell_at_world(&self, map: Entity, world: Vec2) -> Option<(Floor, IVec2)>;
}
```

- Entities' `floor` in `DrawOrder` stays 0 here; Task 4 fills it in. Tile rows and tile objects take their layer's floor.

- [ ] **Step 1: Write the failing tests**
  - In `tests/e2e/tilemap.rs`, using its `render`/`engine` and `assert_pixels` helpers and the Task 2 fixture:
    - `a_floor_s_tiles_draw_lifted_where_tiled_draws_them`: the floors fixture matches Tiled's render at the measured points, as the grid-shape render tests do.
    - `a_lifted_chunk_is_not_culled_while_its_drawing_shows`.
    - `a_character_sorts_with_a_raised_floor_from_both_sides`: a y-sorted floor-1 block, with an entity on the ground floor south of it drawn in front and north of it drawn behind.
    - `under_a_bridge_the_upper_floor_draws_over_the_lower`: a ground-floor entity at a floor-1 row's ground y is drawn under it.
    - `on_a_map_with_floors_every_y_sorted_layer_draws_at_one_z`.
  - In `entities.rs` tests: `equal_sort_y_draws_the_lower_floor_first`.
  - In `view` tests:
    - `picking_checks_floors_from_the_top_down`: a point on a lifted floor-1 tile gives `(Floor::On(1), cell)`, and the point beside it gives the ground cell. Loop over orthogonal, isometric, staggered, hexagonal and oblique.
    - `picking_through_floors_follows_a_moved_map` (Review Focus 1).
    - `y_sort_z_is_the_first_y_sorted_layer_s`, with and without floors.
  - Existing guards that must stay green unchanged: `layers_draw_bottom_first_at_the_map_s_z_among_entities`, `a_layer_offset_shifts_its_tiles` and the `Woods` sorting tests.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib render:: tilemap::view` and `cargo test --test e2e tilemap`

  Expected: FAIL. The floor-1 tiles draw unlifted, and every layer keeps its own z.

- [ ] **Step 3: Implement** the drawer changes, the `DrawOrder` floor and the two `MapsView` methods.

- [ ] **Step 4: Run the tests**

  Run: the Step 2 commands, plus `cargo test --test tilemap_alloc`.

  Expected: PASS.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: a floor's tiles draw lifted and sort with characters across floors"
  ```

---

### Task 4: Entities on a floor draw lifted

**Files:**
- Modify: `src/core/ecs/floor.rs`, adding `DrawLift`.
- Modify: `src/core/tilemap/floors.rs`, adding the `lift_floored` system and the map-under-an-entity lookup. Register it in `src/core/tilemap/mod.rs` in `Phase::PostUpdate`, `.after(PostUpdateSet::Transforms).before(PostUpdateSet::Camera)`.
- Modify: `src/core/render/entities.rs`:
  - `collect_entity`: add the lift right after `render_global` :657, so sprites, tiled runs and drop, blob and contact shadows move. `YSort::calculate_depth_scale` keeps the ground y :661;
  - `collect_composite` :829;
  - the entity's `DrawOrder::floor`, from `DrawLift` (`Every` draws as 0).
- Modify: `src/core/render/text.rs` (`emit_world_text` :1326).
- Modify: `src/core/particles/mod.rs` (emission :612-627: emit at the lifted point).
- Modify: `src/core/systems/trail.rs` (record the lifted point :9).
- Modify: `src/core/tilemap/view.rs`, adding `drawn_lift`.
- Docs:
  - ecs module notes (`floor.rs`);
  - render module notes, Placement :119-126;
  - ryo-ecs-internals 2.3.0: `Floor` in `ecs`, and the crate-internal `DrawLift` and who writes it;
  - ryo-tilemap: a Floors section covering what draws lifted, what stays in ground space (lines, the F2 overlay), and that a camera follows `GlobalTransform + drawn_lift`.

**Interfaces:**
- Consumes: `Floor` (Task 1), `MapFloors` and `MapsView::lift` (Task 2), and `DrawOrder::floor` (Task 3).
- Produces:

```rust
// src/core/ecs/floor.rs (crate-internal)
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawLift { pub(crate) lift: Vec2 /* world */, pub(crate) floor: Floor }
impl MapsView<'_> { pub fn drawn_lift(&self, entity: Entity) -> Vec2 } // ZERO when not lifted
```

**`lift_floored`'s rules:**
- **Which entities.** Every entity with `Floor`, `Without<ParentPlaced>`, gets a lift. A map's own parts and chains are drawn by their layers and stay out of the scan.
- **The lift.** It comes from the map whose world bounds hold the entity's ground position: `MapsView::lift` of its floor, or zero when no map with floors holds it. When several maps hold it, use the first in entity order.
- **Writing it.** Write `DrawLift` with `set_if_neq` on the entity and every descendant, walking `Children` with a `Local` stack. Skip writing it where the floor is `GROUND` and the lift is zero; remove it there instead.
- **When it runs.**
  - Each run takes the entities whose `Floor`, `GlobalTransform` or `Children` changed.
  - It takes every floored entity when some `MapFloors` map's `GlobalTransform` or `MapFloors` changed.
  - Climbers are taken every frame; Task 7 extends this.
  - `RemovedComponents<Floor>` removes `DrawLift` from the entity and its descendants.
- **Allocation.** A warm frame allocates nothing.

- [ ] **Step 1: Write the failing tests**
  - In `tests/e2e/tilemap.rs`: `a_floor_entity_draws_lifted_by_its_floor_s_height`. A sprite with a blob shadow, on `On(1)`, over a map whose floor 1 is 32 px high, draws 32 px up (pixels). The same sprite on `GROUND` draws at its `Transform`.
  - In `floors.rs` tests (`MapWorld`):
    - `a_floor_entity_s_children_draw_lifted_with_it` (Review Focus 5): a child sprite and a grandchild get `DrawLift`.
    - `a_turned_map_lifts_its_floor_entities_along_its_own_up` (Review Focus 1).
    - `an_entity_off_every_map_with_floors_draws_unlifted`.
    - `removing_a_floor_drops_the_lift_from_the_entity_and_its_children`.
    - `lifting_never_moves_an_entity_s_sort_point`: the ground-y sort key is unchanged, and at equal y the floor-1 entity sorts after the ground-floor one.
  - Unit tests at each draw site: `world_text_on_a_floor_draws_lifted` (`text.rs`), `a_floor_emitter_emits_at_its_lifted_point` (particles) and `a_trail_on_a_floor_records_lifted_points` (`trail.rs`).
  - In `tests/tilemap_alloc.rs`: `a_frame_with_entities_moving_on_floors_allocates_nothing`.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib tilemap::floors render:: particles:: systems::trail`, `cargo test --test e2e tilemap` and `cargo test --test tilemap_alloc`

  Expected: FAIL.

- [ ] **Step 3: Implement** `DrawLift`, `lift_floored`, the draw sites and `drawn_lift`.

- [ ] **Step 4: Run the tests**

  Run: the Step 2 commands.

  Expected: PASS.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: entities on a floor draw lifted, with their children, text, particles and trails"
  ```

---

### Task 5: Collision per floor, and a raised floor's edge

**Files:**
- Modify: `src/core/tilemap/collision.rs`:
  - `GroupKey` :72-100 and `Planned` :105-113 gain `floor`;
  - `Group::target` :117-131: the map entity stays the target only for the ground floor;
  - `plan` :263-303 and `land` :503-532 set each group's `TileColliders::floor` to `Floor::On(layer floor)`;
  - the footprint groups;
  - `MapCollision::targets`/`tear_down` :551-570 and `update` :573-615.
- Modify: `src/core/tilemap/kinds.rs`. Add an off-thread stairs table per tileset beside `ShapeSource` :124-188, which `TilesetStamp` :54-106 and `shapes_kept` :108 compare. Classify the footprint cells.
- Modify: `src/core/tilemap/floors.rs`, adding `MapFloor`.
- Modify: `src/core/tilemap/spawn.rs`. `root_discarded` :1396-1424 tears down floor edges.
- Modify: `src/core/tilemap/view.rs`. `navigation` :500-507 skips `MapFloor` owners. Task 8 makes navigation floor-aware; this keeps it unchanged here.
- Docs:
  - tilemap module notes, `collision.rs` :42-49;
  - physics module notes, Tile Colliders :148;
  - ryo-tilemap §8:
    - floors collide apart;
    - a higher floor's edge;
    - on the ground floor a raised area blocks only where it is given collision, so a bridge stands over open ground;
    - "Which entity" covers `MapFloor`.

**Interfaces:**
- Consumes: `TileColliders::floor` (Task 1), and `LayerInfo::floor`, `TileData::stairs`, `MapFloors` and `ground_offset` (Task 2).
- Produces:

```rust
/// A child of the map in ground space holding one higher floor's edge: the
/// floor's footprint traced as a grid of its own (`TileColliders`, floor On(floor)).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFloor { pub floor: u8 }
```

**The footprint of floor f ≥ 1:**
- It holds the cells that have a tile in any tile layer on `f` sharing the grid and ground offset of the floor's bottom tile layer. Layers that differ are left out, with a warning once per map.
- It also holds the cells of tiles with `stairs == Some(f)` in other floors' tile layers on that grid.
- Footprint cells are `Solid`, traced on the `MapFloor` entity at that ground offset, with the map's material.
- `Tracked` over all those layers rebuilds it on edits, reloads and landed chunks.

- [ ] **Step 1: Write the failing tests**
  - In `collision.rs` tests (`code_map`, `parts_of`, `MapWorld`, and `HeadlessEngine` where a character moves):
    - `layers_on_different_floors_collide_on_their_own_grids`: equal offsets, two `TileColliders` with floors 0 and 1.
    - `a_raised_floor_s_edge_keeps_a_character_on_it`: a top-down character on `On(1)` inside the footprint cannot walk off. A ground-floor character walks under it freely.
    - `the_floor_filter_holds_between_tiles_and_entities`: a floor-1 wall stops a floor-1 character and not a ground-floor one.
    - `stairs_join_the_upper_floor_s_footprint`.
    - `a_floor_s_edge_follows_edits`: painting a floor-1 cell rebuilds only that chunk's edge parts.
    - `a_reload_rebuilds_a_floor_s_edge` (Review Focus 3).
    - `a_streamed_floor_s_edge_follows_its_chunks` (Review Focus 4): an unbounded floor-1 layer and a generator. A landed chunk adds edge parts, and an unloaded one removes them.
    - `a_floor_s_edge_traces_on_every_grid`: over orthogonal, isometric, staggered, hexagonal and oblique, the edge equals the footprint's outline.
    - `a_map_without_floors_keeps_its_groups`: the same `TileColliders`, targets and part count as before.
    - `a_discarded_map_takes_its_floor_edges_with_it`.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib tilemap::collision`

  Expected: FAIL.

- [ ] **Step 3: Implement** the floor in the group keys, the footprint groups, `MapFloor`, the stairs table and the teardown.

- [ ] **Step 4: Run the tests**

  Run: `cargo test -p ryo2d --lib tilemap:: physics::tiles` and `cargo test --test tilemap_alloc`

  Expected: PASS.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: each floor collides on its own, and a raised floor's edge keeps characters on it"
  ```

---

### Task 6: Shadows per floor

**Files:**
- Modify: `src/core/tilemap/occluders.rs`:
  - `Lit` :89-101 and `plan` :1339-1369 gain the layer's floor;
  - grouping :1384-1388 keys on `(grid, offset, floor)`;
  - chains of floors ≥ 1 spawn with `Floor::On(f)` in `place` :1246-1254 and `land` :1484-1494.
- Modify: `src/core/engine/extract.rs`:
  - `lights_to_draw` :705-724 and `OccluderItem` :726-746 resolve each light's and occluder's floor and lift: `DrawLift` if present, else its own `Floor` with a zero lift, else `GROUND`;
  - god-ray light positions :163-170 take the lift.
- Modify: `src/core/lighting/batch.rs`. `FrameLight` :13-18 gains `floor` and `lift`; `light_instance` places the quad lifted.
- Modify: `src/core/lighting/shadow.rs`. `push_shadow_volumes` :18-87 skips occluders whose floor doesn't meet the light's, before the reach test, and translates each vertex and its light position by the lift. Shadow volumes are still built in ground space.
- Docs:
  - lighting module notes;
  - ryo-lighting 2.4.0: Floors, covering:
    - a light on a floor is blocked by that floor's and every-floor occluders;
    - a light and its shadows draw lifted;
    - a light still brightens everything under it on screen, whatever its floor (one light image per camera, a recorded limit);
    - an `Every` light draws at ground height;
    - a game's own occluders without `Floor` are on the ground floor;
  - ryo-tilemap §9.

**Interfaces:**
- Consumes: `Floor` (Task 1), `DrawLift` (Task 4), and `LayerInfo::floor` and `ground_offset` (Task 2).
- Produces (crate):

```rust
pub(crate) struct FrameLight { pub position: Vec2, pub light: Light2D, pub layers: RenderLayers, pub floor: Floor, pub lift: Vec2 }
pub(crate) fn push_shadow_volumes(
    position: Vec2, lift: Vec2, light: &Light2D, floor: Floor,
    occluders: &[(GlobalTransform, &LightOccluder2D, Floor)], out: &mut Triangles,
)
```

- [ ] **Step 1: Write the failing tests**
  - In `shadow.rs` tests (extend the `quads` helper with floors):
    - `a_light_on_a_floor_is_blocked_only_by_that_floor_s_occluders`: an `On(1)` light casts no quads from an `On(0)` occluder, and does cast from `On(1)` and `Every` occluders.
    - `a_light_on_every_floor_is_blocked_by_all`.
  - In `occluders.rs` tests: `map_shadow_chains_carry_their_floor`. A floor-1 layer's chains are `Floor::On(1)`, and two floors with equal offsets make separate groups.
  - In `tests/e2e/lighting.rs`: `a_floor_light_and_its_shadows_draw_lifted`, a pixel test modelled on `a_shadow_darkens_only_its_own_light`.
  - In `extract.rs` tests: `a_held_light_on_a_floor_casts_that_floor_s_shadows_drawn_lifted` (Review Focus 5). It is a light child of an `On(1)` entity, extended from `interpolated_lights_and_occluders_draw_where_their_sprites_do`.
  - `tests/tilemap_alloc.rs` `a_frame_drawing_a_static_map_allocates_nothing` stays green. Add a lit floored-map variant.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib lighting:: engine::extract tilemap::occluders`, `cargo test --test e2e lighting` and `cargo test --test tilemap_alloc`

  Expected: FAIL.

- [ ] **Step 3: Implement** the occluder floors, the light floor and lift resolution, and the filtered, lifted shadow volumes.

- [ ] **Step 4: Run the tests**

  Run: the Step 2 commands.

  Expected: PASS.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: a light on a floor is blocked only by that floor's occluders, and draws lifted"
  ```

---

### Task 7: Stairs

**Files:**
- Create: `src/core/tilemap/stairs.rs`, holding:
  - stair runs;
  - `Climber` and `FloorChanged`;
  - the `climb_stairs` system, registered in `FixedSet::Movement`, before `FixedSet::Physics`.
- Register `FloorChanged` beside `MapEvent` (`register_message`, `spawn.rs:1427`). Export both types from the prelude.
- Modify: `src/core/tilemap/floors.rs`. `MapFloors` keeps the runs, rebuilt when the tracked layers' chunk versions or the stairs table change. `lift_floored` eases climbers.
- Docs:
  - tilemap module notes;
  - ryo-tilemap, Stairs (and ramps, which are the same tiles):
    - `stairs` tiles;
    - `Climber`;
    - the halfway switch, and why;
    - `FloorChanged`;
    - an example of a climbing character with a child sprite;
  - ryo-character (already 1.5.0): a section on climbing between floors;
  - ryo-messages 2.7.0: `FloorChanged`.

**Interfaces:**
- Consumes: `Floor` (Task 1); `MapFloors`, `TileData::stairs` and `MapsView::lift` (Task 2); `DrawLift`, `lift_floored` and the map lookup (Task 4); the footprints and stairs table (Task 5).
- Produces:

```rust
/// An entity whose floor changes on stairs, and whose drawn height eases along them.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(Floor)]
pub struct Climber;
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloorChanged { pub entity: Entity, pub from: Floor, pub to: Floor }
```

- Crate-internal: `StairRun { from: u8, to: u8, cells, bottom: Vec2, top: Vec2 }` in the map's ground pixels.

**Rules:**
- **Runs.**
  - A run is a set of stair cells on one layer with the same `to`, connected through `MapGrid::neighbours`.
  - `top` is the mean centre of the sides it shares with floor `to`'s footprint cells. `bottom` is the mean centre of the sides it shares with cells outside the run and outside that footprint.
  - A run with no top or no bottom is warned once per map and ignored. So are stairs naming a floor the map lacks, their own floor or a lower one.
- **Progress:** `t(p) = clamp(dot(p - bottom, top - bottom) / |top - bottom|², 0, 1)`.
- **`climb_stairs`.** Each fixed step, for each `Climber` with `Floor::On(n)`:
  - find the map with floors under its ground point, as Task 4 does, and the cell there;
  - if the cell is in a run with `n ∈ {from, to}`, the floor becomes `to` when `t ≥ 0.5`, else `from`;
  - on a change, write `Floor` and send `FloorChanged`.
- **The eased lift.** In `lift_floored`, every frame, a climber on a run takes `lift(from).lerp(lift(to), t)` at its drawn (interpolated) ground position.
- **Non-climbers** keep their floor and their floor's lift on stairs.

- [ ] **Step 1: Write the failing tests**
  - In `stairs.rs` tests (`MapWorld` and `HeadlessEngine`):
    - `stairs_join_into_runs_with_a_bottom_and_a_top`.
    - `a_climber_changes_floor_halfway_up_and_halfway_down`. This is the spec's "stairs changing floor both ways". A top-down character walks from ground cells up a two-cell run onto the floor-1 footprint, with exactly one `FloorChanged { from: GROUND, to: On(1) }`. It ends on `On(1)`, held in by the edge. Walking back gives one `FloorChanged { from: On(1), to: GROUND }`, and a ground wall stops it afterwards.
    - `a_climber_s_lift_eases_along_the_stairs`: at `t` = 0, 0.5 and 1 the drawn lift is `lift(from)`, their midpoint and `lift(to)`. Between two fixed steps, the drawn lift follows the interpolated position.
    - `a_non_climber_keeps_its_floor_on_stairs`.
    - `stairs_to_a_missing_floor_or_down_warn_once_and_stay_plain`.
    - `a_run_that_leads_nowhere_is_warned_once_and_ignored`.
    - `runs_follow_edits_to_the_stairs`.
    - `a_climber_climbs_stairs_on_a_hexagonal_map`.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib tilemap::stairs tilemap::floors`

  Expected: FAIL.

- [ ] **Step 3: Implement** the runs, `Climber`, `FloorChanged`, `climb_stairs` and the eased lift.

- [ ] **Step 4: Run the tests**

  Run: `cargo test -p ryo2d --lib tilemap:: physics::` and `cargo test --test tilemap_alloc`

  Expected: PASS.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: climbers change floor on stairs, their drawn height eased along them"
  ```

---

### Task 8: Paths across floors

**Files:**
- Modify: `src/core/navigation/pathfinder.rs`. The A* loop (`PathfindingState` :126-159, `step_pathfinding` :329-400, `reconstruct_path` :403-427) becomes one crate-private core over a node key: `GridCoord` for one grid and `(floor index, GridCoord)` across floors. Add `find_floor_path` and `find_floor_path_grid`.
- Modify: `src/core/navigation/path.rs`:
  - `PathNode` :8-16 gains `floor`;
  - `smooth_line_of_sight` :177-209 and `smooth_spline` :214-271 work within each run of one floor and keep the nodes at a link.
- Modify: `src/core/navigation/grid.rs`. `NavigationGrid` :79-99 gains a floor, which is `GROUND` unless it was built for one.
- Create: `src/core/navigation/floors.rs`, holding `FloorNavigation` and `StairLink`, exported from the prelude.
- Modify: `src/core/tilemap/view.rs`:
  - `navigation` :469-509 becomes floor-aware;
  - add `floor_navigation`.
- Docs:
  - navigation module notes;
  - ryo-tilemap §8 Navigation and §12 paths;
  - the ryo-api Navigation rows;
  - ryo-game-dev 2.9.0 Navigation (`PathNode::floor`, `find_floor_path`);
  - every skill example that builds a `PathNode`, which must compile.

**Interfaces:**
- Consumes: `Floor` (Task 1), `MapFloors` (Task 2), the footprints and `MapFloor` (Task 5), and the stair runs (Task 7).
- Produces:

```rust
pub struct PathNode { pub x: f32, pub y: f32, pub grid_coord: GridCoord, pub floor: Floor }
impl NavigationGrid { pub fn floor(&self) -> Floor }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StairLink { pub cell: GridCoord, pub lower: Floor, pub upper: Floor }
#[derive(Clone, Debug)]
pub struct FloorNavigation { /* one grid per floor, the links */ }
impl FloorNavigation {
    pub fn grid(&self, floor: Floor) -> Option<&NavigationGrid>;
    pub fn grids(&self) -> impl Iterator<Item = &NavigationGrid>;
    pub fn links(&self) -> &[StairLink];
}
impl Pathfinder {
    pub fn find_floor_path(&self, nav: &FloorNavigation, start: Vec2, start_floor: Floor, goal: Vec2, goal_floor: Floor) -> PathResult;
    pub fn find_floor_path_grid(&self, nav: &FloorNavigation, start: (Floor, GridCoord), goal: (Floor, GridCoord)) -> PathResult;
}
impl MapsView<'_> { pub fn floor_navigation(&mut self, map: Entity, window: CellRect) -> Option<FloorNavigation> }
```

**Rules:**
- **`navigation(map, layer, window)` on a map with floors** gives the grid of that layer's floor. It blocks only owners whose `TileColliders::floor` is that floor, and never a `MapFloor` edge. On a floor `f ≥ 1`, every cell outside its footprint is `Blocked`. The grid's `floor()` is the layer's floor.
- **`floor_navigation`** builds one such grid per floor over that floor's bottom tile layer grid. A floor whose bottom layer has another grid than the ground floor's is left out, with a warning once. It links every cell of every stair run between its `from` and `to`, at cost 0.
- **The search.**
  - One A* core serves both searches, with no duplicated loop.
  - The heuristic stays the ground metric, which stays admissible since every link joins one ground cell.
  - The tie-break is unchanged, and `max_total_nodes` is per search.
  - Start and goal points map through each floor's grid.

- [ ] **Step 1: Write the failing tests**
  - In `tests/integration/navigation.rs`:
    - `a_path_climbs_stairs_to_another_floor_and_back`. This is the spec's pathfinding test. From floor 0 to a floor-1 cell, the path passes a stair cell and its nodes' floor changes once, at a link; the reverse also works.
    - `floor_paths_are_the_cheapest`: a reference Dijkstra extending `cheapest` across floors, over seeded random small two-floor grids with links.
    - `smoothing_never_skips_across_a_stair_link`.
  - In `view` tests:
    - `a_floor_s_navigation_blocks_only_that_floor`.
    - `an_upper_floor_s_navigation_is_open_only_on_its_footprint`.
    - `navigation_on_a_map_without_floors_is_unchanged`.
  - The existing single-floor navigation and pathfinder tests stay green unchanged, apart from `PathNode` literals gaining `floor`.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p ryo2d --lib navigation:: tilemap::view` and `cargo test --test integration navigation`

  Expected: FAIL.

- [ ] **Step 3: Implement** the shared core, the floor grids, the links, `find_floor_path*` and floor-aware smoothing.

- [ ] **Step 4: Run the tests and the timings**

  Run: the Step 2 commands. Then, once in release on a quiet machine, `cargo test --release -p ryo2d --lib path_timing -- --ignored --nocapture` and `cargo test --release -p ryo2d --lib navigation_window_timing -- --ignored --nocapture`.

  Expected: PASS. The single-floor timings read as `docs/backlog/tilemap.md` records them, within the run-to-run spread; report the figures.

- [ ] **Step 5: Update the docs, run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: paths cross floors through the stairs"
  ```

---

### Task 9: The town gains a raised floor with stairs

**Files:**
- Modify: `examples/demos/tile-grids/scripts/make-maps.js`, town section :575-636:
  - a y-sorted floor-1 raised-ground layer, one block high (`offsety` -110);
  - a floor-1 props layer;
  - stairs tiles with `stairs: 1` on the ground layer, leading onto it;
  - the ground floor given collision under the raised ground, the stairs' own cells left open (a climber is on the upper floor by the time it reaches the top).

  Regenerate with the `make-grid-maps` binary.
- Modify: `scripts/derive-art.sh` and `assets/` (stairs art, see below), and `assets/README.md`.
- Modify: `src/isometric.rs`. The walker is a `Climber`. It sorts at `y_sort_z`, picks with `floor_cell_at_world`, walks paths from `floor_navigation` and `find_floor_path`, and the HUD shows its floor.
- Modify: `src/walker.rs`. `CellPath` carries floors, and the camera follows `GlobalTransform + drawn_lift`.
- Modify: `src/lib.rs:21`. Drop "Floors join these later".
- Modify: `tests/grids.rs` and `tests/pages.rs`.

**Art (my call, in order):**
1. Fetch Kenney's Sketch Town zip, check its sha256 against `assets/README.md:9`, and use its stairs or slope pictures if it has any.
2. Otherwise use another Kenney CC0 isometric pack that has stairs, recording its licence and sha256 in the README.
3. Otherwise draw the stairs as the raised block's picture, tinted and outlined, and say so in the README and the page's HUD.

If the network is unreachable, stop and report BLOCKED. Don't guess at art.

**Interfaces:** consumes everything above. Produces the demo only.

- [ ] **Step 1: Write the failing tests** in `tests/grids.rs` (`engine_on(TOWN)`, `walker`, `teleport`, `shot`, `click`):
  - `the_town_character_climbs_the_stairs_onto_the_raised_floor_and_back`: one `FloorChanged` each way; it ends on `On(1)` and is drawn lifted.
  - `the_town_character_draws_behind_and_in_front_of_the_raised_floor` (pixels).
  - `the_town_picks_cells_on_the_raised_floor`.
  - `the_town_walks_a_path_up_the_stairs` (a click on a floor-1 cell from the ground).

  `every_page_plays_for_five_seconds_without_a_warning` and the existing town tests stay green.

- [ ] **Step 2: Run them to see them fail**

  Run: `cargo test -p tile-grids-demo`

  Expected: FAIL.

- [ ] **Step 3: Build the map, art and page** as above.

- [ ] **Step 4: Run the tests and capture the page**

  Run: `cargo test -p tile-grids-demo`. Then `RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=90 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<scratchpad>/town RYO_INPUT="60:<the town page's key>" cargo run -p tile-grids-demo`, taking the key from the demo-kit's page keys. Look at the PNG.

  Expected: PASS. The capture shows the raised floor and its stairs.

- [ ] **Step 5: Run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "feat: the tile-grids town gains a raised floor with stairs"
  ```

---

### Task 10: Measure, record and clear the backlog

**Files:**
- Modify: `src/core/tilemap/floors.rs`. Add an ignored timing test, `floors_frame_timing`: 64 climbers walking a 256×256 two-floor isometric map. It reports the median and p99 of `lift_floored` plus `climb_stairs`, and holds no target.
- Modify: `docs/backlog/tilemap.md`. Add a dated section, "Floors (date)", in the style of "Hexagonal collision, shadows and paths": the method, the guards' runs, the timing, and the Limits that floors add. The Limits are:
  - a light brightens every floor under it;
  - an `Every` light draws at ground height;
  - lines and the F2 overlay stay in ground space;
  - particles collide by layer, not floor;
  - navigation's `rebuild_from_world` ignores floors;
  - incremental floor paths: only `find_floor_path`'s one-shot search exists.
- Modify: `docs/backlog/tilemap-features.md`. Remove the Floors section and its mention in the opening paragraph.
- Modify: any module note or skill line still saying floors are to come. `grep -rn -i "floors join\|stage 6\|floors later" src docs .claude examples`.

- [ ] **Step 1: Measure on a quiet machine, with `uptime` before each set**
  - `cargo bench --bench tilemap -- static_frame` and `-- edit`, three runs each, and the same with `RYO_TILEMAP_GRID=hexagonal`;
  - `cargo test --release -p ryo2d --lib floors_frame_timing -- --ignored --nocapture`, three runs.

  Expected: every held static and edit target met on every grid, in every run, as recorded before floors. Report any miss with its figures.

- [ ] **Step 2: Write the record and clear the backlog** as above. Every number a Limit quotes must match a row in the same file.

- [ ] **Step 3: Run the gate and commit**

  Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

  ```bash
  git commit -m "docs: floors are measured and recorded, and the tile map backlog no longer lists them"
  ```
