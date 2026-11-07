---
name: ryo-tilemap
description: "Tile maps - the workflow from Tiled (.tmx, .tmj) or LDtk (.ldtk) through assets/ to a MapRoot entity, layers and the reserved layer properties (collides, occludes, y_sort, lit, filter, floor), storeys (MapFloors) and the entities on them (Floor, drawn lifted), stairs and ramps that characters climb between storeys (Climber, FloorChanged), tilesets and typed tile data, objects and class tiles as entities, MapEvent, reading and editing tiles with ctx.maps() and Wang terrain, collision from tile shapes and IntGrids, tile shadows, y-sorting and parallax, worlds of levels, maps built in code and by procgen, endless worlds streamed from a generator (MapStream, StreamAnchor, ChunkEvent, SavedChunks, ChunkOwned), hot reload, the web, performance and testing."
metadata:
  version: 1.8.0
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

The loaders also read Tiled's object templates (`.tx`, `.tj`) and its project file (`.tiled-project`) as the maps need them; a game never loads those itself. Every type here is in the prelude; complete signatures are in `ryo-api` (Tilemaps). The worked example is the tilemap demo, `examples/demos/tilemap` (`./dev.sh tilemap`), in eleven pages: a Tiled map and an LDtk level side by side, flips and a layer of two tilesets, animated tiles, a top-down garden (y-sorted trees, parallax clouds, an image layer), collision with slopes and a one-way bridge, objects given behaviour, painting terrain, a procgen dungeon, a world of rooms, a 1024 x 1024 map, and hot reload. The tile grids demo (`examples/demos/tile-grids`) shows a Tiled infinite map with chunks at negative cells streamed into an endless world (the file's chunks hand-made, a generator filling the rest), an infinite map built in code that grows wherever the mouse paints, and an endless world that keeps what the player digs and builds in a `SavedChunks`. The character demo's platformer level is an LDtk level (`examples/demos/character`), and breakout's ten levels are Tiled maps whose bricks are class tiles (`examples/games/breakout`).

## 1. From the editor to the game

1. **Make the level** in Tiled (1.12) or LDtk (1.5).
   - **Tiled:** a map of any of Tiled's grids, of a fixed size or infinite: orthogonal, isometric (any tile ratio), staggered (rows or columns shifted, the odd or the even ones), hexagonal (pointy- or flat-topped, either index, any side length, 0 and longer than the tile included) or oblique (any skew, negative too). It loads with its grid (`TileMap::shape`, section 11), and a tile layer's tiles keep their flips, and on a hexagonal map their 60° and 120° turns (section 4). A hexagonal map's tile layers collide and cast shadows (sections 8 and 9). Save maps as `.tmx` or `.tmj`, and tilesets inside the map or as `.tsx` / `.tsj` files. Every layer format Tiled writes loads: CSV, Base64 uncompressed or compressed with gzip, zlib or zstd, and the TMX's `<tile>` elements. An orientation, stagger axis or stagger index Tiled has none of, or a negative hex side length, fails the load naming it, as does a layer blend mode other than normal, add, multiply or screen.
   - **A Tiled infinite map** loads too: each tile layer's chunks land where Tiled saved them, at negative cells as well, whatever the map's output chunk size, in every layer format. The map's `infinite` is true and its `size` zero; `TileMap::bounds()` gives the cells its tile layers cover: the smallest rectangle holding every cell with a tile, for camera limits and world rectangles. Its tile layers are unbounded grids (section 11), so an edit past its chunks makes new ones, and a generator can stream an endless world around its chunks (section 12). A chunk whose cells' pixels (their outlines on the map's grid) would pass what a 32-bit integer holds fails the load, naming the layer and the chunk.
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

### Maps on other grids

The steps are the same on every grid; these come on top. The tile-grids demo (`examples/demos/tile-grids`) shows each, with Kenney's art, its maps written by Tiled:

1. **Give the map its grid in Tiled** (New Map: orientation isometric, staggered, hexagonal or oblique, with the stagger axis and index, the hex side or the skew) and its cell's size as the tile size: an isometric cell is its diamond's box. The map loads with that grid; nothing is set in code.
2. **Stand tall art on its cell with the tileset's offset.** A picture's bottom-left goes on its cell box's bottom-left, moved by the tileset's tile offset (section 10): set the offset so a wall's or a tree's base sits on the cell. A block a cell high (raised ground) is two tilesets of the same kind of picture: the blocks drawn a cell lower, their tops on the cells, and the props standing on the cells, so the cells characters walk, pick and collide on are the blocks' tops.
3. **Draw collision shapes in the picture's pixels**, keeping the tileset's own grid orthogonal: a polygon on the four vertices of the cell's diamond, where the picture draws it, collides as the whole cell, so a row of walls is one seamless outline (section 8). On a hexagonal map a polygon on the cell's corners does the same, and any other shape (a rock, a trunk) collides as drawn, turned and flipped with its tile. Hexagonal layers cast their shadows along the same sides and shapes (section 9).
4. **Y-sort the layer characters walk among** (`y_sort`), and give the character a sprite with `YSort` at that layer's z (the map's z plus the layer's index), anchored at its feet (`Anchor::BottomCenter`): it draws behind a tile whose cell's centre is below its feet and in front of one above (section 10).
5. **Pick, outline and measure cells through the layer's grid:** `ctx.maps().grid(map, layer)` gives the cell under the pointer (the cell whose outline holds it, the hexagon drawn there on a hex map), its outline in the world to draw, and the board tools (section 6). Terrain paints on every grid but the hexagonal (section 7).
6. **Show the same place on another grid with another map**, saved by Tiled with that grid, spawned in place of the first, rather than reshaping a map that has spawned.

**Big art costs about its own size.** Every tileset draws from a padded copy in which each tile sits in a cell a little bigger than itself, each side rounded up to a multiple of a power of two by at most a quarter (a 256 x 352 picture takes a 256 x 384 cell, a 120 x 140 hexagon 128 x 160, a power-of-two tile its own size), each picture in a cell of its own size in an image collection, with the copy's smaller copies (mip levels): with them a tileset's copy takes about 1.3 times its pictures' own pixels for power-of-two tiles and up to about 1.7 for others, and at most about 2.5 times for a collection mixing one big picture with small ones. It is made off the main thread on native platforms and within the landing frame on the web, in time in proportion. Kenney's pictures at their own size in the demo: Sketch Town's 14 ground and 11 prop pictures (256 x 256 each) pad to 4.7 MiB and 3.7 MiB, and the Hexagon Pack's 24 board hexagons (120 x 140) to 2.5 MiB, each page's map landing in 10 to 17 ms. A headless test waits for a landing in time rather than in steps (section 17).

The demo's pages: **An isometric town** (Sketch Town's walls and trees on a y-sorted layer, a knight walking behind and in front of them, the hovered cell outlined, the walls and the water colliding, water and grass painted through a Wang set, Space walking the knight to the pointer's cell by a path round the walls and the water, on a grid taken again when the tiles change), **A staggered map** (an island on rows shifted and on columns shifted, two files, a setting swapping them), **A hex board** (the hex under the pointer, a click selecting one, its range, ring and line and the hovered hex's neighbours marked on layers of their own, the distance in the HUD, the sand's ripples turned in sixths, rocks whose whole-hexagon shapes collide and occlude, casting hexagonal shadows from a light the walker carries over a dusk ambient (the marks' rim layers unlit, `lit` false, so they read in it), a collider round the board's outer hex sides keeping the walker on the board, and the cheapest path from the walker to the hovered hex round the rocks, trees dearer through a `cost` tile property, walked on a right click), **An oblique map** (a dungeon skewed and its mirror, a knight stopped by the slanted walls) and **Speed on every grid** (the speed guards' 1024 x 1024 four-layer map on the grid chosen, flown over, with the frame's cost; section 16).

## 2. A map in the world

`MapRoot` is a component; its builders say how the map spawns:

| Builder | What it does |
|---|---|
| `MapRoot::new(Handle<TileMap>)` | A Tiled map, or one built in code |
| `MapRoot::ldtk(Handle<LdtkProject>, level: &str)` | A level of an LDtk project, by identifier or iid |
| `MapRoot::world_level(&Handle<TileWorld>, level)`, `MapRoot::ldtk_world_level(project, world: &str, level)` | A level of a world, by its index in the world's layout (section 13) |
| `.with_entity_tiles(&["brick", "spikes"])` | Tiles of these tileset classes spawn as entities instead of drawing (section 5) |
| `.with_intgrid_collision(key, TileCell)` | An LDtk IntGrid value, by number or name, collides as this cell kind (section 8) |
| `.without_collision()` | Nothing of the map collides; its layers still cast shadows as their flags say |
| `.with_z(z)` | The bottom layer draws at z, each layer above one higher (default 0) |

What lands, all of it children of the map entity:

- **A child per layer**, bottom first, with `MapLayerRef { map, index, name }`, the layer's `LayerInfo` as a component, and a `Transform` at the layer's offset (on a storey above the ground, its offset less the storey's height: below); a tile layer also has `TileLayerData` (its cells), an image layer `ImageLayer`.
- **Objects**, children of their object layer, each with a `MapObject` (section 5); **class tiles**, children of their tile layer, each with a `MapTile`.
- **The colliders** (children of the map, of a layer that collides on its own, or of a storey's edge, a `MapFloor` child of the map: section 8) and **the shadow chains** (`MapOccluder`, children of the first occluding layer), which the map keeps in step with its tiles.
- `MapIds` on the map entity: its objects' entities by their editor id.
- `MapFloors` on the map entity, when some layer is on a floor above the ground (its `floor` property, section 3): each storey's height, `lift(Floor) -> Option<Vec2>` (in map pixels; `Floor::GROUND` gives zero, a floor no layer is on `None`), `floors()` (ascending, the ground floor first) and `highest()`. A map with every layer on the ground floor has none, and lands as it always has.

**Storeys stand in ground space.** A storey's height is how far up Tiled draws it: the vertical offset of its bottom layer (the first on it in draw order), as designers shift upper storeys in Tiled. A layer on a storey above the ground stands at its offset less that height, so what it holds (objects, class tiles, colliders, shadow chains) sits in ground space, where the characters stand: a point object marking a spot on the upper floor is at that spot's ground position, not lifted by the storey's height. It draws lifted by that height, where Tiled draws it: its tiles, an image layer and tile objects (section 10). Its objects and class tiles carry `Floor::On(n)`, its storey, so what the game gives them collides on that storey and draws lifted (section 10, Floors). A layer on the ground floor keeps its whole offset. `ctx.maps().floors(map)` reads the `MapFloors`, and `ctx.maps().lift(map, floor)` gives a storey's height in the world, through the map's transform as it is now:

<!-- given: map: Entity -->
```rust
// How far up the first storey is, on the map as it is turned and scaled now
let maps = ctx.maps();
let storeys = maps.floors(map).map_or(1, |floors| floors.floors().count());
if let Some(up) = maps.lift(map, Floor::On(1)) {
  game_info!("{storeys} storeys, the first {} px up", up.length());
}
```

Then:

- **Move the map** with its `Transform`: drawing and cell conversions follow at once, and its colliders, which have no body, are put where it now is. For a map that moves while things stand on it (an airship), give the map entity `RigidBody::kinematic()`: its colliders join that body and it carries what stands on it.
- **Disable it** (`Disabled` on the map entity): it draws nothing, and its colliders and shadows go; enabled, they are built again from its tiles as they then are, on the main thread in that frame: a few milliseconds for a 256 x 256 four-layer map, about 70 ms for 1024 x 1024. Despawn a big level rather than park it, or enable it behind a loading screen. `Disabled` does not pass from the map to its layers: the map's systems check the map entity. A layer entity disabled on its own stops drawing, colliding and casting shadows; to hide a layer and keep its collision, set its `LayerInfo::visible` to false.
- **Unload it** by despawning the map entity: everything it spawned goes with it. `SceneCleanup::WORLD` does this at a scene switch, as for any entity. Putting another `MapRoot` over it, or removing it, takes the old map down first, and the new root spawns afresh.

`MapEvent` is a message (`ctx.messages::<MapEvent>()`, `MessageReader<MapEvent>` in systems):

| Variant | When |
|---|---|
| `Spawned { map }` | It landed whole this frame |
| `Reloaded { map }` | It followed a saved file, in the frame the save landed (section 14) |
| `Failed { map, error: Arc<AssetError> }` | Its asset did not load, its LDtk project has no such level, its world has no such level, or working out its landing failed: nothing spawned. Sent once for each failure; saving the file fixed spawns it |

## 3. Layers and their properties

What the editor gives a layer carries over:

- **Order:** bottom first (`MapLayerRef::index`). Tiled's group layers flatten into their children: offsets add, opacity and tint multiply, parallax multiplies, visibility needs every group visible, and a group's blend mode passes down unless a child has its own. LDtk lists its top layer first; it draws last here too.
- **Opacity, tint** (Tiled), **offset, visibility, blend mode, parallax** (section 10). A hidden layer draws nothing but still collides and casts shadows as its flags say: a collision-only layer is a hidden tile layer.
- **Image layers** draw their picture, repeated across or down where Tiled's repeat settings say. One left without a picture (Tiled makes a new image layer so) loads with `ImageLayer::image` `None` and draws nothing. A transparent colour set on its image (`ImageLayer::transparent`) draws clear.
- **LDtk:** an IntGrid layer draws its auto-tiles and collides by its values; each layer keeps its own grid size, so a level may mix an 8 px collision grid with 16 px art.
- **LDtk's stacked, offset and translucent tiles** draw as the editor shows them. Rules and stamps stack several tiles in a cell, a rule can move its tiles off the grid or fade them, and a layer holds one tile a cell, so such a layer loads with sub-layers: one for each distinct stack depth, offset and alpha its tiles need, named `Ground#1`, `Ground#2` and on, with `LayerInfo::part_of` naming the layer's editor id. They are parts of the layer, not layers of their own: each draws with the layer's settings as they are now (visibility, `y_sorted`, `lit`, parallax, blend, tint, its entity's `RenderLayers` and `Disabled`), at its z, just after it, its own offset and opacity over the layer's, so a game sets a flag on the layer alone and the whole layer follows, and a y-sorted layer's stacked tiles sort with a character as its own do. They are listed after the editor's layers in `MapLayers`, so every layer keeps the index and z LDtk's order gives it whatever stacking a save adds. The IntGrid and the collision are the layer's own. Edits by the layer's name (`set_tile`, `fill`, `set_terrain`) replace the whole cell, clearing what its sub-layers held there, and `tile` reads the layer's own tile. A layer has 16 sub-layers at most: past that, the tiles left over draw in the nearest, and the dev log says how many.

Six layer properties are reserved; the engine reads them into the layer's `LayerInfo`:

| Property | Type | Default | What it does |
|---|---|---|---|
| `collides` | bool | true for a Tiled tile layer, false for other layers | Its tiles collide (section 8) |
| `occludes` | bool | left out: as `collides` | Its tiles cast 2D shadows (section 9) |
| `y_sort` | bool | false | Its tiles sort with the characters by y, by where each stands (section 10) |
| `lit` | bool | true | Lights and the ambient shade it; false draws it at full brightness |
| `filter` | string | left out: the target's (nearest for pixel art, smooth otherwise) | How a tile or object layer's tiles are sampled drawn at another size: `linear` reads the padded copy's smaller copies (painted art drawn zoomed out), `nearest` keeps the full-size texels (pixel art); image layers sample as the target does |
| `floor` | int | 0, the ground floor | The storey the layer is on: raised ground, an upper floor, a bridge. A whole number from 0 to 255 (a float with no fraction counts); any other value refuses the map, naming the layer. A storey's height is its bottom layer's vertical offset, and its layers stand in ground space (section 2) |

- **In Tiled:** select the layer and, in its Properties, add a custom property of type bool with the name (of type string for `filter`, int for `floor`). Properties on a group layer are its own and do not pass to its children: set them on the tile layers. `floor` is the exception, as designers group a storey's layers: set `floor` and the offset on the group, and each of its layers is on that storey at that height (nested groups too; a layer with a `floor` of its own keeps it). A group's class from the Tiled project can give its `floor` as a default; nothing else of a group is read. A layer class from the Tiled project (section 4) can give these as its members' defaults.
- **Hexagonal maps:** their tile layers collide and cast shadows as on every grid (sections 8 and 9), a tile layer added in code (`add_tile_layer`) too. Hexagons over 16,384 px, or under 2 px across, neither collide nor cast shadows: the dev log warns once per layer, `layer "walls" has hexagonal cells over 16,384 px or under 2 px, which don't collide` (and `..., which cast no shadows` for a layer that occludes).
- **In LDtk:** layers have no custom properties. IntGrid layers collide, by the names of their values (section 8); every other layer neither collides nor occludes; no layer is y-sorted, every layer is lit, every layer samples as the target does, and every layer is on the ground floor (LDtk's world depths are levels, not storeys). To change a flag, set it on the layer entity's `LayerInfo` once the map has landed: drawing, collision and shadows read it from there and rebuild. A reload of the file puts the file's flags back.

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

A material property of the wrong type is warned once, and what is under it counts (the map's, else the default); a reserved flag of another type than bool counts as left out, as does a `filter` other than the string `linear` or `nearest` (in any case); a `floor` that is not a whole number from 0 to 255 refuses the map: `layer "roof": property "floor": "two" is not a whole number from 0 to 255`. All other properties are the game's, read from `LayerInfo::properties`, `TileMap::properties`, a tile's data or an object.

## 4. Tilesets and tile data

- **A cell holds a `Tile { tileset, id, flip }`:** `tileset` indexes the map's tilesets (`TileMap::tilesets`), `id` is the tile in it, and `flip` is its `TileFlip`, the flips as Tiled stores them: a diagonal flip first, then horizontal, then vertical (a 90° turn is a diagonal and a horizontal flip; `TileFlip::rotated_cw()` turns one). On a hexagonal map Tiled turns tiles instead of flipping them diagonally: the diagonal bit is a 60° turn clockwise (`TileFlip::HEX_ROTATE_60`, the same bit as `DIAGONAL`) and a fourth bit a 120° one (`HEX_ROTATE_120`), applied after the flips, both together a half turn; `rotated_hex_cw()` turns a hexagonal tile 60° (six turns come back to where they started). Tiled ignores the 120° bit on every other grid, and so does the load. A turned hexagonal tile draws turned as in Tiled, about its own middle (section 10). A layer may mix tilesets, and every tile keeps the flips and turns the editor gave it.
- **What a tile carries:** `ctx.maps().tile_data(map, tile) -> Option<TileData>` gives its `class`, `properties`, `shapes` (its collision shapes, each `TileShape { points, one_way, occluder, properties }` in tile pixels from the top-left), `animation` (`TileFrame { tile, duration }`), `probability`, `sort_offset` (the reserved tile property of that name, in pixels, 0 unless set: where the tile sorts on a y-sorted layer, section 10), `stairs` (the reserved int tile property of that name: the storey a stair or ramp tile leads up to from its layer's floor, whose edge it joins (section 8) and which a `Climber` climbs (section 10, Stairs), `None` unless set; a whole number from 0 to 255, any other value refusing the map, naming the tileset and tile: `tileset "steps", tile 2: property "stairs": -1 is not a whole number from 0 to 255`) and `image` (a collection tileset's own picture). A loaded `Tileset` reads the same with `tile(id)`.
- **Typed properties:** `Properties` reads `bool`, `int` (an `i64`; a whole float counts), `float` (an `f64`; an int counts), `string` and `color` by name, and `get(name) -> Option<&PropertyValue>` for the rest: `File(path)` as written, `Object(id)` (a reference to an object), `Enum { ty, value }`, `Class { ty, fields }` and `List` (Tiled 1.12's list properties, each item read by its own type, nested lists too, and an enum's flags). They keep the order they were written in (`iter()`).
- **Classes and enums from the Tiled project.** Tiled keeps custom classes, enums and the members' defaults in its project file, and a map it saves writes only the members a designer changed. Put the project at `assets/project.tiled-project`, or name it in `Ryo.toml` (`[tilemap] tiled_project = "tiled/game.tiled-project"`) or with `EngineBuilder::tiled_project(path)`: a member a file leaves out then takes the project's default (a list member's too; a list the file writes replaces it whole), nested classes layer, and enums read by name (flags as a `List`). Without it what the file wrote is kept, and the dev log names the class once per file. Saving the project reloads the maps. Classes holding members of classes multiply: one thing's properties may resolve to at most 65,536 values and a file's to 4,194,304, past which the map fails to load, naming the place.
- **LDtk** writes every field with its type in the project, so nothing more is needed for entities and levels. A tileset's per-tile data reads as Tiled's tile classes and properties do, so the same game code serves both editors: a tile's enum tags (the tileset's tag enum, "Enum tags" in LDtk's tileset panel) give it its class, the first of its tags in the enum's order (one tag is the usual case, and `with_entity_tiles` and class checks then work for LDtk levels too), and a bool property `true` for each of its tags (for a tile with several: `data.properties.bool("Water")`); its custom data, LDtk's one free text a tile, is the string property `custom_data`, as written (parse it yourself if it holds JSON).
- **Animated tiles** (Tiled's tile animation editor) play on the game clock: they pause with the game and slow with its speed, and nothing is rebuilt as they play. Frames of different lengths keep their lengths, unless repeating them to a common beat would take over 128 cells of the padded tileset, when every frame takes the mean length.
- **No bleeding at any zoom:** each tileset is drawn from a padded copy made when a map first needs it, each tile in a cell a little bigger than itself with its edges stretched out to the cell's, and a smoothly drawn tile picks its own mip level and keeps every read inside its cell at that level, so no filter, mip level or sub-pixel camera samples a neighbour's texels (pixel art keeps the full-size texels). Painted art in a pixel-art game, drawn smaller than its pictures, wants `filter` set to `linear` on its layers: it then reads the smaller copies, cleanly, rather than skipping texels. A tile drawn more than a few pixels wide shows its own texels in proportion; drawn smaller, it reads the coarsest level its cell keeps clean, which can shimmer slightly as the camera moves but never shows a neighbour. A tileset's transparent colour (Tiled's, on its image: `Tileset::transparent`) draws clear, keyed out in that copy.
- **What loads otherwise than Tiled or LDtk shows it is warned**, once per file for each feature, naming its first place: a Tiled tileset's tile render size "grid" (its tiles draw at their own size), a collection tile drawing part of its image (it draws all of it), a render order other than right-down on an orthogonal or oblique map (it differs only where tiles reach past their cells; Tiled draws every other grid right-down whatever it says), a top-down object layer two of whose tile objects overlap in the other order from their y (they draw in file order), a polyline among a tile's collision shapes (it has no inside: ellipses and capsules collide as their outlines), a tileset whose grid is isometric and whose tiles have collision shapes (they are read in the tile image's pixels, as Tiled's collision editor shows them on an orthogonal grid: set the tileset's orientation to orthogonal in Tiled to see them as they collide); an LDtk entity whose tile LDtk draws otherwise than stretched over its box (`Repeat`, `NineSlice`, or fitted into a box of another shape), and a parallax layer LDtk also scales (it moves, keeping its size).

## 5. Objects and class tiles

**Objects** are Tiled's object layers (rectangles, points, ellipses, capsules, polygons, polylines, text, tile objects and instances of templates) and LDtk's entities. Each spawns as a child of its object layer:

- `MapObject { map, id, name, class, shape, size, tile, visible, properties }`, and a `Transform` at the centre of its rectangle (a point at its point; a polygon or polyline at the origin of its points, which are relative to it), turned by its rotation.
- `id` is Tiled's object id, or LDtk's iid, as a string, unique in the map; `MapIds` on the map entity finds an object's entity by it (`get(id)`). `class` is Tiled's Class (Type, in older files) or LDtk's entity identifier, which is its `name` too.
- An object hidden in the editor spawns all the same, with `visible: false` (games use hidden markers); it does not draw.
- A tile object draws its tile, with its flips and animation, at its place and size, on the layer's z. Set `MapObject::tile` to show another.
- **Objects on other grids stand where Tiled shows them**, in the layer's pixels. On an isometric map Tiled stores a position in tile-height units along the diamond's axes (a stored point `(px, py)` is at `((px - py) / th * tw / 2 + origin.x, (px + py) / 2)`), and on an oblique one in unskewed pixels, so the loader projects them: a rectangle, ellipse (32 points) or capsule becomes an `ObjectShape::Polygon` of its projected outline with a zero `size`, a polygon's or polyline's points are projected, and points and text are placed at the projected position. A tile object keeps its picture's pixel size and stands on its tileset's alignment point after projection: bottom-centre when the tileset says nothing on an isometric map (Tiled's rule), bottom-left everywhere else. Staggered and hexagonal maps store plain pixels, as orthogonal ones do. A rotation turns the object in screen space about its projected position, as Tiled draws it (measured on isometric and oblique maps; a rotated rectangle there is a parallelogram turned as a whole, not turned first and projected after).
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

`ctx.maps()` is a `MapsView` over the world (in a system or a test with the world: `MapsView::new(world)`). Every call names the map entity and, where it reads cells, a layer by name (the first so called) or by index (bottom first, a `usize`). A map that has not landed, a layer it does not have, a layer that is not a tile layer, or a cell outside the layer reads `None` (or `false`, or 0) and changes nothing. An unbounded layer (section 11) has no outside: its cells are signed, and `set_tile`, `fill` and `set_terrain` take any cell, negative ones included, making the 32 x 32-cell chunk a tile goes in, which then draws, collides and casts shadows. On a streamed map (section 12) only the chunks it has landed take edits: `set_tile` and `set_terrain` elsewhere return false, and `fill` changes only the cells in landed chunks. Two writes are refused there, changing nothing: a tile `fill` that would make more than 131,072 chunks (the 2^27 cells a map may hold; `IVec2::MIN` to `IVec2::MAX`, say) returns 0 and the dev log warns once a layer, and `set_terrain` within a few cells of the ends of `i32` (the cells its paint reaches would not fit) returns false.

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
| `world_to_cell(map, layer, world: Vec2) -> Option<IVec2>`, `cell_to_world(map, layer, cell) -> Option<Vec2>` | through the map's and the layer's transforms and the layer's grid (section 11): the cell whose outline holds the point; a cell's centre |
| `grid(map, layer) -> Option<LayerGrid>` | the layer's grid and where its pixels are in the world, on every grid: picking, outlines in the world and the board tools (below); in ground space, so on a storey above the ground its cells are where the characters on that storey stand, the storey's height below where it draws |
| `floor_cell_at_world(map, world: Vec2) -> Option<(Floor, IVec2)>` | the storey and the cell drawn under a world point, through every storey's height: picking on a map with floors (below) |
| `y_sort_z(map) -> Option<i32>` | the z a character draws at to sort among the map's y-sorted layers (section 10); `None` with none |
| `navigation(map, layer, window: CellRect) -> Option<NavigationGrid>` | a navigation grid over a window of the layer's cells on any grid, the map's collision and a stream's chunks not landed blocked (section 8) |
| `layer_entity(map, layer) -> Option<Entity>` | the layer's entity |
| `floors(map) -> Option<&MapFloors>`, `lift(map, Floor) -> Option<Vec2>` | the map's storeys and their heights in its pixels (`None` with every layer on the ground floor); a storey's height in the world, through the map's transform as it is now (section 2) |
| `drawn_lift(entity) -> Vec2` | how far up an entity on a storey draws, in the world (zero when it is not lifted): for a camera that follows it (section 10, Floors) |
| `bounds(map) -> Option<CellRect>`, `layer_bounds(map, layer) -> Option<CellRect>` | the cells the map, or a tile layer, covers as it is now: a bounded one's size; an unbounded one's cells holding tiles, class tiles included, following every edit (empty with none; it looks at every chunk, so on a map grown to many chunks read it when the map changes, not every frame) |
| `spawn_levels_in(world, area: Rect) -> Vec<Entity>`, `spawn_levels_in_with(world, area, depth, configure)` | the levels of a world near a place, the second setting each root up (section 13) |

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
- **Conversions use the transforms as they are now**, so a map moved this frame converts at once; `world_to_cell` gives cells outside the layer too, and `None` only for a map scaled to nothing (or a layer whose cells have no size).
- **Picking and edits land on the cell under the point on every grid:** orthogonal, isometric, staggered, hexagonal and oblique, on a map moved, turned or scaled, at negative cells too. The cell is the one whose outline holds the point, the one that draws there; a point on an edge or a corner goes to one of the cells that share it.
- **The board tools work through the layer.** `grid(map, layer)` gives a `LayerGrid { grid: MapGrid, to_world: Affine2 }`: `cell_at_world(Vec2) -> IVec2` (the cell under a world point), `cell_center_world(IVec2) -> Vec2` and `outline_world(IVec2) -> CellOutline` (the cell's outline in the world, clockwise on screen from its top-most point, to draw a cursor or a move range over the map), and through `grid` the questions of any grid (section 11): `neighbours`, `distance`, `line`, `range` and `ring`, on a hexagonal board Red Blob's hex maths. It holds the transforms as they were when read: read it again after the map moves.

<!-- self: board: Entity -->
```rust
// The hexes a unit under the pointer can reach in two moves, and the line
// from it to the hex at (9, 4), each step a neighbour
let (x, y) = ctx.input.mouse_position();
let pointer = ctx.camera().screen_to_world(Vec2::new(x, y));
let maps = ctx.maps();
if let Some(layer) = maps.grid(self.board, "ground") {
  let home = layer.cell_at_world(pointer);
  let reach: Vec<IVec2> = layer.grid.range(home, 2);
  let path = layer.grid.line(home, IVec2::new(9, 4));
  let cursor = layer.outline_world(home);
  let corners: &[Vec2] = cursor.points();
  let _ = (reach, path, corners);
}
```
- **Picking through storeys.** On a map with floors (section 2) a storey draws its height above where it stands, so the cell under the pointer is not the cell of `world_to_cell` on its layer. `floor_cell_at_world(map, point)` checks the storeys from the highest down, each through its bottom tile layer's grid lifted by its height: a storey above the ground answers only where one of its tile layers on that grid holds a tile (a class tile too), the ground floor anywhere. It gives the storey and the cell in ground space, where a character on that storey stands to be drawn there, so the cell goes straight to `grid`, `navigation` and edits on that storey's layers. A layer that does not draw (`LayerInfo::visible` false, or its entity disabled) holds nothing here: a roof hidden while the player is inside lets the pointer through to the floor under it. It works on every grid and through the map's transform as it is now; on a map without floors it is the ground floor's first tile layer's cell.

<!-- self: map: Entity -->
```rust
// Click a cell on whatever storey draws under the pointer, and mark it
let (x, y) = ctx.input.mouse_position();
let pointer = ctx.camera().screen_to_world(Vec2::new(x, y));
if let Some((floor, cell)) = ctx.maps().floor_cell_at_world(self.map, pointer) {
  game_info!("cell {cell} on {floor:?}");
}
```
- **Edits change the spawned map, not its asset:** two maps from one file keep their own tiles, and a reload of the file brings dug tiles back (section 14).
- `TileLayerData { tiles, intgrid, grid }` on a layer entity is the map's own copy of the layer: editing its `tiles` (a `TileGrid`: `get`, `set`, `fill`, `clear`) draws, collides and casts as `ctx.maps()` does, though without class tiles. Its `grid` is the layer's `MapGrid` (section 11). Change IntGrid values with `set_intgrid` and `fill_intgrid`, which re-collide only the chunks they change; values written into `TileLayerData::intgrid` by hand are not seen until those chunks change.

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

- **Every grid but the hexagonal paints.** On an isometric or oblique map the cells sharing a corner or an edge are the layer's own, as on a square map. On a staggered map they are the isometric lattice's: the cells around a corner are the four diamonds that meet there (above, left, right and below it on screen), whichever axis and index the map has, as Tiled's brush paints them. A hexagonal map has no corner shared by four cells, so `set_terrain` warns once, paints nothing and returns false.
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

- **Isometric, staggered and oblique maps** collide along their cells' outlines: diamonds, and skewed cells. A tile's shapes are in its image's pixels, placed where it draws, and a shape on the four vertices of its diamond (an oblique tile's: a rectangle filling the tile, taken in the cell unskewed, which skews with the grid) is a solid cell, so a row of wall tiles is one seamless outline along its diamonds; a triangle on three of a diamond's vertices is a slope of it, and any other shape collides as drawn, clipped to its diamond. The grid is the map's square lattice (`MapGrid::lattice`), whose cells the collision grid holds, traced as on squares and mapped through `Lattice::to_pixel` (`TileColliders::lattice`); a staggered map's cell `c` is at `lattice.lattice_cell(c)` there, so `tiles.get(lattice.lattice_cell(c))` reads it. One-way platforms are for orthogonal maps: on any other grid a one-way tile collides both ways, and the dev log warns once a map.

<!-- given: map: Entity, cell: IVec2 -->
```rust
// What a staggered map's cell collides as: its grid holds its lattice's cells
let grid = ctx.maps().grid(map, "walls").map(|layer| layer.grid);
let lattice = grid.and_then(|grid| grid.lattice());
let kind: Option<TileCell> = lattice.and_then(|lattice| {
  let tiles = ctx.world.get::<TileColliders>(map)?;
  Some(tiles.get(lattice.lattice_cell(cell)))
});
```

- **Hexagonal maps** collide along their hexagons' sides, the collision grid holding the map's own cells (`tiles.get(cell)` reads one). A tile's shapes are in its image's pixels, placed where it draws: its flips mirror them, then its 60° and 120° turns (`TileFlip::HEX_ROTATE_60`, `HEX_ROTATE_120`) turn them about the tile's middle, as the tile is drawn. A tile with a polygon on its cell's corners (each within half a pixel, as it stands before its turns; a point along a side is neither needed nor in the way) is a solid cell however it is turned, and solid cells merge, so a clump of wall hexes is one seamless outline. Any other shape is its tile's own: its polygons, flipped and turned, collide as closed rings in that cell, as drawn, wherever they lie, neither clipped to the hexagon nor merged with the neighbours (a rock on a grass hex, a trunk on a forest hex); a polygon that crosses itself collides as its convex hull, and the dev log says so. One-way tiles collide both ways, warned once a map, as on every grid but the orthogonal.
- **Seamless and hollow:** full cells merge into outlines, so a long floor is one edge and nothing snags where two tiles meet. An outline is hollow: a body that ends up inside a wall is not pushed out.
- **Layers together:** the colliding layers on the ground floor that share the map's grid (its shape, cell size and origin), no offset and one material collide as one grid on the map entity, each cell taking the strongest kind among them: solid, then a shape, then a slope, then one-way. A colliding layer with a grid, offset, storey or material of its own collides as a grid of its own on its layer entity (one storey's layers that share all four, as one grid on the first of them); where two grids meet there can be a seam.
- **Storeys collide apart:** a storey's colliding layers collide on grids of their own, on that storey (`TileColliders::floor` is `Floor::On(n)`), where its layers stand in ground space (section 2): a wall upstairs stops a character on that storey (`Floor::On(n)`, `ryo-physics`, Floors) and lets one on the ground walk under it.
- **A raised storey's edge:** each storey above the ground has an edge that keeps what is on it from walking off into the air. It is the storey's footprint, the cells its tile layers fill plus its stairs (tiles whose `stairs` names it, on a lower storey's layers: section 4), traced as a grid of its own, `Solid` cells, on a `MapFloor { floor }` entity, a child of the map where the storey's layers stand, on that storey, with the map's collision material. Its outline is hollow: a character on the storey walks anywhere inside it and up or down its stairs, and stops at its rim; the storey's walls, a grid of their own, still stop it inside. A layer of the storey that stands elsewhere than its bottom tile layer (another grid or offset) is left out of the footprint, and the dev log says so once. Stairs on a lower storey's layer count only if that layer stands where the storey's bottom tile layer does (its grid, and its offset less its storey's height); stairs elsewhere are left out without a word. **Limit:** a cell whose only tile on the storey is a class tile is a hole in its edge (the tile leaves the grid to spawn, and is not drawn there either, so the hole matches what is drawn; picking with `floor_cell_at_world` does count it), so keep class tiles on a layer over the storey's floor tiles. The edge follows edits, hot reloads and streamed chunks as the map's other grids do, and goes with the map's colliders (disabled, its root put over or taken away, despawned). An edit keeps its `MapFloor` entity, but every full rebuild of the map's collision replaces it (the map enabled again, a change of its layers, their places, storeys or materials, a tileset saved with other shapes or stairs): don't keep the entity, find it by query (a `MapFloor` whose `ChildOf` is the map), or again after `MapEvent::Reloaded`.

<!-- given: part: Entity -->
```rust
// A tile collider part the hero ran into: a storey's rim, or a wall
let world = &*ctx.world;
let holder = world.get::<ChildOf>(part).map(ChildOf::parent);
if let Some(edge) = holder.and_then(|holder| world.get::<MapFloor>(holder)) {
  game_info!("the rim of storey {}", edge.floor);
}
```

- **On the ground floor a raised storey blocks only where it is given collision,** so a bridge stands over open ground: a character on the ground walks under a storey and its edge, and what should block there (a bridge's pillars) goes on a colliding layer of the ground floor.
- **Collision ignores parallax:** a colliding layer with parallax collides where it would be without it (parallax is a drawing effect, as in Tiled), and the dev log warns once per layer. Keep parallax for layers that do not collide.
- **Edits:** a changed tile re-collides in that step. Only the outlines through the changed cells are traced again, and only the colliders those outlines belong to change, taking their new shape in place, the same entities. So anything touching the rest of the map keeps its contacts: a character standing on the map keeps its `floor`, and no `CollisionStopped` or `CollisionStarted` is sent for it. On a hexagonal map only the colliders holding the changed cells' sides (at most four) are built again, and the one holding a tile's own shapes only where that tile's shape changed.
- **Which entity:** the outlines are packed by area, one collider for those starting in each 32 x 32-cell chunk (and one for each row of one-way runs in a chunk), each a child of the entity holding the grid: the map entity, or a layer that collides on its own. Each sits at its chunk's top-left (on other grids, its chunk's first corner on the map: lattice cell `chunk * 32` taken through the grid), unturned and unscaled (its `Transform`), its collider's points relative to that, so they stay small however far from (0, 0) the chunk is: add the translation to read them where they are. With no body on the map each is its own fixed body, so collision messages, `CharacterHit` and `CharacterState::floor` name such a part; follow `ChildOf` to the entity holding the `TileColliders` (the `MapRoot`, a layer with its `MapLayerRef`, or a storey's edge with its `MapFloor`, whose own parent is the map). With `RigidBody::kinematic()` on the map entity they name the map. On a hexagonal map a chunk's collider holds its cells' sides and another its tiles' own shapes (rings); a dense chunk's are its 16 x 16 quarters' or 8 x 8 blocks' instead, each kind by its own count, and the rings of a block heavy with them its four 4 x 4 sub-blocks' (at most 128 edges each where it can), each at its first cell's box corner.
- **A moving platform flush with the ground:** a character level with both the map's ground and a moving platform rides the platform.
- **A disabled map** has no colliders; enabled, it builds them from its tiles as they then are, on the main thread (section 2).
- **Long levels:** a grid holds the chunks its layers have, of any length: a 10,000 x 64 runner level collides end to end. A cell collides within 186,411 cells of (0, 0) on either axis (about 3 million px of 16 px tiles); a tile farther out, on an unbounded layer or on a bounded one long enough to reach it, draws and casts its shadow but does not collide, and the dev log says so once. That reach is where the outline lattice's 32-bit points run out, not where f32 positions stay precise: a `Transform`'s f32 pixels are an eighth of a pixel apart a million px out (2^20), a quarter of a pixel at the reach (Rapier's f32 metres about 0.2 px), and a whole pixel past 8.4 million px (2^23), so slow movement steps visibly well before the reach; keep precise play within about a million px of the origin.
- **Navigation:** `ctx.maps().navigation(map, layer, window)` is a `NavigationGrid` over `window` (a `CellRect`, max exclusive) of a tile layer's own cells, on any grid: six neighbours on a hexagonal map, the eight of its square lattice on every other, each step costing its world distance, so an isometric path takes its screen distance (`ryo-game-dev`, Navigation). Every colliding layer of the map blocks it: solid and slope cells, and shape cells whose shape covers the cell's centre; one-way cells stay walkable (off an orthogonal map one-way tiles collide both ways, and block as solid ones). A storey's edge is not a colliding layer and blocks nothing, here or in `rebuild_from_world`: it marks where the storey can be walked. It reads the map's collision as its tiles are now: the fixed step brings a map's colliders up to its tiles after `update`, so `navigation` runs that step's sync first, and a grid taken in `update` reads a tile set earlier in the same `update` and a chunk that landed this frame (under a microsecond a map when nothing changed; physics still builds the changed colliders in that step, once). Its tiles are placed by the map's transform as it is now, so a map moved this frame blocks where it now is. On a streamed map every cell of a chunk not landed is blocked too (section 12). `None` until the map lands, or for a layer that is not a tile layer. It takes `&mut self` (`let mut maps = ctx.maps();`). The grid is a copy of the map as it is now: add the game's own obstacles (`block_collider`, `block_rect`) and costs (`set`), and take it again after an edit. A grid taken again is a new grid whose `generation` is counted afresh, so find a path found on the old grid again rather than asking `path.is_stale(grid.generation())` of the new one. It is `NavigationGrid::for_layer(ctx.maps().grid(map, layer)?, window)` with each of the map's `TileColliders` blocked through `block_tiles(&tiles, to_world)`, `to_world` the whole world transform of the entity holding it; `rebuild_from_world` blocks every collider in the world, tile maps through `block_tiles` with their `GlobalTransform`, onto a grid of any kind (on a square world grid a map's diamonds and hexagons block every square they share area with).

<!-- self: board: Entity, paths: Option<NavigationGrid> -->
```rust
// The board's hexes as a navigation grid, its walls blocked and a marsh
// hex costing three steps: taken again after the board changes
let mut maps = ctx.maps();
if let Some(bounds) = maps.bounds(self.board) {
  self.paths = maps.navigation(self.board, "ground", bounds);
}
if let Some(paths) = &mut self.paths {
  paths.set(GridCoord::new(4, 2), CellState::Difficult(3.0));
}
```

## 9. Shadows

- A layer casts shadows as its `occludes` says, or as it collides when `occludes` is left out (`without_collision` does not change it). A light casts shadows only with `with_shadows()` (`ryo-lighting`).
- **What blocks light:** a tile's shapes marked `occluder` if it has any, else its collision shapes (one-way ones too). An IntGrid's solid cells block whole and its slopes as their half cell; a one-way cell has no area and casts nothing.
- The map draws its occluders as the boundary of what occludes: `LightOccluder2D::chain`s, a chunk's worth each, children of the first occluding layer and marked `MapOccluder { chunk }` (an `IVec2`: chunks are signed, as `TileGrid::chunk_of` gives them, so cell -1 is in chunk -1; on a staggered map, chunks of its lattice's cells). A chunk owns the borders left of and above its cells, so the right and bottom edges of a chunk's last cells can be the next chunk's chains. Tiles that touch cast no shadow between them, so lit art shows no seam. Edits rebuild only the chains of the chunks they touch.
- **Isometric, staggered and oblique maps** cast their shadows along the outlines they collide along (section 8): a wall of diamonds shadows from its diamonds' sides.
- **Hexagonal maps** cast theirs along what they collide along (section 8): a clump of wall hexes shadows from the clump's outer sides, and a tile's own shape (a rock, a trunk) from its own outline, wherever it is drawn, turned and flipped with its tile (its `occluder` shapes alone, if it has any), unmerged with the hexes beside it: where that outline lies along a neighbour's side (a solid hex's, or another shape's), both cast there, so under a light whose shadows are not at full strength that stretch of shadow is darker. A side belongs to the earlier of its two hexes in row order, so the chains of a hex's sides on its left and above can be the chunk's on its left or above, and an edit next to a chunk's left or top edge builds again that chunk's chains through the edited hex too. An edit builds again only the chains through the hexes it changed, and a tile's own shape only where that tile changed; on a hexagonal grid whose sides have length 0 (diamonds), or for an edit that changes more than 64 of the cells a chunk reads at once (its own and those just across its sides), it builds every chain of that chunk's sides again (its tiles' own shapes still only where they changed).
- **Storeys** (a map with floors, section 2): occluding layers cast as one where they share a grid, a storey and where they stand (a higher storey's layers stand in ground space), so each storey casts on its own, its chains children of its first occluding layer standing there. A higher storey's chains carry its `Floor::On(n)`: a light on a storey is blocked only by that storey's chains and by occluders on every storey, and it and its shadows draw lifted with the storey, lined up with its art (`ryo-lighting`, Floors). A light still brightens whatever lies under it on screen, on every storey: only shadows are kept per storey.
- The map owns its chains: a game can find them by `MapOccluder`, and a change it makes to one lasts until a change to its chunk builds it again. A chain's and a collider part's placement is the map's: a game's change to their `Transform` is not applied until the map next moves, and the map's rebuilds replace it. A disabled map casts nothing.
- Every lit frame gathers every map's occluders, and each shadowed light tests all of them (section 16).

## 10. Draw order, y-sorting and parallax

- **Z:** layer `i` draws at the map's z (`with_z`) plus `i`, among the entities; on a map with floors every y-sorted layer draws at one z (below). A plain tile layer draws as one item at its z, so entities at a higher z draw over it. Give a character a z above every layer it should cover and below every layer that covers it (the tilemap demo's hero draws at 10, over the platformer's seven layers at 0 to 6); at a tile layer's own z, the character and the layer draw in entity index order, which nothing should rely on; on a map with floors a plain layer on a storey above the ground draws after whatever is on a lower storey at its z (a character on the ground floor there draws under it, as under a roof), and before what is on a higher one. An image layer draws as a sprite does, after the entities at its z, so at an equal z it covers them.
- **Lit or not:** as `lit` says. **Cameras:** a map draws on the map entity's `RenderLayers`; a layer entity's own `RenderLayers` replace them for that layer.
- **Every grid draws as Tiled does.** A tile stands where Tiled puts it on its layer's grid (`MapGrid::tile_anchor`): its picture's bottom-left on the bottom-left of its cell's box, however large, moved by the tileset's offset (on an oblique map, on the skewed corner below the cell: the picture moves with the skew, it is not skewed). A hexagonal map's turned tiles turn after their flips, about their own middle where they stand unturned (the tileset's offset included), as Tiled turns them. Inside a plain layer tiles draw in Tiled's order whatever their tileset: rows top to bottom, each left to right (on maps staggered or hexagonal along x, a row's unshifted cells before its shifted ones, which sit half a cell lower), and on an isometric map the diagonals (cells of equal x + y, a row on screen), each left to right; so a tile taller or wider than its cell covers the tiles before it and is covered by those after it, as in the editor. A layer whose tiles fit their cells draws a draw per tileset per chunk; tall art one per chunk too where it is one tileset's on an orthogonal or isometric map, but art wider than its cell, and tall art on staggered and hexagonal maps or of several tilesets, draw more (section 16). On a pixel-art target an isometric map of odd cell sizes, whose half cells fall on half pixels, draws each tile where Tiled's renderer rounds it to, with no gap between tiles at any zoom; on a resolution-independent target its tiles draw at the exact points.
- **Y-sorted layers** (`y_sort`) draw their tiles as items at the layer's z, sorted among the y-sorted entities there by where the tiles stand: a character with `YSort` at that z walks behind a tree whose cell is below its feet and in front of one above them, on every grid. On an orthogonal map a tile stands on its row's bottom edge. On every other grid it stands at its cell's centre, where a character in the cell has its feet. A tall tile sorts at its base, whatever its height. Cells that share a sort point draw together as one item: a row on orthogonal, oblique (skewed across its rows only), staggered-along-y and hexagonal-along-y maps, half a row on maps staggered or hexagonal along x (its shifted cells sit half a cell lower), a diagonal (cells of equal x + y) on an isometric map. An oblique map skewed down its columns (`skew.y`) has no shared point in a row, so each of its cells is an item of its own. Give a tile the reserved tile property `sort_offset` (a float, in pixels, positive moving the sort point down, as `YSort::sort_offset` does for an entity) to move its sort point, such as a canopy that should cover a character standing a little behind its trunk: that tile is an item of its own too. Tile objects on a y-sorted object layer sort by their bottom edges. A character sorts with a y-sorted layer at that layer's z: `ctx.maps().y_sort_z(map)` gives the first y-sorted layer's (the map's z plus its index, an image layer not counting), once the map has landed.

<!-- given: hero: Handle<Image>, at: Vec2, garden: Entity -->
```rust
// The hero sorts among the garden's y-sorted rows ("trees", its layer 2,
// at the map's z of 0 plus 2), every storey's on a map with floors
let z = ctx.maps().y_sort_z(garden).unwrap_or(2);
ctx.world.spawn((
  Transform::from_translation(at),
  Renderable::image(hero).with_z_order(z),
  YSort::new(),
));
```

- **Storeys draw where Tiled draws them** (a map with floors, section 2). A layer on a storey above the ground stands in ground space and draws lifted by the storey's height: its tiles, an image layer, and tile objects; a camera culls it where it draws. Its y-sorted rows and tile objects sort where they stand, before the lift, as a character on the ground or on that storey sorts by where its feet stand; at one ground y the lower storey draws first, so a character walking under a bridge, its feet at the bridge row's ground y, draws under the bridge. **On a map with floors every y-sorted layer draws at one z**, the first y-sorted layer's (`y_sort_z`), so characters at that z sort with every storey's rows; plain layers keep their own z, and a map without floors keeps a z per layer as above. So make raised ground (a block, a wall top, an upper storey's floor) a y-sorted layer, and characters walk behind it and in front of it from both sides; a plain storey layer draws over or under every character at the y-sorted z, by its z, as any plain layer does. A character on a storey draws lifted as its tiles do (Floors, below).

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

### Floors: entities on a storey

An entity's `Transform` is where it stands on the ground, on every storey. Give it `Floor::On(n)` to put it on storey `n`: physics keeps it apart from what is on other storeys (`ryo-physics`), and it **draws lifted by that storey's height** on the map with floors under it, the map's own up (turned and scaled with the map), as the storey's tiles do. The map under it is the first map with floors, in entity order, whose cells' box holds where it stands; off every one, and on the ground floor, it draws where it stands. A disabled map is under nothing.

- **What a map spawns on a storey is on it.** Objects and class tiles of a layer on storey `n` (`n` from 1) carry `Floor::On(n)`, from its landing, an edit (a class tile placed by `set_tile`), a stream's chunks and a reload (an object layer moved to another storey takes its objects with it): so a sprite or collider the game gives a class tile on the bridge is on the bridge. The layers themselves, and what the ground floor holds, carry no `Floor`. A tile object draws through its layer, lifted once. One a game takes off its layer (a key picked up and carried) keeps its layer's `Floor`: the game that carries it sets its `Floor` to the carrier's, or removes it.
- **What draws lifted:** the entity and everything below it in the hierarchy, down to a child with a `Floor` of its own: sprites with their drop, blob and contact shadows, tiled runs, composite tiles, world text (`Text2d`) and lights with their shadows (section 9), so a sprite on a child, a held weapon, a torch and a name tag all go up with the character. A trail records the lifted point, from its first. Particles start at the lifted point, then fly, collide and are attracted where things stand: a lifted emitter's sparks meet colliders a storey's height below its drawn surface. A burst spawned and fired in the same `update` fires before its lift is known, where it stands: fire it on a later step, or spawn it unparented and without a `Floor` where the holder draws (its position plus `drawn_lift`).
- **What stays in ground space:** `Line2D` lines, the F2 overlay (colliders are where the entity stands, a storey's height under its sprite), debug draws, and every gameplay position: `GlobalTransform`, physics, triggers, cells (`grid`, `world_to_cell`). Sorting stays there too: a y-sorted character sorts by where its feet stand, so its order never jumps as it changes storey, and at one ground y the lower storey draws first (above).
- **When:** the lift is worked out after transforms propagate, in each fixed step and each frame, for the entities whose `Floor`, position or children changed (and every one when a map with floors moves or changes its storeys), so a storey or position changed in `update` draws lifted that frame. A map's cells changing on their own (a stream landing chunks, an edit past its box) or the map being disabled reaches an entity standing still when it next moves; a map enabled again lifts everything on it at once. `ctx.maps().drawn_lift(entity)` reads the lift (zero when not lifted), as it was last worked out.
- **A camera that follows a character** adds its lift, or it centres a storey's height under the sprite:

<!-- given: hero: Entity -->
```rust
// The camera on the hero where it draws, on whatever storey it is
let stands = ctx.world.get::<GlobalTransform>(hero).map(GlobalTransform::translation);
if let Some(stands) = stands {
  let at = stands + ctx.maps().drawn_lift(hero);
  ctx.camera().set_position(at.x, at.y);
}
```

### Stairs (and ramps)

Stairs and ramps are the same tiles to the engine: a tile whose reserved int property `stairs` names a storey (section 4) leads up to it from the storey of the layer it is painted on. Paint them on a lower storey's layer that stands where the upper storey's bottom tile layer does (its grid, and its offset less its storey's height: the ground layer, for the first storey up), as a run sticking out from the upper storey's cells. They join the upper storey's edge (section 8), so what is up there walks down them and no further.

- **A run** is a set of stair cells on one layer leading to one storey, joined through their sides. Its top is where it meets the upper storey's cells (the middle of the sides it shares with them), and its bottom is its foot: the side it shares with neither that is farthest from the top along the run. A run of three cells leading right into a storey climbs from its left side to its right.
- **`Climber`** opts an entity in (it brings a `Floor`, the ground floor unless you give one). Each fixed step, a climber on a run, on either of its storeys, **changes storey halfway along it**: onto the upper storey once it is halfway up or more, back onto the lower one below that. Each change is a `FloorChanged { entity, from, to }` message (`ryo-messages`), and physics filters by the new storey in that same step. An entity without `Climber` keeps its storey on stairs: a crate pushed onto them, or an NPC you move between storeys yourself.
- **Why halfway, not at the ends:** the upper storey's edge encloses its stairs, and the lower storey may have walls where the stairs arrive (the cliff under a raised garden, the wall under a balcony). Switching at the middle lets a character step off the run at both ends without either being opened.
- **Its drawn height eases** along the run, from the lower storey's height at the foot to the upper one's at the top, by how far up it is, and its children (its sprite, a held torch) with it. It is worked out every frame from where the climber draws, between fixed steps too, so it rises as smoothly as it walks. `drawn_lift` reads it, so a camera that adds it climbs with the character. Sorting keeps where the feet stand, as on any storey.
- **When:** the runs follow edits (`set_tile`, `fill`, Wang painting), hot reloads, tilesets saved with other stairs and streamed chunks, at the next fixed step. A climber is placed by where the last step left it, so it changes storey one step after crossing the middle.
- **Warnings:** stairs naming a storey the map lacks, their own or a lower one stay plain tiles, warned once a map (`stairs on layer "ground" lead to floor 5, which is not a floor of the map above the layer's floor 0, so they are plain tiles`). A run that leads nowhere changes no one's storey, warned once a map (`stairs on layer "ground" up to floor 1 lead nowhere (they ...)`): stairs on a layer standing elsewhere than the upper storey's bottom tile layer, a run that shares no side with the upper storey's cells or shares every side with them, or one that meets them as much on one side as on the other (a cell between two of them).
- **Limit: a run meets its upper storey only at its top.** Stairs running along a platform's face, touching it down their long side, take their top from that side and climb across themselves; turn them out from the face, so they meet it only at their top end.

<!-- given: at: Vec2, art: Handle<Image> -->
```rust
// A top-down hero who walks up and down the map's stairs; its sprite, a
// child, draws at its eased height
let hero = ctx
  .world
  .spawn((
    Transform::from_translation(at),
    Collider::circle(5.0),
    TopDownMovement::default(),
    Climber,
  ))
  .id();
ctx.world.spawn((
  Transform::from_xy(0.0, -8.0),
  Renderable::image(art).with_anchor(Anchor::BottomCenter),
  ChildOf(hero),
));
```

<!-- self: hero: Entity, upstairs: bool -->
```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  // Whether the hero is upstairs, kept as it goes up or down a storey
  for change in ctx.messages::<FloorChanged>() {
    if change.entity == self.hero {
      self.upstairs = change.to != Floor::GROUND;
    }
  }
  SceneAction::Continue
}
```

## 11. Maps built in code

A `TileMap` built in code spawns like a file's: added to the assets and given to a `MapRoot`, it draws, collides and edits the same way.

<!-- given: tileset: Handle<Tileset> -->
```rust
let mut map = TileMap::new(UVec2::new(40, 23), UVec2::splat(16)); // cells, a cell's pixels
let terrain = map.add_tileset(TilesetSource::Asset(tileset.clone()));
let ground = map.add_tile_layer("ground");
if let Some(tiles) = map.tiles_mut(ground) {
  tiles.fill(IVec2::new(0, 20), IVec2::new(39, 22), Some(Tile::new(terrain, 0)));
}
let map = ctx.assets().add(map);
ctx.world.spawn((Transform::from_xy(0.0, 0.0), MapRoot::new(map)));
```

- `TilesetSource::Asset(handle)` shares a tileset asset with other maps; `TilesetSource::Embedded(Arc<Tileset>)` is one the map owns. A map holds up to 256 tilesets of up to 1,048,575 tiles each.
- **A tileset in code:** `Tileset::new(name, tile_size, TilesetImage::Atlas { image, size })` with its `columns` and `tile_count` set (and `spacing`, `margin`), or `TilesetImage::Collection` with each tile's own image in its `TileData`. Tiles collide through their shapes: `insert_tile(id, TileData { shapes: vec![TileShape { points, ..TileShape::default() }], ..TileData::default() })`, the points in tile pixels from the top-left.
- `add_object_layer(name)` adds an object layer, whose objects are `MapObjectData` in `LayerKind::Objects`; `layer_index(name)`, `tiles(layer)` and `tiles_mut(layer)` find layers.
- **An unbounded layer:** a tile layer whose grid is `TileGrid::unbounded()` (set through `tiles_mut`) holds any cell, negative ones included, in chunks made where tiles are written. Spawned, it draws, collides, casts shadows and spawns class tiles wherever it has chunks, and costs the chunks it has, however far apart: never the rectangle between them. Its colliding layers' grid is unbounded too; a cell collides within 186,411 cells of (0, 0).
- **Other grids:** a map's `shape` lays its cells out as Tiled's other grids do: `TileMap::new(size, tile_size).with_shape(shape)` with `GridShape::Isometric` (diamonds), `Staggered { axis, index }` (diamonds in rows or columns, every other one shifted half a cell), `Hexagonal { axis, index, side }` (pointy-topped hexagons in rows for `StaggerAxis::Y`, flat-topped ones in columns for `X`; `side` is Tiled's hex side length) or `Oblique { skew }` (each row shifted `skew.x` pixels across, each column `skew.y` down); `GridShape::Orthogonal` is the default. `map.grid()` is the map's `MapGrid`, with Tiled's origin for its size (a bounded isometric map's cell (0, 0) has its top vertex the map's height in half cells across), and each tile layer copies it as it is added: `with_shape` moves the layers already added, setting `map.shape` moves none. A `MapGrid` answers where a cell is in the layer's pixels (`cell_center`, `cell_outline`, `cell_bounds`, `tile_anchor`), which cell is under a point (`cell_at`), and board questions on any grid (`neighbours`, `distance`, `line`, `range`, `ring`). `board.lattice()` is the square lattice the cells of every grid but the hexagonal make (`Lattice`: `to_pixel` its affine to the layer's pixels, `lattice_cell(cell)` and `grid_cell(lattice)` between its cells and the grid's; `None` on a hexagonal grid): the grid's own cells on orthogonal, isometric and oblique grids, re-indexed on staggered ones, so code that works on a square grid, a generator's, places its cells through `grid_cell` and they keep their neighbours, as procgen's conversions do (`ryo-procgen`). Spawned, the layer's grid is `TileLayerData::grid`, and `ctx.maps().grid(map, layer)` gives it with the layer's place in the world (section 6): class tiles stand at their cell's centre on it, `ctx.maps()` picks cells, edits them and gives their centres through it, a stream loads the chunks round its anchors on it and its `ChunkRequest` carries it (section 12), a save that changes it respawns the layer (section 14), and its tiles draw where Tiled places them on it (section 10). Its tiles collide and cast shadows along its cells' outlines on isometric, staggered and oblique grids, and along its hexagons' sides and its tiles' own shapes on a hexagonal one (sections 8 and 9), and a y-sorted layer on one sorts as section 10 says. A Tiled map of another grid loads with it (section 1).

<!-- given: tileset: Handle<Tileset> -->
```rust
let mut map = TileMap::new(UVec2::new(20, 20), UVec2::new(64, 32)).with_shape(GridShape::Isometric);
let terrain = map.add_tileset(TilesetSource::Asset(tileset.clone()));
let ground = map.add_tile_layer("ground");
if let Some(tiles) = map.tiles_mut(ground) {
  tiles.fill(IVec2::new(0, 0), IVec2::new(19, 19), Some(Tile::new(terrain, 0)));
}
// Where cell (3, 2) is in the map's pixels, which cell is under a point, and the cells round it
let board: MapGrid = map.grid();
let centre: Vec2 = board.cell_center(IVec2::new(3, 2));
let picked: IVec2 = board.cell_at(centre + Vec2::new(0.0, 10.0));
let around: Vec<IVec2> = board.neighbours(IVec2::new(3, 2)).collect();
let map = ctx.assets().add(map);
ctx.world.spawn((Transform::from_xy(0.0, 0.0), MapRoot::new(map)));
```

- **A map grown at run time:** `TileMap::infinite(tile_size)` is an empty infinite map, with `size` zero and `infinite` true, whose `add_tile_layer` layers are unbounded. A game spawns it empty and grows it by writing tiles through `ctx.maps()`: a write at any cell, negative or far out, makes the chunk it lands in, which then draws, collides and casts shadows like any other. Nothing is paid for the space between chunks.
- **Chunks persist.** A chunk, once made, stays for as long as the spawned map does, emptied or not: only a stream unloads chunks (section 12), so a map a game grows by writing keeps every chunk it ever wrote in. A bounded map has every chunk of its size from its landing (its collision, shadows and change tracking keep an entry for each), so a huge bounded map built with `TileMap::new` is slow to land and costly to keep however few tiles it holds; an infinite one costs nothing until a tile is written.

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
- **Procgen:** `GenerationGrid::to_tile_map` and `to_terrain_map` (`ryo-procgen`) turn a dungeon, a cave or noise terrain into a `TileMap` of any grid but the hexagonal: the generator's square grid is the map's square lattice, so on an isometric or staggered map its cells keep their neighbours. The tilemap demo's Procgen dungeon page draws one through a Wang set.

## 12. Streaming an endless world

A map can go on forever. Give an infinite map a generator, in a `MapStream` on its root, and the engine fills its 32 x 32-cell chunks as cameras and `StreamAnchor` entities come near, and unloads them once they are far behind. The generator is a function from a chunk to its tiles; it runs on a thread of its own, so the frame never waits for it. The chunks a player changed can be kept, in a `SavedChunks` that goes into a save game, and come back as the player left them.

<!-- given: tileset: Handle<Tileset> -->
```rust
/// The ground's top row at column `x` of a side-on world: a smooth noise
/// of the column, from row 20 to row 31
fn ground_top(x: i32) -> i32 {
  let hash = |n: i32| {
    let n = (n as u32).wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    let n = (n ^ (n >> 15)).wrapping_mul(0x2C1B_3C6D);
    (n ^ (n >> 12)) as f32 / u32::MAX as f32
  };
  let (knot, t) = (x.div_euclid(16), x.rem_euclid(16) as f32 / 16.0);
  let t = t * t * (3.0 - 2.0 * t);
  20 + ((hash(knot) * (1.0 - t) + hash(knot + 1) * t) * 12.0) as i32
}

let mut map = TileMap::infinite(UVec2::splat(16));
let terrain = map.add_tileset(TilesetSource::Asset(tileset.clone()));
map.add_tile_layer("ground");
let map = ctx.assets().add(map);
let (grass, dirt) = (Tile::new(terrain, 0), Tile::new(terrain, 1));
let stream = MapStream::new(move |request: ChunkRequest| {
  let mut tiles = ChunkTiles::default();
  for x in 0..32 {
    let top = ground_top(request.first_cell.x + x as i32);
    for y in 0..32 {
      let row = request.first_cell.y + y as i32;
      let tile = if row == top { Some(grass) } else if row > top { Some(dirt) } else { None };
      tiles.set("ground", UVec2::new(x, y), tile);
    }
  }
  tiles
});
ctx.world.spawn((Transform::from_xy(0.0, 0.0), MapRoot::new(map), stream));
// The player, a platformer character, keeps the chunks round it loaded, wherever the camera looks
ctx.world.spawn((
  Transform::from_xy(64.0, 256.0),
  Collider::rect(14.0, 26.0),
  PlatformerMovement::default(),
  MoveBindings::default(),
  StreamAnchor,
));
```

- **The generator** gets a `ChunkRequest { chunk, first_cell, grid }` (`grid` the streamed layers' `MapGrid`, section 11) and returns a `ChunkTiles`: `set(layer, cell, Option<Tile>)` with `cell` inside the chunk (0 to 31 each way; the map's cell is `first_cell + cell`), layers by the map's names. A layer it gives nothing lands empty, and a name the map has no tile layer of is left out, warned once. It runs on the `ryo2d chunk builder` thread, so it must be `Send + Sync` and cannot touch the world: make it a pure function of the chunk and what it captured (a seed, a `ryo-procgen` noise). It is asked again for a chunk that comes back after unloading (unless the stream kept it, below), so the same chunk must come out the same.
- **Noise from `ryo-procgen`** (the game adds `ryo2d-procgen`) is sampled at the chunk's own cells by offsetting it to `first_cell` with `NoiseConfig::with_offset`. Procgen does not normalize a call's values to that call (each is the noise at its cell), so chunks side by side meet, and the same chunk comes out the same:

  <!-- with: use ryo2d_procgen::prelude::*; -->
  ```rust
  // Caves: open where the noise at a cell is low, the same seed for every chunk
  let caves = NoiseConfig::new().with_scale(0.075).with_octaves(2);
  let stream = MapStream::new(move |request: ChunkRequest| {
    let first = request.first_cell;
    let at_cells = caves.clone().with_offset(f64::from(first.x), f64::from(first.y));
    let field = generate_heightmap(32, 32, &at_cells, &mut SeededRng::new(77));
    let mut tiles = ChunkTiles::default();
    for (i, value) in field.iter().enumerate() {
      if *value >= 0.36 {
        let cell = UVec2::new(i as u32 % 32, i as u32 / 32);
        tiles.set("ground", cell, Some(Tile::new(0, 1)));
      }
    }
    tiles
  });
  ```
- **Distances** are in world pixels, along each axis: chunks within `load_distance` (512) of a camera's view or an anchor load, nearest first, and chunks farther than `unload_distance` (1,536) from every one unload; the gap keeps a chunk at the edge from loading and unloading as the player paces. Set them as fields: `MapStream { load_distance: 256.0, ..MapStream::new(generator) }`. Every camera's view is an anchor unless `cameras` is false (a minimap camera zoomed far out would load a wide area: an anchor's area is capped at 64 chunks a side, warned). A view loads what each tile layer shows there: a layer with parallax (section 10), or offset from the map, also has its chunks loaded where it shows them, however far that is from the view's own, at the cost of those chunks (each resident in every layer). An `unload_distance` below `load_distance` is raised to it, warned.
- **Every grid streams.** A chunk is 32 x 32 cells by index on every grid, so on an isometric map it is a big diamond, on a staggered or hexagonal map a strip of shifted rows or columns, and on an oblique one a parallelogram. An orthogonal map loads the chunks a view's or an anchor's area overlaps, as it always has; any other map loads every chunk with a cell in the area, and of the others only those whose box (the box round its cells) reaches it (a 1080p view grown by 512 px over 32 x 16 isometric cells takes about 39 chunks, those holding a cell in it). A camera turned or zoomed out loads the chunks of the box round its view, so it never shows a chunk not landed once the stream has caught up. The generator gets the chunk's cells by index (`first_cell + cell`) and the grid in the request (`ChunkRequest::grid`), with which it can place things in pixels (`cell_center`, section 11); a square generator writes on an isometric or staggered map through the grid's lattice (`lattice()`, section 11). The map's streamed layers (its unbounded tile layers) share the first one's grid for this: one on another grid is streamed by the first one's, warned once.
- **Landing:** at most `land_per_frame` (2) chunks land a frame, nearest the anchors first. Each chunk landed or unloaded builds or takes down its collision and shadows in that frame (about 0.1 ms each, natively), and a camera crossing a 1080p view at 2,000 px a second needs about half a chunk landed and half unloaded a frame on average, in bursts as a row or column of chunks comes in, so the defaults keep up with room to spare; raise them for a faster or zoomed-out camera, at that cost a frame. A landed chunk draws, collides, casts shadows and spawns its class tiles (`with_entity_tiles`) like a map's own, and `ChunkEvent::Loaded { map, chunk }` is sent, its class tiles already in: place the chunk's enemies and pickups then, each with `ChunkOwned { map, chunk }` so it goes with the chunk (example below).
- **Unloading:** at most `unload_per_frame` (2) chunks unload a frame, the farthest from the anchors first; the others stay resident (drawn, colliding, editable) until their turn, so after a teleport the old area goes over a few frames and the resident chunks are within the distances again once it has. A frame that lands chunks unloads none while eight or fewer wait, whatever the budget, so landing and unloading rarely add their costs to one frame: the chunks a crossing leaves behind go in the frames after its landings, and while chunks keep landing at most eight stay resident past the unload distance (with more waiting, after a teleport or under a camera faster than the land budget, frames landing unload too, at the budget). An unloading chunk's tiles and class tiles go, and so do the entities marked `ChunkOwned` with it (their children too, and disabled ones), in `PreUpdate`; its colliders and shadows follow within the frame. `ChunkEvent::Unloading { map, chunk, changed }` says so (in a frame that both lands and unloads, after the frame's `Loaded` messages), and by the time the game reads it nothing of the chunk is left to read from the map: `changed` holds its tiles when the player changed it (below), and is `None` for a chunk as it landed. Entities the game spawned there without `ChunkOwned` are the game's to despawn, and a map's root despawned or put over takes the `ChunkOwned` entities of all its chunks, sending no `Unloading`. A shadow chain along an unloaded chunk's left or top border can stay while the cells beside it remain, closing their shadow at the gap: `MapOccluder::chunk` names the unloaded chunk.
- **What the player changed stays changed** when the stream has a `SavedChunks` (`MapStream::new(generator).with_saved(saved)`). A chunk changed since it landed (a tile edit through `ctx.maps()`, a class tile an edit placed or replaced, or one the game despawned: a coin collected, an enemy killed) goes into it as it unloads, its tiles and the class tiles still living there, and comes back from it instead of the generator, its class tiles spawning again. The changed chunks still resident go in as the map's root goes too (despawned, a scene's cleanup included, or a `MapRoot` put over it), with no `ChunkEvent`, so leaving the scene keeps the player's edits. So a coin left there is back, and a coin collected stays collected, whether or not anything else changed. A chunk as it landed is not kept: it comes back from the generator. `SavedChunks` is a shared store (a clone is the same store): `get`, `insert`, `remove` (forget a chunk, so it regenerates: an area the game resets), `contains`, `keys` (the chunks kept, by row: a copy, for a map of what the player changed), `len`, `is_empty`. It grows with every chunk the player changes and never forgets one by itself. It keeps chunks by their place alone, so each streamed map needs its own `SavedChunks`: two maps sharing one would bring back each other's chunks. A saved chunk holds tiles, not IntGrid values (a game's own, on an unbounded layer): during play they stay in the layer, which unloading leaves alone, but a save game written from the store loses them.
- **Saving the world:** `SavedChunks` serializes with serde (each layer's cells packed small), so it goes in a save game; give the copy loaded back to the new stream. A chunk goes in only as it unloads or as the map goes, so before writing a save during play call `ctx.maps().save_changed_chunks(map)`, which puts in the changed chunks still round the player.
- **Edits** through `ctx.maps()` reach only the chunks that have landed (section 6), so a write never makes a chunk the stream would not unload; a write to a layer's `TileLayerData::tiles` itself is not checked, so keep it to landed chunks too.
- **A respawn or a teleport:** the chunks round the old place unload, the farthest first, and those round the new one land nearest first, each within its budget a frame, over the next frames. Hold a loading screen until `ctx.maps().stream_ready(map, area)` says every chunk a world rectangle takes (as a view's area takes them, on any grid) has landed, rather than dropping a body into ground not made yet.
- **What a stream has now:** `ctx.maps().stream_status(map)` is a `StreamStatus { resident, generated, hand_made, in_flight, draining }` (`Copy`), as this frame's stream left it (the stream runs in `PreUpdate`, so a scene's update and UI read this frame's): the chunks resident, of them those the stream landed and the file's, the chunks asked for and not landed (out on the chunk builder or waiting their turn to land; 64 held at most), and the resident chunks past every unload distance waiting their turn to unload. `None` for a map with no `MapStream`, or not landed yet. For a HUD, a loading bar or a test.
- **A generator that panics** on a chunk is caught on native platforms: the chunk lands empty, is not asked for again, and the dev log warns once a map, naming the chunk. A web build aborts on a panic, so there a panicking generator ends the game: keep it from panicking (no `unwrap` on its inputs, no indexing past an array).
- **The web has no threads:** the generator runs on the main thread, within `web_budget` (2 ms) of each frame, at least one chunk a frame. Keep it cheap there, or set a smaller budget and let chunks arrive over more frames.
- **A Tiled infinite map streams too**, its saved chunks the hand-made part of the world and the generator filling everything else: spawn it with a `MapStream` as any map. Every engine chunk (32 x 32 cells) that any layer's Tiled chunk touches belongs to the file, the cells Tiled left empty included, so a deliberately empty plaza beside hand-made ground stays empty: the generator is never asked for those chunks, and fills only chunks the designer never touched. The file's chunks are resident from the start, wherever the anchors are: they draw, collide, take edits and count for `stream_ready`, and never unload, so a big hand-made area stays in memory. They land with the map (`MapEvent::Spawned`) and send no `ChunkEvent`. Since they never unload, a save game keeps their edits only through `save_changed_chunks`, which puts in each one changed since the map landed, or as the map's root goes; a stream given that `SavedChunks` puts them back as it starts.
- A map built in code with tiles in its asset keeps those chunks the same way; one built empty with `TileMap::infinite` leaves every chunk to the stream. A map with no unbounded tile layer streams nothing, warned.
- **Paths over a streamed map:** take a grid over the window of cells the game paths in, such as the cells within the load distance of the player, with `ctx.maps().navigation(map, layer, window)` (section 8). Every cell of a chunk not landed is blocked, so a path keeps to ground that has landed and never runs into a chunk not generated yet. The grid is a copy and does not follow the chunks as they come and go: take it again after `ChunkEvent`s or edits, in the same `update` (example below): it reads the collision of a chunk that landed this frame. A grid filled with `rebuild_from_world` treats chunks not landed as open.
- **Limits:**
  - A `ChunkOwned` entity goes only with the chunk it names. One tagged with a chunk not resident, or placed from a `Loaded` read after its chunk unloaded again, stays until that chunk next lands and unloads or the root goes; one that moves goes when the chunk it was tagged with unloads, wherever it is (change its `ChunkOwned` as it crosses into another chunk).
  - Replacing a live root's `MapStream` keeps the chunks the old generator made, and later unloads and the root's going put the player's changes into the new store: to stream afresh, despawn the old root (its changes go to the store it holds) and spawn a new one with its `MapRoot` and `MapStream` together.
  - A save of the map that makes a chunk hand-made where the player had changed the generated one leaves the player's copy in `SavedChunks`, and the next stream of the map puts it back over the new hand-made chunk: `remove` that chunk from the store.
  - Every anchor's area is checked against the others' each frame, so hundreds of anchors cost far more than their chunks; a few cost nothing to speak of.
  - A hot reload unloads every chunk it touches in its frame, the budgets aside: a hitch while developing.
  - Streaming on the web has not been measured.

<!-- self: world: Entity, player: Entity, loading: bool -->
```rust
// After a respawn: the loading screen stays until the view round the player has landed
if self.loading {
  let at = ctx.world.get::<Transform>(self.player).map(|t| t.translation).unwrap_or_default();
  let view = Rect::new(at.x - 960.0, at.y - 540.0, 1920.0, 1080.0);
  self.loading = !ctx.maps().stream_ready(self.world, view);
}
for event in ctx.messages::<ChunkEvent>() {
  if let ChunkEvent::Loaded { chunk, .. } = event {
    game_info!("chunk {chunk} landed: its class tiles are in");
  }
}
```

Enemies and pickups placed as a chunk lands, which go when it unloads and are placed afresh when it lands again:

<!-- self: world: Entity, slime: Handle<Image> -->
```rust
for event in ctx.messages::<ChunkEvent>() {
  if let ChunkEvent::Loaded { map, chunk } = event
    && map == self.world
  {
    // A slime on the middle of the chunk's top row: 32 cells of 16 px a side
    let at = chunk.as_vec2() * 512.0 + Vec2::new(256.0, 8.0);
    ctx.world.spawn((
      Transform::from_translation(at),
      Renderable::image(self.slime.clone()),
      ChunkOwned { map, chunk },
    ));
  }
}
```

Paths kept to the landed ground on an orthogonal map, the grid taken again as its chunks come and go:

<!-- self: world: Entity, player: Entity, paths: Option<NavigationGrid> -->
```rust
let chunks_moved = ctx.messages::<ChunkEvent>().iter().any(|event| match event {
  ChunkEvent::Loaded { map, .. } | ChunkEvent::Unloading { map, .. } => *map == self.world,
});
if chunks_moved || self.paths.is_none() {
  // The cells within 512 px of the player
  let at = ctx.world.get::<Transform>(self.player).map(|t| t.translation).unwrap_or_default();
  let mut maps = ctx.maps();
  let near = Vec2::splat(512.0);
  let corners = (
    maps.world_to_cell(self.world, "ground", at - near),
    maps.world_to_cell(self.world, "ground", at + near),
  );
  if let (Some(lo), Some(hi)) = corners {
    let window = CellRect::new(lo, hi + IVec2::ONE);
    self.paths = maps.navigation(self.world, "ground", window);
  }
}
```

Keeping the player's changes in a save game, with `SaveManager` (`ryo-config-save`; the game adds `serde`):

<!-- with: use serde::{Deserialize, Serialize}; -->
<!-- self: world: Entity, saves: SaveManager, chunks: SavedChunks -->
<!-- given: map: Handle<TileMap>, stream: MapStream -->
```rust
/// What a save game holds of the world
#[derive(Serialize, Deserialize)]
struct WorldSave {
  chunks: SavedChunks,
}

// Entering the world: the chunks the player changed, from the last save
self.chunks = match self.saves.load_game::<WorldSave>("world") {
  Ok(save) => save.data.chunks,
  Err(_) => SavedChunks::default(),
};
// The stream keeps the changed chunks in the same store: a clone shares it
let stream = stream.with_saved(self.chunks.clone());
self.world = ctx.world.spawn((MapRoot::new(map), stream)).id();

// Saving: the changed chunks still round the player go in, then the store is written
ctx.maps().save_changed_chunks(self.world);
let save = WorldSave { chunks: self.chunks.clone() };
if let Err(error) = self.saves.save_game("world", &save) {
  game_warn!("the world was not saved: {error}");
}
```

## 13. Worlds of levels

A world is a set of levels with their places and neighbours: a Tiled world file (`.world`, a `TileWorld`) or an LDtk project's worlds (`LdtkProject::worlds`). Each world's `WorldLayout` lists its levels (`WorldLevel { name, position, size, origin, depth, neighbours }`, in world pixels: `position` and `size` its rectangle, `origin` where its map's pixel (0, 0) sits, where its root goes). Which levels exist when is the game's call; `ctx.maps().spawn_levels_in` spawns the levels overlapping an area that are not spawned yet, each a `MapRoot` at its level's place with a `WorldLevelOf { world, level }`:

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
- **Where a Tiled world puts a map:** at its tile origin. The world's `x` and `y` are where the map's pixel (0, 0) sits (`origin`, where `spawn_levels_in` puts its root): cell (0, 0)'s corner (on an isometric grid its top vertex), or on a bounded isometric map the top-left of its diamond's box. Its rectangle is its cells' box on its grid (`MapGrid::bounds_of`) from there, read from its file: a bounded map's every cell, so an isometric map of 6 x 4 cells of 64 x 32 is 320 x 160 (`(6 + 4)` half cells each way), and an infinite map's tiles' bounds (`bounds()`), whatever `width` and `height` the world lists it with (Tiled writes them for every map). `position` is `origin` plus the box's top-left, which is `origin` itself for a bounded map unless an oblique skew is negative, and reaches left and up for an infinite map's tiles at negative cells. An infinite map with no tile takes its origin cell as its rectangle, so it can still spawn. A bounded map listed with a `width` and `height` takes that size. Tiled documents no rule for infinite or non-orthogonal maps in worlds, and its world loading could not be run headless to check one, so this is the engine's reading. The world reads each map it lists, so saving one loads the world again.
- Nothing spawns until the world's asset is loaded. A Tiled world's maps are its dependencies, so it is ready when they are; a map that fails fails the world.
- **Neighbours:** each level's `neighbours` (`Neighbour { level, direction }`, `level` an index in the same layout). A Tiled world's are worked out from the rectangles: sharing an edge is `North`, `South`, `East` or `West`, a corner a diagonal (`NorthEast` ...), an overlap `Overlap`. LDtk's are LDtk's own, `Above` and `Below` for levels at another depth.
- **Tiled world patterns** (`"regexp"` with `multiplierX`/`multiplierY`): the registry cannot list a folder (the web has none), so a pattern must be literal text with up to two numbers, `(\d+)` or `(-?\d+)`, as in `room_(\d+)_(\d+)\.tmx` (one number is x, two are x then y). The loader tries names row by row: from y = 0, x from 0 up to the first missing file, stopping at the first row whose x = 0 file is missing, in at most 1,024 tries. It builds each name, writing the numbers plainly, so it finds `room_1_2.tmx` but never `room_01_02.tmx` (a literal `0` in the pattern, `room_0(\d+)\.tmx`, reaches `room_00` to `room_09` and stops at `room_010`), and maps numbered from 1 are not found at all: the load fails, no map matching. Negative numbers and gaps in the numbering are never reached either; a world named any of these ways lists its maps instead. Any other regular expression fails the load.
- **Saving the `.world` file** leaves the levels already spawned where they are; the next `spawn_levels_in` uses the new layout. A save that reorders the levels can make it spawn a level that is already there: despawn and respawn the levels after such a save.

## 14. Hot reload

In debug builds with a window the engine watches `assets/` (`EngineBuilder::hot_reload` turns it on or off in any build). Saving a map, an LDtk project or one of its level files, a tileset, a template or the Tiled project updates every map spawned from it in the frame the save lands, before `update`, and sends `MapEvent::Reloaded`. The camera, the player and everything else keep their state.

- **Saving the Tiled project** loads the maps and the tilesets that use it again, and their loads can land a frame or two apart: the map follows each as it lands, with a `Reloaded` for each, the last with the save in full.
- **Tiles:** each tile layer takes the saved cells, and only the chunks the save changed draw, collide and cast shadows again. A chunk's collider changes shape in place, so a character standing on ground the save did not change keeps its contacts and its floor. An infinite map's layers follow their chunks: a chunk the save added draws, one it dropped empties, one it changed moves on, and the rest are untouched.
- **A streamed map** (section 12) takes the save in the file's chunks alone, and a tileset save changes none of its generated chunks (its file chunks come back to the file, edits reverted, as any map's do): the chunks its stream generated or brought back keep their tiles, class tiles and versions, so they are not counted as changed. A chunk the save added over a generated one takes the file's tiles (the generated one unloads first, with `ChunkEvent::Unloading`), and one the save dropped unloads and is the generator's from then on. A save that adds, removes or reshapes a tile layer unloads every generated chunk (kept in `SavedChunks` when the player changed it) and streams it again into the layers as they now are.
- **Layers** keep their entities, matched by the editor's layer id; a layer the save added spawns, one it removed despawns, and a tile layer saved at another size, grid or offset spawns afresh.
- **A map saved with another grid** (its orientation, stagger, hex side, skew or tile size changed in Tiled) respawns its tile layers on the new grid: new layer entities, their class tiles spawned again where the new grid puts their cells, and their colliders and shadows built again from them, nothing of the old grid left. Code holding a layer's entity or a `LayerGrid` (section 6) reads it again after `Reloaded`.
- **Objects** match by id. One the save left as it was keeps its entity untouched: its `Transform` and every component the game added, as the game has them. One the save changed takes its new `MapObject` and `Transform`, keeping the game's other components. One the save added spawns, and one it removed despawns. Class tiles match the same way, by layer and cell.
- **What the game took away stays away:** an object or class tile the game despawned (a coin collected, an enemy killed) is not spawned again unless the save changed it.
- **The file wins over the game's tile edits:** a dug tile comes back with the save.
- A tileset image's save redraws the maps that use it, with no reload and no `Reloaded`.
- A save that fails to load (a typo in the XML) keeps the last good map and sends nothing; the dev log names the file and the place in it.
- A map added in code never reloads, though it follows its tilesets' files. An LDtk project saved without the map's level leaves the map as it is, warned once.
- **Limits:** a tileset save that changes collision shapes rebuilds the colliders of every map using it whole: new collider entities, so a standing character's `floor` changes and `CollisionStopped` is sent, though nothing falls through. A tileset saved naming a larger image at the same moment as that image (a `git pull`, a script) can stay as it was until it is saved again.
- **Cost:** a reload runs on the main thread, about 1 ms for each 4 million cells whatever the save changed, plus what changed: a hitch while developing, never in a release build.

## 15. The web

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
- **A streamed map's generator** runs on the main thread there, within its `MapStream::web_budget` of each frame, and a panic in it aborts the game rather than being caught (section 12).
- Every layer format loads on the web, zstd included. There is no hot reload.
- The demo builds for the web: `trunk serve --release --open` in `examples/demos/tilemap`.

## 16. Performance

Measured on an Apple M5 Pro in a release build, at 1920 x 1080 with 16 px tiles; the numbers are approximate and machine-dependent. The record, with the method and every measurement, is `docs/backlog/tilemap.md` in the Ryo2D repository.

- **How big a map loads:** a tile layer holds at most 67,108,864 cells (8192 x 8192), and a map's tile layers together 134,217,728 (2^27, 512 MiB of 4-byte cells: an 8192 x 8192 map of two layers, or a 1024 x 1024 map of 128). Cells count as the engine stores them, in whole chunks of 32 x 32: a layer counts every chunk it spans (a 10,000 x 64 runner 641,024 cells, a 1 x 1 layer 1,024, so a 1-cell-wide layer 2^26 tall is refused), and an infinite map's layer the chunks its chunks reach, or its chunks' cells if more. An LDtk level counts each layer, empty or not, and an IntGrid layer's values once more, and all the levels of a project share the same total, since a project loads whole. Past it the load fails, naming the map, level or layer, before anything is allocated.

- **Cheap:**
  - **Drawing** costs what is on screen: a frame draws only the chunks in view, so a static 1024 x 1024 map of four layers costs about 35 µs a frame to draw, as a small one would; about 37 µs with its colliders and shadow chains, and about 39 with its layers unbounded.
  - **A one-tile edit**, an outline split or joined included: about 17 µs to redraw its chunk, about 30 to 43 µs to rebuild the collision outlines through it and about 33 to 66 µs for the shadow chains. An edit also checks every chunk of the layer it wrote in, so its cost grows with the layer's chunk count: largest on a big unbounded layer.
  - **A still map's colliders and shadow chains:** each is a child entity, but transform propagation leaves them out of its check for changes (the map's own moves place them), so the colliders and shadow chains of a still 1024 x 1024 map (about 2,900 children) add only about 1 to 3 µs a frame to drawing it, physics' look at its colliders.
  - **Loading** runs off the main thread: a 256 x 256 four-layer Tiled map with its tileset and image loads in about 1.2 ms on the workers (about 4.8 ms saved as an infinite map in 16 x 16 chunks), and its colliders and shadows are worked out on the map builder thread.
- **What costs a game:**
  - **Moving a map moves every collider and shadow chain under it:** moving its root or a layer makes transform propagation recompute every one of them, in up to three runs a frame: at least 17 to 24 ns each a frame, what merely visiting them cost (at least 50 µs for the 1024 x 1024 cave's 2,900). So does a fixed step that changes a fifth of the world's entities (and more than 64: about 1,000 movers beside that map's 4,096 entities): propagation then walks every hierarchy, at about what the changed entities' own walks would cost. Keep big maps still, and turn collision off (`without_collision`, or `collides` false) and shadows off (`occludes` false) on maps and layers that need neither: they cost physics and lighting too.
  - **Hexagonal maps collide and cast shadows at a higher cost.** A hexagonal outline has no straight runs to merge, so its colliders split into more parts and its shadow chains keep every side. The 1024 x 1024 cave map on hexagons, colliding and casting shadows, costs about 46 µs a frame against about 37 on squares (about 5,250 colliders and chains against 2,900, and more chunks to draw), and an edit costs up to about 56 µs to rebuild its collision and up to about 45 µs for its shadow chains, only the chains through the cells it changed being built again (about 27 µs where four chunks of scattered islands meet, about 45 µs where four chunks of rock four cells in ten meet). A tile's own shapes collide in colliders of their own, apart from the hexagons' sides, so an edit beside them leaves them be, and one of them rebuilds only the shapes near it: in a chunk full of tiles with shapes of their own, about 31 to 36 µs for its collision and 11 to 14 µs for its shadows. Turn shadows off (`occludes` false) on hexagonal layers that need none.
  - **A y-sorted layer** draws each visible row (or diagonal, or half row) of each chunk as an item of its own: a dense y-sorted layer of the 1024 x 1024 map cost about 70 µs more a frame. A tile with a `sort_offset`, and every cell of an oblique layer skewed down its columns, is an item of its own, so such a layer costs more. Y-sort only the layers characters walk among.
  - **Art that leaves its cells** keeps Tiled's order with more draws. Tall art of one tileset on an orthogonal or isometric map draws as art that fits, a draw per chunk; of several tilesets, a draw per tileset per row per chunk. Art wider than its cell (or moved across by the tileset's offset, or a turned hexagonal tile), and tall art on staggered and hexagonal maps, draw a run of one tileset per row per chunk: four layers of 48 x 48 art on 32 x 16 isometric cells came to about 4,000 draws a frame at 1080p, against 96 for art that fits. Keep tall art to one tileset per layer, and art no wider than its cell, where it can be.
  - **A lit level** gathers every map occluder each frame, and each shadowed light loops over all of them, however far away: one shadowed light over the 1024 x 1024 cave cost about 30 µs more than the unlit map.
  - **A landing frame** builds and uploads the chunks in view (about 0.5 ms at 1080p), and a tileset's first landing also makes its padded texture (about 0.5 ms more, about 1 ms in all, the first time; a level restarted, or the next one with the same tileset, within about two seconds reuses it). A 1024 x 1024 four-layer colliding map takes about 60 ms on the builder thread and about 2.5 ms in its landing frame, bounded or unbounded; on the web all of it is one frame (section 15). Cover a big map's landing with a loading screen.
- **Warnings in the dev log:**
  - `tileset "..." is too big to pad`: its padded copy (each tile in a cell a little bigger than itself) would pass the GPU's largest texture, 8,192 texels a side: about 16,000 tiles of 64 px, 65,000 of 32 px, 3,200 hexagons of 120 x 140 px, 670 pictures of 256 x 352 px, or a collection of over 1,000 pictures of 256 px. Its tiles draw nothing until it changes. Split the tileset.
  - `layer "..." collides and has parallax`: it collides where it would be without parallax.
  - `the tile chunk arena is full`: the cameras keep more than about two million tiles in view at once (with those seen in the last two seconds), past the GPU's largest buffer: a whole 1024 x 1024 four-layer map at a small zoom, or a minimap camera over a big map. The chunks seen least recently are let go first; past that, the chunks met last draw nothing. Zoom in, show fewer tile layers to the wide camera (`RenderLayers`), or draw the overview from a picture of the map.
  - `a TileColliders cell ... is past the reach of tile collision`: a tile of any layer, unbounded or a bounded one long enough to reach it, more than 186,411 cells from (0, 0) on an axis draws but does not collide.
  - A repeated image layer too small to cover a view covers its middle.

## 17. Testing

- **A headless engine** (`ryo2d::testing::HeadlessEngine`) lands maps as a windowed one does. Step it until `MapsView::new(engine.world_mut()).is_ready(map)`, then read tiles with `MapsView`, objects and class tiles with queries, and colliders after a step through the world's `Physics` resource (`engine.world().resource::<Physics>()`: `raycast`, `overlap_point`, `contacts` and the rest). A landing takes a frame or more (its job runs on another thread), so wait for it rather than counting frames. Its tilesets are padded on another thread too (on the web, within the landing frame), in time in proportion to the padded texels (section 1, "Maps on other grids"): a few hundred milliseconds for a few dozen big pictures, which a test stepping as fast as it can outpaces by thousands of steps, so bound the wait in time (a step and a millisecond's rest, say), not in a few hundred steps.
- **Events:** read `MapEvent`s between steps with a `MessageCursor<MapEvent>` over the world's `Messages<MapEvent>`; fail the test on `Failed`.
- **Hot reload:** headless engines do not watch files. Give the engine a `MemorySource` the test keeps (`EngineBuilder::asset_source`), save over a file with `files.insert(path, text)`, and ask for its reload with `engine.assets().server().reload(path)`; the next steps bring the map to it and send `Reloaded`.
- **The web's files:** play the levels from the embedded source too (section 15).

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

## 18. When something looks wrong

| Symptom | Look at |
|---|---|
| The map never appears | Its `MapEvent::Failed`: the error names the file and the place in it (`line L, column C`, `layer "name"`, `chunk (x, y)`, `object id`, `tile id`, `tileset "name"`). A missing tileset or image (an image that did not load is named with its tileset or layer), a gid outside every tileset, an image too small for its tileset, an orientation, stagger or hex side length Tiled does not have, an infinite map's chunk with no cells, too little data or cells too far out, an LDtk level name that is not there; or the map entity disabled; on the web, a file missing from the embedded source |
| An object has none of its class's properties | The Tiled project was not found (the dev log names the class once per file): `assets/project.tiled-project`, or `[tilemap] tiled_project` |
| A character falls through as the map arrives | It was placed before `MapEvent::Spawned`, or by a ray cast in the `update` that read it (section 1) |
| Tiles draw but do not collide | The layer's `collides` is false; the root has `without_collision`; hexagons over 16,384 px or under 2 px across (warned); the tiles have no collision shapes in the tileset, or only `occluder` ones; an LDtk value named other than the collision names (`with_intgrid_collision`); a class tile; the map disabled; a cell more than 186,411 cells from (0, 0) on an axis, on any layer (warned) |
| Collision sits off the art | The layer has parallax (warned): collision ignores it |
| A seam in the floor where two layers meet | The layers differ in cell size, offset, storey or material, so each collides as a grid of its own |
| A character on a storey stops short of where it should walk | The storey's edge is its footprint: give the cell a tile on one of the storey's layers (at its bottom tile layer's offset), or stairs up to it on a lower storey's layer standing there too; a cell whose only tile on the storey is a class tile is a hole (section 8) |
| Tiles cast no shadows | The light lacks `with_shadows()`; the layer does not occlude (`occludes` false, or it does not collide and `occludes` is not set); hexagons over 16,384 px or under 2 px across (warned) |
| A character walks over a tree instead of behind it | The layer is not y-sorted, or the character lacks `YSort` or has another z than the layer (section 10) |
| A tileset draws nothing, warned as too big to pad | Its padded copy would pass the GPU's largest texture, 8,192 texels a side (about 16,000 tiles of 64 px): split it |
| Painting terrain leaves a cell as it was, warned | The Wang set has no tile for what the cell needs |
| A dug tile came back | The map's file was saved: the file wins over tile edits |
| A coin collected did not come back after a save | As meant: what the game despawned stays away unless the save changed it |
| A world pattern found fewer maps than there are files, or none | Gaps in the numbering, negative numbers, zero-padded numbers (`room_01`) or numbering from 1: list those maps |
| A saved `.world` did not move the rooms | Levels already spawned stay where they are; respawn them |
| A streamed world stays empty | No anchor near it: `cameras` false and no `StreamAnchor`; the map is not infinite (warned); the generator names layers the map lacks (warned) or panicked (warned, that chunk empty) |
| An edit on a streamed map changes nothing | Its chunk has not landed, or has unloaded: only landed chunks take edits (section 12) |
| An edited chunk of a streamed world came back as generated | The stream has no `SavedChunks` (`with_saved`); or the save game was written without `save_changed_chunks` first, leaving out the chunks changed round the player |
| Enemies placed on a streamed chunk pile up as the player travels | They lack `ChunkOwned { map, chunk }`, so they outlive their chunk |
