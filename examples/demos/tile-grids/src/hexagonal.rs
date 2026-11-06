//! The page "A hex board": `maps/hex-board.tmx`, 8 x 9 pointy-topped
//! hexagons of 120 x 140 (Kenney's Hexagon Pack) in rows, every other row
//! half a hex right, written by Tiled. The sand's dunes are turned a sixth
//! of a turn at a time (Tiled's hex turns, `TileFlip::HEX_ROTATE_60` and
//! `HEX_ROTATE_120`), so their ripples run three ways; the trees and rocks
//! stand unturned.
//!
//! The pointer picks the hex under it (`LayerGrid::cell_at_world`: the
//! hex whose drawn outline holds the point, which on these hexes, close to
//! regular but not quite, is not always the one with the nearest centre),
//! outlined in white, and a click selects one, outlined in gold. Through the layer's
//! grid (`ctx.maps().grid`) the board then marks the selected hex's range
//! and its ring at the Radius setting, the line from it to the hovered hex
//! and the hovered hex's neighbours, each on an empty layer of its own the
//! map holds above the board, in the rim its `mark` property names; the
//! HUD gives the distance between them.
//!
//! The board's rocks and stone collide and occlude along their hexagons
//! (each such tile's shape is its whole hexagon), so they stop the walker
//! WASD walks, and the lantern it carries (a `Light2D` with shadows) casts
//! their hexagonal shadows, which turn and stretch as it walks. The page
//! sets a dusk ambient for the lantern to light (its scene's cleanup puts
//! the lighting back when the page is left); the marks' rims are unlit
//! (their layers' `lit` property is false), so they read in the dusk, and
//! the range's wash is lit, so the shadows under it show. A collider round
//! the board's outer edge, made from the layer grid's own hex outlines,
//! keeps the walker on the board; it casts no shadow. The board's
//! navigation grid (`ctx.maps().navigation`) blocks the rocks from their
//! collision, and the trees' `cost` property makes them dearer to cross,
//! so hovering a hex shows the cheapest path to it from the walker's hex,
//! a blue line through hex centres, with its length and cost in the HUD; a
//! right click walks the walker along it, the walk still ahead drawn in
//! violet

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, PATH, View, WALK, beside_panel, clear_to, draw_outline, draw_path, hovered_cell,
  hud, load, reloaded, wasd,
};
use crate::walker::{CellPath, PathWalk, Walk, face, place_of, spawn_walker};

/// The board
const BOARD: &str = "maps/hex-board.tmx";
/// Its layers: the terrain, and the marks over it, bottom first
const TERRAIN: &str = "board";
const MARKS: [&str; 4] = ["range", "ring", "neighbours", "line"];
/// Each mark's place in `MARKS` and in the page's `marks`
const RANGE: usize = 0;
const RING: usize = 1;
const NEIGHBOURS: usize = 2;
const LINE: usize = 3;
/// The view's background, should the map give none
const NIGHT: Color = Color::rgb(36, 49, 63);
/// The zoom the board fits the view at: 60 x 70 screen pixels a hex
const ZOOM: f32 = 0.5;
/// The hex selected until a click picks another: the board's middle
const FIRST: IVec2 = IVec2::new(4, 4);
/// The settings rows, for the notes under the panel
const ROWS: usize = 1;
/// The hex the walker starts on
const START: IVec2 = IVec2::new(1, 4);
/// The walker: Tiny Dungeon's wizard, 64 px tall on hexes 140 tall (32
/// screen pixels at the board's zoom). Its collider is under half a hex's
/// side (35), so on a path between two hexes' centres it passes the
/// corners of the rocks either side
const WALKER: Walk = Walk {
  sprite: 84,
  scale: 4.0,
  radius: 20.0,
  speed: 240.0,
};
/// The light the walker carries, at its feet: warm, reaching about three
/// hexes, casting the rocks' shadows
const LANTERN_COLOR: Color = Color::rgb(255, 222, 170);
const LANTERN_RADIUS: f32 = 420.0;
const LANTERN_INTENSITY: f32 = 1.5;
const LANTERN_FALLOFF: f32 = 1.5;
/// The board's light away from the lantern: a cool dusk, dim enough for
/// the lantern and its shadows to read and light enough to read the board
const DUSK: Color = Color::rgb(150, 165, 210);
const DUSK_INTENSITY: f32 = 0.35;
/// How near two corners of the board's rim count as one, in world pixels
const SAME_CORNER: f32 = 0.5;

