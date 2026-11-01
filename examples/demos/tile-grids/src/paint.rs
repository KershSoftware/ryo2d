//! The page "Paint anywhere": an empty infinite map built in code
//! (`TileMap::infinite`, one layer over the Pixel Platformer tileset),
//! spawned with a `MapRoot` and grown at run time. The left button paints
//! ground through `ctx.maps().set_tile` at any cell, negative or far out,
//! and the right button erases; each painted cell and its four neighbours
//! pick their tiles from which cells beside them are ground. A write in a
//! chunk the layer does not have makes it: it draws, collides and casts
//! shadows like the rest, and the overlay flashes it. A chunk stays once
//! made, emptied or not. WASD or a pad pans without limit.
//!
//! A ball (a dynamic body) bounces on what is painted and comes to rest on
//! it; Space drops it at the pointer. The HUD shows the cell under the
//! pointer, the chunks there are, and the layer's bounds as painted, the
//! cells holding tiles, which `ctx.maps().layer_bounds` reads (the map's
//! whole is `ctx.maps().bounds`)

use demo_kit::prelude::*;

use crate::overlay::{self, BOUNDS_LINE, FRESH, Grid, Minimap};
use crate::shared::{
  CELL, DIM, GOOD, HOT, TILESET, View, beside_panel, clear_to, ground_tile, hud, load, pan_input,
  wasd,
};

/// The paint page's ball
#[derive(Component, Debug)]
pub struct Ball;

/// The layer painted
const LAYER: &str = "ground";
/// The view's background
const SKY: Color = Color::rgb(159, 220, 244);
/// The ball's radius in pixels, and its z (over the map's one layer at 0)
const BALL_RADIUS: f32 = 7.0;
const BALL_Z: i32 = 10;
/// How fast WASD pans, in screen pixels a second
const PAN_SPEED: f32 = 480.0;
/// Seconds a chunk just made glows
const FLASH: f32 = 1.5;
/// The settings rows, for the notes under the panel
const ROWS: usize = 3;
/// Where the camera starts: a little up and left of cell (0, 0), so cells
/// at negative coordinates fill much of the view
const START: Vec2 = Vec2::new(-4.0 * CELL, -2.0 * CELL);
/// What the page paints as it opens, so the ball has somewhere to land: a
/// floor across the origin and two ledges, [from, to] in cells
const STARTER: [(IVec2, IVec2); 3] = [
  (IVec2::new(-10, 3), IVec2::new(6, 3)),
  (IVec2::new(-14, 1), IVec2::new(-12, 1)),
  (IVec2::new(9, 1), IVec2::new(12, 1)),
];
/// Where the ball starts, in cells
const BALL_START: IVec2 = IVec2::new(-2, -6);

/// The four cells beside a cell
const BESIDE: [IVec2; 4] = [IVec2::NEG_Y, IVec2::Y, IVec2::NEG_X, IVec2::X];

pub(crate) struct PaintPage {
  view: View,
  root: Option<Entity>,
  /// The map's index of its one tileset
  tileset: u16,
  ball: Option<Entity>,
  /// The world point at the view's centre, panned
  at: Vec2,
  grid: Option<Grid>,
  /// The cell under the pointer, when it is over the world
  hover: Option<IVec2>,
  /// The last cell a held button painted or erased, and which
  last: Option<(IVec2, bool)>,
  /// The chunks there are (by row), and those made just now, with how
  /// long they have glowed
  chunks: Vec<IVec2>,
  scratch: Vec<IVec2>,
  fresh: Vec<(IVec2, f32)>,
  /// Chunks made since the page opened
  made: usize,
  /// The layer's bounds: the cells holding tiles
  bounds: CellRect,
  painted: u32,
  erased: u32,
  /// Where the page's notes are on screen: the pointer paints nothing there
  covered: Vec<Rect>,
  /// The camera has been put in place
  placed: bool,
  overlay: bool,
  zoom: i32,
  bounce: f32,
}

