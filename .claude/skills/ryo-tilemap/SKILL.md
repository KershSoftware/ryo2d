---
name: ryo-tilemap
description: "Tile maps - the workflow from Tiled (.tmx, .tmj) or LDtk (.ldtk) through assets/ to a MapRoot entity, layers and the reserved layer properties (collides, occludes, y_sort, lit), tilesets and typed tile data, objects and class tiles as entities, MapEvent, reading and editing tiles with ctx.maps() and Wang terrain, collision from tile shapes and IntGrids, tile shadows, y-sorting and parallax, worlds of levels, maps built in code and by procgen, hot reload, the web, performance and testing."
metadata:
  version: 1.2.0
---

# Ryo2D Tile Maps

Levels are made in an editor, Tiled or LDtk, and handed over as files. The engine reads both editors' files itself, through the asset registry: a level loads by asset path like an image, plays on the web from the files a build embeds, and follows every save while the game runs. A `MapRoot` entity spawns a map. Its tile layers draw in cached chunks through the world batch, so lights, cameras, `RenderLayers` and z order apply to them as to sprites; its tiles collide and cast 2D shadows from the shapes the designer drew; its objects arrive as entities; and code reads and edits its tiles, with the designer's Wang sets picking edge and corner tiles.

Four registry assets do the work, each loaded by asset path (`ryo-assets`):

| Asset | Files | What it is |
|---|---|---|
| `TileMap` | `.tmx` (XML) and `.tmj` (JSON): Tiled maps | Layers over tilesets, with properties; a map can also be built in code |
| `Tileset` | `.tsx` and `.tsj`: Tiled's external tilesets | Tiles in one atlas image or an image each, with each tile's class, properties, collision shapes and animation, and the Wang sets |
| `LdtkProject` | `.ldtk`, with its `.ldtkl` level files | Every level of every world, the worlds' layouts and the tilesets |
| `TileWorld` | `.world`: a Tiled world | Maps placed in one world, with their neighbours |

The loaders also read Tiled's object templates (`.tx`, `.tj`) and its project file (`.tiled-project`) as the maps need them; a game never loads those itself. Every type here is in the prelude; complete signatures are in `ryo-api` (Tilemaps). The worked example is the tilemap demo, `examples/demos/tilemap` (`./dev.sh tilemap`), in eleven pages: a Tiled map and an LDtk level side by side, flips and a layer of two tilesets, animated tiles, a top-down garden (y-sorted trees, parallax clouds, an image layer), collision with slopes and a one-way bridge, objects given behaviour, painting terrain, a procgen dungeon, a world of rooms, a 1024 x 1024 map, and hot reload. The tile grids demo (`examples/demos/tile-grids`) shows a Tiled infinite map with chunks at negative cells, and an infinite map built in code that grows wherever the mouse paints. The character demo's platformer level is an LDtk level (`examples/demos/character`), and breakout's ten levels are Tiled maps whose bricks are class tiles (`examples/games/breakout`).

## 1. From the editor to the game

1. **Make the level** in Tiled (1.12) or LDtk (1.5).
   - **Tiled:** an orthogonal map, of a fixed size or infinite. Save maps as `.tmx` or `.tmj`, and tilesets inside the map or as `.tsx` / `.tsj` files. Every layer format Tiled writes loads: CSV, Base64 uncompressed or compressed with gzip, zlib or zstd, and the TMX's `<tile>` elements. Isometric, staggered, hexagonal and oblique maps fail to load with an error saying so (they arrive in sub-project 6b), as does a layer blend mode other than normal, add, multiply or screen.
   - **A Tiled infinite map** loads too: each tile layer's chunks land where Tiled saved them, at negative cells as well, whatever the map's output chunk size, in every layer format. The map's `infinite` is true and its `size` zero; `TileMap::bounds()` gives the cells its tile layers cover: the smallest rectangle holding every cell with a tile, for camera limits and world rectangles. Its tile layers are unbounded grids (section 11), so an edit past its chunks makes new ones. A chunk whose cells' pixels (cell times tile size) would pass what a 32-bit integer holds fails the load, naming the layer and the chunk.
   - **LDtk:** a project of one world or several, its levels inside the `.ldtk` file or saved beside it as `.ldtkl` files ("Save levels to separate files"). LDtk runs its auto-layer rules in the editor and saves the tiles they place; the game draws what was saved. A project saved by an LDtk newer than 1.5 fails to load, naming both versions.
2. **Put the files under `assets/`**, in any folders: `assets/maps/level1.tmx`, `assets/tilesets/ground.tsx`, `assets/images/ground.png`. A path written inside a file (a tileset, an image, a template, an LDtk level file) is read from that file's folder, as the editors write them; one that leaves `assets/` fails the load.
3. **Spawn it** with a `MapRoot`. The entity's `Transform` places the map's cell (0, 0): its top-left corner, or an infinite map's tile origin, its chunks lying all around it.
4. **React when it lands.** Once its files are loaded, a map's colliders and shadows are worked out on a thread of their own, and then it arrives whole, in one frame: its layers, objects, class tiles, colliders and shadows, with a `MapEvent::Spawned`. That is when to give objects their behaviour and put the player down (or as each object arrives, with an observer: section 5).

```rust
use ryo2d::prelude::*;

#[derive(Default)]
struct Level {
  map: Option<Entity>,
  player: Option<Entity>,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Loads on the workers; the map lands a frame or so after its files do
    let level = ctx.assets().load::<TileMap>("maps/level1.tmx");
    let root = MapRoot::new(level).with_entity_tiles(&["spikes"]);
    self.map = Some(ctx.world.spawn((Transform::from_xy(0.0, 0.0), root)).id());
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    for event in ctx.messages::<MapEvent>() {
      match event {
        MapEvent::Spawned { map } if Some(map) == self.map => self.landed(ctx, map),
        MapEvent::Failed { error, .. } => game_warn!("the level did not load: {}", error.report()),
        _ => {}
      }
    }
    SceneAction::Continue
  }
}

impl Level {
  /// The designer's objects get their behaviour, and the player starts at
  /// the point marked `player_start`
  fn landed(&mut self, ctx: &mut SceneContext, map: Entity) {
    let mut objects = ctx.world.query::<(Entity, &MapObject, &GlobalTransform)>();
    let mut start = None;
    let mut coins = Vec::new();
    for (entity, object, global) in objects.iter(ctx.world) {
      if object.map != map {
        continue;
      }
      match object.class.as_deref() {
        Some("player_start") => start = Some(global.translation()),
        Some("coin") => coins.push(entity),
        _ => {}
      }
    }
    for coin in coins {
      ctx.world.entity_mut(coin).insert((Collider::circle(6.0).as_trigger(), Pickup::new()));
    }
    if let Some(at) = start {
      let player = ctx.world.spawn((
        Transform::from_translation(at),
        Collider::rect(12.0, 22.0),
        PlatformerMovement::default(),
        MoveBindings::default(),
      ));
      self.player = Some(player.id());
    }
  }
}
```

- An LDtk level spawns the same way, named by its identifier or iid: `MapRoot::ldtk(ctx.assets().load::<LdtkProject>("ldtk/world.ldtk"), "Level_0")`.
- `ctx.maps().is_ready(map)` says whether a map has landed, for code that polls instead of reading messages.
- **Put characters where the designer marked**, at a `player_start` object as above, not by casting a ray down on `Spawned`. The colliders come with the map and physics takes them in during the next fixed step, before it moves anything, so a character placed on `Spawned` stands on its ground from its first move. Physics queries (`ctx.physics().raycast` and the rest) see the world as the last step left it, though, so a ray cast in the `update` that reads `Spawned` misses the map.

## 2. A map in the world

`MapRoot` is a component; its builders say how the map spawns:

