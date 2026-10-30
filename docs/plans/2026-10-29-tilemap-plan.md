# Tilemap (6a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Ryo2D's tilemap stub with professional orthogonal tilemaps: Tiled and LDtk levels as registry assets with hot reload, drawn in cached chunks through the world batch, colliding and casting shadows from editor data, carrying objects into the game as entities, editable in code with Wang auto-tiling.

**Architecture:** New `src/core/tilemap/` (data model, Tiled and LDtk readers, runtime spawning and editing, collision and occluder sync, Wang auto-tiling, worlds). Drawing adds a retained chunk payload to the world batch (`src/core/render/tiles.rs`, `src/core/render/batch/chunks.rs`) with a per-draw slot uniform for the map's transform, per-camera parallax and the tileset's texture slot. Collision extends `TileColliders` (`src/core/physics/tiles.rs`) with authored shapes and loop-local rebuilds. Old tilemap API is deleted.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (as `ryo2d::ecs`), wgpu 30, the 5a asset registry; `serde_json` (TMJ/TSJ/LDtk), `roxmltree` 0.20 (TMX/TSX), `flate2`, `base64` 0.22, `ruzstd` (pure-Rust zstd decoder, every target).

**Spec:** `docs/specs/2026-10-29-tilemap-design.md` (with `docs/research/2026-10-29-tilemap-recon.md` and `docs/research/2026-10-29-tilemap-survey.md`)

## Global Constraints

