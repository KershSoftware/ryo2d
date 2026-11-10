//! The page "A hand-made meadow in an endless world": `maps/endless-meadow.tmx`,
//! a map Tiled saved with no fixed size, as the chunks of 16 x 16 cells the
//! designer painted, at any cells, negative ones too, spawned with a
//! `MapRoot` and a `MapStream`. Its cells are signed: the meadow runs from
//! cell -96 to cell 63 with its grass at row 0 (the scenery and the sky
//! above it at negative rows), and a small island sits far up and left at
//! cell (-204, -40).
//!
//! The file's chunks are the hand-made part of an endless world. The
//! engine keeps each layer in chunks of 32 x 32 cells, and every one a
//! Tiled chunk touches is the file's: resident from the start, never
//! unloaded, never generated. The stream's generator fills every other
//! chunk round the view as it comes near (`terrain::MeadowWorld`): the
//! meadow's ground going on past both its ends with the same tiles, picked
//! by the same rule, at the grass row of each end and down to the meadow's
//! bottom row, its hills rising from the seam on noise. The meadow ends on
//! the edges of its chunks, its end columns laid as ground going on, so the
//! seam reads as one world.
//!
//! A runner runs by itself from `player_start`, and keeps going: across
//! the meadow, jumping steps and dips, hopping now and then and collecting
//! the coins (tile objects taking the coin tile's class, back after a few
//! seconds), then on over generated ground, which streams in ahead of it.
//! WASD or a pad takes over. The overlay outlines the chunks resident, the
//! file's in white and those the stream landed (its `ChunkEvent`s) in
//! green, labels them in world text, and outlines the file's bounds; the
//! HUD shows the stream's own counts (`ctx.maps().stream_status`): the
//! hand-made chunks, the generated ones resident, every chunk resident and
//! those in flight; a minimap follows the view. A save of the map in Tiled (hot reload)
//! redraws the overlay, the hand-made chunks and the bounds from the map as
//! saved

use demo_kit::prelude::*;

use crate::overlay::{
  self, BOUNDS_LINE, CHUNK_FILL, CHUNK_LINE, ChunkLabels, FRESH, GENERATED, Grid, Minimap,
  Streamed, Tag,
};
use crate::shared::{
  CELL, DIM, GOOD, HOT, View, beside_panel, bindings, clear_to, hero, hud, load, pan_input, wasd,
};
use crate::terrain::{GROUND, MeadowWorld};

/// The map
const MEADOW: &str = "maps/endless-meadow.tmx";
/// The meadow's background, should its file give none
const SKY: Color = Color::rgb(159, 220, 244);
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
/// The chunks a side the minimap shows round the view
const WINDOW: IVec2 = IVec2::new(9, 5);

/// The meadow's runner
#[derive(Component, Debug)]
pub struct Runner;

pub(crate) struct InfinitePage {
  view: View,
  root: Option<Entity>,
  map: Option<Handle<TileMap>>,
  /// The file's bounds: the cells its tile layers hold tiles in
  bounds: CellRect,
  /// Where the runner starts (its feet), and the way it faces
  start: Vec2,
  facing: f32,
  runner: Option<Entity>,
  sprite: Option<Entity>,
  /// The landed map's grid, and its lowest edge (world y): the meadow's
  /// bottom, which the generated ground keeps to
  grid: Option<Grid>,
  floor: f32,
  /// What the stream has: its counts, the chunks resident, and which of
  /// them it generated
  streamed: Streamed,
  /// The camera's centre, easing after the runner
  eye: Option<Vec2>,
  /// Seconds until the next hop
  hop: f32,
  coins: u32,
  overlay: bool,
  /// The chunks' labels, world text placed after the camera moves
  labels: ChunkLabels,
  zoom: i32,
  auto: bool,
}

impl InfinitePage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      map: None,
      bounds: CellRect::default(),
      start: Vec2::ZERO,
      facing: -1.0,
      runner: None,
      sprite: None,
      grid: None,
      floor: f32::INFINITY,
      streamed: Streamed::default(),
      eye: None,
      hop: HOP_EVERY,
      coins: 0,
      overlay: true,
      labels: ChunkLabels::default(),
      zoom: 2,
      auto: true,
    }
  }

  /// The designer's marks in the asset: the start, and the way the runner
  /// faces there
  fn read_marks(&mut self, map: &TileMap) {
    for layer in &map.layers {
      let LayerKind::Objects(objects) = &layer.kind else {
        continue;
      };
      for object in objects {
        if object.class.as_deref() == Some("player_start") {
          self.start = layer.info.offset + object.position;
          if let Some(PropertyValue::Enum { value, .. }) = object.properties.get("facing") {
            self.facing = if value == "left" { -1.0 } else { 1.0 };
          }
        }
      }
    }
  }

  /// The file's bounds, as it is now: as the page opens, and again after
  /// each save of the file
  fn read_file(&mut self, map: &TileMap) {
    self.bounds = map.bounds();
  }

  /// Once the map has landed: its grid, the coins' triggers, and the runner
  fn land(&mut self, ctx: &mut SceneContext, root: Entity) {
    self.grid = Grid::of(ctx, root, GROUND, CELL);

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
        // The chunks round it stay loaded, wherever the camera looks
        StreamAnchor,
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
    let focus = self.runner_at(ctx).unwrap_or(self.start);
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

  fn runner_at(&self, ctx: &SceneContext) -> Option<Vec2> {
    self
      .runner
      .and_then(|runner| ctx.world.get::<Transform>(runner))
      .map(|transform| transform.translation)
  }
}

/// What a chunk's label says it is: one the stream landed, or one of the
/// file's
fn tag(streamed: &Streamed, chunk: IVec2) -> Tag {
  if streamed.is_generated(chunk) {
    Some(("generated", GENERATED))
  } else {
    Some(("hand-made", CHUNK_LINE))
  }
}