| Builder | What it does |
|---|---|
| `MapRoot::new(Handle<TileMap>)` | A Tiled map, or one built in code |
| `MapRoot::ldtk(Handle<LdtkProject>, level: &str)` | A level of an LDtk project, by identifier or iid |
| `MapRoot::world_level(&Handle<TileWorld>, level)`, `MapRoot::ldtk_world_level(project, world: &str, level)` | A level of a world, by its index in the world's layout (section 12) |
| `.with_entity_tiles(&["brick", "spikes"])` | Tiles of these tileset classes spawn as entities instead of drawing (section 5) |
| `.with_intgrid_collision(key, TileCell)` | An LDtk IntGrid value, by number or name, collides as this cell kind (section 8) |
| `.without_collision()` | Nothing of the map collides; its layers still cast shadows as their flags say |
| `.with_z(z)` | The bottom layer draws at z, each layer above one higher (default 0) |

What lands, all of it children of the map entity:

- **A child per layer**, bottom first, with `MapLayerRef { map, index, name }`, the layer's `LayerInfo` as a component, and a `Transform` at the layer's offset; a tile layer also has `TileLayerData` (its cells), an image layer `ImageLayer`.
- **Objects**, children of their object layer, each with a `MapObject` (section 5); **class tiles**, children of their tile layer, each with a `MapTile`.
- **The colliders** (children of the map, or of a layer that collides on its own) and **the shadow chains** (`MapOccluder`, children of the first occluding layer), which the map keeps in step with its tiles.
- `MapIds` on the map entity: its objects' entities by their editor id.

Then:

- **Move the map** with its `Transform`: drawing and cell conversions follow at once, and its colliders, which have no body, are put where it now is. For a map that moves while things stand on it (an airship), give the map entity `RigidBody::kinematic()`: its colliders join that body and it carries what stands on it.
- **Disable it** (`Disabled` on the map entity): it draws nothing, and its colliders and shadows go; enabled, they are built again from its tiles as they then are, on the main thread in that frame: a few milliseconds for a 256 x 256 four-layer map, about 70 ms for 1024 x 1024. Despawn a big level rather than park it, or enable it behind a loading screen. `Disabled` does not pass from the map to its layers: the map's systems check the map entity. A layer entity disabled on its own stops drawing, colliding and casting shadows; to hide a layer and keep its collision, set its `LayerInfo::visible` to false.
- **Unload it** by despawning the map entity: everything it spawned goes with it. `SceneCleanup::WORLD` does this at a scene switch, as for any entity. Putting another `MapRoot` over it, or removing it, takes the old map down first, and the new root spawns afresh.

`MapEvent` is a message (`ctx.messages::<MapEvent>()`, `MessageReader<MapEvent>` in systems):

| Variant | When |
|---|---|
| `Spawned { map }` | It landed whole this frame |
| `Reloaded { map }` | It followed a saved file, in the frame the save landed (section 13) |
| `Failed { map, error: Arc<AssetError> }` | Its asset did not load, its LDtk project has no such level, its world has no such level, or working out its landing failed: nothing spawned. Sent once for each failure; saving the file fixed spawns it |

## 3. Layers and their properties

What the editor gives a layer carries over:

- **Order:** bottom first (`MapLayerRef::index`). Tiled's group layers flatten into their children: offsets add, opacity and tint multiply, parallax multiplies, visibility needs every group visible, and a group's blend mode passes down unless a child has its own. LDtk lists its top layer first; it draws last here too.
- **Opacity, tint** (Tiled), **offset, visibility, blend mode, parallax** (section 10). A hidden layer draws nothing but still collides and casts shadows as its flags say: a collision-only layer is a hidden tile layer.
- **Image layers** draw their picture, repeated across or down where Tiled's repeat settings say. One left without a picture (Tiled makes a new image layer so) loads with `ImageLayer::image` `None` and draws nothing. A transparent colour set on its image (`ImageLayer::transparent`) draws clear.
- **LDtk:** an IntGrid layer draws its auto-tiles and collides by its values; each layer keeps its own grid size, so a level may mix an 8 px collision grid with 16 px art.
- **LDtk's stacked, offset and translucent tiles** draw as the editor shows them. Rules and stamps stack several tiles in a cell, a rule can move its tiles off the grid or fade them, and a layer holds one tile a cell, so such a layer loads with sub-layers: one for each distinct stack depth, offset and alpha its tiles need, named `Ground#1`, `Ground#2` and on, with `LayerInfo::part_of` naming the layer's editor id. They are parts of the layer, not layers of their own: each draws with the layer's settings as they are now (visibility, `y_sorted`, `lit`, parallax, blend, tint, its entity's `RenderLayers` and `Disabled`), at its z, just after it, its own offset and opacity over the layer's, so a game sets a flag on the layer alone and the whole layer follows, and a y-sorted layer's stacked tiles sort with a character as its own do. They are listed after the editor's layers in `MapLayers`, so every layer keeps the index and z LDtk's order gives it whatever stacking a save adds. The IntGrid and the collision are the layer's own. Edits by the layer's name (`set_tile`, `fill`, `set_terrain`) replace the whole cell, clearing what its sub-layers held there, and `tile` reads the layer's own tile. A layer has 16 sub-layers at most: past that, the tiles left over draw in the nearest, and the dev log says how many.

Four layer properties are reserved; the engine reads them into the layer's `LayerInfo`:

| Property | Type | Default | What it does |
|---|---|---|---|
| `collides` | bool | true for a Tiled tile layer, false for other layers | Its tiles collide (section 8) |
| `occludes` | bool | left out: as `collides` | Its tiles cast 2D shadows (section 9) |
| `y_sort` | bool | false | Its tiles sort with the characters by y, row by row (section 10) |
| `lit` | bool | true | Lights and the ambient shade it; false draws it at full brightness |

- **In Tiled:** select the layer and, in its Properties, add a custom property of type bool with the name. Properties on a group layer are its own and do not pass to its children: set them on the tile layers. A layer class from the Tiled project (section 4) can give these as its members' defaults.
- **In LDtk:** layers have no custom properties. IntGrid layers collide, by the names of their values (section 8); every other layer neither collides nor occludes; no layer is y-sorted, and every layer is lit. To change a flag, set it on the layer entity's `LayerInfo` once the map has landed: drawing, collision and shadows read it from there and rebuild. A reload of the file puts the file's flags back.

<!-- given: map: Entity -->
```rust
// The LDtk level's "Trees" layer (its stacked tiles with it) sorts with the characters
let mut layers = ctx.world.query::<(&MapLayerRef, &mut LayerInfo)>();
for (layer, mut info) in layers.iter_mut(ctx.world) {
  if layer.map == map && layer.name == "Trees" {
    info.y_sorted = true;
  }
}
```

Other properties the engine reads, on a layer or on the map (a layer's win over the map's):

| Property | Type | Default | What it does |
|---|---|---|---|
| `collision_layer` | int | 1 | The colliders' collision layer bits (`ryo-physics`, Layers and masks) |
| `collision_mask` | int | every bit (-1) | What they collide with |
| `friction` | float | 0.5 | |
| `restitution` | float | 0 | |

And on a tileset's collision shapes (the objects in Tiled's tile collision editor):

| Property | Type | What it does |
|---|---|---|
| `one_way` | bool | The shape is solid only from above: things jump up through it and land on it |
| `occluder` | bool | The shape blocks light and does not collide |

