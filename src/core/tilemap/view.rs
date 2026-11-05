//! `ctx.maps()`: reading and editing spawned maps.

use glam::{Affine2, IVec2, UVec2, Vec2};

use super::collision;
use super::edit::{self, LayerRef};
use super::spawn::{self, MapIds, MapLayers, MapRoot, TileLayerData};
use super::stream;
use super::world::{self, WorldRef};
use super::{CHUNK_SIZE, CellOutline, MapGrid, Tile, TileData, TileGrid, WorldLevel, wang};
use crate::core::ecs::{Entity, World};
use crate::core::hash::FastSet;
use crate::core::navigation::{CellState, GridCoord, NavigationGrid};
use crate::core::physics::{CellRect, TileColliders};
use crate::core::ui::Rect;

/// A spawned tile layer's grid and where its pixels are in the world
/// ([`MapsView::grid`]): the cell under a world point, a cell's centre and
/// outline in the world, and through `grid` the board questions of any grid
/// (`neighbours`, `distance`, `line`, `range`, `ring`), whose cells are the
/// layer's.
///
/// `to_world` is the layer's transform and its map's as they were when it
/// was read: a map that moves later needs reading again.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// The hexes a unit standing under the pointer can reach in `moves`
/// fn reach(ctx: &mut SceneContext<'_>, board: Entity, pointer: Vec2, moves: u32) -> Vec<IVec2> {
///   match ctx.maps().grid(board, "ground") {
///     Some(layer) => layer.grid.range(layer.cell_at_world(pointer), moves),
///     None => Vec::new(),
///   }
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerGrid {
  /// Where the layer's cells are in its own pixels
  pub grid: MapGrid,
  /// The layer's pixels to the world
  pub to_world: Affine2,
}

impl LayerGrid {
  /// The cell whose outline holds the world point `world`
  /// ([`MapGrid::cell_at`]), inside the layer or not. A layer scaled to
  /// nothing, of cells with no size, or a point that is not a number,
  /// gives cell (0, 0) ([`MapsView::world_to_cell`] gives `None` there)
  pub fn cell_at_world(&self, world: Vec2) -> IVec2 {
    self.try_cell_at_world(world).unwrap_or(IVec2::ZERO)
  }

  /// The world position of `cell`'s centre ([`MapGrid::cell_center`])
  pub fn cell_center_world(&self, cell: IVec2) -> Vec2 {
    self.to_world.transform_point2(self.grid.cell_center(cell))
  }

  /// `cell`'s outline in the world ([`MapGrid::cell_outline`] through the
  /// layer's transform): clockwise on screen from its top-most point, on a
  /// turned or mirrored map too. For drawing a cursor or a range over the
  /// map, or a hit test against it
  pub fn outline_world(&self, cell: IVec2) -> CellOutline {
    self.grid.cell_outline(cell).mapped(self.to_world)
  }

  /// `cell_at_world`, `None` where the layer is scaled to nothing, its
  /// cells have no size, or the point is not a number
  pub(crate) fn try_cell_at_world(&self, world: Vec2) -> Option<IVec2> {
    if self.to_world.matrix2.determinant() == 0.0 || self.grid.cell.cmpeq(UVec2::ZERO).any() {
      return None;
    }
    let local = self.to_world.inverse().transform_point2(world);
    local.is_finite().then(|| self.grid.cell_at(local))
  }
}

/// The spawned maps of a world: read and edit their tiles, read their
/// IntGrids and tile data, and convert between world positions and cells.
/// Every call names the map entity and, where it reads cells, a layer by
/// name or index; a map not yet spawned, a layer it does not have, a layer
/// that is not a tile layer, or a cell outside the layer reads `None` and
/// changes nothing. A layer whose grid is unbounded
/// ([`TileGrid::unbounded`](super::TileGrid::unbounded)) has no outside:
/// its cells are signed, and an edit anywhere makes the chunk it writes
/// in, which then draws, collides and casts shadows. A streamed map
/// ([`MapStream`](super::MapStream)) takes edits in its resident chunks
/// alone: a cell in any other is refused, as a cell outside a bounded
/// layer is.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn dig(ctx: &mut SceneContext<'_>, map: Entity, at: Vec2) -> bool {
///   let mut maps = ctx.maps();
///   match maps.world_to_cell(map, "ground", at) {
///     Some(cell) => maps.set_tile(map, "ground", cell, None),
///     None => false,
///   }
/// }
/// ```
pub struct MapsView<'w> {
  world: &'w mut World,
}

