//! The page "A Tiled infinite map": `maps/endless-meadow.tmx`, an infinite
//! map Tiled saved in its chunks of 16 x 16 cells, loaded by asset path and
//! spawned with a `MapRoot` like any map. Its cells are signed: the meadow
//! runs from cell -88 to cell 51 with its grass at row 0 (the scenery and
//! the sky above it at negative rows), and a small island sits far up and
//! left at cell (-204, -40). The engine keeps each layer in chunks of
//! 32 x 32 cells where it has tiles, so the empty sky between the island and
//! the meadow costs nothing; `ctx.maps().bounds()` is the rectangle of the
//! cells holding tiles, as the map is now (the asset's `TileMap::bounds()`
//! until a save or an edit changes it).
//!
//! A runner patrols the meadow by itself, between the ends of the
//! designer's `patrol` rectangle, starting at `player_start`: it jumps
//! walls and dips, hops now and then, and collects the coins (tile objects
//! taking the coin tile's class, back after a few seconds). WASD or a pad
//! takes over. The overlay outlines the chunks there are and the map's
//! bounds, labelled with their coordinates, and a minimap shows them all
//! with the view. A save of the map in Tiled (hot reload) redraws the
//! overlay and the bounds from the map as saved

use demo_kit::prelude::*;

use crate::overlay::{self, BOUNDS_LINE, Grid, Minimap};
use crate::shared::{
  CELL, DIM, GOOD, HOT, View, beside_panel, clear_to, hero, hud, load, pan_input, wasd,
};

/// The map
const MEADOW: &str = "maps/endless-meadow.tmx";
/// The meadow's background, should its file give none
const SKY: Color = Color::rgb(159, 220, 244);
/// The layer whose cells the page names
const GROUND: &str = "ground";
/// The runner's sprite on Tiny Dungeon's sheet (an adventurer), drawn over
/// the map's three tile layers (z 0 to 2)
const RUNNER_SPRITE: u32 = 88;
const RUNNER_Z: i32 = 10;
/// The runner's box: it fits under the planks and through the trees
const RUNNER_SIZE: Vec2 = Vec2::new(10.0, 14.0);
/// How fast it runs (pixels a second)
const RUN_SPEED: f32 = 130.0;
/// Seconds between the runner's hops on open ground
const HOP_EVERY: f32 = 1.6;
/// Seconds before a collected coin comes back
const COIN_BACK: f32 = 6.0;
/// The settings rows, for the notes under the panel
const ROWS: usize = 3;

/// The meadow's runner
#[derive(Component, Debug)]
pub struct Runner;

pub(crate) struct InfinitePage {
  view: View,
  root: Option<Entity>,
  /// The landed map's bounds: the cells its tile layers hold tiles in
  bounds: CellRect,
  /// Where the runner starts (its feet), and the way it faces
  start: Vec2,
  facing: f32,
  /// The x range it patrols
  patrol: (f32, f32),
  runner: Option<Entity>,
  sprite: Option<Entity>,
  /// The landed map's grid and chunks, and its lowest edge (world y)
  grid: Option<Grid>,
  chunks: Vec<IVec2>,
  floor: f32,
  /// The camera's centre, easing after the runner
  eye: Option<Vec2>,
  /// Seconds until the next hop
  hop: f32,
  coins: u32,
  overlay: bool,
  zoom: i32,
  auto: bool,
}

