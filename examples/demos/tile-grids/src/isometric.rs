//! The page "An isometric town": `maps/town.tmx`, an isometric map of 16 x
//! 16 cells of 232 x 110 drawn with Kenney's Sketch Town, written by Tiled.
//! Its `ground` layer holds grass and water blocks, their tops on the cells
//! (the ground tileset draws them a cell lower), and its `props` layer,
//! y-sorted, the castle's walls and towers and the trees, taller than their
//! cells, standing on those tops. A character (Tiny Dungeon's knight,
//! scaled to the cells) walks among them at the y-sorted layers' z
//! (`ctx.maps().y_sort_z`), so it draws behind a wall or a tree whose cell
//! is below its feet and in front of one above them. The walls and the
//! water collide along their cells' diamonds.
//!
//! Right of the castle stands raised ground a block up: floor 1, its layers'
//! reserved property `floor`, drawn a block up by their offset (the floor's
//! height). Its `raised` layer of grass blocks and its `raised-props` (a
//! tree on its back corner) are y-sorted with the rest, so the knight on
//! the ground walks behind the raised ground and in front of it. On the
//! ground floor the raised ground's body collides, and a ramp on the props
//! layer (its tile's `stairs` names floor 1), walled by a bush on each
//! side, climbs onto it. The knight is a `Climber`: on the ramp it changes
//! floor halfway (a `FloorChanged`), drawn eased up the ramp and a block up
//! on the raised ground, whose edge keeps it there until it comes down the
//! ramp. The camera follows it where it draws (its `GlobalTransform` plus
//! `drawn_lift`), and the HUD says which floor it is on.
//!
//! A y-sorted row sorts at its cells' middles, where a character in the
//! cell has its feet, so a character standing in the back half of a raised
//! cell would draw under the block it stands on. While the knight is drawn
//! lifted it sorts half a cell down (its sprite's `YSort::sort_offset`),
//! and so does the tree up there (its tile's `sort_offset`): it draws over
//! the ground it stands on, and goes behind and in front of the tree as it
//! does the trees on the ground.
//!
//! The pointer picks the cell drawn under it, through the raised ground's
//! height (`ctx.maps().floor_cell_at_world`), outlined where it draws. On
//! the ground the left button paints water there and the right grass,
//! through the ground tileset's Wang set (`ctx.maps().set_terrain`), its
//! edges picked from the corners round it, and the painted water collides
//! at once; the raised ground and its ramp aren't painted.
//!
//! Space, or a click on the raised ground, walks the knight to the cell
//! under the pointer by the cheapest path round the walls and the water,
//! up or down the ramp, on the town's floors joined at their stairs
//! (`ctx.maps().floor_navigation`, searched by
//! `Pathfinder::find_floor_path_grid`; the points are where the knight
//! stands, not where it draws). The floors are taken again when the town's
//! tiles change, a paint or a save of its files, and a walk under way whose
//! way they now block finds another

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, View, WALK, beside_panel, clear_to, draw_outline_lifted, draw_path,
  hovered_floor_cell, hud, load, reloaded, wasd,
};
use crate::walker::{CellPath, PathWalk, Walk, drawn_at, face, map_start, place_of, spawn_walker};

/// The town
const TOWN: &str = "maps/town.tmx";
/// Its layers: the blocks, the props the character walks among (the ramp
/// up to the raised ground among them), and the raised ground's blocks
const GROUND: &str = "ground";
const PROPS: &str = "props";
const RAISED: &str = "raised";
/// The ground tileset's Wang set and its colours
const TERRAIN: &str = "terrain";
const WATER: &str = "water";
const GRASS: &str = "grass";
/// The view's background, should the map give none
const SKY: Color = Color::rgb(120, 185, 216);
/// The knight, 64 px tall on cells 232 wide, 16 screen pixels at a quarter
/// zoom
const KNIGHT: Walk = Walk {
  sprite: 97,
  scale: 4.0,
  radius: 20.0,
  speed: 360.0,
};
/// The zooms the page offers: a quarter shows most of the town, a half
/// draws the knight's pixels two to a texel
const ZOOMS: [f32; 2] = [0.25, 0.5];
/// The settings rows, for the notes under the panel
const ROWS: usize = 1;
/// How far below its feet the knight sorts while it is drawn lifted, up the
/// ramp or on the raised ground: half a cell, as the raised ground's tree
/// does, so it never draws under the block it stands on
const LIFTED_SORT: f32 = 55.0;