impl<'w> MapsView<'w> {
  /// The maps of `world`
  pub fn new(world: &'w mut World) -> Self {
    Self { world }
  }

  /// Whether `map` has spawned its layers
  pub fn is_ready(&self, map: Entity) -> bool {
    self.world.get::<MapIds>(map).is_some()
  }

  /// Whether every chunk `area` (world pixels) takes is resident on the
  /// streamed map `map` ([`MapStream`](super::MapStream)): landed, or one
  /// of the map's own chunks (a Tiled infinite map's, or those a map built
  /// in code holds in its asset), so it draws and collides. The chunks are
  /// those a view's area takes: on an orthogonal map the chunks overlapping
  /// it, where an area of no width or height (a point, or a line) asks for
  /// the chunks holding it, even on a chunk boundary; on any other grid
  /// every chunk with a cell in it, and the others whose boxes reach it.
  /// False for a map not streamed or not landed. For a loading screen
  /// after a respawn or a teleport, rather than dropping a body into chunks
  /// not generated yet
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// Whether the view round `at` has landed, for the loading screen to go
  /// fn arrived(ctx: &mut SceneContext<'_>, world: Entity, at: Vec2) -> bool {
  ///   let view = Rect::new(at.x - 960.0, at.y - 540.0, 1920.0, 1080.0);
  ///   ctx.maps().stream_ready(world, view)
  /// }
  /// ```
  pub fn stream_ready(&self, map: Entity, area: Rect) -> bool {
    stream::stream_ready(self.world, map, area)
  }

  /// What the streamed map `map` ([`MapStream`](super::MapStream)) has
  /// now, as its stream's last frame left it: the chunks resident, the
  /// file's and the generated among them, those in flight and those
  /// waiting to unload. `None` for a map not streamed, or not landed yet.
  /// The stream runs in `PreUpdate`, so the status read in a scene's
  /// update or UI is this frame's
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// A HUD line: what the world has round the player
  /// fn chunks_line(ctx: &mut SceneContext<'_>, world: Entity) -> String {
  ///   match ctx.maps().stream_status(world) {
  ///     Some(status) => format!(
  ///       "{} chunks ({} hand-made), {} on the way",
  ///       status.resident, status.hand_made, status.in_flight
  ///     ),
  ///     None => "not streamed".to_string(),
  ///   }
  /// }
  /// ```
  pub fn stream_status(&self, map: Entity) -> Option<stream::StreamStatus> {
    stream::stream_status(self.world, map)
  }

  /// Puts each chunk resident on the streamed map `map` that a player
  /// changed since it landed into its stream's
  /// [`SavedChunks`](super::SavedChunks), as unloading it would. A chunk
  /// goes in only as it unloads, so call this just before writing the
  /// `SavedChunks` into a save game, or the chunks changed round the player
  /// are left out (the map's root going puts them in too). A Tiled
  /// infinite map's own chunks never unload, so this and the root going are
  /// the only ways their changes are kept: one changed since the map
  /// landed goes in, and comes back from the save as a new stream of the
  /// map starts. Returns how many it put: 0 for a map with no `MapStream`,
  /// or whose stream keeps no `SavedChunks`
  pub fn save_changed_chunks(&self, map: Entity) -> usize {
    stream::save_changed(self.world, map)
  }