impl PaintPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      tileset: 0,
      ball: None,
      at: START,
      grid: None,
      hover: None,
      last: None,
      chunks: Vec::new(),
      scratch: Vec::new(),
      fresh: Vec::new(),
      made: 0,
      bounds: CellRect::default(),
      painted: 0,
      erased: 0,
      covered: Vec::new(),
      placed: false,
      overlay: true,
      zoom: 2,
      bounce: 0.5,
    }
  }

  /// Paints (or erases) `cell`, and gives it and its four neighbours the
  /// tiles their neighbours call for; whether the cell changed
  fn brush(&mut self, ctx: &mut SceneContext, root: Entity, cell: IVec2, paint: bool) -> bool {
    let mut maps = ctx.maps();
    if maps.tile(root, LAYER, cell).is_some() == paint {
      return false;
    }
    let tile = paint.then_some(Tile::new(self.tileset, 0));
    maps.set_tile(root, LAYER, cell, tile);
    for at in std::iter::once(cell).chain(BESIDE.map(|step| cell + step)) {
      if maps.tile(root, LAYER, at).is_none() {
        continue;
      }
      let ground = |step: IVec2| maps.tile(root, LAYER, at + step).is_some();
      let id = ground_tile(
        ground(IVec2::NEG_Y),
        ground(IVec2::Y),
        ground(IVec2::NEG_X),
        ground(IVec2::X),
      );
      maps.set_tile(root, LAYER, at, Some(Tile::new(self.tileset, id)));
    }
    if paint {
      self.painted += 1;
    } else {
      self.erased += 1;
    }
    true
  }

  /// Paints (or erases) every cell on the line from `from` to `to`, so a
  /// quick drag leaves no gaps
  fn stroke(&mut self, ctx: &mut SceneContext, root: Entity, from: IVec2, to: IVec2, paint: bool) {
    let steps = (to - from).abs().max_element().max(1);
    for i in 0..=steps {
      let t = i as f32 / steps as f32;
      let cell = (from.as_vec2() + (to - from).as_vec2() * t)
        .round()
        .as_ivec2();
      self.brush(ctx, root, cell, paint);
    }
  }

  /// The ball at `at` (world pixels), still
  fn drop_ball(&mut self, ctx: &mut SceneContext, at: Vec2) {
    let Some(ball) = self.ball else {
      return;
    };
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(ball) {
      transform.translation = at;
    }
    if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(ball) {
      *velocity = Velocity::zero();
    }
  }

  /// Once the map has landed: the starter ground, and the ball over it
  fn land(&mut self, ctx: &mut SceneContext, root: Entity) {
    self.grid = Grid::of(ctx, root, LAYER, CELL);
    for (from, to) in STARTER {
      for y in from.y..=to.y {
        for x in from.x..=to.x {
          self.brush(ctx, root, IVec2::new(x, y), true);
        }
      }
    }
    // What the page painted is not the player's
    self.painted = 0;
    let image = ctx.assets().add(ball_image((BALL_RADIUS * 2.0) as u32));
    let at = ctx
      .maps()
      .cell_to_world(root, LAYER, BALL_START)
      .unwrap_or(START);
    let ball = ctx
      .world
      .spawn((
        Transform::from_translation(at),
        Interpolated,
        RigidBody::dynamic().lock_rotation().with_ccd(),
        Collider::circle(BALL_RADIUS)
          .with_restitution(self.bounce)
          .with_friction(0.6),
        Renderable::image(image).with_z_order(BALL_Z),
        Ball,
      ))
      .id();
    self.ball = Some(ball);
  }

  /// The chunks there are now, those just made glowing, and the bounds
  fn read_chunks(&mut self, ctx: &mut SceneContext, root: Entity, dt: f32) {
    overlay::map_chunks(ctx.world, root, &mut self.scratch);
    for &chunk in &self.scratch {
      let key = |c: &IVec2| (c.y, c.x);
      if self.chunks.binary_search_by_key(&key(&chunk), key).is_err() {
        self.fresh.push((chunk, FLASH));
        self.made += 1;
      }
    }
    std::mem::swap(&mut self.chunks, &mut self.scratch);
    for (_, left) in &mut self.fresh {
      *left -= dt;
    }
    self.fresh.retain(|(_, left)| *left > 0.0);
    self.bounds = ctx.maps().layer_bounds(root, LAYER).unwrap_or_default();
  }
}