- Work on branch `tilemap` in the main checkout. No git worktrees. Stage files by name; never `git add -A`, never a bare `git stash`; never stage `data/` or `target/`, nor `examples/games/breakout/data/system.json`.
- Commits: `type: what a user of the engine gets`, present tense, lower case. No Co-Authored-By, Claude-Session or any other trailer. Never name AGENTS.md in a message (say "module notes").
- New direct dependencies only: `roxmltree = "0.20"`, `base64 = "0.22"` (both already in Cargo.lock at these versions), and `ruzstd` (its current release; a pure-Rust, decode-only zstd decoder used on every target, so zstd layers load on the web too: the owner's call, 2026-10-29). No `tiled` crate: the readers are the engine's own.
- Loaders read only through the registry (`cx.load`, `cx.load_with`, `cx.read`); never `std::fs`. Paths written in a file resolve against that file's folder (`cx.path().parent()` joined with the written path, normalised by the registry); a path leaving the asset root is a load error.
- Library code never panics on file data: bad input is `AssetError::decode(&path, "<place>: <what>")`, with the place as `line L, column C` (XML, JSON where known), `layer "<name>"`, `object <id>` or `tile <id>`.
- Unsupported until 6b, each a load error naming the feature and "6b": isometric, staggered and hexagonal orientation; infinite maps; Tiled 1.12's oblique orientation. Tiled 1.12 layer blend modes other than normal, add, multiply and screen are a load error naming the mode.
- A cell is 4 bytes (Task 1); a map holds at most 256 tilesets of at most 1,048,575 tiles each; more is a load error.
- Spec targets, verbatim: a static 1024x1024 map of four layers adds at most 50 µs of CPU a frame over an empty frame, with a 1080p view of 16 px tiles, whatever the map's size; editing one tile redraws only its chunk and rebuilds only the outline loops through it (and its occluders), each within 100 µs for loops spanning up to four chunks; a 256x256 four-layer Tiled map in compressed base64 loads off the main thread within 20 ms, and adds no hitch to the frame it lands in.
- Steady frames create no bind groups (`steady_frames_create_no_bind_groups`), and a frame drawing a static map allocates nothing (Task 20's guard).
- Any partial texture write covers whole 16x16 texel blocks (`glyph_atlas::BLOCK`; the Apple GPU fault of 2026-10-29).
- Disk: check `df -h /` before long builds; below 40 GiB free, stop and report. Never delete under `target/`. Scratch target dirs for wasm and bench builds.
- Skills: every edited `.claude/skills/<name>/SKILL.md` is copied byte for byte to `src/cli/skills/<name>.md` and its `metadata.version` bumped once per task; its Rust examples compile (`cargo check -p skill-examples`).
- Gate for every task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`, the task's tests, then `cargo test --workspace`.

## Review Focus

1. A tileset whose image fails to load, or is smaller than its declared tiles: the map fails to load with an error naming the image and the tileset, never draws garbage (Task 2, `a_tileset_image_too_small_for_its_tiles_fails_the_load`).
2. Edits at cells outside a layer, at negative cells, or on a layer name that does not exist: nothing happens and the call says so (`false`/`None`), no panic (Task 6, `edits_outside_a_layer_or_on_a_missing_layer_change_nothing`).
3. A map entity despawned (or its scene left) while chunks, colliders and occluders exist: GPU chunk buffers, collider parts and occluder entities all go within a frame (Task 8, `a_despawned_map_frees_its_chunks`; Task 11, `a_despawned_map_leaves_no_colliders_or_occluders`).
4. A map spawned while its asset is still loading, and a scene left before it lands: it spawns when ready; nothing spawns into a scene that has gone (Task 6, `a_map_spawns_when_its_asset_lands_and_not_after_its_scene_left`).
5. Two cameras with different `RenderLayers`, zoom and parallax over one map: each sees only its layers, each culls its own chunks, each applies its own parallax (Task 9, `two_cameras_each_draw_their_own_view_of_a_map`).

---

### Task 1: The data model, and the old module removed

**Files:**
- Delete: `src/core/tilemap/{autotile,chunk,collision,layer,manager,map,tiled,tileset}.rs`, `tests/integration/tilemap.rs` (and its `mod` line), `examples/demos/tilemap/` (rebuilt in Task 17).
- Create: `src/core/tilemap/{mod,tile,property,tileset,grid,map}.rs`; rewrite `src/core/tilemap/AGENTS.md` (short: what exists after this task; Task 21 completes it).
- Modify: `src/lib.rs` (prelude block for tilemaps, lines ~210-217), `src/core/scene/context.rs` (remove `tilemaps()`), `src/core/engine/mod.rs` (remove `TilemapManager` insert ~409 and `forget_despawned_tilemaps` ~500), `src/core/scene/cleanup.rs` (remove the tilemap forget), `src/core/asset/mod.rs` (`insert_registry`: `register_asset::<TileMap>` and `::<Tileset>`, no loaders yet).

**Interfaces:**
- Produces (prelude):
  - `pub struct TileFlip(u8)` with `NONE`, `HORIZONTAL = 1`, `VERTICAL = 2`, `DIAGONAL = 4`; `contains(self, TileFlip) -> bool`, `BitOr`, `rotated_cw(self) -> TileFlip` (90° clockwise, as Tiled composes it: diagonal, then horizontal, then vertical).
  - `#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub struct Tile { pub tileset: u16, pub id: u32, pub flip: TileFlip }`; `Tile::new(tileset: u16, id: u32) -> Tile`; `with_flip(self, TileFlip) -> Tile`.
  - `pub enum PropertyValue { Bool(bool), Int(i64), Float(f64), String(String), Color(Color), File(String), Object(String), Enum { ty: String, value: String }, Class { ty: String, fields: Properties }, List(Vec<PropertyValue>) }`.
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct Properties` (keeps insertion order): `get(&self, &str) -> Option<&PropertyValue>`, `bool/int/float/string/color(&self, &str) -> Option<..>` (`int` accepts a whole `Float`, `float` accepts `Int`), `insert(&mut self, impl Into<String>, PropertyValue)`, `iter()`.
  - `pub struct Tileset { pub name: String, pub class: Option<String>, pub tile_size: UVec2, pub columns: u32, pub tile_count: u32, pub spacing: u32, pub margin: u32, pub image: TilesetImage, pub wang_sets: Vec<WangSet>, pub properties: Properties, tiles: FastMap<u32, TileData> }` with `tile(&self, id: u32) -> Option<&TileData>`, `tile_rect(&self, id: u32) -> Option<URect>` (texels in the source image, margin and spacing honoured; `None` past `tile_count` or `columns == 0`), `insert_tile(&mut self, id, TileData)`. `impl Asset`.
  - `pub enum TilesetImage { Atlas { image: Handle<Image>, size: UVec2 }, Collection }`.
  - `pub struct TileData { pub class: Option<String>, pub properties: Properties, pub shapes: Vec<TileShape>, pub animation: Vec<TileFrame>, pub probability: f32, pub image: Option<(Handle<Image>, UVec2)> }` (`image` for collection tilesets).
  - `pub struct TileShape { pub points: Vec<Vec2>, pub one_way: bool, pub occluder: bool, pub properties: Properties }` (closed polygon in tile pixels, top-left origin, y down; rectangles as four points, ellipses as 16).
  - `pub struct TileFrame { pub tile: u32, pub duration: Duration }`.
  - `pub struct WangSet { pub name: String, pub kind: WangKind, pub colors: Vec<WangColor>, pub tiles: Vec<(u32, [u8; 8])> }`, `pub enum WangKind { Corner, Edge, Mixed }`, `pub struct WangColor { pub name: String, pub color: Color, pub tile: Option<u32>, pub probability: f32, pub properties: Properties }`.
  - `pub const CHUNK_SIZE: u32 = 32;` (Task 20 may tune it if a guard asks) `pub struct TileGrid` (dense, a cell is a `u32`: 0 empty, else flips in bits 28..=31, tileset in 20..=27, tile id + 1 in 0..=19): `new(size: UVec2)`, `size()`, `get(&self, cell: IVec2) -> Option<Tile>`, `set(&mut self, cell: IVec2, tile: Option<Tile>) -> bool` (false outside or unchanged), `fill(&mut self, min: IVec2, max: IVec2, tile: Option<Tile>) -> usize` (inclusive corners, clipped; cells changed), `clear(&mut self)`, `chunks(&self) -> UVec2`, `drain_dirty(&mut self, out: &mut Vec<UVec2>)` (chunks changed since the last drain, each once, in row order), `mark_all_dirty(&mut self)`.
  - `pub struct IntGrid { pub size: UVec2, pub values: Vec<i32>, pub names: FastMap<i32, String> }` with `get(&self, IVec2) -> Option<i32>` (0 is empty).
  - `pub enum TilesetSource { Asset(Handle<Tileset>), Embedded(Arc<Tileset>) }`.
  - `pub struct LayerInfo { pub id: u32, pub name: String, pub class: Option<String>, pub offset: Vec2, pub opacity: f32, pub tint: Color, pub parallax: Vec2, pub visible: bool, pub blend: BlendMode, pub collides: bool, pub occludes: Option<bool>, pub y_sorted: bool, pub properties: Properties }` (`collides` default true for tile layers; `occludes: None` means "as `collides`").
  - `pub enum ObjectShape { Rect, Ellipse, Point, Capsule, Polygon(Vec<Vec2>), Polyline(Vec<Vec2>), Text(String) }`; `pub struct MapObjectData { pub id: String, pub name: String, pub class: Option<String>, pub position: Vec2, pub size: Vec2, pub rotation: f32, pub shape: ObjectShape, pub tile: Option<Tile>, pub visible: bool, pub properties: Properties }`.
  - `pub struct ImageLayer { pub image: Handle<Image>, pub repeat_x: bool, pub repeat_y: bool }`.
  - `pub enum LayerKind { Tiles { grid: TileGrid, intgrid: Option<IntGrid> }, Objects(Vec<MapObjectData>), Image(ImageLayer) }`; `pub struct MapLayer { pub info: LayerInfo, pub kind: LayerKind }`.
  - `pub struct TileMap { pub size: UVec2, pub tile_size: UVec2, pub tilesets: Vec<TilesetSource>, pub layers: Vec<MapLayer>, pub class: Option<String>, pub background: Option<Color>, pub properties: Properties }`: `new(size, tile_size)`, `add_tileset(&mut self, TilesetSource) -> u16`, `add_tile_layer(&mut self, name: &str) -> usize`, `add_object_layer(&mut self, name: &str) -> usize`, `layer_index(&self, name: &str) -> Option<usize>`, `tiles(&self, layer: usize) -> Option<&TileGrid>`, `tiles_mut(&mut self, layer: usize) -> Option<&mut TileGrid>`. `impl Asset`.

- [ ] **Step 1: Write the failing tests** in `src/core/tilemap/{tile,property,tileset,grid,map}.rs`:
  - `flips_compose_as_tiled_does`: `TileFlip::DIAGONAL | TileFlip::HORIZONTAL` equals `TileFlip::NONE.rotated_cw()`; four `rotated_cw` return to `NONE`.
  - `a_cell_round_trips_every_field`: `Tile { tileset: 255, id: 1_048_574, flip: HORIZONTAL | VERTICAL | DIAGONAL }` set then got equals itself; `Tile::new(0, 0)` too.
  - `set_reports_change_and_marks_one_chunk`: set (40, 3) returns true, again with the same tile false; `drain_dirty` yields `[UVec2(1, 0)]` once, then nothing.
  - `fill_clips_and_counts`: on a 10x10 grid, `fill((-5,-5), (2,2), Some(t))` returns 9; outside cells untouched.
  - `edits_outside_change_nothing`: `set((-1,0))`, `set((10,0))` false; `get` outside `None`.
  - `tile_rect_honours_margin_and_spacing`: 16 px tiles, margin 1, spacing 2, columns 4: tile 5 is `URect::new(19, 19, 35, 35)`; tile past `tile_count` is `None`; `columns == 0` gives `None`.
  - `properties_read_typed_and_keep_order`, `a_map_built_in_code_holds_its_layers`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib tilemap`. Expected: fails to compile (types missing).
- [ ] **Step 3: Implement** the types above; delete the old module, its prelude exports, `ctx.tilemaps()`, the manager resource and its systems, `tests/integration/tilemap.rs`, and `examples/demos/tilemap/`. Register the two asset types in `insert_registry` (update its doc comment and `src/core/asset/AGENTS.md`'s type list).
- [ ] **Step 4: Run** the tests, then the gate. Expected: pass; nothing else references the old API (`grep -rn "TilemapManager\|load_tiled_map\|tilemaps()" src examples tests` empty).
- [ ] **Step 5: Commit** `feat: a tilemap data model with flips, several tilesets per layer and typed properties replaces the old tilemap module`.

### Task 2: Tiled JSON maps and tilesets

**Files:**
- Create: `src/core/tilemap/tiled/{mod,raw,build,data}.rs` (`raw`: serde structs for TMJ/TSJ; `data`: layer data decoding; `build`: raw to `TileMap`/`Tileset`), `src/core/tilemap/tiled/json.rs`; fixtures `tests/fixtures/tilemap/tiled/` (`.tmj`, `.tsj`, PNGs).
- Modify: `Cargo.toml` (deps per Global Constraints), `src/core/asset/mod.rs` (register `TiledMapLoader` for `tmj`, `TiledTilesetLoader` for `tsj`), `src/lib.rs`.

**Interfaces:**
- Consumes: Task 1's types.
- Produces: `pub struct TiledMapLoader;` (`type Asset = TileMap`), `pub struct TiledTilesetLoader;` (`type Asset = Tileset`); `pub(crate) mod raw` with `RawMap`, `RawLayer`, `RawTileset`, `RawTile`, `RawObject`, `RawProperty` (shared with Task 4's XML reader, which fills the same structs); `pub(crate) fn decode_gid(gid: u32) -> (u32, TileFlip)` (clears bits 28..=31; bit 28 is 6b's hex rotation and is ignored in orthogonal maps); `pub(crate) fn decode_layer_data(data: &RawData, encoding: Option<&str>, compression: Option<&str>, expected: usize) -> Result<Vec<u32>, String>`; `pub(crate) fn build_map(raw: RawMap, cx: &mut LoadContext) -> Result<TileMap, AssetError>`.

- [ ] **Step 1: Write the failing tests** in `src/core/tilemap/tiled/mod.rs` (fixtures fed through `MemorySource`, loaded with `AssetsView::load_now`):
  - `flipped_tiles_keep_their_flips`: gids with each flag bit and all three resolve to the right tile and `TileFlip`.
  - `a_layer_mixing_tilesets_keeps_each_tile_s_tileset` (the recon's bug 1).
  - `every_encoding_decodes_the_same_layer`: CSV, base64, base64+gzip, base64+zlib, base64+zstd give equal grids; a wrong cell count is an error naming the layer.
  - `group_layers_flatten_with_their_offset_opacity_tint_and_parallax`, `image_layers_load_their_image`, `layer_visible_defaults_to_true`.
  - `external_and_inline_tilesets_load_alike`; `tileset_paths_resolve_against_the_tileset_s_folder`; `a_path_leaving_the_asset_root_fails_the_load`.
  - `a_tileset_image_too_small_for_its_tiles_fails_the_load`; `a_gid_outside_every_tileset_fails_with_its_layer_and_cell`; `a_tile_id_past_the_cell_limit_fails`.
  - `tile_collision_shapes_animation_class_and_probability_load` (rect, polygon, ellipse as 16 points; `one_way` and `occluder` from bool properties on the shape).
  - `isometric_hexagonal_staggered_oblique_and_infinite_maps_fail_naming_6b`; `an_unsupported_blend_mode_fails_naming_it`; `normal_add_multiply_and_screen_map_to_blend_modes`.
  - `a_map_is_ready_only_when_its_tilesets_and_images_are` (queued load).
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib tilemap::tiled`. Expected: fails (loaders missing).
- [ ] **Step 3: Implement** the loaders. Tilesets referenced by `source` load as `Handle<Tileset>` dependencies; inline ones become `TilesetSource::Embedded`. Tile layers fill a `TileGrid` per layer with each gid's tileset found by `firstgid`. The tileset image size check is in `check` (images loaded). Errors carry the place.
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: Tiled JSON maps and tilesets load as assets, with flips, every layer encoding, group and image layers, and collision shapes`.

### Task 3: Tiled objects, templates, Wang sets and the project file

**Files:**
- Create: `src/core/tilemap/tiled/{objects,project,wang}.rs`; fixtures under `tests/fixtures/tilemap/tiled/` (a `.tiled-project`, `.tj` templates).
- Modify: `src/core/tilemap/tiled/build.rs`, `src/core/engine/mod.rs` (`EngineBuilder::tiled_project(self, path: &str) -> Self`), the config resolution for `[tilemap] tiled_project` in `Ryo.toml` (follow `audio_buffer_frames`: `ProjectLayer`, `ResolvedSettings`, `RYO_EMBED_TILED_PROJECT` in `src/build.rs` and `src/cli/commands/build.rs`), `src/core/asset/mod.rs` (the loaders take the project path at registration).

**Interfaces:**
- Consumes: Task 2's raw model and `build_map`.
- Produces: `pub(crate) struct TiledProject { classes: FastMap<String, ClassDef>, enums: FastMap<String, EnumDef> }` read with `cx.read` (so a saved project reloads every map and tileset that used it) from the configured path, else `project.tiled-project` at the asset root if present, else none; `pub(crate) fn resolve_properties(raw: &[RawProperty], class: Option<&str>, project: Option<&TiledProject>) -> Result<Properties, String>` (members a file omits take the class's defaults; nested classes; enums by name or flags as `List`); `MapObjectData` for every object shape including tile objects (`tile: Some`, positioned by the tileset's object alignment) and templates (read with `cx.read`, instance fields override the template); object-reference properties as `PropertyValue::Object(id)`; Wang sets on `Tileset::wang_sets`.

- [ ] **Step 1: Write the failing tests**:
  - `objects_of_every_shape_load_with_class_and_typed_properties` (rect, ellipse, point, polygon, polyline, capsule, text, tile object).
  - `a_class_member_left_at_its_default_reads_the_project_default`; `nested_class_and_enum_properties_resolve`; `without_a_project_class_typed_properties_keep_what_is_written_and_warn_once`.
  - `a_template_instance_overrides_only_what_it_sets`; `a_tile_object_sits_by_its_tileset_alignment`.
  - `wang_sets_load_with_their_colours_and_tile_ids` (corner, edge, mixed).
  - `the_project_path_comes_from_the_builder_then_ryo_toml_then_the_default`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib tilemap::tiled` and the config tests. Expected: fail.
- [ ] **Step 3: Implement.** Ruling (controller's, recorded here): the default project file name is `project.tiled-project` at the asset root, as the registry has no directory listing; the builder and `Ryo.toml` name another.
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: Tiled objects, templates, Wang sets and the project file's class and enum types load with their defaults`.

### Task 4: Tiled XML (TMX, TSX, TX)

**Files:**
- Create: `src/core/tilemap/tiled/xml.rs`; an XML twin of every Task 2 and 3 fixture under `tests/fixtures/tilemap/tiled/xml/`.
- Modify: `src/core/asset/mod.rs` (register `tmx`, `tsx`; templates `tx` are read, not assets).

**Interfaces:**
- Consumes: the raw model (Task 2), objects and project (Task 3).
- Produces: `pub(crate) fn parse_tmx(text: &str) -> Result<RawMap, String>`, `parse_tsx`, `parse_tx` filling the same raw structs, with `line L, column C` from `roxmltree::Document::text_pos_at` in errors; `TiledMapLoader` and `TiledTilesetLoader` gain the XML extensions.

- [ ] **Step 1: Write the failing test** `every_fixture_loads_the_same_from_xml_and_json`: for each fixture pair, the two `TileMap`s (or `Tileset`s) are equal field by field (handles compared by path). Plus `a_broken_tmx_names_its_line_and_column`.
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement** with `roxmltree`; CSV and base64 data, `<chunk>` (infinite) rejected as in Task 2, properties with `<properties>` children and multi-line string values.
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: Tiled's XML maps, tilesets and templates load as the JSON ones do`.

### Task 5: LDtk projects and levels

**Files:**
- Create: `src/core/tilemap/ldtk/{mod,raw,build}.rs`; fixtures `tests/fixtures/tilemap/ldtk/` (a project with internal levels, one with external `.ldtkl`, a multi-world one).
- Modify: `src/core/asset/mod.rs` (register `LdtkLoader` for `ldtk`), `src/lib.rs`.

**Interfaces:**
- Consumes: Task 1's types.
- Produces:
  - `pub struct LdtkProject { pub levels: Vec<LdtkLevel>, pub worlds: Vec<WorldLayout>, pub tilesets: Vec<Arc<Tileset>> }` (`impl Asset`); `level(&self, name_or_iid: &str) -> Option<&LdtkLevel>`.
  - `pub struct LdtkLevel { pub identifier: String, pub iid: String, pub world: Option<String>, pub position: IVec2, pub size: UVec2, pub fields: Properties, pub neighbours: Vec<Neighbour>, pub map: TileMap }` (the level as a `TileMap` whose tilesets are `Embedded` from `tilesets`).
  - `pub struct WorldLayout { pub name: String, pub levels: Vec<WorldLevel> }`, `pub struct WorldLevel { pub name: String, pub position: Vec2, pub size: Vec2, pub neighbours: Vec<Neighbour> }`, `pub struct Neighbour { pub level: usize, pub direction: NeighbourDirection }`, `pub enum NeighbourDirection { North, South, East, West, NorthEast, NorthWest, SouthEast, SouthWest, Above, Below, Overlap }` (shared with Task 14).
  - IntGrid layers become `LayerKind::Tiles { grid: <its auto-tiles>, intgrid: Some(IntGrid { names from the value identifiers }) }`; tile and auto-layers fill `grid` from `gridTiles`/`autoTiles` (flip bits `f`); entity layers become `Objects` with `id = iid`, typed fields (Int, Float, Bool, String, Color, Enum, Point, EntityRef as `Object(iid)`, Tile, FilePath, arrays as `List`).
  - External levels are read with `cx.read` (a saved `.ldtkl` reloads the project).

- [ ] **Step 1: Write the failing tests**:
  - `internal_and_external_levels_load_alike`; `int_grid_values_keep_their_identifiers`; `auto_tiles_and_grid_tiles_keep_their_flips`.
  - `entities_load_with_every_field_type`; `entity_references_name_the_iid`.
  - `worlds_keep_level_positions_and_neighbours` (n/s/e/w, `<`/`>`, `o`, corners).
  - `a_newer_schema_than_supported_fails_naming_both_versions` (supported: `1.5.x`).
  - `tileset_images_resolve_against_the_project_s_folder`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib tilemap::ldtk`. Expected: fail.
- [ ] **Step 3: Implement** with `serde_json`.
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: LDtk projects load as assets, levels as maps with IntGrid, auto-tiles, typed entities and worlds`.

### Task 6: Spawning maps and editing them at runtime

**Files:**
- Create: `src/core/tilemap/{spawn,edit,view}.rs`.
- Modify: `src/core/scene/context.rs` (`pub fn maps(&mut self) -> MapsView<'_>`), the engine's schedule (spawn system in `PreUpdate` after the asset pump; layer/object bookkeeping), `src/lib.rs`.

**Interfaces:**
- Consumes: `TileMap`, `LdtkProject`, `TilesetSource`.
- Produces:
  - `pub enum IntGridKey { Value(i32), Name(String) }` with `From<i32>` and `From<&str>`.
  - `#[derive(Component)] #[require(Transform)] pub struct MapRoot`: `new(Handle<TileMap>) -> Self`, `ldtk(Handle<LdtkProject>, level: &str) -> Self`, `with_entity_tiles(self, classes: &[&str]) -> Self`, `with_intgrid_collision(self, key: impl Into<IntGridKey>, cell: TileCell) -> Self`, `without_collision(self) -> Self`, `with_z(self, z: i32) -> Self` (base z; layers take `z + index`). The map entity's transform places the map's top-left corner.
  - Children spawned when the asset is ready: one per layer with `MapLayerRef { pub map: Entity, pub index: usize, pub name: String }`; tile layers also get `TileLayerData` (an editable `TileGrid` copy plus the layer's `IntGrid`); objects get `MapObject { pub id: String, pub name: String, pub class: Option<String>, pub shape: ObjectShape, pub size: Vec2, pub properties: Properties }` and a `Transform`; tiles of a registered class get `MapTile { pub map: Entity, pub layer: usize, pub cell: IVec2, pub tile: Tile, pub class: String, pub properties: Properties }` and a `Transform` at the cell's centre, and are cleared from the layer's grid (neither drawn nor collided).
  - `MapObject::reference(&self, field: &str, world: &World) -> Option<Entity>` (object-reference fields resolved through the map's id table, `MapIds` on the map entity).
  - `#[derive(Message, Clone, Debug)] pub enum MapEvent { Spawned { map: Entity }, Reloaded { map: Entity }, Failed { map: Entity, error: Arc<AssetError> } }`.
  - `pub enum LayerRef<'a> { Name(&'a str), Index(usize) }` with `From<&str>` and `From<usize>`.
  - `MapsView<'_>`: `is_ready(map) -> bool`, `tile(map, layer, cell) -> Option<Tile>`, `set_tile(map, layer, cell, Option<Tile>) -> bool`, `fill(map, layer, min, max, Option<Tile>) -> usize`, `intgrid(map, layer, cell) -> Option<i32>`, `tile_data(map, Tile) -> Option<TileData>` (cloned), `world_to_cell(map, Vec2) -> Option<IVec2>`, `cell_to_world(map, IVec2) -> Option<Vec2>` (centre), `layer_entity(map, layer) -> Option<Entity>`. All `layer` arguments are `impl Into<LayerRef>`.
  - Despawning the map despawns its children (hierarchy); `SceneCleanup::WORLD` takes maps with everything else.

- [ ] **Step 1: Write the failing tests** (`asset_world`-style worlds, no GPU):
  - `a_map_spawns_its_layers_and_objects_when_ready`; `objects_carry_their_fields_and_resolve_references`.
  - `tiles_of_a_registered_class_spawn_as_entities_and_leave_the_grid`.
  - `edits_outside_a_layer_or_on_a_missing_layer_change_nothing` (Review Focus 2).
  - `world_and_cell_conversions_follow_the_map_s_transform` (translated, scaled).
  - `a_map_spawns_when_its_asset_lands_and_not_after_its_scene_left` (Review Focus 4).
  - `a_map_whose_asset_fails_reports_failed_once`.
  - `an_ldtk_level_spawns_by_identifier_and_by_iid`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib tilemap::spawn`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: maps spawn as entities with their layers and objects, and are read and edited through ctx.maps()`.

### Task 7: Padded tilesets and chunk instances

**Files:**
- Create: `src/core/tilemap/padded.rs`, `src/core/render/tiles.rs` (instance building only in this task).

**Interfaces:**
- Consumes: `Tileset`, `TileGrid`, `Assets<Image>`.
- Produces:
  - `pub(crate) struct PaddedLayout { pub cell: u32, pub columns: u32, pub size: UVec2 }` and `pub(crate) fn pad_tileset(tileset: &Tileset, images: &Assets<Image>) -> Option<(Image, PaddedLayout)>`: each tile copied into its own power-of-two square cell (side `next_power_of_two(max(w, h) + 2)`, at least 2 px of extrusion), centred, its edge texels extruded to fill the cell; collection tilesets packed the same way. Cells are aligned, so every mip level of the padded image averages one tile's texels only.
  - `pub(crate) struct PaddedTilesets` (resource; keyed by tileset asset id or `Arc` pointer plus image version; one padded `Handle<Image>` each, freed when unused).
  - `pub(crate) fn chunk_instances(grid: &TileGrid, chunk: UVec2, tile_size: Vec2, layouts: &[Option<PaddedLayout>], look: &LayerLook, out: &mut Vec<(u16, SpriteInstance)>)`: instances in map-local pixels (no world transform), grouped by tileset; UVs in padded-tileset space; `uv_bounds` the tile's padded cell; flips: horizontal and vertical as UV swaps, diagonal as the transform's swapped axes (the same corner composition as Tiled); `tint` from `LayerLook { tint, opacity }`; animated tiles' descriptor in `effect_params = [frame_count, frame_duration_s, uv_step_u, uv_step_v]` and `effect_extra = [first_frame_u, first_frame_v, 0, 0]` (frames must be consecutive cells in the padded layout: the padded image lays each animation's frames out consecutively).
- [ ] **Step 1: Write the failing tests**:
  - `a_padded_tileset_extrudes_every_tile_into_its_own_cell` (pixel checks on a 2x2-tile 16 px tileset: cell 32, edges extruded, neighbours never adjacent).
  - `each_flip_maps_the_tile_s_corners_as_tiled_does` (8 combinations: the instance's four corner UVs match Tiled's documented orientation).
  - `a_chunk_groups_instances_by_tileset_in_row_order`; `empty_cells_make_no_instance`; `an_animated_tile_carries_its_frames`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib tiles`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: tilesets are padded so tiles never bleed, and chunks build their tile instances once`.

### Task 8: Retained chunks in the world batch

**Files:**
- Create: `src/core/render/batch/chunks.rs`; shader entries in `src/core/shaders/core/sprite.wgsl` (or a new `tile.wgsl` included by it) for `vs_tile`/`fs_tile` (+ premultiplied/linear variants as the pipeline matrix needs).
- Modify: `src/core/render/draw_list.rs` (`DrawPayload::Chunk(usize)`, `DrawChunk`, `push_chunk`, `sees`, `clear`), `src/core/render/batch/{plan,key,gpu,pipelines}.rs` (`BatchShader::Tile`, a retained source on `Draw`/`Placed` that `plan_draws` never merges across, `prepare` uploads dirty chunks, `draw` binds the chunk buffer and the slot group), `src/core/render/ui_renderer/build.rs` (`DrawPayload::Chunk(_) => {}`), `src/core/render/tiles.rs` (extraction: `collect_tile_layers`), `src/core/engine/extract.rs` (call it after entities), `src/core/graphics/sprite_gpu.rs` (`prepare_textures` makes padded tilesets resident), `src/core/testing/gpu_world.rs` (`(BatchShader::Tile, Stream)` in `the_batch_pipelines_build_in_both_formats`).

**Interfaces:**
- Consumes: Task 7's `chunk_instances`, `PaddedTilesets`; Task 6's `MapRoot`, `MapLayerRef`, `TileLayerData`.
- Produces:
  - `pub(crate) struct ChunkKey { pub layer: Entity, pub chunk: UVec2, pub tileset: u16 }`; `pub(crate) struct ChunkStore` (owned by `Batch2D`): one GPU instance range per key in a growing arena, uploaded in `prepare` only for keys rebuilt this frame; keys not drawn for 120 frames, or whose layer is gone, are freed.
  - A per-draw slot uniform (group 2, dynamic offset, 256-byte aligned, as the UI's scroll slots): the map's interpolated world transform (2x3 affine) with the layer's offset, the camera's parallax offset for that layer, and the tileset's slot UV offset and scale (`textures.slot(padded)`), so a chunk's instances never change when the map moves, the camera moves, or the atlas moves the tileset. Animated frames resolve from `CameraGlobals::game_time` in `vs_tile`.
  - `DrawChunk { key: ChunkKey, texture: TextureId, count: u32, blend: BlendMode, filter: Filter, layers: RenderLayers, min: Vec2, max: Vec2, slot: u32 }` (world AABB of the chunk under the map's transform and the camera's parallax).
  - Each visible chunk is one `DrawItem { z: base + layer index, kind: DrawKind::Entity }`, lit or unlit by a layer property `lit` (default true), culled per camera by its AABB.
- [ ] **Step 1: Write the failing tests** (`src/core/testing/gpu_world.rs` and `tests/e2e/tilemap.rs`, Clamp tonemap, pixel art):
  - `a_tile_layer_draws_its_tiles` (pixel colours at four tiles); `flipped_and_rotated_tiles_draw_as_tiled_shows_them` (an asymmetric tile, 8 flip combinations).
  - `a_layer_mixing_tilesets_draws_each_tile_from_its_own`.
  - `an_animated_tile_shows_the_frame_for_the_game_time_and_holds_while_paused`.
  - `opacity_and_tint_apply`; `a_moving_map_draws_where_it_is_without_rebuilding_chunks` (chunk rebuild counter stays 0 while the map moves).
  - `only_visible_chunks_draw` (draw count equals visible chunks per tileset); `an_edit_rebuilds_one_chunk`.
  - `a_disabled_map_or_a_hidden_layer_draws_nothing`; `a_layer_offset_shifts_its_tiles`.
  - `a_despawned_map_frees_its_chunks` (Review Focus 3); `steady_frames_create_no_bind_groups` still passes with a map on screen.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib gpu_world` and `cargo test -p ryo2d --test e2e tilemap`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, the gate, and `./dev demos:tour` (every page identical to before: maps draw nowhere yet in demos).
- [ ] **Step 5: Commit** `feat: tile layers draw from cached chunks through the world batch, with lighting, layers, culling and animated tiles`.

### Task 9: Y-sorted layers, parallax, image layers, and no seams

**Files:**
- Modify: `src/core/render/tiles.rs`, `src/core/render/entities.rs` (y-sorted rows take their place in the entity draw order), `src/core/render/batch/chunks.rs`; tests in `tests/e2e/tilemap.rs` and `src/core/testing/atlas_bleeding.rs` (or a sibling `tile_bleeding.rs`).

**Interfaces:**
- Consumes: Task 8.
- Produces: rows of a `y_sorted` layer drawn as items ordered with y-sorted entities at the same z by the row's bottom edge (`y_sort_key` rule), each row's instances a sub-range of its chunk's; parallax per (layer, camera) slot with chunk culling shifted by the same offset; image layers as sprites per camera (`DrawKind::Sprite`), repeating when the editor says so (bounded like `MAX_PARALLAX_TILES`); Tiled blend modes mapped (normal `Alpha`, add `Additive`, multiply `Multiply`, screen `Screen`).
- [ ] **Step 1: Write the failing tests**:
  - `a_character_walks_behind_a_y_sorted_tree_and_in_front_of_its_roots`.
  - `a_parallax_layer_moves_by_its_factor`; `two_cameras_each_draw_their_own_view_of_a_map` (Review Focus 5).
  - `an_image_layer_draws_and_repeats`; `blend_modes_draw_as_their_batch_modes`.
  - `no_seams_at_any_zoom_or_subpixel_camera` (scales 0.1, 0.33, 0.5, 1, 1.5, 3, 4.25; offsets in quarter pixels; linear and nearest; every pixel between two solid tiles is a tile colour, never background or a neighbour's).
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: tile layers sort with characters by row, scroll with parallax, carry image layers and never show seams`.

### Task 10: Authored shapes and loop-local rebuilds in TileColliders

**Files:**
- Modify: `src/core/physics/tiles.rs`, `src/core/physics/AGENTS.md`, `src/core/navigation/grid.rs` (`block_tiles` treats `Shape` cells as blocking when the shape covers the cell's centre).

**Interfaces:**
- Produces:
  - `TileCell::Shape(u16)` (index into the map's shapes); `pub struct CellShape { pub polygons: Vec<Vec<Vec2>>, pub one_way: bool }` (tile units 0..=1, y down); `TileColliders::add_shape(&mut self, CellShape) -> u16`, `shape(&self, u16) -> Option<&CellShape>`.
  - `set` records the changed cell; the rebuild keeps the cancelled-edge set and each outline loop's edges between steps, retraces only the loops through edges at a changed cell's four corners (pinch corners included), and despawns and spawns only those loops' parts (each part tagged `pub(crate) struct TileLoop(u32)`); shape cells and one-way runs rebuild per changed cell and row; `solid_rects` mode keeps its whole rebuild; settings changes and first builds rebuild everything.
- [ ] **Step 1: Write the failing tests**:
  - `an_edit_rebuilds_only_the_loops_it_touches` (two separate islands; edit one; the other's part entities are the same); `a_loop_split_and_merged_by_edits_matches_a_full_trace`; `a_pinch_corner_edit_retraces_both_loops`.
  - `shape_cells_build_their_polygons_and_one_way_shapes_are_one_way`.
  - `a_floor_under_a_standing_character_keeps_its_entity_when_a_far_tile_changes`.
  - Every existing test in `tiles.rs`, the four character and platformer tile tests, and the three navigation tests pass unchanged.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib physics::tiles` and `navigation`. Expected: new tests fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, the gate, and `cargo bench --bench physics` (the "100 characters on tiles" frame no slower than before by more than 5%; record both numbers in the report).
- [ ] **Step 5: Commit** `feat: tile colliders take authored shapes and rebuild only the outlines an edit touches`.

### Task 11: Map collision and light occluders

**Files:**
- Create: `src/core/tilemap/{collision,occluders}.rs`.
- Modify: the tilemap systems' schedule (sync in `FixedSet::Movement` before `rebuild_tile_colliders`; occluders in `PostUpdate` before extraction).

**Interfaces:**
- Consumes: Task 6's components, Task 10's `TileColliders` API, `LightOccluder2D::{polygon, rect_offset}`.
- Produces: a map with collision gets a `TileColliders` on its entity, sized to the map, kept in step with its colliding layers' dirty chunks. A cell's collision is the union across colliding layers, ranked Solid over Shape over Slope over OneWay. A tile's shapes, after its flips, classify as: one full-cell rectangle → `Solid` (or `OneWay` if `one_way`); a half-cell right triangle on a cell diagonal → `Slope(corner)`; anything else → `Shape`, deduplicated. LDtk IntGrid names `solid`, `one_way`, `slope_bottom_left`, `slope_bottom_right`, `slope_top_left` and `slope_top_right` map by default; `with_intgrid_collision` adds or overrides. Material from map properties `collision_layer`, `collision_mask`, `friction`, `restitution`. Occluding layers keep one occluder child per merged rectangle (greedy within each chunk) and per shape polygon (`occluder` shapes, else collision shapes), each entity at its geometry's centre (the lighting cull reads the entity's translation), rebuilt per dirty chunk; `MapOccluder { chunk: UVec2 }` marks them.
- [ ] **Step 1: Write the failing tests**:
  - `tiled_collision_shapes_become_cells_after_flips` (a flipped slope tile turns its corner).
  - `int_grid_names_collide_by_default_and_overrides_apply`.
  - `a_layer_with_collides_false_adds_nothing`; `an_edit_updates_its_cells_before_the_next_step`.
  - `occluders_follow_tiles_and_sit_on_their_geometry`; `an_edit_rebuilds_only_its_chunk_s_occluders`.
  - `a_despawned_map_leaves_no_colliders_or_occluders` (Review Focus 3).
  - `a_character_runs_and_lands_on_a_loaded_tiled_level` (headless: physics with the map's colliders).
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: maps collide from their tiles' shapes or IntGrid names and cast 2D shadows, both kept in step with edits`.

### Task 12: Wang auto-tiling at runtime

**Files:**
- Create: `src/core/tilemap/wang.rs`.
- Modify: `src/core/tilemap/view.rs` (`set_terrain`).

**Interfaces:**
- Consumes: `WangSet`, `TileLayerData`.
- Produces: `MapsView::set_terrain(&mut self, map: Entity, layer: impl Into<LayerRef>, cell: IVec2, wang_set: &str, color: Option<&str>) -> bool` (`None` erases to no colour): the cell's corners and/or edges take the colour (by the set's kind), then the cell and its eight neighbours pick the tile whose `wangid` matches; ties pick by tile probability with a stable per-cell hash (the same map edits always give the same tiles); no match leaves the cell's tile as it was and warns once per set. `pub(crate) fn best_tile(set: &WangSet, wanted: [u8; 8], cell: IVec2) -> Option<u32>` pure.
- [ ] **Step 1: Write the failing tests**: `painting_grass_into_dirt_picks_edge_and_corner_tiles` (corner set; the 3x3 result equals a fixture painted in Tiled), `an_edge_set_and_a_mixed_set_pick_their_tiles`, `ties_pick_by_probability_and_are_stable`, `an_unmatched_pattern_keeps_the_tile_and_warns_once`.
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: code edits paint terrain through the Wang sets designers made in Tiled`.

### Task 13: Hot reload

**Files:**
- Create: `src/core/tilemap/reload.rs`.

**Interfaces:**
- Consumes: Task 6's spawned structure; `AssetEvent<TileMap>`, `AssetEvent<Tileset>`, `AssetEvent<LdtkProject>`, `AssetEvent<Image>` (through a `MessageCursor`; never `take_changes` on `Assets<Image>`, which `ImageSizes` follows).
- Produces: at the start of the next frame after a reload, every map spawned from the changed asset replaces its layers' grids and intgrids (chunks, colliders and occluders rebuild), and reconciles objects and class tiles by id (`pub(crate) fn reconcile(existing: &[(String, Entity)], new: &[MapObjectData]) -> Reconcile { keep, update, spawn, despawn }`): kept entities keep every component the game added; updated ones get new `Transform` and `MapObject`; class tiles reconcile by (layer, cell). A tileset image reload redraws only (its padded copy rebuilds). A failed reload changes nothing and sends no `MapEvent`. `MapEvent::Reloaded { map }` after each applied reload. The file wins over runtime edits made since it loaded.
- [ ] **Step 1: Write the failing tests** (`MemorySource::insert` over existing paths, hot reload on):
  - `saving_a_map_updates_its_tiles_and_keeps_unchanged_objects`; `a_moved_object_moves_and_keeps_game_components`; `new_and_removed_objects_spawn_and_despawn`.
  - `saving_a_tileset_or_its_image_redraws_every_map_using_it`; `saving_an_ldtk_level_file_reloads_its_project`; `saving_the_tiled_project_reapplies_class_defaults`.
  - `a_broken_save_keeps_the_last_good_map`.
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: saving a map, tileset, template, project or LDtk level updates the running game, keeping its objects' state`.

### Task 14: Worlds

**Files:**
- Create: `src/core/tilemap/world.rs`, `src/core/tilemap/tiled/world_file.rs`; fixtures (a `.world` with explicit maps and a pattern).
- Modify: `src/core/asset/mod.rs` (register `TileWorldLoader` for `world`).

**Interfaces:**
- Consumes: `WorldLayout`, `WorldLevel`, `Neighbour` (Task 5).
- Produces: `pub struct TileWorld { pub layout: WorldLayout, maps: Vec<Handle<TileMap>> }` (`impl Asset`; a Tiled world's maps load as dependencies; patterns expand by the ruling in Step 3; neighbours are computed for Tiled worlds from touching edges); `pub enum WorldRef { Tiled(Handle<TileWorld>), Ldtk { project: Handle<LdtkProject>, world: String } }` with `From<&Handle<TileWorld>>`; `WorldLayout::levels_in(&self, area: Rect) -> impl Iterator<Item = (usize, &WorldLevel)>`; `MapRoot::world_level(world: &Handle<TileWorld>, level: usize) -> MapRoot` and `MapRoot::ldtk_world_level(project, world, level)`; `MapsView::spawn_levels_in(&mut self, world: impl Into<WorldRef>, area: Rect) -> Vec<Entity>` spawning those not already spawned for that world (tracked by a `WorldLevelOf { world, level }` component) at their positions.
- [ ] **Step 1: Write the failing tests**: `a_tiled_world_places_its_maps_and_finds_neighbours`, `ldtk_worlds_place_levels`, `levels_in_an_area_spawn_once`, `a_pattern_with_no_match_fails`.
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement.** Ruling (controller's): Tiled world patterns match only names the registry can be asked for (no directory listing): a pattern names a numbered sequence, and the loader tries indices until the first missing file, up to 1,024.
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: Tiled world files and LDtk worlds place their levels, and a game spawns the ones near the player`.

### Task 15: Procgen grids as maps

**Files:**
- Modify: `crates/ryo2d-procgen/src/grid.rs` (or a new `tilemap.rs`), its prelude, `.claude/skills/ryo-procgen/SKILL.md` (+ copy), `examples/demos/procgen` stays as is.

**Interfaces:**
- Produces: `GenerationGrid::to_tile_map(&self, tile_size: UVec2, tileset: TilesetSource, tile_of: impl Fn(TileType) -> Option<u32>) -> TileMap` (one layer `"tiles"`) and `GenerationGrid::to_terrain_map(&self, tile_size: UVec2, tileset: TilesetSource, wang_set: &str, color_of: impl Fn(TileType) -> Option<&str>) -> TileMap` (colours resolved through Task 12's `best_tile` with every cell's corners set, so the dungeon gets edge tiles).
- [ ] **Step 1: Write the failing tests** in the crate: `a_dungeon_becomes_a_map_of_its_tiles`, `a_terrain_map_picks_wang_edge_tiles`.
- [ ] **Step 2: Run** `cargo test -p ryo2d-procgen`. Expected: fail.
- [ ] **Step 3: Implement**; update the skill's tile placement section to the conversion.
- [ ] **Step 4: Run** the tests and the gate.
- [ ] **Step 5: Commit** `feat: procgen grids turn into tile maps, with terrain edges from a Wang set`.

### Task 16: Editors, art and authored content

**Files:**
- Create: `examples/demos/tilemap/assets/` (Kenney art, Tiled maps, tilesets, a `project.tiled-project`, an LDtk project), `examples/demos/tilemap/assets/README.md` (sources and licences), `examples/demos/tilemap/scripts/make-maps.rs` or a `src/bin/make-maps.rs` (writes the scripted files).

**Interfaces:**
- Produces the files Tasks 17 and 19 load. The owner approved installing Tiled and LDtk (2026-10-29); Tiled from Homebrew (`brew install --cask tiled`), LDtk 1.5.3 from its GitHub release (macOS build), and the Kenney packs from kenney.nl: Pixel Platformer and Tiny Dungeon (CC0; keep their `License.txt`).
- [ ] **Step 1: Install** the editors and download the packs; record versions in the README.
- [ ] **Step 2: Script the files** in Tiled 1.12 and LDtk 1.5.3 formats: a platformer map (TMX, CSV and zlib layers, Wang corner set for ground, animated coin and water tiles, collision shapes including slopes and a one-way bridge, objects: `player_start`, `coin`, a `door` referencing a `switch`), a top-down map (TMJ, y-sorted trees, a parallax cloud layer, an image layer), a three-room Tiled world, an LDtk project (IntGrid `solid`/`one_way`/slope names with auto-layer rules over Tiny Dungeon, entities with fields, two levels with neighbours). Validate every Tiled file with `tiled --export-map json <in> <tmp>` (exits 0) and load each through the engine's loaders in a test.
- [ ] **Step 3: Leave for the owner's hand-play:** opening and saving each file in its editor (LDtk fills its auto-tiles then). Until then the LDtk level's auto-layer holds tiles the script resolved with the same rules, and the test in Step 2 pins what loads.
- [ ] **Step 4: Run** the gate.
- [ ] **Step 5: Commit** `chore: the tilemap demo's CC0 art and its Tiled and LDtk levels`.

### Task 17: The tilemap demo

**Files:**
- Create: `examples/demos/tilemap/{Cargo.toml,Ryo.toml,index.html,Trunk.toml}`, `src/{lib,main,embedded,shared}.rs`, one module per page, `tests/pages.rs`.

**Interfaces:**
- Consumes: everything above; demo-kit (`Demo`, `DemoPage`, `Settings`, `Control`); the audio demo's embedding pattern.
- Produces pages: 1 Tiled and LDtk side by side; 2 Flips and mixed tilesets; 3 Animated tiles (pause toggle); 4 Top-down (y-sorted trees, parallax clouds, image layer); 5 Collision (a character on slopes and a one-way bridge, collider outlines toggle); 6 Objects (coins spawn and are collected, the door opens from its referenced switch); 7 Editing (mouse paints terrain through the Wang set, erases, fills); 8 Procgen dungeon as a map (regenerate key); 9 World of rooms (walk between three rooms, levels spawned near the player); 10 Big map (1024x1024 four layers, camera flies; frame cost shown); 11 Hot reload (instructions; counts `MapEvent::Reloaded`).
- [ ] **Step 1: Write the failing tests** in `tests/pages.rs`: `every_page_plays_for_two_seconds_without_a_warning`, `every_page_plays_from_the_files_the_web_build_embeds`, `the_web_build_embeds_every_asset_file`, `the_objects_page_collects_a_coin`, `the_editing_page_paints_terrain`, `the_world_page_spawns_the_next_room`.
- [ ] **Step 2: Run** `cargo test -p tilemap-demo`. Expected: fail.
- [ ] **Step 3: Implement** the pages (keyboard and pad; reserved keys avoided; content left of 55% width on pages with settings).
- [ ] **Step 4: Run** the tests, the gate, `./dev demos:tour tilemap` (11 pages captured), and a web build (`trunk build --release` in the demo, scratch target dir).
- [ ] **Step 5: Commit** `feat: a tilemap demo shows Tiled and LDtk levels, collision, objects, editing, worlds, a big map and hot reload`.

### Task 18: Breakout's levels as Tiled maps

**Files:**
- Create: `examples/games/breakout/assets/levels/level{1..10}.tmx`, `examples/games/breakout/assets/levels/bricks.tsx`, `examples/games/breakout/assets/levels/bricks.png` (the bordered rects the game draws today, one tile each of 80x25 plus padding for the 84x29 grid), a converter `examples/games/breakout/src/bin/convert-levels.rs` (reads the `.txt` files once, writes the `.tmx`; kept for reference).
- Modify: `examples/games/breakout/src/levels/mod.rs` (load through the registry), `src/scenes/game.rs` (`spawn_blocks` spawns the map with `MapRoot::new(level).with_entity_tiles(&["brick"])` and turns each `MapTile` into today's brick entity), delete `examples/games/breakout/levels/*.txt` after conversion, the breakout tests' temp-folder setup.

**Interfaces:**
- Consumes: Tasks 2, 4, 6.
- Produces: each brick tile's class is `brick` with properties `color` (enum `BlockColor` names), `hits`, `points`; the map's cell grid is 84x29 px (`BLOCK_WIDTH + BLOCK_PADDING`, `BLOCK_HEIGHT + BLOCK_PADDING`) placed so cell centres equal today's block centres; the brick entity, its collider, restitution, friction, z and image are exactly today's. The seven Cyrillic cells (level 5 line 5 column 11: `О` → `O`; level 7 line 5 columns 2, 4, 6, 8, 10 and line 10 column 9: `Р` → `P`) convert to the bricks they were meant to be (the owner's call).
- [ ] **Step 1: Write the failing tests** in `examples/games/breakout/tests/levels.rs`: `every_level_spawns_the_bricks_its_text_file_held` (count, centre, colour, hits, points per brick, against a table the converter printed from the `.txt` files before their deletion, with the seven corrected cells), `levels_load_on_the_web_from_embedded_files`.
- [ ] **Step 2: Run** `cargo test -p breakout`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, the gate, and a scripted hand-play: levels 1-4, 6, 8-10 played headless for 600 frames with a fixed input script on the branch and on main produce identical frames (`RYO_CAPTURE_*`).
- [ ] **Step 5: Commit** `feat: breakout's levels are Tiled maps whose bricks spawn as the game's blocks, and levels 5 and 7 get their missing bricks`.

### Task 19: The character demo's level from LDtk

**Files:**
- Modify: `examples/demos/character/main.rs` (the platformer page loads its level from LDtk; the other pages stay), add `examples/demos/character/assets/` (the LDtk project from Task 16 trimmed to this level, Tiny Dungeon tiles), embedding for the web (the demo loads files now), `index.html`/`Trunk.toml` unchanged.

**Interfaces:**
- Consumes: Tasks 5, 6, 8, 11.
- Produces: the `PLATFORMER_LEVEL` rows (`#` solid, `=` one-way, `/` and `\` slopes) as an LDtk IntGrid with values `solid`, `one_way`, `slope_bottom_right`, `slope_bottom_left`, auto-tiled art, at `ORIGIN` with `TILE` 16; the map's `TileColliders` cells equal the old `level(&PLATFORMER_LEVEL)` output exactly.
- [ ] **Step 1: Write the failing test** `the_ldtk_level_collides_exactly_as_the_ascii_level_did` (compares every cell).
- [ ] **Step 2: Run.** Expected: fail.
- [ ] **Step 3: Implement**; the collider outline toggle draws over the art.
- [ ] **Step 4: Run** the tests, the gate, `./dev demos:tour character`, and a scripted hand-play of the platformer page on branch and main (same character positions frame by frame for 600 frames of scripted input).
- [ ] **Step 5: Commit** `feat: the character demo's platformer level is an LDtk level with art, colliding as before`.

### Task 20: Speed targets and guards

**Files:**
- Create: `benches/tilemap.rs` (guards in the 5b style: `guards::runs`, `miss`, `verdict`), `tests/tilemap_alloc.rs`, `docs/backlog/tilemap.md` (measured numbers, method as `docs/backlog/assets.md`).
- Modify: `Cargo.toml` (`[[bench]] name = "tilemap" harness = false`).

**Interfaces:**
- Produces guards, each printing its median and verdict: `static_frame` (1024x1024, four layers, 16 px tiles, 1920x1080 view, camera moving; CPU frame time minus an empty frame's; target 50 µs), `edit` (one tile set on a map with loops spanning up to four chunks: chunk rebuild, loop rebuild and occluder rebuild, each timed; target 100 µs each), `load` (256x256 four layers, base64+zlib TMX, off the main thread; target 20 ms; and the landing frame's extra time under 0.5 ms). `tests/tilemap_alloc.rs`: `a_frame_drawing_a_static_map_allocates_nothing`.
- [ ] **Step 1: Write the bench and the allocation test.**
- [ ] **Step 2: Run** `cargo bench --bench tilemap` (scratch target dir) and `cargo test --test tilemap_alloc`. Expected: either meets its target or the report names the miss; a miss is fixed in this task (profile with the guard's profile command), or reported to the controller with numbers if the fix belongs to an earlier task.
- [ ] **Step 3: Record** the numbers in `docs/backlog/tilemap.md`.
- [ ] **Step 4: Run** the gate.
- [ ] **Step 5: Commit** `test: tilemap speed guards for a static big map, an edit and a load`.

### Task 21: Documentation and skills

**Files:**
- Create: `.claude/skills/ryo-tilemap/SKILL.md` (+ `src/cli/skills/ryo-tilemap.md`, an `EMBEDDED_SKILLS` entry in `src/cli/commands/install_skills.rs` whose description equals the frontmatter's).
- Modify: `src/core/tilemap/AGENTS.md` (complete), `src/core/render/AGENTS.md` (chunks, the slot uniform, y-sorted rows), `src/core/physics/AGENTS.md` (shapes, loop-local rebuilds), `src/core/asset/AGENTS.md` (the new types and loaders), `src/core/lighting/AGENTS.md` (tile occluders), the skills `ryo-api`, `ryo-character`, `ryo-physics`, `ryo-game-dev`, `ryo-engine-dev`, `ryo-assets` (+ copies, versions), `docs/backlog/tilemap-features.md` (6b's scope: isometric, hexagonal, staggered and infinite maps, oblique; what stays later), `docs/architecture/overview.md` (the tilemap's place).

**Interfaces:**
- Produces: `ryo-tilemap` teaches the workflow from Tiled or LDtk to `MapRoot`, layers and properties, objects and class tiles, editing and Wang terrain, collision and occluders, y-sorting and parallax, worlds, hot reload, the web, and testing; its Rust examples compile.
- [ ] **Step 1: Write** the docs and skills.
- [ ] **Step 2: Run** `cargo check -p skill-examples` and `cargo test -p ryo2d --lib cli::` (embedded skills match).
- [ ] **Step 3: Run** the gate.
- [ ] **Step 4: Commit** `docs: the ryo-tilemap skill, and the module notes and skills for tilemaps`.