A material property of the wrong type is warned once, and what is under it counts (the map's, else the default); a reserved flag of another type than bool counts as left out. All other properties are the game's, read from `LayerInfo::properties`, `TileMap::properties`, a tile's data or an object.

## 4. Tilesets and tile data

- **A cell holds a `Tile { tileset, id, flip }`:** `tileset` indexes the map's tilesets (`TileMap::tilesets`), `id` is the tile in it, and `flip` is its `TileFlip`, the flips as Tiled stores them: a diagonal flip first, then horizontal, then vertical (a 90° turn is a diagonal and a horizontal flip; `TileFlip::rotated_cw()` turns one). A layer may mix tilesets, and every tile keeps the flips and turns the editor gave it.
- **What a tile carries:** `ctx.maps().tile_data(map, tile) -> Option<TileData>` gives its `class`, `properties`, `shapes` (its collision shapes, each `TileShape { points, one_way, occluder, properties }` in tile pixels from the top-left), `animation` (`TileFrame { tile, duration }`), `probability` and `image` (a collection tileset's own picture). A loaded `Tileset` reads the same with `tile(id)`.
- **Typed properties:** `Properties` reads `bool`, `int` (an `i64`; a whole float counts), `float` (an `f64`; an int counts), `string` and `color` by name, and `get(name) -> Option<&PropertyValue>` for the rest: `File(path)` as written, `Object(id)` (a reference to an object), `Enum { ty, value }`, `Class { ty, fields }` and `List` (Tiled 1.12's list properties, each item read by its own type, nested lists too, and an enum's flags). They keep the order they were written in (`iter()`).
- **Classes and enums from the Tiled project.** Tiled keeps custom classes, enums and the members' defaults in its project file, and a map it saves writes only the members a designer changed. Put the project at `assets/project.tiled-project`, or name it in `Ryo.toml` (`[tilemap] tiled_project = "tiled/game.tiled-project"`) or with `EngineBuilder::tiled_project(path)`: a member a file leaves out then takes the project's default (a list member's too; a list the file writes replaces it whole), nested classes layer, and enums read by name (flags as a `List`). Without it what the file wrote is kept, and the dev log names the class once per file. Saving the project reloads the maps. Classes holding members of classes multiply: one thing's properties may resolve to at most 65,536 values and a file's to 4,194,304, past which the map fails to load, naming the place.
- **LDtk** writes every field with its type in the project, so nothing more is needed for entities and levels. A tileset's per-tile data reads as Tiled's tile classes and properties do, so the same game code serves both editors: a tile's enum tags (the tileset's tag enum, "Enum tags" in LDtk's tileset panel) give it its class, the first of its tags in the enum's order (one tag is the usual case, and `with_entity_tiles` and class checks then work for LDtk levels too), and a bool property `true` for each of its tags (for a tile with several: `data.properties.bool("Water")`); its custom data, LDtk's one free text a tile, is the string property `custom_data`, as written (parse it yourself if it holds JSON).
- **Animated tiles** (Tiled's tile animation editor) play on the game clock: they pause with the game and slow with its speed, and nothing is rebuilt as they play. Frames of different lengths keep their lengths, unless repeating them to a common beat would take over 128 cells of the padded tileset, when every frame takes the mean length.
- **No bleeding at any zoom:** each tileset is drawn from a padded copy made when a map first needs it, each tile's edges stretched outward, so no filter, mip level or sub-pixel camera samples a neighbour's texels. A tileset's transparent colour (Tiled's, on its image: `Tileset::transparent`) draws clear, keyed out in that copy.
- **What loads otherwise than Tiled or LDtk shows it is warned**, once per file for each feature, naming its first place: a Tiled tileset's tile render size "grid" (its tiles draw at their own size), a collection tile drawing part of its image (it draws all of it), a render order other than right-down (it differs only where tiles reach past their cells), a top-down object layer two of whose tile objects overlap in the other order from their y (they draw in file order), a polyline among a tile's collision shapes (it has no inside: ellipses and capsules collide as their outlines); an LDtk entity whose tile LDtk draws otherwise than stretched over its box (`Repeat`, `NineSlice`, or fitted into a box of another shape), and a parallax layer LDtk also scales (it moves, keeping its size).

## 5. Objects and class tiles

**Objects** are Tiled's object layers (rectangles, points, ellipses, capsules, polygons, polylines, text, tile objects and instances of templates) and LDtk's entities. Each spawns as a child of its object layer:

- `MapObject { map, id, name, class, shape, size, tile, visible, properties }`, and a `Transform` at the centre of its rectangle (a point at its point; a polygon or polyline at the origin of its points, which are relative to it), turned by its rotation.
- `id` is Tiled's object id, or LDtk's iid, as a string, unique in the map; `MapIds` on the map entity finds an object's entity by it (`get(id)`). `class` is Tiled's Class (Type, in older files) or LDtk's entity identifier, which is its `name` too.
- An object hidden in the editor spawns all the same, with `visible: false` (games use hidden markers); it does not draw.
- A tile object draws its tile, with its flips and animation, at its place and size, on the layer's z. Set `MapObject::tile` to show another.
- **A Tiled tile object takes what its tile carries**, as Tiled shows it: with no class of its own it takes its tile's class, and its own properties lie over its tile's (which hold the tile class's defaults). So a coin placed from a tile of class `coin` arrives as a `coin` with the tile's `value`. This happens as the map lands (an external tileset is not loaded while its map loads), so the asset's `MapObjectData` holds only what the file wrote. An object with a class of its own keeps all the tile's properties under its own, the tile class's defaults among them, where Tiled keeps only those the tile wrote. An LDtk entity's tile is its picture in the editor and gives it nothing.
- An object reference (Tiled's object property type, LDtk's entity reference) reads as `PropertyValue::Object(id)`, and `object.reference("switch", ctx.world)` gives that object's entity (`None` once it is despawned).
- LDtk fields: a point reads as a `Class` "Point" with `cx` and `cy`, a tile as a `Class` "Tile" with `tileset`, `x`, `y`, `w` and `h`, an enum as `Enum`, an array as `List`; an unset field is left out.

Give objects behaviour in one of three places, each seeing the objects a save adds too:

- **As each arrives**, with an observer of `Add<MapObject>` (`On<Add<MapObject>>`). It runs for each object as its `MapObject` goes on, its fields filled. The map's layers, colliders and shadows are in by then, and so is every object's entity, so `reference` resolves; a body put down there stands on the map's ground. Objects later in the file have no `MapObject` yet, class tiles come after the objects, and physics queries see nothing of the map until its next step. An object a save changes fires `Insert<MapObject>` only, and `Remove<MapObject>` fires as an object goes. Observers outlive scenes: add one in `on_enter` and despawn it in `on_exit` (`ryo-messages`, Observers).
- **When the map lands** (section 1), reading `MapEvent::Spawned`.
- **With a system** over `Added<MapObject>`, which sees them at its next run.

<!-- self: observer: Option<Entity> -->
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  let observer = ctx.world.add_observer(
    |add: On<Add<MapObject>>, objects: Query<&MapObject>, mut commands: Commands| {
      if objects.get(add.entity).is_ok_and(|o| o.class.as_deref() == Some("coin")) {
        commands
          .entity(add.entity)
          .insert((Collider::circle(6.0).as_trigger(), Pickup::new()));
      }
    },
  );
  self.observer = Some(observer.id());
}

fn on_exit(&mut self, ctx: &mut SceneContext) {
  if let Some(observer) = self.observer.take() {
    ctx.world.despawn(observer);
  }
}
```

The same as a system:

```rust
fn coins_arrive(mut commands: Commands, objects: Query<(Entity, &MapObject), Added<MapObject>>) {
  for (entity, object) in &objects {
    if object.class.as_deref() == Some("coin") {
      commands
        .entity(entity)
        .insert((Collider::circle(6.0).as_trigger(), Pickup::new()));
    }
  }
}
// In the level's on_enter: ctx.add_systems(Phase::FixedUpdate, coins_arrive).ok();
```

**Class tiles** are tiles with behaviour of their own: a breakable brick, spikes, a door. Give the tile a Class in Tiled's tileset editor (in LDtk, an enum tag: section 4), and name the classes in `MapRoot::with_entity_tiles`. Each such tile is taken out of its layer, so the map neither draws nor collides it, and spawns as a child of the layer at its cell's centre with `MapTile { map, layer, cell, tile, class, properties }` (the properties are its tileset's). The game gives it what it needs; breakout reads its brick tiles' colour, hits and points this way, and the tilemap demo gives its spikes their art and a kill zone.

<!-- with: #[derive(Component)] struct Brick { hits: i64 } -->
<!-- given: map: Entity -->
```rust
let mut tiles = ctx.world.query::<(Entity, &MapTile)>();
let bricks: Vec<(Entity, i64)> = tiles
  .iter(ctx.world)
  .filter(|(_, tile)| tile.map == map && tile.class == "brick")
  .map(|(entity, tile)| (entity, tile.properties.int("hits").unwrap_or(1)))
  .collect();
for (entity, hits) in bricks {
  ctx.world.entity_mut(entity).insert((Collider::rect(32.0, 16.0), Brick { hits }));
}
```

- `ctx.maps().set_tile` and `fill` place class tiles the same way: a class tile spawns a `MapTile` on its cell. Placing another tile on that cell, or clearing it, despawns it.
- `ctx.maps().tile(..)` reads `None` on a class tile's cell: the entity is the tile.

## 6. Reading and editing tiles

`ctx.maps()` is a `MapsView` over the world (in a system or a test with the world: `MapsView::new(world)`). Every call names the map entity and, where it reads cells, a layer by name (the first so called) or by index (bottom first, a `usize`). A map that has not landed, a layer it does not have, a layer that is not a tile layer, or a cell outside the layer reads `None` (or `false`, or 0) and changes nothing. An unbounded layer (section 11) has no outside: its cells are signed, and `set_tile`, `fill` and `set_terrain` take any cell, negative ones included, making the 32 x 32-cell chunk a tile goes in, which then draws, collides and casts shadows. Two writes are refused there, changing nothing: a tile `fill` that would make more than 131,072 chunks (the 2^27 cells a map may hold; `IVec2::MIN` to `IVec2::MAX`, say) returns 0 and the dev log warns once a layer, and `set_terrain` within a few cells of the ends of `i32` (the cells its paint reaches would not fit) returns false.

| Method | |
|---|---|
| `is_ready(map) -> bool` | it has landed |
| `tile(map, layer, cell: IVec2) -> Option<Tile>` | the tile at a cell |
| `set_tile(map, layer, cell, Option<Tile>) -> bool` | put a tile, or clear it with `None`; false if nothing changed |
| `fill(map, layer, min: IVec2, max: IVec2, Option<Tile>) -> usize` | every cell from `min` to `max`, both included, clipped to a bounded layer; how many changed (0 for a tile fill that would make more than 131,072 chunks of an unbounded layer) |
| `set_terrain(map, layer, cell, wang_set: &str, color: Option<&str>) -> bool` | paint a Wang colour (section 7) |
| `intgrid(map, layer, cell) -> Option<i32>` | an LDtk IntGrid value; `None` where it is 0 |
| `set_intgrid(map, layer, cell, value: i32) -> bool`, `fill_intgrid(map, layer, min, max, value) -> usize` | change IntGrid values (0 empties), and so what the layer collides with (section 8) |
| `tile_data(map, tile) -> Option<TileData>` | what the tile's tileset says about it |
| `world_to_cell(map, layer, world: Vec2) -> Option<IVec2>`, `cell_to_world(map, layer, cell) -> Option<Vec2>` | through the map's and the layer's transforms; a cell's centre |
| `layer_entity(map, layer) -> Option<Entity>` | the layer's entity |
| `bounds(map) -> Option<CellRect>`, `layer_bounds(map, layer) -> Option<CellRect>` | the cells the map, or a tile layer, covers as it is now: a bounded one's size; an unbounded one's cells holding tiles, class tiles included, following every edit (empty with none; it looks at every chunk, so on a map grown to many chunks read it when the map changes, not every frame) |
| `spawn_levels_in(world, area: Rect) -> Vec<Entity>`, `spawn_levels_in_with(world, area, depth, configure)` | the levels of a world near a place, the second setting each root up (section 12) |

<!-- self: map: Entity -->
```rust
let (x, y) = ctx.input.mouse_position();
let clicked = ctx.input.mouse_pressed(MouseButton::Left);
let at = ctx.camera().screen_to_world(Vec2::new(x, y));
let mut maps = ctx.maps();
if clicked && let Some(cell) = maps.world_to_cell(self.map, "ground", at) {
  // Dig out a breakable tile: its tileset says which are
  let breakable = maps
    .tile(self.map, "ground", cell)
    .and_then(|tile| maps.tile_data(self.map, tile))
    .is_some_and(|data| data.properties.bool("breakable") == Some(true));
  if breakable {
    maps.set_tile(self.map, "ground", cell, None);
  }
}
// A wall of tile 7 of the map's first tileset, mirrored
let brick = Tile::new(0, 7).with_flip(TileFlip::HORIZONTAL);
maps.fill(self.map, "walls", IVec2::new(2, 2), IVec2::new(9, 3), Some(brick));
```

- **An edit costs what it touches.** It redraws only its 32 x 32-cell chunk, and re-collides and re-shadows only what changed, in time for that step: an edit made in `update` collides in that step's physics, and casts its shadow that frame.
- **Conversions use the transforms as they are now**, so a map moved this frame converts at once; `world_to_cell` gives cells outside the layer too, and `None` only for a map scaled to nothing.
- **Edits change the spawned map, not its asset:** two maps from one file keep their own tiles, and a reload of the file brings dug tiles back (section 13).
- `TileLayerData { grid, intgrid, cell_size }` on a layer entity is the map's own copy of the layer: editing its `grid` (a `TileGrid`: `get`, `set`, `fill`, `clear`) draws, collides and casts as `ctx.maps()` does, though without class tiles. Change IntGrid values with `set_intgrid` and `fill_intgrid`, which re-collide only the chunks they change; values written into `TileLayerData::intgrid` by hand are not seen until those chunks change.

## 7. Wang terrain

`ctx.maps().set_terrain(map, layer, cell, set, Some(color))` paints a cell with a colour of a Wang set the designer made in Tiled (Terrain Sets in the tileset editor), as Tiled's terrain brush does with its corrections on; `None` erases. The cell and the cells sharing its corners or edges pick the tiles that fit what their neighbours show, flipped or turned where the tileset allows it (Tiled's tileset transformations), and a change that leaves a neighbour disagreeing passes on to it, as far as the set needs.

<!-- self: map: Entity -->
```rust
let (x, y) = ctx.input.mouse_position();
let (paint, erase) = (ctx.input.mouse_down(MouseButton::Left), ctx.input.mouse_down(MouseButton::Right));
let at = ctx.camera().screen_to_world(Vec2::new(x, y));
let mut maps = ctx.maps();
if let Some(cell) = maps.world_to_cell(self.map, "ground", at) {
  if paint {
    maps.set_terrain(self.map, "ground", cell, "terrain", Some("grass"));
  } else if erase {
    maps.set_terrain(self.map, "ground", cell, "terrain", None);
  }
}
```

- Corner, edge and mixed sets all paint. A corner set over art drawn on the cells' edges paints a 3 x 3 block for each cell: the painted cell is the block's middle, so ground is at least two cells thick. The tilemap demo's Editing page (page 7) paints this way.
- **Results match Tiled's** on everything checked against it, with one difference: among tiles that fit equally well, Tiled draws one at random and the engine by a hash of the cell, so it picks the same tile there every time, but not Tiled's.
- A cell the set has no tile for keeps its tile, and the set warns once per version of its tileset (an incomplete set); so does erasing in a set with no tile for a bare corner or edge.
- A class tile keeps its colours out of its neighbours' corrections, so painting beside one leaves a seam; painting its own cell replaces it.
- **Code can ask a set itself:** a tileset's sets are `Tileset::wang_sets` (`wang_set(name)`), read through `name()`, `kind()`, `colors()`, `tiles()` and `transformations()`; `color_index(name)` gives a colour's index, and `best_tile(wanted: [u8; 8], cell) -> Option<(u32, TileFlip)>` the tile and flip closest to a wangid (its eight positions clockwise from the top edge, 0 for no colour, `WangSet::ANY` for any). A call is about 16 µs for a 255-tile set that allows every flip.
- **Procgen:** `GenerationGrid::to_terrain_map` (`ryo-procgen`) builds a map through a Wang set. Where cells of two colours meet, the higher colour index covers the lower, so the order of the colours in Tiled is the designer's control over which grows into which.

## 8. Collision

A layer collides when its `collides` is set (section 3) and its map has collision.

**What a Tiled tile layer collides with** is each tile's collision shapes, from Tiled's tile collision editor, placed where the tile draws (its flips and the tileset's tile offset turn and move them):

- a rectangle covering the cell is solid (one-way if marked `one_way`);
- a right triangle on three of the cell's corners is a 45° slope;
- any other shape (a half block, a ramp, a polygon at any angle) collides as drawn, and joins the outlines as solid cells do, so a row of half blocks, or a ramp of shapes over solid ground, is one seamless outline;
- a shape marked `one_way` is solid only from above, along its top edges (those facing within 45° of up). A tile with solid and one-way shapes collides with its solid ones only;
- a shape marked `occluder` blocks light and does not collide;
- an ellipse or a capsule collides as its outline, cut into a polygon; a polyline or a point has no inside and does not collide (a polyline warns once a file).

**An LDtk IntGrid layer** collides by the names of its values, ignoring case (LDtk capitalises them):

| Value name | Collides as |
|---|---|
| `solid` | a solid cell |
| `one_way` | a one-way platform along the cell's top |
| `slope_bottom_left`, `slope_bottom_right`, `slope_top_left`, `slope_top_right` | a 45° half cell filled at that corner (`slope_bottom_right` rises to the right) |
| anything else | nothing |

```rust
let project = ctx.assets().load::<LdtkProject>("ldtk/world.ldtk");
let root = MapRoot::ldtk(project, "Cave")
  .with_intgrid_collision("ice", TileCell::Solid) // by name
  .with_intgrid_collision(4, TileCell::OneWay); // by value, over its name
ctx.world.spawn(root);
```

- A `TileCell::Shape` names no shape here: cells of that value collide as nothing, and the dev log warns once a map.
- **Changing an IntGrid at run time:** `ctx.maps().set_intgrid(map, layer, cell, value)` (0 empties the cell) and `fill_intgrid(map, layer, min, max, value)` change the values, and only the chunks they change collide and cast shadows again, in that step. The art does not follow: LDtk runs its auto-layer rules in the editor, never in the game, so change the layer's tiles with `set_tile` too. `set_tile` on an IntGrid layer changes only its art.

<!-- given: map: Entity, cell: IVec2, rubble: Tile -->
```rust
// A cracked wall gives way: its cell stops colliding, and shows rubble
let mut maps = ctx.maps();
maps.set_intgrid(map, "Collision", cell, 0);
maps.set_tile(map, "Collision", cell, Some(rubble));
```

- **Seamless and hollow:** full cells merge into outlines, so a long floor is one edge and nothing snags where two tiles meet. An outline is hollow: a body that ends up inside a wall is not pushed out.
- **Layers together:** the colliding layers that share the map's cell size, no offset and one material collide as one grid on the map entity, each cell taking the strongest kind among them: solid, then a shape, then a slope, then one-way. A colliding layer with a cell size, offset or material of its own collides as a grid of its own on its layer entity; where two grids meet there can be a seam.
- **Collision ignores parallax:** a colliding layer with parallax collides where it would be without it (parallax is a drawing effect, as in Tiled), and the dev log warns once per layer. Keep parallax for layers that do not collide.
- **Edits:** a changed tile re-collides in that step. Only the outlines through the changed cells are traced again, and only the colliders those outlines belong to change, taking their new shape in place, the same entities. So anything touching the rest of the map keeps its contacts: a character standing on the map keeps its `floor`, and no `CollisionStopped` or `CollisionStarted` is sent for it.
- **Which entity:** the outlines are packed by area, one collider for those starting in each 32 x 32-cell chunk (and one for each row of one-way runs in a chunk), each a child of the entity holding the grid: the map entity, or a layer that collides on its own. Each sits at its chunk's top-left (its `Transform`), its collider's points relative to that, so they stay small however far from (0, 0) the chunk is: add the translation to read them where they are. With no body on the map each is its own fixed body, so collision messages, `CharacterHit` and `CharacterState::floor` name such a part; follow `ChildOf` to the entity holding the `TileColliders` (the `MapRoot`, or a layer with its `MapLayerRef`). With `RigidBody::kinematic()` on the map entity they name the map.
- **A moving platform flush with the ground:** a character level with both the map's ground and a moving platform rides the platform.
- **A disabled map** has no colliders; enabled, it builds them from its tiles as they then are, on the main thread (section 2).
- **Long levels:** a grid holds the chunks its layers have, of any length: a 10,000 x 64 runner level collides end to end. A cell collides within 186,411 cells of (0, 0) on either axis (about 3 million px of 16 px tiles); a tile farther out, on an unbounded layer or on a bounded one long enough to reach it, draws and casts its shadow but does not collide, and the dev log says so once. That reach is where the outline lattice's 32-bit points run out, not where f32 positions stay precise: a `Transform`'s f32 pixels are an eighth of a pixel apart a million px out (2^20), a quarter of a pixel at the reach (Rapier's f32 metres about 0.2 px), and a whole pixel past 8.4 million px (2^23), so slow movement steps visibly well before the reach; keep precise play within about a million px of the origin.
- **Navigation:** `NavigationGrid::rebuild_from_world` blocks a map's solid and slope cells, and the shape cells whose shape covers the cell's centre; one-way cells stay walkable.

## 9. Shadows

- A layer casts shadows as its `occludes` says, or as it collides when `occludes` is left out (`without_collision` does not change it). A light casts shadows only with `with_shadows()` (`ryo-lighting`).
- **What blocks light:** a tile's shapes marked `occluder` if it has any, else its collision shapes (one-way ones too). An IntGrid's solid cells block whole and its slopes as their half cell; a one-way cell has no area and casts nothing.
- The map draws its occluders as the boundary of what occludes: `LightOccluder2D::chain`s, a chunk's worth each, children of the first occluding layer and marked `MapOccluder { chunk }` (an `IVec2`: chunks are signed, as `TileGrid::chunk_of` gives them, so cell -1 is in chunk -1). A chunk owns the borders left of and above its cells, so the right and bottom edges of a chunk's last cells can be the next chunk's chains. Tiles that touch cast no shadow between them, so lit art shows no seam. Edits rebuild only the chains of the chunks they touch.
- The map owns its chains: a game can find them by `MapOccluder`, and a change it makes to one lasts until its chunk changes. A disabled map casts nothing.
- Every lit frame gathers every map's occluders, and each shadowed light tests all of them (section 15).

## 10. Draw order, y-sorting and parallax

- **Z:** layer `i` draws at the map's z (`with_z`) plus `i`, among the entities. A plain tile layer draws as one item at its z, so entities at a higher z draw over it. Give a character a z above every layer it should cover and below every layer that covers it (the tilemap demo's hero draws at 10, over the platformer's seven layers at 0 to 6); at a tile layer's own z, the character and the layer draw in entity index order, which nothing should rely on. An image layer draws as a sprite does, after the entities at its z, so at an equal z it covers them.
- **Lit or not:** as `lit` says. **Cameras:** a map draws on the map entity's `RenderLayers`; a layer entity's own `RenderLayers` replace them for that layer.
- **Y-sorted layers** (`y_sort`) draw each row of tiles as an item at the layer's z, sorted among the y-sorted entities there by the row's bottom edge. A character with `YSort` at that z walks behind a tree whose trunk row is below its feet and in front of one above them. Tile objects on a y-sorted object layer sort by their bottom edges.

<!-- given: hero: Handle<Image>, at: Vec2 -->
```rust
// The garden's map is at z 0, and "trees" is its layer 2: the hero sorts among its rows
ctx.world.spawn((
  Transform::from_translation(at),
  Renderable::image(hero).with_z_order(2),
  YSort::new(),
));
```

- **Parallax** (Tiled's parallax factor on a layer, LDtk's parallax factor) moves a layer by that share of the camera's motion, per camera: 1 is fixed to the world, 0.5 a slow background, 1.25 a foreground that drifts past. It is measured from the map's parallax origin (`TileMap::parallax_origin`: Tiled's Parallax Origin map setting, in map pixels; the map's top-left corner unless the file moves it, and always for LDtk), as Tiled draws it: a camera centred on the origin shows every layer where it would be without parallax. Parallax is drawing only: a colliding layer collides where it would be without it (section 8).
- **Image layers** draw as one quad per view, the image repeating across the view along the axes the editor says; a view needing more than 65,536 copies, or more than 524,288 of the image's pixels, along an axis gets that many about its centre, warned once.
- **Opacity and tint** multiply the layer. Blend modes normal, add, multiply and screen draw as in the editor.
- **Hidden:** a layer whose `LayerInfo::visible` is false draws nothing, and still collides and occludes as its flags say. A disabled map draws nothing at all.
- **The background colour is not drawn.** `TileMap::background` holds Tiled's map background colour, or an LDtk level's (`__bgColor`, else the level's own, else the project's default), but a map draws only its layers. Clear the camera to it, as the tilemap demo's pages do, or put a sprite of it behind the map at a lower z (the demo's `backdrop`, for a map smaller than the view):

<!-- given: level: Handle<TileMap> -->
```rust
// Once the level's asset is loaded, the camera clears to its background
let background = ctx.assets().get(&level).and_then(|map| map.background);
if let Some(color) = background {
  let camera = ctx.camera().entity();
  if let Some(mut camera) = ctx.world.get_mut::<Camera2d>(camera) {
    camera.clear = CameraClear::Color(color);
  }
}
```

## 11. Maps built in code

A `TileMap` built in code spawns like a file's: added to the assets and given to a `MapRoot`, it draws, collides and edits the same way.

<!-- given: tileset: Handle<Tileset> -->
```rust
let mut map = TileMap::new(UVec2::new(40, 23), UVec2::splat(16)); // cells, a cell's pixels
let terrain = map.add_tileset(TilesetSource::Asset(tileset.clone()));
let ground = map.add_tile_layer("ground");
if let Some(grid) = map.tiles_mut(ground) {
  grid.fill(IVec2::new(0, 20), IVec2::new(39, 22), Some(Tile::new(terrain, 0)));
}
let map = ctx.assets().add(map);
ctx.world.spawn((Transform::from_xy(0.0, 0.0), MapRoot::new(map)));
```

- `TilesetSource::Asset(handle)` shares a tileset asset with other maps; `TilesetSource::Embedded(Arc<Tileset>)` is one the map owns. A map holds up to 256 tilesets of up to 1,048,575 tiles each.
- **A tileset in code:** `Tileset::new(name, tile_size, TilesetImage::Atlas { image, size })` with its `columns` and `tile_count` set (and `spacing`, `margin`), or `TilesetImage::Collection` with each tile's own image in its `TileData`. Tiles collide through their shapes: `insert_tile(id, TileData { shapes: vec![TileShape { points, ..TileShape::default() }], ..TileData::default() })`, the points in tile pixels from the top-left.
- `add_object_layer(name)` adds an object layer, whose objects are `MapObjectData` in `LayerKind::Objects`; `layer_index(name)`, `tiles(layer)` and `tiles_mut(layer)` find layers.
- **An unbounded layer:** a tile layer whose grid is `TileGrid::unbounded()` (set through `tiles_mut`) holds any cell, negative ones included, in chunks made where tiles are written. Spawned, it draws, collides, casts shadows and spawns class tiles wherever it has chunks, and costs the chunks it has, however far apart: never the rectangle between them. Its colliding layers' grid is unbounded too; a cell collides within 186,411 cells of (0, 0).
- **A map grown at run time:** `TileMap::infinite(tile_size)` is an empty infinite map, with `size` zero and `infinite` true, whose `add_tile_layer` layers are unbounded. A game spawns it empty and grows it by writing tiles through `ctx.maps()`: a write at any cell, negative or far out, makes the chunk it lands in, which then draws, collides and casts shadows like any other. Nothing is paid for the space between chunks.
- **Chunks persist.** A chunk, once made, stays for as long as the spawned map does, emptied or not: nothing unloads chunks yet, so a map a game keeps growing keeps every chunk it ever wrote in. A bounded map has every chunk of its size from its landing (its collision, shadows and change tracking keep an entry for each), so a huge bounded map built with `TileMap::new` is slow to land and costly to keep however few tiles it holds; an infinite one costs nothing until a tile is written.

<!-- given: tileset: Handle<Tileset> -->
```rust
let mut map = TileMap::infinite(UVec2::splat(16));
let terrain = map.add_tileset(TilesetSource::Asset(tileset.clone()));
map.add_tile_layer("ground");
let map = ctx.assets().add(map);
let root = ctx.world.spawn((Transform::from_xy(0.0, 0.0), MapRoot::new(map))).id();
// Later, once it has landed: a floor of 11 cells left of the origin, and a tile far away
let solid = Some(Tile::new(terrain, 0));
let floor = ctx.maps().fill(root, "ground", IVec2::new(-100, 5), IVec2::new(-90, 5), solid);
let far = ctx.maps().set_tile(root, "ground", IVec2::new(5000, -5000), solid);
// The cells it covers now, (-100, -5000) to (5000, 5) both included: for the camera's limits
let grown: Option<CellRect> = ctx.maps().bounds(root);
```

  `TileMap::bounds()` of the asset is what the map held when it was added; `ctx.maps().bounds(root)` (the whole map) and `ctx.maps().layer_bounds(root, "ground")` (one layer) follow the writes: the cells holding tiles as they are now, for camera limits on a map that grows. A cell collides within 186,411 cells of (0, 0) (above).
- A map added in code never reloads (it has no file), but follows its tilesets' files.
- **Procgen:** `GenerationGrid::to_tile_map` and `to_terrain_map` (`ryo-procgen`) turn a dungeon, a cave or noise terrain into a `TileMap`. The tilemap demo's Procgen dungeon page draws one through a Wang set.

## 12. Worlds of levels

A world is a set of levels with their places and neighbours: a Tiled world file (`.world`, a `TileWorld`) or an LDtk project's worlds (`LdtkProject::worlds`). Each world's `WorldLayout` lists its levels (`WorldLevel { name, position, size, origin, depth, neighbours }`, in world pixels: `position` and `size` its rectangle, `origin` where its map's cell (0, 0) sits). Which levels exist when is the game's call; `ctx.maps().spawn_levels_in` spawns the levels overlapping an area that are not spawned yet, each a `MapRoot` at its level's place with a `WorldLevelOf { world, level }`:

<!-- self: world: Handle<TileWorld>, player: Entity -->
```rust
// Each step: the rooms within 64 px of the player exist
let Some(at) = ctx.world.get::<Transform>(self.player).map(|t| t.translation) else {
  return;
};
let near = Rect::new(at.x - 64.0, at.y - 64.0, 128.0, 128.0);
for room in ctx.maps().spawn_levels_in(&self.world, near) {
  game_info!("room {room} is on its way");
}
```

- Despawn a level's root to unload it; the next call whose area reaches it spawns it again. A root the game disabled to park its level still counts as spawned. The tilemap demo's World of rooms page despawns rooms the hero has left 96 px behind.
- **Set the roots up** with `spawn_levels_in_with(world, area, depth, configure)`: `configure` is given each level and its bare root and returns the root as the game wants it, with any of `MapRoot`'s builders (section 2). The map stays the level's whatever root it returns. The World of rooms page draws each room from z -2, so its y-sorted props layer is at z 0 with the hero:

<!-- self: world: Handle<TileWorld> -->
```rust
let near = Rect::new(0.0, 0.0, 320.0, 180.0);
ctx.maps().spawn_levels_in_with(&self.world, near, 0, |level, root| {
  let root = root.with_z(-2).with_entity_tiles(&["coin", "spikes"]);
  // The sky rooms are backdrops: nothing in them collides
  if level.name.starts_with("sky") { root.without_collision() } else { root }
});
```

- **Depth:** an LDtk world's levels each have a world depth (`WorldLevel::depth`, `LdtkLevel::depth`: LDtk's `worldDepth`), a floor of its own; a Tiled world's are all at 0. `spawn_levels_in` spawns depth 0 only, and `spawn_levels_in_with` the depth it is given, so the floors above and below never stack onto the one being played; `WorldLayout::levels_in(area, depth)` lists the levels the same way. To change floors, despawn the roots of the one left and stream the other.
- An LDtk world: `spawn_levels_in(WorldRef::Ldtk { project, world: "World".to_string() }, area)` (a single-world project's world is called "World"; with several, each has its name). Its levels are also `LdtkProject::levels` (`LdtkLevel { identifier, iid, world, position, size, depth, fields, neighbours, map }`; `level(name_or_iid)`).
- One level by hand: `MapRoot::world_level(&world, index)` (or `ldtk_world_level`), put at its `WorldLevel::origin` with the root's `Transform`. A level the world lacks fails the root (`MapEvent::Failed`).
- **An infinite map in a Tiled world** is placed by its tile origin: the world's `x` and `y` are where its cell (0, 0) sits (`origin`, where `spawn_levels_in` puts its root), and its rectangle is its tiles' bounds from there, read from its file (`position` is `origin` plus `bounds().min` times the tile size, `size` its `bounds()` in pixels), whatever `width` and `height` the world lists it with (Tiled writes them for every map). An infinite map with no tile takes its origin cell as its rectangle, so it can still spawn. A bounded map's `origin` is its `position`, and one listed with a `width` and `height` takes that rectangle from `x` and `y` as written. Tiled documents no rule for infinite maps in worlds, and its world loading could not be run headless to check one, so this is the engine's reading. The world reads each map it lists, so saving one loads the world again.
- Nothing spawns until the world's asset is loaded. A Tiled world's maps are its dependencies, so it is ready when they are; a map that fails fails the world.
- **Neighbours:** each level's `neighbours` (`Neighbour { level, direction }`, `level` an index in the same layout). A Tiled world's are worked out from the rectangles: sharing an edge is `North`, `South`, `East` or `West`, a corner a diagonal (`NorthEast` ...), an overlap `Overlap`. LDtk's are LDtk's own, `Above` and `Below` for levels at another depth.
- **Tiled world patterns** (`"regexp"` with `multiplierX`/`multiplierY`): the registry cannot list a folder (the web has none), so a pattern must be literal text with up to two numbers, `(\d+)` or `(-?\d+)`, as in `room_(\d+)_(\d+)\.tmx` (one number is x, two are x then y). The loader tries names row by row: from y = 0, x from 0 up to the first missing file, stopping at the first row whose x = 0 file is missing, in at most 1,024 tries. It builds each name, writing the numbers plainly, so it finds `room_1_2.tmx` but never `room_01_02.tmx` (a literal `0` in the pattern, `room_0(\d+)\.tmx`, reaches `room_00` to `room_09` and stops at `room_010`), and maps numbered from 1 are not found at all: the load fails, no map matching. Negative numbers and gaps in the numbering are never reached either; a world named any of these ways lists its maps instead. Any other regular expression fails the load.
- **Saving the `.world` file** leaves the levels already spawned where they are; the next `spawn_levels_in` uses the new layout. A save that reorders the levels can make it spawn a level that is already there: despawn and respawn the levels after such a save.

## 13. Hot reload

In debug builds with a window the engine watches `assets/` (`EngineBuilder::hot_reload` turns it on or off in any build). Saving a map, an LDtk project or one of its level files, a tileset, a template or the Tiled project updates every map spawned from it in the frame the save lands, before `update`, and sends `MapEvent::Reloaded`. The camera, the player and everything else keep their state.

- **Saving the Tiled project** loads the maps and the tilesets that use it again, and their loads can land a frame or two apart: the map follows each as it lands, with a `Reloaded` for each, the last with the save in full.
- **Tiles:** each tile layer takes the saved cells, and only the chunks the save changed draw, collide and cast shadows again. A chunk's collider changes shape in place, so a character standing on ground the save did not change keeps its contacts and its floor. An infinite map's layers follow their chunks: a chunk the save added draws, one it dropped empties, one it changed moves on, and the rest are untouched.
- **Layers** keep their entities, matched by the editor's layer id; a layer the save added spawns, one it removed despawns, and a tile layer saved at another size, cell size or offset spawns afresh.
- **Objects** match by id. One the save left as it was keeps its entity untouched: its `Transform` and every component the game added, as the game has them. One the save changed takes its new `MapObject` and `Transform`, keeping the game's other components. One the save added spawns, and one it removed despawns. Class tiles match the same way, by layer and cell.
- **What the game took away stays away:** an object or class tile the game despawned (a coin collected, an enemy killed) is not spawned again unless the save changed it.
- **The file wins over the game's tile edits:** a dug tile comes back with the save.
- A tileset image's save redraws the maps that use it, with no reload and no `Reloaded`.
- A save that fails to load (a typo in the XML) keeps the last good map and sends nothing; the dev log names the file and the place in it.
- A map added in code never reloads, though it follows its tilesets' files. An LDtk project saved without the map's level leaves the map as it is, warned once.
- **Limits:** a tileset save that changes collision shapes rebuilds the colliders of every map using it whole: new collider entities, so a standing character's `floor` changes and `CollisionStopped` is sent, though nothing falls through. A tileset saved naming a larger image at the same moment as that image (a `git pull`, a script) can stay as it was until it is saved again.
- **Cost:** a reload runs on the main thread, about 1 ms for each 4 million cells whatever the save changed, plus what changed: a hitch while developing, never in a release build.

## 14. The web

- **The web has no files.** A web build gives the engine every asset file in a `MemorySource` (`EngineBuilder::asset_source`): the maps, tilesets and images, and also the templates, the Tiled project, LDtk level files and a world's maps, which the loaders read through the same source.

  ```rust
  #[cfg(target_arch = "wasm32")]
  fn web_assets() -> MemorySource {
    let files = MemorySource::new();
    files.insert("maps/level1.tmx", include_bytes!("../assets/maps/level1.tmx").as_slice());
    files.insert("tilesets/ground.tsx", include_bytes!("../assets/tilesets/ground.tsx").as_slice());
    files.insert("images/ground.png", include_bytes!("../assets/images/ground.png").as_slice());
    files.insert("project.tiled-project", include_bytes!("../assets/project.tiled-project").as_slice());
    files
  }
  // main: builder.asset_source(web_assets())
  ```

- The tilemap demo keeps its files as one table of `include_bytes!`, compiled on every target (`src/embedded.rs`: `EMBEDDED`, and `embedded()` making the `MemorySource`), so its tests play every page from the table and check that it holds exactly the files in `assets/` (`tests/pages.rs`). Do the same, and a file left out of the web build fails a test instead of a level.
- **No threads:** a map's landing, its colliders and shadows, is worked out in the frame its asset is ready, so that frame pays for it: about 4 ms for a 256 x 256 map of four layers, and about 60 ms for 1024 x 1024 (estimates from native runs; wasm runs slower). Show a loading screen over a big map's landing. Loads run in the asset pump, at most 4 ms a frame.
- Every layer format loads on the web, zstd included. There is no hot reload.
- The demo builds for the web: `trunk serve --release --open` in `examples/demos/tilemap`.

## 15. Performance

Measured on an Apple M5 Pro in a release build, at 1920 x 1080 with 16 px tiles; the numbers are approximate and machine-dependent. The record, with the method and every measurement, is `docs/backlog/tilemap.md` in the Ryo2D repository.

- **How big a map loads:** a tile layer holds at most 67,108,864 cells (8192 x 8192), and a map's tile layers together 134,217,728 (2^27, 512 MiB of 4-byte cells: an 8192 x 8192 map of two layers, or a 1024 x 1024 map of 128). Cells count as the engine stores them, in whole chunks of 32 x 32: a layer counts every chunk it spans (a 10,000 x 64 runner 641,024 cells, a 1 x 1 layer 1,024, so a 1-cell-wide layer 2^26 tall is refused), and an infinite map's layer the chunks its chunks reach, or its chunks' cells if more. An LDtk level counts each layer, empty or not, and an IntGrid layer's values once more, and all the levels of a project share the same total, since a project loads whole. Past it the load fails, naming the map, level or layer, before anything is allocated.

- **Cheap:**
  - **Drawing** costs what is on screen: a frame draws only the chunks in view, so a static 1024 x 1024 map of four layers costs about 35 µs a frame to draw, as a small one would; about 47 µs with its colliders and shadow chains, and the same with its layers unbounded.
  - **A one-tile edit**, an outline split or joined included: about 17 µs to redraw its chunk, about 30 to 43 µs to rebuild the collision outlines through it and about 33 to 66 µs for the shadow chains. An edit also checks every chunk of the layer it wrote in, so its cost grows with the layer's chunk count: largest on a big unbounded layer.
  - **A still map's colliders and shadow chains:** each is a child entity, and transform propagation walks only what changed, so the colliders and shadow chains of a still 1024 x 1024 map (about 2,900 children) add about 10 µs a frame to drawing it: propagation's check of each entity for changes, and physics.
  - **Loading** runs off the main thread: a 256 x 256 four-layer Tiled map with its tileset and image loads in about 1.2 ms on the workers (about 4.8 ms saved as an infinite map in 16 x 16 chunks), and its colliders and shadows are worked out on the map builder thread.
- **What costs a game:**
  - **Moving a map moves every collider and shadow chain under it:** moving its root or a layer makes transform propagation recompute every one of them, in up to three runs a frame: at least 17 to 24 ns each a frame, what merely visiting them cost (at least 50 µs for the 1024 x 1024 cave's 2,900). So does a fixed step that changes a fifth of the world's entities (and more than 64: about 1,000 movers beside that map's 4,096 entities): propagation then walks every hierarchy, at about what the changed entities' own walks would cost. Keep big maps still, and turn collision off (`without_collision`, or `collides` false) and shadows off (`occludes` false) on maps and layers that need neither: they cost physics and lighting too.
  - **A y-sorted layer** draws each visible row as an item of its own: a dense y-sorted layer of the 1024 x 1024 map cost about 70 µs more a frame. Y-sort only the layers characters walk among.
  - **A lit level** gathers every map occluder each frame, and each shadowed light loops over all of them, however far away: one shadowed light over the 1024 x 1024 cave cost about 30 µs more than the unlit map.
  - **A landing frame** builds and uploads the chunks in view (about 0.5 ms at 1080p), and a tileset's first landing also makes its padded texture (about 0.5 ms more, about 1 ms in all, the first time; a level restarted, or the next one with the same tileset, within about two seconds reuses it). A 1024 x 1024 four-layer colliding map takes about 60 ms on the builder thread and about 2.5 ms in its landing frame, bounded or unbounded; on the web all of it is one frame (section 14). Cover a big map's landing with a loading screen.
- **Warnings in the dev log:**
  - `tileset "..." is too big to pad`: its padded copy (each tile in a cell twice its size) would pass the GPU's largest texture, 8,192 texels a side: about 3,800 tiles of 64 px, 15,800 of 32 px, or a collection of over 196 images up to 256 px (36 up to 512 px). Its tiles draw nothing until it changes. Split the tileset.
  - `layer "..." collides and has parallax`: it collides where it would be without parallax.
  - `the tile chunk arena is full`: the cameras keep more than about two million tiles in view at once (with those seen in the last two seconds), past the GPU's largest buffer: a whole 1024 x 1024 four-layer map at a small zoom, or a minimap camera over a big map. The chunks seen least recently are let go first; past that, the chunks met last draw nothing. Zoom in, show fewer tile layers to the wide camera (`RenderLayers`), or draw the overview from a picture of the map.
  - `a TileColliders cell ... is past the reach of tile collision`: a tile of any layer, unbounded or a bounded one long enough to reach it, more than 186,411 cells from (0, 0) on an axis draws but does not collide.
  - A repeated image layer too small to cover a view covers its middle.

## 16. Testing

- **A headless engine** (`ryo2d::testing::HeadlessEngine`) lands maps as a windowed one does. Step it until `MapsView::new(engine.world_mut()).is_ready(map)`, then read tiles with `MapsView`, objects and class tiles with queries, and colliders after a step through the world's `Physics` resource (`engine.world().resource::<Physics>()`: `raycast`, `overlap_point`, `contacts` and the rest). A landing takes a frame or more (its job runs on another thread), so wait for it rather than counting frames.
- **Events:** read `MapEvent`s between steps with a `MessageCursor<MapEvent>` over the world's `Messages<MapEvent>`; fail the test on `Failed`.
- **Hot reload:** headless engines do not watch files. Give the engine a `MemorySource` the test keeps (`EngineBuilder::asset_source`), save over a file with `files.insert(path, text)`, and ask for its reload with `engine.assets().server().reload(path)`; the next steps bring the map to it and send `Reloaded`.
- **The web's files:** play the levels from the embedded source too (section 14).

```rust
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

struct Empty;
impl Scene for Empty {}

/// The map events sent since `cursor` last read
fn map_events(engine: &HeadlessEngine, cursor: &mut MessageCursor<MapEvent>) -> Vec<MapEvent> {
  engine
    .world()
    .get_resource::<Messages<MapEvent>>()
    .map(|messages| cursor.read(messages).cloned().collect())
    .unwrap_or_default()
}

#[test]
fn level_one_lands_with_its_floor() {
  // None (with a "skipping: no GPU" line) on machines without a GPU
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new().resolution(320, 180).start_scene(Empty),
  ) else {
    return;
  };
  let level = engine.assets().load_now::<TileMap>("maps/level1.tmx").expect("level 1 loads");
  let map = engine.world_mut().spawn(MapRoot::new(level)).id();
  let mut cursor = MessageCursor::<MapEvent>::default();
  let mut steps = 0;
  while !MapsView::new(engine.world_mut()).is_ready(map) {
    assert!(steps < 600, "level 1 never landed");
    engine.step().unwrap();
    for event in map_events(&engine, &mut cursor) {
      if let MapEvent::Failed { error, .. } = event {
        panic!("level 1 failed: {error}");
      }
    }
    steps += 1;
  }
  let maps = MapsView::new(engine.world_mut());
  assert!(maps.tile(map, "ground", IVec2::new(0, 20)).is_some(), "the floor's first tile");
}
```

## 17. When something looks wrong

| Symptom | Look at |
|---|---|
| The map never appears | Its `MapEvent::Failed`: the error names the file and the place in it (`line L, column C`, `layer "name"`, `chunk (x, y)`, `object id`, `tile id`, `tileset "name"`). A missing tileset or image (an image that did not load is named with its tileset or layer), a gid outside every tileset, an image too small for its tileset, an unsupported orientation, an infinite map's chunk with no cells, too little data or cells too far out, an LDtk level name that is not there; or the map entity disabled; on the web, a file missing from the embedded source |
| An object has none of its class's properties | The Tiled project was not found (the dev log names the class once per file): `assets/project.tiled-project`, or `[tilemap] tiled_project` |
| A character falls through as the map arrives | It was placed before `MapEvent::Spawned`, or by a ray cast in the `update` that read it (section 1) |
| Tiles draw but do not collide | The layer's `collides` is false; the root has `without_collision`; the tiles have no collision shapes in the tileset, or only `occluder` ones; an LDtk value named other than the collision names (`with_intgrid_collision`); a class tile; the map disabled; a cell more than 186,411 cells from (0, 0) on an axis, on any layer (warned) |
| Collision sits off the art | The layer has parallax (warned): collision ignores it |
| A seam in the floor where two layers meet | The layers differ in cell size, offset or material, so each collides as a grid of its own |
| Tiles cast no shadows | The light lacks `with_shadows()`; the layer does not occlude (`occludes` false, or it does not collide and `occludes` is not set) |
| A character walks over a tree instead of behind it | The layer is not y-sorted, or the character lacks `YSort` or has another z than the layer (section 10) |
| A tileset draws nothing, warned as too big to pad | Its padded copy would pass the GPU's largest texture, 8,192 texels a side (about 3,800 tiles of 64 px): split it |
| Painting terrain leaves a cell as it was, warned | The Wang set has no tile for what the cell needs |
| A dug tile came back | The map's file was saved: the file wins over tile edits |
| A coin collected did not come back after a save | As meant: what the game despawned stays away unless the save changed it |
| A world pattern found fewer maps than there are files, or none | Gaps in the numbering, negative numbers, zero-padded numbers (`room_01`) or numbering from 1: list those maps |
| A saved `.world` did not move the rooms | Levels already spawned stay where they are; respawn them |