/// The cheapest path from the walker's hex to the hovered one, and the
/// hexes it was found for
struct Route {
  from: IVec2,
  to: IVec2,
  /// `None` when nothing reaches the hovered hex: a rock, or walled off
  path: Option<CellPath>,
}

pub(crate) struct HexPage {
  view: View,
  root: Option<Entity>,
  hover: Option<IVec2>,
  selected: IVec2,
  /// Each mark layer's rim, from its `mark` property, and the cells it
  /// marks now
  marks: [(u32, Vec<IVec2>); 4],
  /// What the marks were drawn for: selected, hovered and radius
  marked: Option<(IVec2, Option<IVec2>, i32)>,
  covered: Vec<Rect>,
  radius: i32,
  /// The z the walker draws at: above the board and its marks
  walker_z: i32,
  walker: Option<Entity>,
  /// The collider round the board's outer edge, made once it lands and
  /// again when a save of its files changes it
  rim: Option<Entity>,
  /// The board as the walker crosses it, taken once it lands and again
  /// when a save of its files changes it
  nav: Option<NavigationGrid>,
  route: Option<Route>,
  walk: Option<PathWalk>,
}

impl HexPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      hover: None,
      selected: FIRST,
      marks: Default::default(),
      marked: None,
      covered: Vec::new(),
      radius: 2,
      walker_z: MARKS.len() as i32 + 1,
      walker: None,
      rim: None,
      nav: None,
      route: None,
      walk: None,
    }
  }

  /// Each mark layer's rim tile, from the map's layer properties
  fn read_marks(&mut self, map: &TileMap) {
    for (i, name) in MARKS.iter().enumerate() {
      let rim = map
        .layer_index(name)
        .and_then(|index| map.layers[index].info.properties.int("mark"))
        .and_then(|id| u32::try_from(id).ok());
      if let Some(rim) = rim {
        self.marks[i].0 = rim;
      }
    }
  }

  /// The marks again, if what they show changed: the old cells cleared, the
  /// new ones set, each layer in its own rim
  fn mark(&mut self, ctx: &mut SceneContext, root: Entity) {
    let now = (self.selected, self.hover, self.radius);
    if self.marked == Some(now) {
      return;
    }
    let (Some(layer), Some(bounds)) = (ctx.maps().grid(root, TERRAIN), ctx.maps().bounds(root))
    else {
      return;
    };
    self.marked = Some(now);
    let grid = layer.grid;
    let radius = self.radius.max(0) as u32;
    let mut wanted: [Vec<IVec2>; 4] = Default::default();
    wanted[RANGE] = grid.range(self.selected, radius);
    wanted[RING] = grid.ring(self.selected, radius);
    if let Some(hover) = self.hover {
      wanted[NEIGHBOURS] = grid.neighbours(hover).collect();
      wanted[LINE] = grid.line(self.selected, hover);
    }
    for cells in &mut wanted {
      cells.retain(|cell| bounds.contains(*cell));
    }
    let mut maps = ctx.maps();
    for ((name, (rim, cells)), new) in MARKS.iter().zip(&mut self.marks).zip(wanted) {
      for cell in cells.drain(..) {
        maps.set_tile(root, *name, cell, None);
      }
      for &cell in &new {
        maps.set_tile(root, *name, cell, Some(Tile::new(0, *rim)));
      }
      *cells = new;
    }
  }

  /// The cheapest path from the walker's hex, `from`, to the hovered one,
  /// found again when either changes; none when it hovers no hex, or its
  /// own
  fn find_route(&mut self, from: Option<IVec2>) {
    let (Some(from), Some(to)) = (from, self.hover.filter(|&hover| Some(hover) != from)) else {
      self.route = None;
      return;
    };
    if self
      .route
      .as_ref()
      .is_some_and(|route| route.from == from && route.to == to)
    {
      return;
    }
    let path = self
      .nav
      .as_ref()
      .and_then(|grid| CellPath::find(grid, from, to));
    self.route = Some(Route { from, to, path });
  }

  /// How the hovered hex's tile is turned, as the HUD says it
  fn turn_line(&self, ctx: &mut SceneContext, root: Entity) -> Option<String> {
    let tile = ctx.maps().tile(root, TERRAIN, self.hover?)?;
    let flip = tile.flip;
    let half = flip.contains(TileFlip::HORIZONTAL) && flip.contains(TileFlip::VERTICAL);
    let degrees = 60 * u32::from(flip.contains(TileFlip::HEX_ROTATE_60))
      + 120 * u32::from(flip.contains(TileFlip::HEX_ROTATE_120))
      + 180 * u32::from(half);
    Some(if degrees == 0 {
      "Its tile: unturned".to_string()
    } else {
      format!("Its tile: turned {degrees} degrees clockwise")
    })
  }
}