pub(crate) struct TownPage {
  view: View,
  root: Option<Entity>,
  /// Where `player_start` stands
  start: Vec2,
  walker: Option<Entity>,
  /// The floor and cell drawn under the pointer
  hover: Option<(Floor, IVec2)>,
  painted: u32,
  /// Where the page's notes are on screen: the pointer paints nothing there
  covered: Vec<Rect>,
  zoom: usize,
  /// The town's floors as the knight crosses them, taken when a walk needs
  /// them and none is up to date
  nav: Option<FloorNavigation>,
  walk: Option<PathWalk>,
  /// The floor of each of the walk's waypoints, and how far up it draws
  legs: Vec<(Floor, Vec2)>,
  /// The floor and cell the knight was last sent to, and whether a path
  /// reached it
  goal: Option<((Floor, IVec2), bool)>,
}

impl TownPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      start: Vec2::ZERO,
      walker: None,
      hover: None,
      painted: 0,
      covered: Vec::new(),
      zoom: 0,
      nav: None,
      walk: None,
      legs: Vec::new(),
      goal: None,
    }
  }

  /// The camera on the character where it draws, a block up on the raised
  /// ground (on the start until it spawns), on whole screen pixels
  fn follow(&self, ctx: &mut SceneContext, jump: bool) {
    let focus = drawn_at(ctx, self.walker).unwrap_or(self.start);
    let zoom = ZOOMS[self.zoom];
    let centre = self.view.centre_for(ctx, focus, zoom);
    self.view.place(ctx, centre, zoom, jump);
  }

  /// The left button paints water at `cell`, the right grass. Painting a
  /// cell turns its four corners to water, so water would wall the knight
  /// in on any cell sharing a corner with a cell it stands on: those are
  /// refused, as are the raised ground's cells and its ramp's, where water
  /// would lie hidden under them
  fn paint(&mut self, ctx: &mut SceneContext, root: Entity, cell: IVec2) {
    let (left, right) = (
      ctx.input.mouse_down(MouseButton::Left),
      ctx.input.mouse_down(MouseButton::Right),
    );
    let color = match (left, right) {
      (true, _) => WATER,
      (false, true) => GRASS,
      _ => return,
    };
    let feet = place_of(ctx, self.walker);
    let mut maps = ctx.maps();
    let walled_in = color == WATER
      && feet
        .zip(maps.grid(root, GROUND))
        .is_some_and(|(at, layer)| beside_knight(&layer, at, cell));
    let raised = maps.tile(root, RAISED, cell).is_some()
      || maps
        .tile(root, PROPS, cell)
        .and_then(|tile| maps.tile_data(root, tile))
        .is_some_and(|data| data.stairs.is_some());
    if !walled_in && !raised && maps.set_terrain(root, GROUND, cell, TERRAIN, Some(color)) {
      self.painted += 1;
    }
  }

  /// Takes the town's floors, joined at the ramp, unless the ones it has
  /// are up to date
  fn take_floors(&mut self, ctx: &mut SceneContext, root: Entity) {
    if self.nav.is_none() {
      let mut maps = ctx.maps();
      self.nav = maps
        .bounds(root)
        .and_then(|bounds| maps.floor_navigation(root, bounds));
    }
  }

  /// Sends the knight to `goal`, a floor and its cell, by the cheapest path
  /// from where it stands on the floor it is on
  fn walk_to(&mut self, ctx: &mut SceneContext, root: Entity, goal: (Floor, IVec2)) {
    let Some(walker) = self.walker else {
      return;
    };
    if let Some(walk) = self.walk.take() {
      walk.stop(ctx);
    }
    self.take_floors(ctx, root);
    let on = ctx.world.get::<Floor>(walker).copied().unwrap_or_default();
    let from = place_of(ctx, self.walker).and_then(|at| ctx.maps().world_to_cell(root, PROPS, at));
    let path = self
      .nav
      .as_ref()
      .zip(from)
      .and_then(|(floors, from)| CellPath::find_across(floors, (on, from), goal));
    self.goal = Some((goal, path.is_some()));
    if let Some(path) = path {
      self.legs = legs(ctx, root, &path);
      self.walk = PathWalk::start(ctx, walker, path.centres);
    }
  }

  /// The floors of the waypoints still ahead and how far up each draws,
  /// the next first
  fn legs_ahead(&self, ahead: usize) -> &[(Floor, Vec2)] {
    let from = self.legs.len().saturating_sub(ahead);
    self.legs.get(from..).unwrap_or_default()
  }
}