impl InfinitePage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      bounds: CellRect::default(),
      start: Vec2::ZERO,
      facing: -1.0,
      patrol: (-1000.0, 1000.0),
      runner: None,
      sprite: None,
      grid: None,
      chunks: Vec::new(),
      floor: f32::INFINITY,
      eye: None,
      hop: HOP_EVERY,
      coins: 0,
      overlay: true,
      zoom: 2,
      auto: true,
    }
  }

  /// The designer's marks in the asset: the start, the way the runner
  /// faces there, and the patrol's ends
  fn read_marks(&mut self, map: &TileMap) {
    for layer in &map.layers {
      let LayerKind::Objects(objects) = &layer.kind else {
        continue;
      };
      for object in objects {
        let at = layer.info.offset + object.position;
        match object.class.as_deref() {
          Some("player_start") => {
            self.start = at;
            if let Some(PropertyValue::Enum { value, .. }) = object.properties.get("facing") {
              self.facing = if value == "left" { -1.0 } else { 1.0 };
            }
          }
          Some("patrol") => self.patrol = (at.x, at.x + object.size.x),
          _ => {}
        }
      }
    }
  }

  /// The landed map's chunks, bounds and lowest edge, as it is now: at
  /// landing, and again after each save of its file
  fn read_map(&mut self, ctx: &mut SceneContext, root: Entity) {
    overlay::map_chunks(ctx.world, root, &mut self.chunks);
    self.bounds = ctx.maps().bounds(root).unwrap_or_default();
    // The map's root sits at the origin, so its cells' rows are world rows
    if !self.bounds.is_empty() {
      self.floor = self.bounds.max.y as f32 * CELL;
    }
  }

  /// Once the map has landed: its grid and chunks, the coins' triggers,
  /// and the runner
  fn land(&mut self, ctx: &mut SceneContext, root: Entity) {
    self.grid = Grid::of(ctx, root, GROUND, CELL);
    self.read_map(ctx, root);

    let mut objects = ctx.world.query::<(Entity, &MapObject)>();
    let coins: Vec<Entity> = objects
      .iter(ctx.world)
      .filter(|(_, object)| object.map == root && object.class.as_deref() == Some("coin"))
      .map(|(entity, _)| entity)
      .collect();
    for coin in coins {
      ctx.world.entity_mut(coin).insert((
        Collider::circle(6.0).as_trigger(),
        Pickup::new().respawn_after(COIN_BACK),
      ));
    }

    let feet = self.start;
    let runner = ctx
      .world
      .spawn((
        Transform::from_translation(feet - Vec2::new(0.0, RUNNER_SIZE.y / 2.0 + 0.5)),
        Interpolated,
        Collider::rect(RUNNER_SIZE.x, RUNNER_SIZE.y),
        // Steps up the rope bridge's ends, 6 px below the grass
        CharacterController::platformer().with_step_height(8.0),
        PlatformerMovement::default()
          .with_run(RUN_SPEED, 0.08, 0.06)
          .with_jump(58.0, 0.36),
        Runner,
      ))
      .id();
    if !self.auto {
      ctx.world.entity_mut(runner).insert(bindings());
    }
    if let Some(image) = hero(ctx, RUNNER_SPRITE) {
      // 16 px tall over a 14 px box: the sprite's feet on the box's
      let sprite = ctx.spawn_child(
        runner,
        (
          Transform::from_xy(0.0, -1.0),
          Renderable::image(image).with_z_order(RUNNER_Z),
        ),
      );
      self.sprite = Some(sprite);
    }
    self.runner = Some(runner);
  }

  /// The auto-run, or the player's bindings once WASD or a pad takes over
  fn steer(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(runner) = self.runner else {
      return;
    };
    let input = &*ctx.input;
    let takes_over = pan_input(input) != Vec2::ZERO
      || input.key_pressed(KeyCode::Space)
      || input.gamepad_button_pressed(GamepadButton::South);
    if self.auto && takes_over {
      self.auto = false;
    }
    let bound = ctx.world.get::<MoveBindings>(runner).is_some();
    if self.auto && bound {
      ctx.world.entity_mut(runner).remove::<MoveBindings>();
    } else if !self.auto && !bound {
      ctx.world.entity_mut(runner).insert(bindings());
    }

    let Some(at) = ctx.world.get::<Transform>(runner).map(|t| t.translation) else {
      return;
    };
    if self.auto {
      let state = ctx
        .world
        .get::<CharacterState>(runner)
        .cloned()
        .unwrap_or_default();
      if at.x <= self.patrol.0 {
        self.facing = 1.0;
      } else if at.x >= self.patrol.1 {
        self.facing = -1.0;
      }
      let mut jump = false;
      if state.on_floor {
        // A wall ahead: its normal points back at the runner
        let wall = state.on_wall && state.wall_normal.x * self.facing < 0.0;
        // A dip ahead: no ground below a point just past its front
        let ahead = at + Vec2::new(self.facing * (RUNNER_SIZE.x / 2.0 + 4.0), 0.0);
        let filter = QueryFilter::new().excluding(runner);
        let dip = ctx
          .physics()
          .raycast(ahead, Vec2::Y, RUNNER_SIZE.y, &filter)
          .is_none();
        self.hop -= dt;
        if wall || dip || self.hop <= 0.0 {
          jump = true;
          self.hop = HOP_EVERY;
        }
      }
      if let Some(mut intent) = ctx.world.get_mut::<MoveIntent>(runner) {
        intent.direction = Vec2::new(self.facing, 0.0);
        intent.jump_held = true;
        if jump {
          intent.jump_pressed = true;
        }
      }
    } else if let Some(velocity) = ctx.world.get::<Velocity>(runner)
      && velocity.dx.abs() > 1.0
    {
      self.facing = velocity.dx.signum();
    }
    if let Some(sprite) = self.sprite
      && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(sprite)
    {
      renderable.flip_x = self.facing < 0.0;
    }
  }

  /// The camera on the runner (on the start until it spawns), the meadow's
  /// lowest edge kept at or below the screen's bottom: there at once the
  /// first time, easing after
  fn follow(&mut self, ctx: &mut SceneContext) {
    let focus = self
      .runner
      .and_then(|runner| ctx.world.get::<Transform>(runner))
      .map_or(self.start, |transform| transform.translation);
    let zoom = self.zoom as f32;
    let mut target = self.view.centre_for(ctx, focus, zoom);
    target.y = target.y.min(self.floor - View::half_screen(ctx, zoom).y);
    let (eye, jump) = match self.eye {
      Some(eye) => (eye + (target - eye) * 0.12, false),
      None => (target, true),
    };
    self.eye = Some(eye);
    self.view.place(ctx, eye, zoom, jump);
  }
}

