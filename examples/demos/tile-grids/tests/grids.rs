//! The pages on the other grids without a window: the isometric town's
//! knight draws behind a tall wall from the cell behind it and over it from
//! the cell in front (pixels sampled where the two overlap), the town's
//! walls stop it at their diamonds, painting water never walls it in, and
//! Space walks it to the cell under the pointer by a path through a gate,
//! round water painted since it last walked, until something unknown holds
//! it up or the player steers; the island's setting and the speed page's
//! choice spawn another grid's map, the speed page's hexagonal rock
//! colliding and casting shadows as whole hexagons; the hex board picks the
//! hex under the pointer, a click selects one, and the board marks the
//! selected hex's range and ring, the line to the hovered hex and the
//! hovered hex's neighbours, each as the layer's grid (`MapsView::grid`)
//! works them out, with the distance in the HUD; the board's rocks stop its
//! walker and occlude, and the HUD gives the cheapest path to the hovered
//! hex round them (trees costing 2, a rock none), which a right click
//! walks, setting off forward from past its hex's middle; and the oblique
//! map's knight stops at a skewed wall, on the map and on its mirror

mod common;

use std::collections::BTreeSet;

use common::{
  LANDING, click, engine_on, landed_maps, layer_grid, spawned_map, step_resting, warnings,
};
use image::RgbaImage;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The pages, in the demo's order
const TOWN: usize = 3;
const STAGGERED: usize = 4;
const HEX: usize = 5;
const OBLIQUE: usize = 6;
const SPEED: usize = 7;
/// The hex board's rocks and stone (`grass_14`, `_15`, `_16`, `dirt_16`,
/// `_18`, `_17` and `stone_07`) and its trees (`grass_10` to `_13` and
/// `dirt_11` to `_14`), by id in its sheet
const HEX_ROCKS: [u32; 7] = [5, 6, 7, 13, 14, 15, 16];
const HEX_TREES: [u32; 8] = [1, 2, 3, 4, 9, 10, 11, 12];
/// The hex the board's walker starts on
const HEX_START: IVec2 = IVec2::new(1, 4);
/// The page's character (the one top-down walker), once it has one
fn walker(engine: &mut HeadlessEngine) -> Option<Entity> {
  let world = engine.world_mut();
  let mut walkers = world.query_filtered::<Entity, With<TopDownMovement>>();
  walkers.iter(world).next()
}

/// Steps until the page's character has spawned
fn wait_for_walker(engine: &mut HeadlessEngine) -> Entity {
  for _ in 0..600 {
    engine.step().unwrap();
    if let Some(walker) = walker(engine) {
      return walker;
    }
  }
  panic!("the character never spawned: {:#?}", warnings());
}

/// The character's sprite: its y-sorted child
fn walker_sprite(engine: &mut HeadlessEngine, walker: Entity) -> Entity {
  let world = engine.world_mut();
  let mut sprites = world.query_filtered::<(Entity, &ChildOf), (With<Renderable>, With<YSort>)>();
  sprites
    .iter(world)
    .find(|(_, parent)| parent.parent() == walker)
    .map(|(sprite, _)| sprite)
    .expect("the character's y-sorted sprite")
}

/// The character's collider radius, and the gap its controller keeps from
/// surfaces (1% of its height unless set)
fn walker_reach(engine: &HeadlessEngine, walker: Entity) -> (f32, f32) {
  let world = engine.world();
  let radius = match world.get::<Collider>(walker).map(|c| &c.shape) {
    Some(ColliderShape::Circle { radius }) => *radius,
    other => panic!("the character's collider is a circle: {other:?}"),
  };
  let skin = world
    .get::<CharacterController>(walker)
    .and_then(|controller| controller.skin)
    .unwrap_or(0.02 * radius);
  (radius, skin)
}

/// Puts the character's feet at `at`, still
fn teleport(engine: &mut HeadlessEngine, walker: Entity, at: Vec2) {
  let world = engine.world_mut();
  if let Some(mut transform) = world.get_mut::<Transform>(walker) {
    transform.translation = at;
  }
  if let Some(mut velocity) = world.get_mut::<Velocity>(walker) {
    *velocity = Velocity::zero();
  }
}

fn walker_at(engine: &HeadlessEngine, walker: Entity) -> Vec2 {
  engine
    .world()
    .get::<Transform>(walker)
    .expect("the character's transform")
    .translation
}

/// The grid of `layer` of `map`, as the page reads it
fn grid_of(engine: &mut HeadlessEngine, map: Entity, layer: &str) -> LayerGrid {
  MapsView::new(engine.world_mut())
    .grid(map, layer)
    .unwrap_or_else(|| panic!("the layer {layer:?}"))
}

/// Every cell of the page's bounded map
fn map_cells(engine: &mut HeadlessEngine, map: Entity) -> Vec<IVec2> {
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the map's bounds");
  (bounds.min.y..bounds.max.y)
    .flat_map(|y| (bounds.min.x..bounds.max.x).map(move |x| IVec2::new(x, y)))
    .collect()
}

/// Whether the tile at `cell` of `layer` has collision shapes: a wall, or
/// on the town's ground, water
fn collides_at(engine: &mut HeadlessEngine, map: Entity, layer: &str, cell: IVec2) -> bool {
  let maps = MapsView::new(engine.world_mut());
  maps
    .tile(map, layer, cell)
    .and_then(|tile| maps.tile_data(map, tile))
    .is_some_and(|data| !data.shapes.is_empty())
}