/// A ball of `side` pixels in Kenney's style: a dark outline, a red body,
/// a highlight up and left
fn ball_image(side: u32) -> Image {
  const OUTLINE: [u8; 4] = [46, 34, 47, 255];
  const BODY: [u8; 4] = [232, 70, 60, 255];
  const SHADE: [u8; 4] = [180, 40, 46, 255];
  const LIGHT: [u8; 4] = [255, 170, 150, 255];
  let r = side as f32 / 2.0;
  let inside = |x: i32, y: i32| {
    let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
    dx * dx + dy * dy <= r * r
  };
  let mut data = vec![0u8; (side * side * 4) as usize];
  for y in 0..side as i32 {
    for x in 0..side as i32 {
      if !inside(x, y) {
        continue;
      }
      let edge = !(inside(x - 1, y) && inside(x + 1, y) && inside(x, y - 1) && inside(x, y + 1));
      let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
      let color = if edge {
        OUTLINE
      } else if (dx + r * 0.35).powi(2) + (dy + r * 0.35).powi(2) < (r * 0.3).powi(2) {
        LIGHT
      } else if dx + dy > r * 0.6 {
        SHADE
      } else {
        BODY
      };
      let i = ((y as u32 * side + x as u32) * 4) as usize;
      data[i..i + 4].copy_from_slice(&color);
    }
  }
  Image::from_rgba(data, side, side)
}

impl DemoPage for PaintPage {
  fn name(&self) -> &str {
    "Paint anywhere"
  }