/// The generator's world from the meadow as its file has it: the ground
/// layer's bottom row (the bounds' last), the first and last columns with
/// ground there, and how high each of those columns' ground reaches
fn meadow_world(map: &TileMap) -> Option<MeadowWorld> {
  let grid = map.tiles(map.layer_index(GROUND)?)?;
  let bounds = map.bounds();
  let floor = bounds.max.y - 1;
  let ground = |x: i32, y: i32| grid.get(IVec2::new(x, y)).is_some();
  let first = (bounds.min.x..bounds.max.x).find(|&x| ground(x, floor))?;
  let last = (bounds.min.x..bounds.max.x)
    .rev()
    .find(|&x| ground(x, floor))?;
  let top = |x: i32| {
    let mut y = floor;
    while y > bounds.min.y && ground(x, y - 1) {
      y -= 1;
    }
    y
  };
  let tileset = grid.get(IVec2::new(first, floor))?.tileset;
  Some(MeadowWorld {
    tileset,
    left: IVec2::new(first, top(first)),
    right: IVec2::new(last, top(last)),
    floor,
  })
}

impl DemoPage for InfinitePage {
  fn name(&self) -> &str {
    "A hand-made meadow in an endless world"
  }

  fn description(&self) -> &str {
    "A Tiled infinite map's chunks are the meadow; a generator streams the ground on past both ends"
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
    // The asset, borrowed: the designer's marks, the file's bounds and
    // chunks, its lowest edge (the bottom of its bounds; the root sits at
    // the origin, so its cells' rows are world rows), its background, and
    // the meadow's ends, where the generated world meets it
    let (background, world) = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.read_marks(data);
      self.read_file(data);
      if !self.bounds.is_empty() {
        self.floor = (self.bounds.max.y * data.tile_size.y as i32) as f32;
      }
      (data.background, meadow_world(data))
    };
    clear_to(ctx, background.unwrap_or(SKY));
    let Some(world) = world else {
      ryo2d::warn!("the meadow has no ground to go on from");
      return;
    };
    let stream = MapStream::new(move |request: ChunkRequest| world.chunk(request));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map.clone()), stream))
        .id(),
    );
    self.map = Some(map);
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
    if saved && let Some(map) = self.map.clone() {
      let assets = ctx.assets();
      if let Some(data) = assets.get(&map) {
        self.read_file(data);
      }
    }
    let events = ctx.messages::<ChunkEvent>();
    self.steer(ctx, dt);
    self.coins += ctx.messages::<PickedUp>().len() as u32;
    self.follow(ctx);
    let Some(grid) = self.grid else {
      return;
    };
    self.streamed.update(ctx, root, &events, dt);
    let streamed = &self.streamed;
    self
      .labels
      .place(ctx, grid, &streamed.chunks, self.overlay, &|chunk| {
        tag(streamed, chunk)
      });

    if self.overlay {
      let mut debug = ctx.debug_draw();
      for &chunk in &self.streamed.chunks {
        let color = if self.streamed.is_generated(chunk) {
          GENERATED
        } else {
          CHUNK_LINE
        };
        overlay::chunk_outline_in(&mut debug, grid.chunk(chunk), color);
      }
      for (chunk, fresh) in self.streamed.freshness() {
        let alpha = (fresh * 255.0) as u8;
        overlay::outline(&mut debug, grid.chunk(chunk), FRESH.with_alpha(alpha), 3.0);
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
    self.labels.measure(ui);
    self.streamed.read_status(ctx, root);

    let runner = self.runner_at(ctx);
    let cell = runner.and_then(|at| ctx.maps().world_to_cell(root, GROUND, at));
    let runner_line = cell.map_or_else(
      || "Runner: waiting for the map".to_string(),
      |cell| {
        let chunk = TileGrid::chunk_of(cell);
        let kind = if self.streamed.is_generated(chunk) {
          "generated"
        } else {
          "hand-made"
        };
        format!(
          "Runner: cell {}, {} in chunk {}, {}, {kind}",
          cell.x, cell.y, chunk.x, chunk.y
        )
      },
    );
    let hint = if self.auto {
      "It runs on by itself: WASD or a pad takes over"
    } else {
      "WASD runs, Space jumps; Auto-run hands it back"
    };
    let last = self.bounds.max - IVec2::ONE;
    let streamed = &self.streamed;
    let status = streamed.status.unwrap_or_default();
    let lines = vec![
      (
        "The file's chunks are the meadow; the rest streams".to_string(),
        Color::WHITE,
      ),
      (
        format!("Hand-made: {} chunks, the file's", status.hand_made),
        CHUNK_FILL,
      ),
      (
        format!(
          "Generated: {} resident, {} landed so far",
          status.generated, streamed.landed
        ),
        GENERATED,
      ),
      (streamed.resident_line(), Color::WHITE),
      (
        if self.bounds.is_empty() {
          "The file's bounds: none".to_string()
        } else {
          format!(
            "The file's bounds: cells ({}, {}) to ({}, {})",
            self.bounds.min.x, self.bounds.min.y, last.x, last.y
          )
        },
        HOT,
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
    let fresh = self.streamed.freshness();
    overlay::minimap(
      ui,
      area,
      &Minimap {
        caption: "Blue: hand-made; green: generated; gold: the file",
        chunks: &self.streamed.chunks,
        generated: &self.streamed.generated,
        fresh: &fresh,
        marked: &[],
        bounds: Some(self.bounds),
        view: Rect::new(a.x, a.y, b.x - a.x, b.y - a.y),
        dot: runner.map(|at| grid.to_cells(at)),
        window: Some(WINDOW),
      },
    );
  }
}