/// The board's navigation grid: the tiles that collide (the rocks and the
/// stone) blocked, and each other tile with a `cost` property that much
/// dearer to enter
fn board_navigation(ctx: &mut SceneContext, root: Entity) -> Option<NavigationGrid> {
  let bounds = ctx.maps().bounds(root)?;
  let mut grid = ctx.maps().navigation(root, TERRAIN, bounds)?;
  let maps = ctx.maps();
  for y in bounds.min.y..bounds.max.y {
    for x in bounds.min.x..bounds.max.x {
      let cell = IVec2::new(x, y);
      let cost = maps
        .tile(root, TERRAIN, cell)
        .and_then(|tile| maps.tile_data(root, tile))
        .and_then(|data| data.properties.float("cost"))
        .map(|cost| cost as f32)
        .filter(|cost| cost.is_finite() && *cost > 0.0);
      if let Some(cost) = cost
        && grid.is_walkable(cell.into())
      {
        grid.set(cell.into(), CellState::Difficult(cost));
      }
    }
  }
  Some(grid)
}

/// The light the walker carries
fn lantern() -> Light2D {
  Light2D::point(LANTERN_COLOR, LANTERN_RADIUS)
    .with_intensity(LANTERN_INTENSITY)
    .with_falloff(LANTERN_FALLOFF)
    .with_shadows()
}

/// The board's outer edge as one fixed collider, in the world: each side
/// of a hex of `bounds` with no hex of `bounds` across it (the hex there
/// is the one whose centre is this one's, mirrored in the side's middle),
/// the sides joined at the corners they share. The sides are the layer
/// grid's own outlines, where the map's collision and navigation lie. It
/// stops the walker, and casts no shadow: an unseen wall's shadow would
/// look out of place. The navigation grid reads only the map's tile
/// colliders, so it never sees this one, and needn't: no path leaves the
/// board
fn board_rim(layer: &LayerGrid, bounds: CellRect) -> Collider {
  let mut corners: Vec<Vec2> = Vec::new();
  let mut sides: Vec<[u32; 2]> = Vec::new();
  let mut corner = |at: Vec2| {
    let index = corners
      .iter()
      .position(|seen| seen.distance(at) < SAME_CORNER)
      .unwrap_or_else(|| {
        corners.push(at);
        corners.len() - 1
      });
    index as u32
  };
  for y in bounds.min.y..bounds.max.y {
    for x in bounds.min.x..bounds.max.x {
      let cell = IVec2::new(x, y);
      let centre = layer.cell_center_world(cell);
      let outline = layer.outline_world(cell);
      let points = outline.points();
      for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        if !bounds.contains(layer.cell_at_world(a + b - centre)) {
          sides.push([corner(a), corner(b)]);
        }
      }
    }
  }
  Collider::segments(corners, sides)
}

impl DemoPage for HexPage {
  fn name(&self) -> &str {
    "A hex board"
  }