  fn description(&self) -> &str {
    "An infinite map built in code grows wherever you paint: each new chunk draws and collides"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Chunk overlay", &mut self.overlay);
    s.int("Zoom", &mut self.zoom, 1..=3);
    s.float("Bounce", &mut self.bounce, 0.0..=0.9, 0.1);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::mouse("Left paints, right erases"),
      wasd("Pan (pad: sticks)"),
      Control::key(KeyCode::Space, "Drop the ball (pad: A)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    clear_to(ctx, SKY);
    let Some(tileset) = load::<Tileset>(ctx, TILESET) else {
      return;
    };
    let mut map = TileMap::infinite(UVec2::splat(CELL as u32));
    self.tileset = map.add_tileset(TilesetSource::Asset(tileset));
    map.add_tile_layer(LAYER);
    let map = ctx.assets().add(map);
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    let zoom = self.zoom as f32;
    let centre = self.view.centre_for(ctx, self.at, zoom);
    self.view.place(ctx, centre, zoom, true);
    self.placed = true;
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if self.ball.is_none() && ctx.maps().is_ready(root) {
      self.land(ctx, root);
    }
    // Pan, and frame the view on whole screen pixels
    let zoom = self.zoom as f32;
    self.at += pan_input(ctx.input) * PAN_SPEED / zoom * dt;
    let centre = self.view.centre_for(ctx, self.at, zoom);
    self.view.place(ctx, centre, zoom, !self.placed);
    self.placed = true;
    if self.ball.is_none() {
      return;
    }

    // The pointer paints and erases, outside the page's notes
    let (mx, my) = ctx.input.mouse_position();
    let world = ctx.camera().screen_to_world(Vec2::new(mx, my));
    let over_notes = self.covered.iter().any(|rect| rect.contains(mx, my));
    self.hover = if over_notes {
      None
    } else {
      ctx.maps().world_to_cell(root, LAYER, world)
    };
    let input = &*ctx.input;
    let (left, right) = (
      input.mouse_down(MouseButton::Left),
      input.mouse_down(MouseButton::Right),
    );
    let drop_here = input.key_pressed(KeyCode::Space);
    let drop_middle = input.gamepad_button_pressed(GamepadButton::South);
    match self.hover {
      Some(cell) if left || right => {
        let paint = left;
        let from = match self.last {
          Some((last, was)) if was == paint => last,
          _ => cell,
        };
        if self.last != Some((cell, paint)) {
          self.stroke(ctx, root, from, cell, paint);
        }
        self.last = Some((cell, paint));
      }
      _ => self.last = None,
    }

    // The ball: dropped at the pointer or the view's middle, back over the
    // view should it fall far below it, and as bouncy as the setting
    let view = ctx.camera().visible_bounds();
    let top = Vec2::new((view.left + view.right) / 2.0, view.top + 2.0 * CELL);
    if drop_here && self.hover.is_some() {
      self.drop_ball(ctx, world);
    } else if drop_here || drop_middle {
      self.drop_ball(ctx, top);
    }
    if let Some(ball) = self.ball {
      let fallen = ctx
        .world
        .get::<Transform>(ball)
        .is_some_and(|t| t.translation.y > view.bottom + view.height().max(600.0) * 2.0);
      if fallen {
        self.drop_ball(ctx, top);
      }
      if let Some(mut collider) = ctx.world.get_mut::<Collider>(ball)
        && collider.restitution != self.bounce
      {
        collider.restitution = self.bounce;
      }
    }

    self.read_chunks(ctx, root, dt);
    if self.overlay
      && let Some(grid) = self.grid
    {
      let mut debug = ctx.debug_draw();
      for &chunk in &self.chunks {
        overlay::chunk_outline(&mut debug, grid.chunk(chunk));
      }
      for &(chunk, left) in &self.fresh {
        let alpha = (left / FLASH * 255.0) as u8;
        overlay::outline(&mut debug, grid.chunk(chunk), FRESH.with_alpha(alpha), 3.0);
      }
      if !self.bounds.is_empty() {
        let bounds = grid.rect(self.bounds.min, self.bounds.max);
        overlay::outline(&mut debug, bounds, BOUNDS_LINE, 2.0);
      }
      if let Some(cell) = self.hover {
        let rect = grid.rect(cell, cell + IVec2::ONE);
        let color = if left || right { HOT } else { Color::WHITE };
        overlay::outline(&mut debug, rect, color, 2.0);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    if self.overlay
      && let Some(grid) = self.grid
    {
      overlay::chunk_labels(ui, ctx, grid, &self.chunks, content_area(ui));
    }

    let pointer = self.hover.map_or_else(
      || "Pointer: off the world".to_string(),
      |cell| {
        let chunk = TileGrid::chunk_of(cell);
        format!(
          "Pointer: cell {}, {} in chunk {}, {}",
          cell.x, cell.y, chunk.x, chunk.y
        )
      },
    );
    let bounds = if self.bounds.is_empty() {
      "Bounds: none yet".to_string()
    } else {
      let last = self.bounds.max - IVec2::ONE;
      format!(
        "Bounds: cells ({}, {}) to ({}, {})",
        self.bounds.min.x, self.bounds.min.y, last.x, last.y
      )
    };
    let ball = self
      .ball
      .and_then(|ball| ctx.world.get::<Transform>(ball))
      .map(|t| t.translation);
    let resting = self
      .ball
      .and_then(|ball| ctx.world.get::<Velocity>(ball))
      .is_some_and(|v| v.as_vec2().length() < 1.0);
    let ball_line = match ball.and_then(|at| ctx.maps().world_to_cell(root, LAYER, at)) {
      Some(cell) if resting => format!("Ball: resting in cell {}, {}", cell.x, cell.y),
      Some(cell) => format!("Ball: moving, in cell {}, {}", cell.x, cell.y),
      None => "Ball: waiting for the map".to_string(),
    };
    let lines = vec![
      (
        "An infinite map built in code: TileMap::infinite".to_string(),
        Color::WHITE,
      ),
      (pointer, GOOD),
      (
        format!(
          "Chunks: {} ({} made here; a chunk stays once made)",
          self.chunks.len(),
          self.made
        ),
        Color::WHITE,
      ),
      (bounds, HOT),
      (
        format!("Painted {}, erased {}", self.painted, self.erased),
        Color::WHITE,
      ),
      (ball_line, DIM),
    ];
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
    let Some(grid) = self.grid else {
      return;
    };
    let view = ctx.camera().visible_bounds();
    let (a, b) = (
      grid.to_cells(Vec2::new(view.left, view.top)),
      grid.to_cells(Vec2::new(view.right, view.bottom)),
    );
    let below = notes.bottom() + 8.0;
    let area = Rect::new(column.x, below, column.width, column.bottom() - below);
    let drawn = overlay::minimap(
      ui,
      area,
      &Minimap {
        caption: "Chunks there are; gold: the bounds; white: the view",
        chunks: &self.chunks,
        fresh: &self
          .fresh
          .iter()
          .map(|&(chunk, left)| (chunk, left / FLASH))
          .collect::<Vec<_>>(),
        bounds: Some(self.bounds),
        view: Rect::new(a.x, a.y, b.x - a.x, b.y - a.y),
        dot: ball.map(|at| grid.to_cells(at)),
      },
    );
    self.covered.push(drawn);
  }
}