/// Each of `path`'s waypoints' floor, and how far up it draws: its floor's
/// height, or at a stair cell (two waypoints, one on each floor) halfway
/// between the two, where the ramp's middle is drawn
fn legs(ctx: &mut SceneContext, root: Entity, path: &CellPath) -> Vec<(Floor, Vec2)> {
  let maps = ctx.maps();
  let mut legs: Vec<(Floor, Vec2)> = path
    .floors
    .iter()
    .map(|&floor| (floor, maps.lift(root, floor).unwrap_or(Vec2::ZERO)))
    .collect();
  for i in 1..legs.len() {
    if path.cells[i - 1] == path.cells[i] && legs[i - 1].0 != legs[i].0 {
      let middle = (legs[i - 1].1 + legs[i].1) / 2.0;
      legs[i - 1].1 = middle;
      legs[i].1 = middle;
    }
  }
  legs
}

/// Sorts the knight's sprite half a cell down while it is drawn lifted, and
/// where its feet are on the ground (`LIFTED_SORT`)
fn sort_where_it_stands(ctx: &mut SceneContext, walker: Entity) {
  let lifted = ctx.maps().drawn_lift(walker) != Vec2::ZERO;
  let offset = if lifted { LIFTED_SORT } else { 0.0 };
  let sprites: Vec<Entity> = ctx
    .world
    .get::<Children>(walker)
    .map(|children| children.iter().copied().collect())
    .unwrap_or_default();
  for sprite in sprites {
    if let Some(mut y_sort) = ctx.world.get_mut::<YSort>(sprite)
      && y_sort.sort_offset != offset
    {
      y_sort.sort_offset = offset;
    }
  }
}

/// What the HUD calls `floor`
fn storey(floor: Floor) -> String {
  match floor.number() {
    Some(0) => "the ground (floor 0)".to_string(),
    Some(n) => format!("the raised ground (floor {n})"),
    None => "every floor".to_string(),
  }
}

/// Whether `cell` shares a corner with a cell the knight's collider, at
/// `feet`, reaches into (on an isometric grid, the cells round a cell by
/// index)
fn beside_knight(layer: &LayerGrid, feet: Vec2, cell: IVec2) -> bool {
  let under = layer.cell_at_world(feet);
  (-1..=1)
    .flat_map(|dy| (-1..=1).map(move |dx| under + IVec2::new(dx, dy)))
    .filter(|&stood| touches(layer.outline_world(stood).points(), feet, KNIGHT.radius))
    .any(|stood| (cell - stood).abs().max_element() <= 1)
}