/// Whether the town's or the island's `cell` is open ground: ground there,
/// not water, and no prop
fn open_ground(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> bool {
  let ground = MapsView::new(engine.world_mut())
    .tile(map, "ground", cell)
    .is_some();
  let prop = MapsView::new(engine.world_mut())
    .tile(map, "props", cell)
    .is_some();
  ground && !prop && !collides_at(engine, map, "ground", cell)
}

/// How far `point` is from the convex polygon `outline` (0 inside it)
fn distance_to(outline: &[Vec2], point: Vec2) -> f32 {
  let n = outline.len();
  let mut inside = true;
  let mut nearest = f32::INFINITY;
  for i in 0..n {
    let (a, b) = (outline[i], outline[(i + 1) % n]);
    let edge = b - a;
    if edge.length_squared() == 0.0 {
      continue;
    }
    let t = ((point - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
    nearest = nearest.min(point.distance(a + edge * t));
    // Clockwise on screen (+y down): the inside is on each edge's right
    if edge.perp_dot(point - a) < 0.0 {
      inside = false;
    }
  }
  if inside { 0.0 } else { nearest }
}

/// Where `at` (world pixels) is in the frame
fn to_screen(engine: &mut HeadlessEngine, at: Vec2) -> Vec2 {
  CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at)
}

/// The frame's pixels in `area` (screen pixels) where `a` and `b` differ
fn differing(a: &RgbaImage, b: &RgbaImage, area: Rect) -> BTreeSet<(u32, u32)> {
  let (x0, y0) = (area.x.max(0.0) as u32, area.y.max(0.0) as u32);
  let x1 = (area.right() as u32).min(a.width());
  let y1 = (area.bottom() as u32).min(a.height());
  (y0..y1)
    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
    .filter(|&(x, y)| a.get_pixel(x, y) != b.get_pixel(x, y))
    .collect()
}

/// The frame with the character's feet at `at`, its sprite shown or not
fn shot(
  engine: &mut HeadlessEngine,
  walker: Entity,
  sprite: Entity,
  at: Vec2,
  shown: bool,
) -> RgbaImage {
  teleport(engine, walker, at);
  if let Some(mut renderable) = engine.world_mut().get_mut::<Renderable>(sprite) {
    renderable.visible = shown;
  }
  engine.step_frames(10).unwrap();
  assert!(
    walker_at(engine, walker).distance(at) < 0.01,
    "the character stays where it was put"
  );
  engine.frame().unwrap()
}

#[test]
fn the_town_character_draws_behind_a_wall_and_in_front_of_it() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  let sprite = walker_sprite(&mut engine, walker);
  // The Zoom row has the panel's focus: in to a half, so the knight's
  // pixels are two a texel; and the pointer over the panel, clear of the
  // view
  engine.tap_key(KeyCode::ArrowRight);
  engine.mouse_move(900.0, 100.0);

  // A tall wall with open ground in the cell behind it (above it on
  // screen) and in the cell in front of it (below it)
  let wall = map_cells(&mut engine, map)
    .into_iter()
    .find(|&cell| {
      collides_at(&mut engine, map, "props", cell)
        && open_ground(&mut engine, map, cell - IVec2::ONE)
        && open_ground(&mut engine, map, cell + IVec2::ONE)
    })
    .expect("a wall with open ground behind it and in front of it");
  let layer = grid_of(&mut engine, map, "props");
  let outline = layer.outline_world(wall);
  let (top, bottom) = outline
    .points()
    .iter()
    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
      (lo.min(p.y), hi.max(p.y))
    });
  // Straight above and below its middle, clear of its diamond by more
  // than the character's radius
  let centre = layer.cell_center_world(wall);
  let (radius, _) = walker_reach(&engine, walker);
  let behind = Vec2::new(centre.x, top - radius - 30.0);
  let front = Vec2::new(centre.x, bottom + radius + 10.0);
  assert_eq!(layer.cell_at_world(behind), wall - IVec2::ONE, "behind");
  assert_eq!(layer.cell_at_world(front), wall + IVec2::ONE, "in front");

  // The frames at each place, the sprite hidden and shown, with the wall
  // and then without it
  let mut frames = Vec::new();
  for at in [behind, front] {
    frames.push((
      shot(&mut engine, walker, sprite, at, false),
      shot(&mut engine, walker, sprite, at, true),
    ));
  }
  MapsView::new(engine.world_mut()).set_tile(map, "props", wall, None);
  let mut bare = Vec::new();
  for at in [behind, front] {
    bare.push((
      shot(&mut engine, walker, sprite, at, false),
      shot(&mut engine, walker, sprite, at, true),
    ));
  }

  for (i, (at, name)) in [(behind, "behind"), (front, "in front of")]
    .into_iter()
    .enumerate()
  {
    // Around the character on screen: where it draws (shown against
    // hidden, the wall gone) and where the wall draws (with it against
    // without), and where both do
    teleport(&mut engine, walker, at);
    engine.step_frames(10).unwrap();
    let (a, b) = (
      to_screen(&mut engine, at - Vec2::new(120.0, 180.0)),
      to_screen(&mut engine, at + Vec2::new(120.0, 20.0)),
    );
    let area = Rect::new(a.x, a.y, b.x - a.x, b.y - a.y);
    let (hidden, shown) = &frames[i];
    let (bare_hidden, bare_shown) = &bare[i];
    let character = differing(bare_hidden, bare_shown, area);
    let wall_art = differing(hidden, bare_hidden, area);
    let both: Vec<(u32, u32)> = character.intersection(&wall_art).copied().collect();
    assert!(
      both.len() >= 20,
      "{name} the wall: the character and the wall overlap on screen ({} pixels)",
      both.len()
    );
    let covered = both
      .iter()
      .filter(|&&(x, y)| shown.get_pixel(x, y) == hidden.get_pixel(x, y))
      .count();
    let over = both
      .iter()
      .filter(|&&(x, y)| shown.get_pixel(x, y) == bare_shown.get_pixel(x, y))
      .count();
    if at == behind {
      // Behind it, the wall draws over the knight: no pixel where they
      // overlap shows the knight as it draws in the open, and nearly all
      // show the wall alone (the rest are its outline's see-through
      // texels, the knight faint behind them)
      assert_eq!(
        over, 0,
        "behind the wall, the knight never draws over it where they overlap"
      );
      assert!(
        covered * 10 >= both.len() * 9,
        "behind the wall, the wall covers the knight: {covered} of {} pixels",
        both.len()
      );
    } else {
      assert_eq!(
        over,
        both.len(),
        "in front of the wall, the character draws over it where they overlap"
      );
      assert!(covered < both.len(), "and so it shows");
    }
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The cells within two steps of `cell` whose tiles collide: the town's
/// and island's walls (props) and water (ground), the oblique dungeon's
/// walls
fn solid_near(
  engine: &mut HeadlessEngine,
  map: Entity,
  layers: &[&str],
  cell: IVec2,
) -> Vec<IVec2> {
  let mut out = Vec::new();
  for dy in -2..=2 {
    for dx in -2..=2 {
      let at = cell + IVec2::new(dx, dy);
      if layers
        .iter()
        .any(|layer| collides_at(engine, map, layer, at))
      {
        out.push(at);
      }
    }
  }
  out
}

/// The character's distance from the nearest outline of `cells` on
/// `layer`'s grid, after it holds `keys` for a second; asserts it is not
/// inside one
fn pushed_into(
  engine: &mut HeadlessEngine,
  walker: Entity,
  layer: &LayerGrid,
  cells: &[IVec2],
  keys: &[KeyCode],
) -> f32 {
  for key in keys {
    engine.press_key(*key);
  }
  engine.step_frames(60).unwrap();
  let at = walker_at(engine, walker);
  for key in keys {
    engine.release_key(*key);
  }
  engine.step_frames(5).unwrap();
  let mut nearest = f32::INFINITY;
  for &cell in cells {
    let outline = layer.outline_world(cell);
    let d = distance_to(outline.points(), at);
    assert!(d > 0.0, "the character at {at} went into cell {cell}");
    nearest = nearest.min(d);
  }
  nearest
}

#[test]
fn the_town_walls_stop_the_character() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  engine.mouse_move(900.0, 100.0);
  // A run of walls along x (a band down and right on screen), with open
  // ground down and left of its second wall, where the character starts
  let wall = map_cells(&mut engine, map)
    .into_iter()
    .find(|&cell| {
      (-1..=2).all(|dx| collides_at(&mut engine, map, "props", cell + IVec2::new(dx, 0)))
        && open_ground(&mut engine, map, cell + IVec2::Y)
    })
    .expect("a run of four walls with open ground down and left of one");
  let layer = grid_of(&mut engine, map, "props");
  let start = layer.cell_center_world(wall + IVec2::Y);
  teleport(&mut engine, walker, start);
  engine.step_frames(5).unwrap();

  // W and D walk it up and right into the band's lower edge: it stops
  // touching the walls' diamonds, its skin away, not short of them and not
  // through them
  let (radius, skin) = walker_reach(&engine, walker);
  let solid = solid_near(&mut engine, map, &["props", "ground"], wall);
  let gap = pushed_into(
    &mut engine,
    walker,
    &layer,
    &solid,
    &[KeyCode::KeyW, KeyCode::KeyD],
  );
  assert!(
    (gap - (radius + skin)).abs() < 2.0,
    "the character stops at the walls' diamonds: {gap} from them, its radius {radius} and skin {skin}"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The cells of `layer` holding a tile, within the map's bounds
fn marked(engine: &mut HeadlessEngine, map: Entity, layer: &str) -> BTreeSet<(i32, i32)> {
  let grid = layer_grid(engine, map, layer);
  map_cells(engine, map)
    .into_iter()
    .filter(|cell| grid.get(*cell).is_some())
    .map(|cell| (cell.x, cell.y))
    .collect()
}

/// `cells` within the map's bounds
fn on_board(engine: &mut HeadlessEngine, map: Entity, cells: Vec<IVec2>) -> BTreeSet<(i32, i32)> {
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the board's bounds");
  cells
    .into_iter()
    .filter(|c| c.cmpge(bounds.min).all() && c.cmplt(bounds.max).all())
    .map(|c| (c.x, c.y))
    .collect()
}

#[test]
fn the_hex_board_picks_the_hex_under_the_pointer_and_shows_its_range() {
  let Some(mut engine) = engine_on(HEX) else {
    return;
  };
  let map = spawned_map(&mut engine);
  engine.step_frames(3).unwrap();
  let layer = grid_of(&mut engine, map, "board");
  let world_at = |engine: &mut HeadlessEngine, screen: Vec2| {
    CameraMut::main(engine.world_mut())
      .expect("the main camera")
      .screen_to_world(screen)
  };

  // A click selects the hex under the view's middle
  let middle = world_at(&mut engine, Vec2::new(260.0, 320.0));
  let selected = layer.cell_at_world(middle);
  let at = to_screen(&mut engine, layer.cell_center_world(selected));
  click(&mut engine, at, MouseButton::Left);
  let shown = engine.ui_snapshot();
  let line = format!("Selected: hex {}, {}", selected.x, selected.y);
  assert!(shown.contains(&line), "{line:?} in {shown}");

  // The pointer over a hex three steps away, on the board and in the view,
  // near one of its corners: the hex under it is picked
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the board's bounds");
  let hovered = layer
    .grid
    .ring(selected, 3)
    .into_iter()
    .find(|cell| {
      let screen = to_screen(&mut engine, layer.cell_center_world(*cell));
      cell.cmpge(bounds.min).all()
        && cell.cmplt(bounds.max).all()
        && (40.0..480.0).contains(&screen.x)
        && (110.0..540.0).contains(&screen.y)
    })
    .expect("a hex three steps away in the view");
  let corner = layer.outline_world(hovered).points()[0];
  let centre = layer.cell_center_world(hovered);
  let near_corner = to_screen(&mut engine, centre + (corner - centre) * 0.8);
  engine.mouse_move(near_corner.x, near_corner.y);
  engine.step_frames(3).unwrap();
  let shown = engine.ui_snapshot();
  for line in [
    format!("Pointer: hex {}, {}", hovered.x, hovered.y),
    format!("Distance: {} steps", layer.grid.distance(selected, hovered)),
  ] {
    assert!(shown.contains(&line), "{line:?} in {shown}");
  }

  // The marks are the grid's: the selected hex's range and ring at the
  // radius, the line to the hovered hex, and the hovered hex's neighbours
  let check = |engine: &mut HeadlessEngine, radius: u32| {
    let grid = layer.grid;
    let expected = [
      ("range", grid.range(selected, radius)),
      ("ring", grid.ring(selected, radius)),
      ("line", grid.line(selected, hovered)),
      ("neighbours", grid.neighbours(hovered).collect()),
    ];
    for (name, cells) in expected {
      let cells = on_board(engine, map, cells);
      assert!(!cells.is_empty(), "{name} has cells on the board");
      assert_eq!(
        marked(engine, map, name),
        cells,
        "the {name} at radius {radius}"
      );
    }
  };
  check(&mut engine, 2);
  // The Radius row has the panel's focus: one more
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(3).unwrap();
  check(&mut engine, 3);
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The id of the board's tile at `cell`
fn hex_tile(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> Option<u32> {
  MapsView::new(engine.world_mut())
    .tile(map, "board", cell)
    .map(|tile| tile.id)
}

/// Whether the board's `cell` holds rocks or stone
fn is_rock(engine: &mut HeadlessEngine, map: Entity, cell: IVec2) -> bool {
  hex_tile(engine, map, cell).is_some_and(|id| HEX_ROCKS.contains(&id))
}

/// Whether `cell`'s centre is in the board's view, clear of the notes
fn in_view(engine: &mut HeadlessEngine, layer: &LayerGrid, cell: IVec2) -> bool {
  let screen = to_screen(engine, layer.cell_center_world(cell));
  (40.0..480.0).contains(&screen.x) && (110.0..540.0).contains(&screen.y)
}

/// The cheapest path from `from` to `to` on `grid`, and its cost: each
/// step's cost as the grid gives it
fn cheapest(grid: &NavigationGrid, from: IVec2, to: IVec2) -> Option<(Vec<IVec2>, f32)> {
  let path: Vec<IVec2> = Pathfinder::new()
    .find_path_grid(grid, from.into(), to.into())
    .path()?
    .waypoints()
    .iter()
    .map(|node| node.grid_coord.into())
    .collect();
  let cost = path
    .windows(2)
    .map(|step| {
      grid
        .neighbors(step[0].into())
        .into_iter()
        .find(|(cell, _)| IVec2::from(*cell) == step[1])
        .map(|(_, cost)| cost)
        .expect("each step to a neighbour")
    })
    .sum();
  Some((path, cost))
}

/// The HUD after the pointer moves over `cell`
fn hovering(engine: &mut HeadlessEngine, layer: &LayerGrid, cell: IVec2) -> String {
  let at = to_screen(engine, layer.cell_center_world(cell));
  engine.mouse_move(at.x, at.y);
  engine.step_frames(3).unwrap();
  engine.ui_snapshot()
}

#[test]
fn the_hex_board_s_walker_paths_around_the_rocks() {
  let Some(mut engine) = engine_on(HEX) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  engine.step_frames(3).unwrap();
  let layer = grid_of(&mut engine, map, "board");
  let start = layer.cell_at_world(walker_at(&engine, walker));
  assert_eq!(start, HEX_START, "the walker starts on its hex");

  // The hex in the view whose straight line from the walker crosses the
  // most rocks: one behind a clump of them
  let cells = map_cells(&mut engine, map);
  let (mut goal, mut most) = (None, 0);
  for &cell in &cells {
    if !in_view(&mut engine, &layer, cell) || is_rock(&mut engine, map, cell) {
      continue;
    }
    let rocks = layer
      .grid
      .line(start, cell)
      .into_iter()
      .filter(|&on| is_rock(&mut engine, map, on))
      .count();
    if rocks > most {
      (goal, most) = (Some(cell), rocks);
    }
  }
  let goal = goal.expect("a hex in the view behind rocks");
  assert!(
    most >= 2,
    "the line to {goal} crosses a clump: {most} rocks"
  );

  // The cheapest path as the board's tiles have it: rocks and stone
  // blocked, trees costing 2
  let bounds = MapsView::new(engine.world_mut())
    .bounds(map)
    .expect("the board's bounds");
  let mut grid = NavigationGrid::for_layer(layer, bounds);
  for &cell in &cells {
    match hex_tile(&mut engine, map, cell) {
      Some(id) if HEX_ROCKS.contains(&id) => grid.set(cell.into(), CellState::Blocked),
      Some(id) if HEX_TREES.contains(&id) => grid.set(cell.into(), CellState::Difficult(2.0)),
      _ => {}
    }
  }
  let (path, cost) = cheapest(&grid, start, goal).expect("a path round the rocks");
  for &cell in &path {
    assert!(
      grid.is_walkable(cell.into()) && !is_rock(&mut engine, map, cell),
      "{cell} on the path is walkable, and no rock"
    );
  }

  // Hovered, the HUD gives that path; a hex among the trees costs 2 to
  // enter, and a rock has no path
  let tree = cells
    .iter()
    .copied()
    .find(|&cell| {
      cell != start
        && in_view(&mut engine, &layer, cell)
        && hex_tile(&mut engine, map, cell).is_some_and(|id| HEX_TREES.contains(&id))
    })
    .expect("trees in the view");
  let (to_tree, dear) = cheapest(&grid, start, tree).expect("a path to the trees");
  assert!(
    dear >= to_tree.len() as f32,
    "entering trees costs 2: {dear}"
  );
  let rock = cells
    .iter()
    .copied()
    .find(|&cell| in_view(&mut engine, &layer, cell) && is_rock(&mut engine, map, cell))
    .expect("a rock in the view");
  for (hex, line) in [
    (
      tree,
      format!("Path: {} hexes, cost {dear:.1}", to_tree.len()),
    ),
    (rock, format!("No path to hex {}, {}", rock.x, rock.y)),
    (goal, format!("Path: {} hexes, cost {cost:.1}", path.len())),
  ] {
    let shown = hovering(&mut engine, &layer, hex);
    assert!(shown.contains(&line), "{line:?} in {shown}");
  }

  // A right click walks it hex by hex, never into a rock, and the walker
  // stops on the goal's middle
  engine.press_mouse(MouseButton::Right);
  let mut walked = vec![start];
  for frame in 0..600 {
    engine.step().unwrap();
    if frame == 0 {
      engine.release_mouse(MouseButton::Right);
    }
    let cell = layer.cell_at_world(walker_at(&engine, walker));
    assert!(
      !is_rock(&mut engine, map, cell),
      "the walker went into the rocks at {cell}"
    );
    if walked.last() != Some(&cell) {
      walked.push(cell);
    }
  }
  assert_eq!(walked, path, "the walker walks the path");
  let feet = walker_at(&engine, walker);
  let off = feet.distance(layer.cell_center_world(goal));
  assert!(off < 6.0, "it stops on the goal's middle: {off} px off");
  assert!(
    engine.world().get::<MoveBindings>(walker).is_some(),
    "the walker has its keys back"
  );
  // Hovering its own hex, the HUD gives no path
  let shown = engine.ui_snapshot();
  assert!(
    !shown.contains("Path:") && !shown.contains("No path"),
    "{shown}"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_hex_board_s_walker_sets_off_forward() {
  let Some(mut engine) = engine_on(HEX) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  engine.step_frames(3).unwrap();
  let layer = grid_of(&mut engine, map, "board");
  // The walker a little past its hex's middle toward the next hex on its
  // row, which the pointer is on
  let (here, next) = (HEX_START, HEX_START + IVec2::X);
  assert!(!is_rock(&mut engine, map, next), "{next} is open");
  let (from, to) = (layer.cell_center_world(here), layer.cell_center_world(next));
  teleport(&mut engine, walker, from + (to - from) * 0.3);
  engine.step_frames(3).unwrap();
  assert_eq!(layer.cell_at_world(walker_at(&engine, walker)), here);
  let shown = hovering(&mut engine, &layer, next);
  assert!(
    shown.contains("Path: 2 hexes"),
    "the path to {next} in {shown}"
  );

  // A right click: it walks on toward the next hex, never back first
  engine.press_mouse(MouseButton::Right);
  let mut nearest = walker_at(&engine, walker).distance(to);
  for frame in 0..20 {
    engine.step().unwrap();
    if frame == 0 {
      engine.release_mouse(MouseButton::Right);
    }
    let away = walker_at(&engine, walker).distance(to);
    assert!(
      away <= nearest + 0.01,
      "frame {frame}: the walker moved away from {next}, {away} px from it after {nearest}"
    );
    nearest = away;
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_hex_board_s_rocks_stop_the_walker() {
  let Some(mut engine) = engine_on(HEX) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  engine.mouse_move(900.0, 100.0);
  engine.step_frames(3).unwrap();
  let layer = grid_of(&mut engine, map, "board");
  let start = layer.cell_at_world(walker_at(&engine, walker));
  assert_eq!(start, HEX_START, "the walker starts on its hex");
  let (radius, skin) = walker_reach(&engine, walker);
  assert!(
    radius < 35.0,
    "its collider is under half a hex's side: {radius}"
  );

  // D walks it right, along its row, into the first rock there: it never
  // goes into the rock's hexagon, and stops at it, its skin away
  let rock = (1..8)
    .map(|dx| start + IVec2::new(dx, 0))
    .find(|&cell| is_rock(&mut engine, map, cell))
    .expect("a rock to the walker's right");
  let outline = layer.outline_world(rock);
  engine.press_key(KeyCode::KeyD);
  for _ in 0..120 {
    engine.step().unwrap();
    let at = walker_at(&engine, walker);
    assert!(
      layer.cell_at_world(at) != rock && distance_to(outline.points(), at) > 0.0,
      "the walker at {at} went into the rock at {rock}"
    );
  }
  engine.release_key(KeyCode::KeyD);
  engine.step_frames(5).unwrap();
  let gap = distance_to(outline.points(), walker_at(&engine, walker));
  assert!(
    (gap - (radius + skin)).abs() < 2.0,
    "the walker stops at the rock's hexagon: {gap} from it, its radius {radius} and skin {skin}"
  );
  // And the rocks occlude: the board has shadow chains
  let world = engine.world_mut();
  let mut chains = world.query_filtered::<(), (With<MapOccluder>, With<LightOccluder2D>)>();
  assert!(chains.iter(world).count() > 0, "the board's shadow chains");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_oblique_character_collides_with_a_skewed_wall() {
  let Some(mut engine) = engine_on(OBLIQUE) else {
    return;
  };
  // The map's own skew, then (the Skew row has the panel's focus) its
  // mirror's
  for mirror in [false, true] {
    if mirror {
      engine.tap_key(KeyCode::ArrowRight);
    }
    let map = spawned_map(&mut engine);
    let walker = wait_for_walker(&mut engine);
    engine.mouse_move(900.0, 100.0);
    let skew = MapsView::new(engine.world_mut())
      .grid(map, "walls")
      .map(|layer| layer.grid.shape);
    let Some(GridShape::Oblique { skew }) = skew else {
      panic!("the dungeon is an oblique map: {skew:?}");
    };
    assert_ne!(skew.x, 0, "skewed across its rows");
    assert_eq!(skew.x < 0, mirror, "the mirror skews the other way");

    // A wall in a column of walls, with floor two cells to its left: D
    // walks the character from there into it, and it stops at the wall's
    // skewed edge
    let wall = map_cells(&mut engine, map)
      .into_iter()
      .find(|&cell| {
        (-1..=1).all(|dy| collides_at(&mut engine, map, "walls", cell + IVec2::new(0, dy)))
          && (1..=2).all(|dx| {
            let floor = cell - IVec2::new(dx, 0);
            !collides_at(&mut engine, map, "walls", floor)
              && MapsView::new(engine.world_mut())
                .tile(map, "floor", floor)
                .is_some()
          })
      })
      .expect("a wall in a column with floor to its left");
    let layer = grid_of(&mut engine, map, "walls");
    teleport(
      &mut engine,
      walker,
      layer.cell_center_world(wall - IVec2::new(2, 0)),
    );
    engine.step_frames(5).unwrap();
    let (radius, skin) = walker_reach(&engine, walker);
    let solid = solid_near(&mut engine, map, &["walls"], wall);
    let gap = pushed_into(&mut engine, walker, &layer, &solid, &[KeyCode::KeyD]);
    assert!(
      (gap - (radius + skin)).abs() < 1.0,
      "skew {skew}: the character stops at the skewed wall: {gap} from it, its radius {radius} and skin {skin}"
    );
    // The wall's left edge leans by the skew: the character met it there
    let outline = layer.outline_world(wall);
    let left: Vec<Vec2> = {
      let mut points = outline.points().to_vec();
      points.sort_by(|a, b| a.x.total_cmp(&b.x));
      points.truncate(2);
      points.sort_by(|a, b| a.y.total_cmp(&b.y));
      points
    };
    assert!(
      ((left[1].x - left[0].x) - skew.x as f32).abs() < 0.01,
      "the wall's left edge leans by the skew, its bottom further right: {left:?}"
    );
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_staggered_and_speed_pages_land_their_maps() {
  // The island's setting switches it to its staggered-X copy, a map of its
  // own; the speed page's choice builds the chosen grid's map, and only it
  for (page, layer) in [(STAGGERED, "props"), (SPEED, "ground")] {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    let first = spawned_map(&mut engine);
    let shape = |engine: &mut HeadlessEngine, map: Entity| grid_of(engine, map, layer).grid.shape;
    let before = shape(&mut engine, first);
    engine.tap_key(KeyCode::ArrowRight);
    engine.step_frames(2).unwrap();
    let mut after = None;
    for _ in 0..LANDING {
      step_resting(&mut engine);
      let (landed, roots) = landed_maps(&mut engine);
      if landed.len() == 1 && roots == 1 {
        let world = engine.world_mut();
        let mut maps = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
        let map = maps.iter(world).next().expect("the map");
        if map != first {
          after = Some(shape(&mut engine, map));
          break;
        }
      }
    }
    let after = after.expect("the setting spawned another map");
    assert_ne!(before, after, "page {}: another grid", page + 1);
  }
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_speed_page_s_hexagonal_rock_collides_and_casts_shadows_as_whole_hexagons() {
  let Some(mut engine) = engine_on(SPEED) else {
    return;
  };
  spawned_map(&mut engine);
  // The Grid setting's fourth choice
  for _ in 0..3 {
    engine.tap_key(KeyCode::ArrowRight);
    engine.step_frames(2).unwrap();
  }
  let mut hex = None;
  for _ in 0..LANDING {
    step_resting(&mut engine);
    let world = engine.world_mut();
    let mut maps = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
    let landed: Vec<Entity> = maps.iter(world).collect();
    let [map] = landed[..] else { continue };
    let shape = grid_of(&mut engine, map, "ground").grid.shape;
    if matches!(shape, GridShape::Hexagonal { .. }) {
      hex = Some(map);
      break;
    }
  }
  let map = hex.expect("the hexagonal map landed");
  // Its first rock in row order
  let ground = layer_grid(&mut engine, map, "ground");
  let rock = (0..1024)
    .flat_map(|y| (0..1024).map(move |x| IVec2::new(x, y)))
    .find(|&cell| ground.get(cell).is_some())
    .expect("a rock");
  engine.step_frames(2).unwrap();
  let world = engine.world_mut();
  let kind = world.get::<TileColliders>(map).map(|tiles| tiles.get(rock));
  assert_eq!(
    kind,
    Some(TileCell::Solid),
    "the rock at {rock} collides as its whole hexagon"
  );
  let chains = world.query::<&MapOccluder>().iter(world).count();
  assert!(chains > 0, "the hexagonal map casts shadows");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn painting_water_never_walls_the_town_s_knight_in() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  // Open ground two cells deep all round, the knight in the middle; the
  // pointer then paints water on every one of those cells
  let dry = |engine: &mut HeadlessEngine, cell: IVec2| {
    MapsView::new(engine.world_mut())
      .tile(map, "ground", cell)
      .is_some()
      && !collides_at(engine, map, "ground", cell)
  };
  let around =
    |home: IVec2| (-2..=2).flat_map(move |dy| (-2..=2).map(move |dx| home + IVec2::new(dx, dy)));
  let home = map_cells(&mut engine, map)
    .into_iter()
    .find(|&home| {
      open_ground(&mut engine, map, home) && around(home).all(|cell| dry(&mut engine, cell))
    })
    .expect("open ground with dry ground two cells deep round it");
  let layer = grid_of(&mut engine, map, "ground");
  teleport(&mut engine, walker, layer.cell_center_world(home));
  engine.step_frames(5).unwrap();
  for cell in around(home) {
    let at = to_screen(&mut engine, layer.cell_center_world(cell));
    click(&mut engine, at, MouseButton::Left);
  }

  // Water lies two cells out, and never on a cell the knight stands on
  let (radius, skin) = walker_reach(&engine, walker);
  let feet = walker_at(&engine, walker);
  let stood: Vec<IVec2> = around(home)
    .filter(|&cell| distance_to(layer.outline_world(cell).points(), feet) < radius + skin)
    .collect();
  assert!(stood.contains(&home), "the knight stands in {home}");
  for &cell in &stood {
    assert!(
      !collides_at(&mut engine, map, "ground", cell),
      "no water in {cell}, under the knight"
    );
  }
  let wet = around(home)
    .filter(|&cell| collides_at(&mut engine, map, "ground", cell))
    .count();
  assert!(wet > 0, "the pointer painted water two cells out");

  // And it can still walk
  engine.press_key(KeyCode::KeyD);
  engine.step_frames(30).unwrap();
  engine.release_key(KeyCode::KeyD);
  let moved = walker_at(&engine, walker).distance(feet);
  assert!(moved > 20.0, "the knight walks on: it moved {moved} px");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_town_knight_walks_a_path_around_the_walls() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  engine.step_frames(3).unwrap();
  let layer = grid_of(&mut engine, map, "props");
  let home = layer.cell_at_world(walker_at(&engine, walker));

  // A wall in a run of walls, with open ground on either side of it: the
  // side nearer the knight's start is inside the castle
  let wall = map_cells(&mut engine, map)
    .into_iter()
    .find(|&cell| {
      (-1..=1).all(|dy| collides_at(&mut engine, map, "props", cell + IVec2::new(0, dy)))
        && open_ground(&mut engine, map, cell + IVec2::X)
        && open_ground(&mut engine, map, cell - IVec2::X)
    })
    .expect("a wall in a run with open ground either side");
  let (mut inside, mut outside) = (wall - IVec2::X, wall + IVec2::X);
  if layer.grid.distance(home, outside) < layer.grid.distance(home, inside) {
    (inside, outside) = (outside, inside);
  }

  // From outside, the pointer on the cell inside; Space walks the knight
  // there, round the walls through a gate
  teleport(&mut engine, walker, layer.cell_center_world(outside));
  engine.step_frames(30).unwrap();
  let at = to_screen(&mut engine, layer.cell_center_world(inside));
  engine.mouse_move(at.x, at.y);
  engine.step_frames(2).unwrap();
  let shown = engine.ui_snapshot();
  let line = format!("Pointer: cell {}, {}", inside.x, inside.y);
  assert!(shown.contains(&line), "{line:?} in {shown}");
  engine.tap_key(KeyCode::Space);
  for _ in 0..900 {
    engine.step().unwrap();
    let cell = layer.cell_at_world(walker_at(&engine, walker));
    assert!(
      !collides_at(&mut engine, map, "props", cell)
        && !collides_at(&mut engine, map, "ground", cell),
      "the knight crossed a wall or water at {cell}"
    );
  }
  let feet = walker_at(&engine, walker);
  assert_eq!(
    layer.cell_at_world(feet),
    inside,
    "the knight walked inside"
  );
  let off = feet.distance(layer.cell_center_world(inside));
  assert!(off < 6.0, "it stops on the cell's middle: {off} px off");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The first of five cells in a row (along x) of the town's open ground,
/// with open ground a cell deep all round them
fn open_row(engine: &mut HeadlessEngine, map: Entity) -> IVec2 {
  map_cells(engine, map)
    .into_iter()
    .find(|&home| {
      (-1..=1).all(|dy| (-1..=5).all(|dx| open_ground(engine, map, home + IVec2::new(dx, dy))))
    })
    .expect("open ground round a row of five cells")
}

/// The pointer on the middle of the town's `cell`, where it is on screen
fn point_at(engine: &mut HeadlessEngine, layer: &LayerGrid, cell: IVec2) -> Vec2 {
  let at = to_screen(engine, layer.cell_center_world(cell));
  engine.mouse_move(at.x, at.y);
  engine.step_frames(2).unwrap();
  at
}

#[test]
fn the_town_knight_s_path_reads_water_painted_since_it_last_walked() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  // The knight at the start of a row of open ground
  let home = open_row(&mut engine, map);
  let layer = grid_of(&mut engine, map, "props");
  teleport(&mut engine, walker, layer.cell_center_world(home));
  engine.step_frames(30).unwrap();
  // Space on its own cell takes the town's grid, and walks nowhere
  point_at(&mut engine, &layer, home);
  engine.tap_key(KeyCode::Space);
  engine.step_frames(5).unwrap();
  // Then water painted in the middle of the row, and Space at its end:
  // the knight walks round the water, on a grid taken again
  let (water, goal) = (home + IVec2::new(2, 0), home + IVec2::new(4, 0));
  let at = point_at(&mut engine, &layer, water);
  click(&mut engine, at, MouseButton::Left);
  assert!(
    collides_at(&mut engine, map, "ground", water),
    "water painted at {water}"
  );
  point_at(&mut engine, &layer, goal);
  engine.tap_key(KeyCode::Space);
  for _ in 0..600 {
    engine.step().unwrap();
    let cell = layer.cell_at_world(walker_at(&engine, walker));
    assert!(
      !collides_at(&mut engine, map, "ground", cell),
      "the knight walked into the water at {cell}"
    );
  }
  let feet = walker_at(&engine, walker);
  assert_eq!(
    layer.cell_at_world(feet),
    goal,
    "the knight walked round it"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_town_knight_finds_its_way_round_water_painted_ahead_of_it() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  let home = open_row(&mut engine, map);
  let layer = grid_of(&mut engine, map, "props");
  teleport(&mut engine, walker, layer.cell_center_world(home));
  engine.step_frames(30).unwrap();

  // Space at the row's end, then, as the knight sets off, water painted
  // ahead of it on its way: it goes round
  let (water, goal) = (home + IVec2::new(3, 0), home + IVec2::new(4, 0));
  point_at(&mut engine, &layer, goal);
  engine.tap_key(KeyCode::Space);
  engine.step_frames(10).unwrap();
  let at = point_at(&mut engine, &layer, water);
  engine.press_mouse(MouseButton::Left);
  engine.step().unwrap();
  engine.release_mouse(MouseButton::Left);
  assert!(
    collides_at(&mut engine, map, "ground", water),
    "water painted at {water} (the pointer at {at})"
  );
  for _ in 0..600 {
    engine.step().unwrap();
    let cell = layer.cell_at_world(walker_at(&engine, walker));
    assert!(
      !collides_at(&mut engine, map, "ground", cell),
      "the knight walked into the water at {cell}"
    );
  }
  let feet = walker_at(&engine, walker);
  assert_eq!(layer.cell_at_world(feet), goal, "the knight went round it");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_town_knight_s_walk_stops_when_held_up_or_steered() {
  let Some(mut engine) = engine_on(TOWN) else {
    return;
  };
  let map = spawned_map(&mut engine);
  let walker = wait_for_walker(&mut engine);
  let home = open_row(&mut engine, map);
  let layer = grid_of(&mut engine, map, "props");
  teleport(&mut engine, walker, layer.cell_center_world(home));
  engine.step_frames(30).unwrap();
  let walk_to = |engine: &mut HeadlessEngine, cell: IVec2| {
    point_at(engine, &layer, cell);
    engine.tap_key(KeyCode::Space);
    engine.step_frames(5).unwrap();
  };
  let walking = |engine: &mut HeadlessEngine| {
    let shown = engine.ui_snapshot();
    let bound = engine.world().get::<MoveBindings>(walker).is_some();
    (shown.contains("Walking to cell"), bound)
  };

  // Space along the row: it walks, its keys taken off while it does
  let goal = home + IVec2::new(4, 0);
  walk_to(&mut engine, goal);
  assert_eq!(walking(&mut engine), (true, false), "the knight walks");

  // Something the grid doesn't know in its way: it gives up after a second
  // held up, short of the goal, its keys back
  let block = layer.cell_center_world(home + IVec2::new(2, 0));
  engine
    .world_mut()
    .spawn((Transform::from_translation(block), Collider::circle(60.0)));
  engine.step_frames(180).unwrap();
  assert_eq!(walking(&mut engine), (false, true), "the walk gave up");
  let held = walker_at(&engine, walker);
  assert!(
    held.distance(block) < 60.0 + 30.0,
    "held up at the block: {held}"
  );

  // Sent home, a movement key takes it over at once
  walk_to(&mut engine, home);
  assert_eq!(walking(&mut engine), (true, false), "the knight walks home");
  engine.press_key(KeyCode::KeyS);
  engine.step_frames(2).unwrap();
  assert_eq!(walking(&mut engine), (false, true), "the player took over");
  let before = walker_at(&engine, walker);
  engine.step_frames(20).unwrap();
  engine.release_key(KeyCode::KeyS);
  assert!(
    walker_at(&engine, walker).y > before.y + 20.0,
    "and S walks it down"
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}