/// WASD moves (the arrows belong to the settings panel), Space jumps, and
/// the first pad's left stick or D-pad moves and South jumps
fn bindings() -> MoveBindings {
  MoveBindings::default()
    .with_left([KeyCode::KeyA])
    .with_right([KeyCode::KeyD])
    .with_up([KeyCode::KeyW])
    .with_down([KeyCode::KeyS])
}

impl DemoPage for InfinitePage {
  fn name(&self) -> &str {
    "A Tiled infinite map"
  }

  fn description(&self) -> &str {
    "Tiled's chunks at negative cells: the engine keeps 32 x 32-cell chunks only where tiles are"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Chunk overlay", &mut self.overlay);
    s.int("Zoom", &mut self.zoom, 1..=3);
    s.toggle("Auto-run", &mut self.auto);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Take over and run (pad: stick)"),
      Control::key(KeyCode::Space, "Jump (pad: A)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let Some(map) = load::<TileMap>(ctx, MEADOW) else {
      return;
    };
    // The asset, borrowed: the designer's marks, its lowest edge (the
    // bottom of its bounds; the root sits at the origin, so its cells' rows
    // are world rows) and its background
    let background = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.read_marks(data);
      let bounds = data.bounds();
      if !bounds.is_empty() {
        self.floor = (bounds.max.y * data.tile_size.y as i32) as f32;
      }
      data.background
    };
    clear_to(ctx, background.unwrap_or(SKY));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    self.follow(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if self.runner.is_none() && ctx.maps().is_ready(root) {
      self.land(ctx, root);
    }
    // A save of the map's file: its chunks and bounds as saved
    let saved = ctx
      .messages::<MapEvent>()
      .iter()
      .any(|event| matches!(event, MapEvent::Reloaded { map } if *map == root));
    if saved && self.runner.is_some() {
      self.read_map(ctx, root);
    }
    self.steer(ctx, dt);
    self.coins += ctx.messages::<PickedUp>().len() as u32;
    self.follow(ctx);

    if self.overlay
      && let Some(grid) = self.grid
    {
      let mut debug = ctx.debug_draw();
      for &chunk in &self.chunks {
        overlay::chunk_outline(&mut debug, grid.chunk(chunk));
      }
      if !self.bounds.is_empty() {
        let bounds = grid.rect(self.bounds.min, self.bounds.max);
        overlay::outline(&mut debug, bounds, BOUNDS_LINE, 2.0);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    let Some(root) = self.root else {
      return;
    };
    if self.overlay
      && let Some(grid) = self.grid
    {
      overlay::chunk_labels(ui, ctx, grid, &self.chunks, content_area(ui));
    }

    // The chunks the bounds reach: the bounds hold the cells with tiles
    let last = self.bounds.max - IVec2::ONE;
    let reach = TileGrid::chunk_of(last) - TileGrid::chunk_of(self.bounds.min) + IVec2::ONE;
    let runner = self
      .runner
      .and_then(|runner| ctx.world.get::<Transform>(runner))
      .map(|transform| transform.translation);
    let cell = runner.and_then(|at| ctx.maps().world_to_cell(root, GROUND, at));
    let runner_line = cell.map_or_else(
      || "Runner: waiting for the map".to_string(),
      |cell| {
        let chunk = TileGrid::chunk_of(cell);
        format!(
          "Runner: cell {}, {} in chunk {}, {}",
          cell.x, cell.y, chunk.x, chunk.y
        )
      },
    );
    let hint = if self.auto {
      "It runs by itself: WASD or a pad takes over"
    } else {
      "WASD runs, Space jumps; Auto-run hands it back"
    };
    let lines = vec![
      (
        "A Tiled infinite map, saved in chunks of 16 x 16 cells".to_string(),
        Color::WHITE,
      ),
      (
        if self.bounds.is_empty() {
          "Bounds: waiting for the map".to_string()
        } else {
          format!(
            "Bounds: cells ({}, {}) to ({}, {})",
            self.bounds.min.x, self.bounds.min.y, last.x, last.y
          )
        },
        HOT,
      ),
      (
        format!(
          "Chunks: {} of the {} the bounds reach, 32 x 32 cells each",
          self.chunks.len(),
          reach.x * reach.y
        ),
        Color::WHITE,
      ),
      (runner_line, GOOD),
      (format!("Coins: {}", self.coins), HOT),
      (hint.to_string(), DIM),
    ];
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
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
    overlay::minimap(
      ui,
      area,
      &Minimap {
        caption: "Chunks there are; gold: the bounds; white: the view",
        chunks: &self.chunks,
        fresh: &[],
        bounds: Some(self.bounds),
        view: Rect::new(a.x, a.y, b.x - a.x, b.y - a.y),
        dot: runner.map(|at| grid.to_cells(at)),
      },
    );
  }
}