/// Whether a circle of `radius` at `at` reaches into the convex polygon
/// `outline` (clockwise on screen)
fn touches(outline: &[Vec2], at: Vec2, radius: f32) -> bool {
  let n = outline.len();
  let mut inside = true;
  for i in 0..n {
    let (a, b) = (outline[i], outline[(i + 1) % n]);
    let edge = b - a;
    if edge.length_squared() == 0.0 {
      continue;
    }
    let t = ((at - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
    if at.distance(a + edge * t) < radius {
      return true;
    }
    inside &= edge.perp_dot(at - a) >= 0.0;
  }
  inside
}

impl DemoPage for TownPage {
  fn name(&self) -> &str {
    "An isometric town"
  }

  fn description(&self) -> &str {
    "Walls, trees and raised ground y-sorted: walk behind and in front, up its ramp, and paint water"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Zoom", &mut self.zoom, &["1/4", "1/2"]);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Walk (pad: left stick)"),
      Control::key(KeyCode::Space, "Walk to the pointer"),
      Control::mouse("Left paints water, right grass; on the raised ground, walk there"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let Some(map) = load::<TileMap>(ctx, TOWN) else {
      return;
    };
    let background = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.start = map_start(data).unwrap_or_default();
      data.background
    };
    clear_to(ctx, background.unwrap_or(SKY));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    self.follow(ctx, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if self.walker.is_none() && ctx.maps().is_ready(root) {
      // At the z every floor's y-sorted layers draw at, climbing the stairs
      let z = ctx.maps().y_sort_z(root).unwrap_or(1);
      let walker = spawn_walker(ctx, self.start, KNIGHT, z);
      if let Ok(mut entity) = ctx.world.get_entity_mut(walker) {
        entity.insert(Climber);
      }
      self.walker = Some(walker);
    }
    if let Some(walker) = self.walker {
      face(ctx, walker);
      sort_where_it_stands(ctx, walker);
    }
    self.follow(ctx, false);
    self.hover = hovered_floor_cell(ctx, root, &self.covered);
    let painted = self.painted;
    if let Some((floor, cell)) = self.hover
      && floor == Floor::GROUND
    {
      self.paint(ctx, root, cell);
    }
    if reloaded(ctx, root) || self.painted != painted {
      // The floors are out of date: a walk takes them again at once, and
      // finds another way if this one is now blocked
      self.nav = None;
      if self.walk.is_some() {
        self.take_floors(ctx, root);
      }
      let blocked = self
        .walk
        .as_ref()
        .zip(self.nav.as_ref())
        .is_some_and(|(walk, floors)| {
          let ahead = walk.ahead();
          ahead
            .iter()
            .zip(self.legs_ahead(ahead.len()))
            .any(|(point, &(floor, _))| {
              floors
                .grid(floor)
                .is_none_or(|grid| !grid.is_walkable(grid.world_to_grid(point.x, point.y)))
            })
        });
      if blocked && let Some((goal, _)) = self.goal {
        self.walk_to(ctx, root, goal);
      }
    }
    // Space walks to the cell under the pointer, and so does a click on the
    // raised ground, where nothing is painted
    let clicked =
      ctx.input.mouse_pressed(MouseButton::Left) || ctx.input.mouse_pressed(MouseButton::Right);
    if let Some(hover) = self.hover
      && (ctx.input.key_pressed(KeyCode::Space) || (clicked && hover.0 != Floor::GROUND))
    {
      self.walk_to(ctx, root, hover);
    }
    self.walk = self.walk.take().and_then(|walk| walk.step(ctx, dt));
    // The player walking the knight themselves: the last Space is done with
    let steered = self
      .walker
      .and_then(|walker| ctx.world.get::<MoveIntent>(walker))
      .is_some_and(|intent| intent.direction != Vec2::ZERO);
    if self.walk.is_none() && steered {
      self.goal = None;
    }

    let Some(layer) = ctx.maps().grid(root, PROPS) else {
      return;
    };
    // What is drawn is where it draws: the knight and the walk ahead a
    // block up on the raised ground, the hovered cell on its floor
    let drawn = drawn_at(ctx, self.walker);
    let lift = self
      .hover
      .and_then(|(floor, _)| ctx.maps().lift(root, floor))
      .unwrap_or(Vec2::ZERO);
    let painting =
      ctx.input.mouse_down(MouseButton::Left) || ctx.input.mouse_down(MouseButton::Right);
    let route: Vec<Vec2> = self.walk.as_ref().map_or_else(Vec::new, |walk| {
      let ahead = walk.ahead();
      ahead
        .iter()
        .zip(self.legs_ahead(ahead.len()))
        .map(|(point, &(_, up))| *point + up)
        .collect()
    });
    let mut debug = ctx.debug_draw();
    if self.walk.is_some()
      && let Some(at) = drawn
    {
      draw_path(&mut debug, Some(at), &route, WALK, 3.0);
    }
    if let Some((floor, cell)) = self.hover {
      let color = if painting && floor == Floor::GROUND {
        HOT
      } else {
        Color::WHITE
      };
      draw_outline_lifted(&mut debug, &layer.outline_world(cell), lift, color, 2.0);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    let pointer = self.hover.map_or_else(
      || "Pointer: off the town".to_string(),
      |(floor, cell)| format!("Pointer: cell {}, {} on {}", cell.x, cell.y, storey(floor)),
    );
    let here = place_of(ctx, self.walker).and_then(|at| ctx.maps().world_to_cell(root, PROPS, at));
    let on = self
      .walker
      .and_then(|walker| ctx.world.get::<Floor>(walker))
      .copied()
      .unwrap_or_default();
    let walker = here.map_or_else(
      || "Knight: waiting for the map".to_string(),
      |cell| format!("Knight: cell {}, {} on {}", cell.x, cell.y, storey(on)),
    );
    let mut lines = vec![
      (
        "Isometric cells of 232 x 110; raised ground a block up".to_string(),
        Color::WHITE,
      ),
      (pointer, GOOD),
      (walker, Color::WHITE),
    ];
    match self.goal {
      Some(((floor, goal), true)) if self.walk.is_some() => {
        lines.push((
          format!(
            "Walking to cell {}, {} on {}",
            goal.x,
            goal.y,
            storey(floor)
          ),
          WALK,
        ));
      }
      Some(((floor, goal), false)) => {
        lines.push((
          format!(
            "No path to cell {}, {} on {}",
            goal.x,
            goal.y,
            storey(floor)
          ),
          WALK,
        ));
      }
      _ => {}
    }
    lines.extend([
      (format!("Cells painted: {}", self.painted), HOT),
      (
        "Walls and water stop the knight; painted water too".to_string(),
        DIM,
      ),
      (
        "The ramp, a bush each side, climbs onto the raised ground".to_string(),
        DIM,
      ),
    ]);
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
  }
}