  fn description(&self) -> &str {
    "Pointy hexagons in rows: a hex's range, ring, line and neighbours, and paths round the rocks"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.int("Radius", &mut self.radius, 1..=4);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Walk (pad: left stick)"),
      Control::mouse("Point at a hex, click to select it, right-click to walk there"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let Some(map) = load::<TileMap>(ctx, BOARD) else {
      return;
    };
    let (background, size) = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.read_marks(data);
      self.walker_z = data.layers.len() as i32;
      let size = data.grid().bounds_of(CellRect::from_size(data.size));
      (data.background, size)
    };
    clear_to(ctx, background.unwrap_or(NIGHT));
    // Dusk, for the walker's lantern to light; the page's scene cleanup
    // puts the lighting back when the page is left
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(DUSK, DUSK_INTENSITY));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    let (cx, cy) = size.center();
    let centre = self.view.centre_for(ctx, Vec2::new(cx, cy), ZOOM);
    self.view.place(ctx, centre, ZOOM, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if !ctx.maps().is_ready(root) {
      return;
    }
    let Some(layer) = ctx.maps().grid(root, TERRAIN) else {
      return;
    };
    let walker = match self.walker {
      Some(walker) => walker,
      None => {
        let walker = spawn_walker(ctx, layer.cell_center_world(START), WALKER, self.walker_z);
        if let Ok(mut entity) = ctx.world.get_entity_mut(walker) {
          entity.insert(lantern());
        }
        *self.walker.insert(walker)
      }
    };
    let reload = reloaded(ctx, root);
    if reload || self.nav.is_none() {
      self.nav = board_navigation(ctx, root);
      self.route = None;
    }
    if reload || self.rim.is_none() {
      if let Some(rim) = self.rim.take() {
        ctx.world.despawn(rim);
      }
      let bounds = ctx.maps().bounds(root);
      self.rim = bounds.map(|bounds| {
        ctx
          .world
          .spawn((Transform::IDENTITY, board_rim(&layer, bounds)))
          .id()
      });
    }
    self.hover = hovered_cell(ctx, root, TERRAIN, &self.covered);
    if let Some(cell) = self.hover
      && ctx.input.mouse_pressed(MouseButton::Left)
    {
      self.selected = cell;
    }
    self.mark(ctx, root);

    let feet = place_of(ctx, Some(walker));
    self.find_route(feet.map(|at| layer.cell_at_world(at)));
    let wanted = self.route.as_ref().and_then(|route| route.path.as_ref());
    if let Some(path) = wanted
      && ctx.input.mouse_pressed(MouseButton::Right)
    {
      let centres = path.centres.clone();
      if let Some(walk) = self.walk.take() {
        walk.stop(ctx);
      }
      self.walk = PathWalk::start(ctx, walker, centres);
    }
    self.walk = self.walk.take().and_then(|walk| walk.step(ctx, dt));
    face(ctx, walker);

    let mut debug = ctx.debug_draw();
    draw_outline(&mut debug, &layer.outline_world(self.selected), HOT, 3.0);
    if let Some(cell) = self.hover {
      draw_outline(&mut debug, &layer.outline_world(cell), Color::WHITE, 2.0);
    }
    // The walk under way over the path to the pointer: where they part,
    // the blue shows where a right click would go instead
    if let Some(path) = self.route.as_ref().and_then(|route| route.path.as_ref()) {
      draw_path(&mut debug, None, &path.centres, PATH, 3.0);
    }
    if let (Some(walk), Some(at)) = (&self.walk, feet) {
      walk.draw(&mut debug, at, WALK, 3.0);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    let column = beside_panel(ui, ROWS);
    let Some(layer) = ctx.maps().grid(root, TERRAIN) else {
      let waiting = [("Board: waiting for the map".to_string(), DIM)];
      hud(ui, column.x, column.y, column.width, &waiting);
      return;
    };
    let grid = layer.grid;
    let radius = self.radius.max(0) as u32;
    let (range, ring) = (self.marks[RANGE].1.len(), self.marks[RING].1.len());
    let mut lines = vec![
      (
        "Pointy hexagons of 120 x 140, sides of 70".to_string(),
        Color::WHITE,
      ),
      (
        format!("Selected: hex {}, {}", self.selected.x, self.selected.y),
        HOT,
      ),
      (
        format!(
          "Range {radius}: {range} hexes on the board; its ring {ring} (of {})",
          grid.ring(self.selected, radius).len()
        ),
        Color::WHITE,
      ),
    ];
    match self.hover {
      Some(cell) => {
        lines.push((format!("Pointer: hex {}, {}", cell.x, cell.y), GOOD));
        lines.push((
          format!(
            "Distance: {} steps; the line {} hexes",
            grid.distance(self.selected, cell),
            self.marks[LINE].1.len()
          ),
          GOOD,
        ));
        match self.route.as_ref().filter(|route| route.to == cell) {
          Some(Route {
            path: Some(path), ..
          }) => lines.push((
            format!("Path: {} hexes, cost {:.1}", path.cells.len(), path.cost),
            PATH,
          )),
          Some(Route { path: None, .. }) => {
            lines.push((format!("No path to hex {}, {}", cell.x, cell.y), PATH));
          }
          None => {}
        }
        if let Some(turn) = self.turn_line(ctx, root) {
          lines.push((turn, DIM));
        }
      }
      None => lines.push(("Pointer: off the board".to_string(), GOOD)),
    }
    lines.push((
      "Beige: the ring; grey: neighbours; red: the line".to_string(),
      DIM,
    ));
    lines.push((
      "Blue: the path there; violet: the walk under way".to_string(),
      DIM,
    ));
    lines.push(("Trees cost more to cross; rocks block".to_string(), DIM));
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
  }
}