  /// The entity of `map`'s layer `layer`
  pub fn layer_entity<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>) -> Option<Entity> {
    edit::layer_entity(self.world, map, layer.into())
  }

  /// The cells `map` covers as it is now, in its tile size: a bounded
  /// map's (0, 0) to its size; an infinite map's the smallest rectangle
  /// holding every cell of its tile layers with a tile (class tiles
  /// included), following every edit since it landed (empty with none).
  /// For camera limits on a map that grows; [`TileMap::bounds`](super::TileMap::bounds)
  /// reads the asset, which edits never change. `None` until it lands. An
  /// infinite map's costs a look at each of its chunks plus the cells of
  /// the chunks on its edges: on a map grown to many chunks, read it when
  /// the map changes rather than every frame
  pub fn bounds(&self, map: Entity) -> Option<CellRect> {
    edit::map_bounds(self.world, map)
  }

  /// The cells of a tile layer as it is now: a bounded layer's bounds, an
  /// unbounded one's smallest rectangle holding every cell with a tile
  /// (class tiles included; empty with none). `None` for a layer the map
  /// lacks or one that is not a tile layer
  pub fn layer_bounds<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>) -> Option<CellRect> {
    edit::layer_bounds(self.world, map, layer.into())
  }

  /// The tile at `cell` of a tile layer's grid: a cell holding a class
  /// tile ([`MapTile`](super::MapTile)) reads `None`, the tile being its
  /// entity. On an LDtk layer with sub-layers, the layer's own tile (the
  /// first in the cell)
  pub fn tile<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>, cell: IVec2) -> Option<Tile> {
    edit::tile_at(self.world, map, layer.into(), cell)
  }

  /// Puts `tile` at `cell` of a tile layer (`None` clears it); false if
  /// nothing changed. On an unbounded layer any cell takes it, negative ones
  /// included, its chunk made if it has none. A tile whose class the map's
  /// root registers
  /// ([`MapRoot::with_entity_tiles`]) spawns as a [`MapTile`](super::MapTile)
  /// there instead of going in the grid, as the map's own do; a class tile
  /// already on the cell is despawned first. Only the cell's chunk redraws,
  /// collides and casts shadows again. A layer with an IntGrid (an LDtk
  /// IntGrid layer) collides and occludes by its values, not its tiles:
  /// change those with [`set_intgrid`](Self::set_intgrid). The whole cell
  /// takes the tile: on an LDtk layer whose stacked, offset or translucent
  /// tiles are in sub-layers ([`LayerInfo::part_of`](super::LayerInfo::part_of)),
  /// they lose what they held there
  pub fn set_tile<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
    tile: Option<Tile>,
  ) -> bool {
    edit::set_tile(self.world, map, layer.into(), cell, tile)
  }

  /// Puts `tile` in every cell from `min` to `max`, both included, clipped
  /// to a bounded layer (an unbounded one makes the chunks a tile goes in),
  /// as [`set_tile`](Self::set_tile) puts one (the whole cell, a sub-layer's
  /// tiles cleared); returns how many cells changed. Only the chunks it
  /// changed redraw, collide and cast shadows again. On an unbounded layer
  /// a tile fill that would make more than 131,072 chunks (the 2^27 cells
  /// a map may hold; a region such as `IVec2::MIN` to `IVec2::MAX`) is
  /// refused before anything is made: it returns 0 and the dev log warns
  /// once a layer. Clearing (`None`) any region reads only the chunks
  /// there are
  pub fn fill<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    min: IVec2,
    max: IVec2,
    tile: Option<Tile>,
  ) -> usize {
    edit::fill(self.world, map, layer.into(), min, max, tile)
  }

  /// Paints `cell` of a tile layer with the color `color` of the Wang set
  /// `wang_set` (named in one of the map's tilesets; `None` paints no color
  /// and empties the cell) as Tiled's terrain brush does with corrections
  /// on: the cell's corners (a corner set), edges (an edge set) or both (a
  /// mixed set) take the color, then the cell and the cells sharing one of
  /// them pick the tile that fits, a transformed one if the tileset allows,
  /// and a change that leaves a neighbour disagreeing is passed on to it,
  /// as far as the set's longest chain of colors plus one cells from `cell`
  /// (two for two colors that share a tile, four for deep water, water,
  /// sand and grass). Cells that nothing fits (an incomplete set) keep their
  /// tile and the set warns once per tileset version: so does erasing in a
  /// set with no tile that has a bare corner or edge, where the neighbours
  /// have nothing to turn into. False if nothing changed, the map, layer,
  /// set or color is not there, or the cell is outside a bounded layer (an
  /// unbounded one takes any cell, making the chunks the paint writes in,
  /// but for a cell so near the ends of `i32` that the cells the paint
  /// reaches would not fit: a few cells from them, or on a staggered map,
  /// whose lattice cells count up to half as far again, a few cells from
  /// ±`i32::MAX / 4` or past it, on either axis).
  ///
  /// On an isometric or oblique map the paint works in the layer's cells, as
  /// on an orthogonal one. On a staggered map it works in the isometric
  /// lattice the diamonds make ([`MapGrid::lattice`](super::MapGrid::lattice)),
  /// where Tiled's brush paints: the cell's four lattice corners take the
  /// color, and the cells around them are the lattice's, not the grid
  /// indexes'. A hexagonal map is refused with one warning (false), as a
  /// hexagon has six neighbours where a Wang id has four corners and four
  /// edges.
  ///
  /// This matches Tiled's results on the cases checked against it (complete,
  /// one-color, incomplete and multi-color sets, color chains, flipped
  /// neighbours and transformations; the fixtures in
  /// `tests/fixtures/tilemap/tiled` were painted by Tiled 1.12). Tiled draws
  /// at random among equal fits and this by a hash of the cell, so those
  /// picks differ. A class tile ([`MapTile`](super::MapTile)) keeps its
  /// colors out of a neighbour's correction, so painting beside one meets
  /// it with a seam; painting that very cell replaces it.
  pub fn set_terrain<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
    wang_set: &str,
    color: Option<&str>,
  ) -> bool {
    wang::set_terrain(self.world, map, layer.into(), cell, wang_set, color)
  }

  /// Puts `value` at `cell` of a tile layer's IntGrid (an LDtk IntGrid
  /// layer's values; 0 empties the cell); false if nothing changed, the
  /// layer has no IntGrid, or the cell is outside it. The layer collides
  /// and casts shadows by its values ([`MapRoot::with_intgrid_collision`]),
  /// so only the cell's chunk collides and casts shadows again, as a tile
  /// edit's does. The art does not follow: LDtk runs its auto-layer rules
  /// in the editor, never in the game, so the layer's tiles stay as they
  /// were; change them with [`set_tile`](Self::set_tile).
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// A cracked wall gives way: its cell no longer collides, and shows rubble
  /// fn break_wall(ctx: &mut SceneContext<'_>, level: Entity, cell: IVec2, rubble: Tile) {
  ///   let mut maps = ctx.maps();
  ///   maps.set_intgrid(level, "Collision", cell, 0);
  ///   maps.set_tile(level, "Collision", cell, Some(rubble));
  /// }
  /// ```
  pub fn set_intgrid<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
    value: i32,
  ) -> bool {
    edit::fill_intgrid(self.world, map, layer.into(), cell, cell, value) > 0
  }

  /// Puts `value` in every cell of a tile layer's IntGrid from `min` to
  /// `max`, both included, clipped to the IntGrid, as
  /// [`set_intgrid`](Self::set_intgrid) puts one; returns how many cells
  /// changed. Only the chunks it changed collide and cast shadows again
  pub fn fill_intgrid<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    min: IVec2,
    max: IVec2,
    value: i32,
  ) -> usize {
    edit::fill_intgrid(self.world, map, layer.into(), min, max, value)
  }

  /// The IntGrid value at `cell` of a tile layer; `None` where it is 0
  pub fn intgrid<'a>(
    &self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
  ) -> Option<i32> {
    edit::intgrid_at(self.world, map, layer.into(), cell)
  }

  /// What `tile` carries in its tileset (class, properties, shapes,
  /// animation), if anything
  pub fn tile_data(&self, map: Entity, tile: Tile) -> Option<TileData> {
    spawn::tile_data(self.world, self.world.get::<MapRoot>(map)?, tile).cloned()
  }

  /// A tile layer's grid and where its pixels are in the world, through
  /// the map's and the layer's transforms as they are now: for picking,
  /// outlines in the world, and the board tools of any grid. `None` for a
  /// map not landed, a layer it lacks or one that is not a tile layer
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// The cells of a line drawn from one point to another, each step a
  /// /// neighbour, on whatever grid the layer has
  /// fn line_between(ctx: &mut SceneContext<'_>, map: Entity, from: Vec2, to: Vec2) -> Vec<IVec2> {
  ///   let Some(layer) = ctx.maps().grid(map, "ground") else {
  ///     return Vec::new();
  ///   };
  ///   layer.grid.line(layer.cell_at_world(from), layer.cell_at_world(to))
  /// }
  /// ```
  pub fn grid<'a>(&self, map: Entity, layer: impl Into<LayerRef<'a>>) -> Option<LayerGrid> {
    edit::layer_grid(self.world, map, layer.into())
  }

  /// The cell of a tile layer under the world position `world` (the one
  /// whose outline holds it on the layer's grid, [`LayerGrid::cell_at_world`]),
  /// whether or not the layer reaches it; `None` for a map scaled to
  /// nothing
  pub fn world_to_cell<'a>(
    &self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    world: Vec2,
  ) -> Option<IVec2> {
    self.grid(map, layer)?.try_cell_at_world(world)
  }

  /// The world position of the centre of `cell` of a tile layer, on the
  /// layer's grid ([`LayerGrid::cell_center_world`])
  pub fn cell_to_world<'a>(
    &self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    cell: IVec2,
  ) -> Option<Vec2> {
    Some(self.grid(map, layer)?.cell_center_world(cell))
  }

  /// A navigation grid over `window` (max exclusive) of `layer`'s own
  /// cells ([`NavigationGrid::for_layer`] on [`grid`](Self::grid)): every
  /// colliding layer of `map` blocks through
  /// [`NavigationGrid::block_tiles`], and on a streamed map every cell of a
  /// chunk not resident is `Blocked`. `None` until the map lands, or when
  /// `layer` isn't a tile layer.
  ///
  /// It reads the map's collision as its tiles are now. The fixed step
  /// brings a map's colliders up to its tiles after the scenes' `update`,
  /// so this runs that sync first: a grid taken in `update` reads a chunk
  /// that landed this frame and an edit made earlier in the same `update`.
  /// The sync takes in every map's changes, as the step would (under a
  /// microsecond a map when nothing changed), and the step then finds
  /// nothing more to do; physics builds the changed colliders in that
  /// step, as for any edit.
  ///
  /// The colliders are the map's [`TileColliders`]: its own, and those of
  /// any layer colliding as a grid of its own (another grid, offset or
  /// material), each placed by its entity's whole transform as it is now,
  /// as `grid` reads the layer's, so a map moved this frame blocks where
  /// its cells now are. Solid and slope cells block, and shape cells whose
  /// shape covers the cell's centre; one-way cells stay walkable. A
  /// disabled map has no colliders, so nothing blocks.
  ///
  /// On a map with a [`MapStream`](super::MapStream) a chunk the stream
  /// has not landed has no tiles to collide with yet, so its cells of an
  /// unbounded layer (the layers that stream) are `Blocked`, and a path
  /// keeps to the ground that has landed. This holds from the stream's
  /// first frame, the frame the map lands (or the frame after a
  /// `MapStream` is put on a landed map), as
  /// [`stream_status`](Self::stream_status) reads it. A map not streamed,
  /// one whose `MapStream` was taken away included, blocks no chunk for
  /// being absent.
  ///
  /// The grid is a copy of the map as it is now: the game adds its own
  /// colliders ([`NavigationGrid::block_collider`]) and costs
  /// ([`NavigationGrid::set`]), and takes the grid again after
  /// [`ChunkEvent`](super::ChunkEvent)s or edits. A grid taken again is a
  /// new grid, its `generation` counted afresh, so find the paths found on
  /// an earlier one again rather than asking `Path::is_stale` of the new
  /// one. It costs the window's cells (8 bytes each; a window past
  /// 4,194,304 cells is cut down as `for_layer` says) and the colliding
  /// cells in it.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// /// The way from one point to another over the board's hexes, round
  /// /// its walls
  /// fn route(ctx: &mut SceneContext<'_>, board: Entity, from: Vec2, to: Vec2) -> Option<Path> {
  ///   let mut maps = ctx.maps();
  ///   let bounds = maps.bounds(board)?;
  ///   let grid = maps.navigation(board, "ground", bounds)?;
  ///   Pathfinder::new().find_path(&grid, from.x, from.y, to.x, to.y).path()
  /// }
  /// ```
  pub fn navigation<'a>(
    &mut self,
    map: Entity,
    layer: impl Into<LayerRef<'a>>,
    window: CellRect,
  ) -> Option<NavigationGrid> {
    if !self.is_ready(map) {
      return None;
    }
    let entity = edit::layer_entity(self.world, map, layer.into())?;
    let data = self.world.get::<TileLayerData>(entity)?;
    let unbounded = data.tiles.bounds().is_none();
    let layer = LayerGrid {
      grid: data.grid,
      to_world: edit::local_to_world(self.world, entity)?,
    };
    // Its params (queries, optional stores, a local and commands) always
    // validate, so it always runs
    let _ = self.world.run_system_cached(collision::sync_map_colliders);
    let mut grid = NavigationGrid::for_layer(layer, window);
    if unbounded && let Some(resident) = stream::streamed_resident(self.world, map) {
      block_unloaded(&mut grid, resident);
    }
    let layers = self.world.get::<MapLayers>(map).map_or(&[][..], |l| &l.0);
    for &owner in std::iter::once(&map).chain(layers) {
      if let Some(tiles) = self.world.get::<TileColliders>(owner)
        && let Some(to_world) = edit::local_to_world(self.world, owner)
      {
        grid.block_tiles(tiles, to_world);
      }
    }
    Some(grid)
  }

  /// Spawns the levels of `world` (a Tiled world's handle, or a
  /// [`WorldRef`] for an LDtk world) that overlap `area`, in world pixels,
  /// at world depth 0, and returns the root entities it spawned this call.
  /// Each root is a [`MapRoot`] for its level, at the level's position,
  /// carrying a [`WorldLevelOf`](super::WorldLevelOf). A level already
  /// spawned for that world (a root with that marker, disabled or not) is
  /// skipped, so calling this every frame with the area around the player
  /// spawns each level once; despawn a level's root to unload it, and it
  /// spawns again when the area reaches it (a disabled root keeps it
  /// parked). Nothing spawns until the world's asset is loaded. Saving the
  /// world's file leaves spawned levels where they are, and the next call
  /// uses the new layout. To set the roots up (their z, class tiles,
  /// IntGrid collision) or stream another depth, use
  /// [`spawn_levels_in_with`](Self::spawn_levels_in_with)
  pub fn spawn_levels_in(&mut self, world: impl Into<WorldRef>, area: Rect) -> Vec<Entity> {
    world::spawn_levels_in(self.world, world.into(), area, 0, |_, root| root)
  }

  /// [`spawn_levels_in`](Self::spawn_levels_in) for the levels at world
  /// depth `depth` (LDtk's `worldDepth`: each depth is a floor of its own,
  /// and a Tiled world's levels are all at 0), each root set up by
  /// `configure` before it spawns. `configure` is given the level and its
  /// bare root, and returns the root with the settings the game wants
  /// ([`MapRoot::with_z`], [`MapRoot::with_entity_tiles`],
  /// [`MapRoot::with_intgrid_collision`], [`MapRoot::without_collision`]);
  /// the map stays the level's, whatever root it returns.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn stream(ctx: &mut SceneContext<'_>, rooms: &Handle<TileWorld>, near: Rect) {
  ///   ctx.maps().spawn_levels_in_with(rooms, near, 0, |_level, root| {
  ///     root.with_z(-10).with_entity_tiles(&["coin", "spikes"])
  ///   });
  /// }
  /// ```
  pub fn spawn_levels_in_with(
    &mut self,
    world: impl Into<WorldRef>,
    area: Rect,
    depth: i32,
    configure: impl FnMut(&WorldLevel, MapRoot) -> MapRoot,
  ) -> Vec<Entity> {
    world::spawn_levels_in(self.world, world.into(), area, depth, configure)
  }
}

/// Blocks every cell of `grid`'s window in a chunk not in `resident`, a
/// chunk at a time
fn block_unloaded(grid: &mut NavigationGrid, resident: &FastSet<IVec2>) {
  let window = grid.window();
  if window.is_empty() {
    return;
  }
  let last = window.max - IVec2::ONE;
  let (first_chunk, last_chunk) = (TileGrid::chunk_of(window.min), TileGrid::chunk_of(last));
  let side = CHUNK_SIZE as i32;
  for cy in first_chunk.y..=last_chunk.y {
    for cx in first_chunk.x..=last_chunk.x {
      let chunk = IVec2::new(cx, cy);
      if resident.contains(&chunk) {
        continue;
      }
      // A chunk holds cells of `i32`: its first and last are cells
      let origin = chunk * side;
      let lo = window.min.max(origin);
      let hi = last.min(origin + IVec2::splat(side - 1));
      for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
          grid.set(GridCoord::new(x, y), CellState::Blocked);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests;
