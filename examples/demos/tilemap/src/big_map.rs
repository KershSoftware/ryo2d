//! The Big map page: a 1024 x 1024 map of four layers (4,194,304 cells,
//! 16 px each: a sand floor, rock walls through the Wang set `dungeon`,
//! props and animated drains) built in code, added to the assets and
//! spawned with `MapRoot`. The camera flies over it. Drawing costs what is
//! on screen, not the map's size: each frame draws only the 32 x 32-cell
//! chunks in view, built once and kept. The page shows what a frame costs
//! the CPU from the engine's `FrameCost` (smoothed; the vsync wait left
//! out), once with the map on and once with it off, so the difference is
//! the map's: its total, and the render, present and update it splits into
//! (the map's draws are encoded as the render passes run, in present). F1
//! swaps the engine's FPS counter (on in this demo) for its frame timing
//! overlay, which draws the same split.
//!
//! The layers neither collide nor cast shadows: nothing on the page touches
//! them, and a colliding map this size takes about 70 ms on the map builder
//! thread to work out its colliders, about 5 ms in the frame it lands, and
//! thousands of collider parts and shadow chains that every frame's
//! transform propagation walks, for nothing the page shows

use std::f32::consts::TAU;

use demo_kit::prelude::*;

use crate::shared::{DIM, DUNGEON_TERRAIN, HOT, View, asset, hud_beside_panel, load};

/// Tiny Dungeon's sheet as a tileset: sand, props and the animated drain
const DUNGEON: &str = "tilesets/dungeon.tsj";

/// Cells a side, and a cell's side in pixels
const SIZE: u32 = 1024;
const CELL: u32 = 16;
/// The map's side in world pixels
const SIDE: f32 = (SIZE * CELL) as f32;
/// A chunk's side in world pixels
const CHUNK_SIDE: f32 = (CHUNK_SIZE * CELL) as f32;
/// The settings rows, for the notes under the panel
const ROWS: usize = 4;
/// Frames after the map is turned on or off before its cost is read: the
/// engine's average spans about 50, so this leaves a few percent of the
/// other state's
const SETTLE: u32 = 150;

/// Tiny Dungeon (tileset 0): sand, sand with specks, stones on sand, the
/// drain (animated), and props
const SAND: u32 = 48;
const SPECKS: u32 = 49;
const STONES: u32 = 42;
const DRAIN: u32 = 43;
const PROPS: [u32; 10] = [63, 64, 65, 66, 72, 73, 74, 75, 82, 89];

pub(crate) struct BigMapPage {
  view: View,
  map: Option<Handle<TileMap>>,
  root: Option<Entity>,
  shown: bool,
  auto: bool,
  zoom: f32,
  speed: f32,
  /// Along the fly-over's path, in radians
  path: f32,
  /// The camera's centre
  at: Vec2,
  /// What a frame cost, settled, with the map off and on
  cost: [Option<FrameCost>; 2],
  /// Frames since the map was last turned on or off (or the page opened)
  settling: u32,
  /// The map's state the last frame
  was_shown: bool,
  cells: u64,
}

impl BigMapPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      map: None,
      root: None,
      shown: true,
      auto: true,
      zoom: 1.0,
      speed: 600.0,
      path: 0.0,
      at: Vec2::splat(SIDE / 2.0),
      cost: [None; 2],
      settling: 0,
      was_shown: true,
      cells: 0,
    }
  }

  /// The fly-over's point at `path`: a long loop over most of the map
  fn flown(path: f32) -> Vec2 {
    let reach = SIDE * 0.42;
    Vec2::new(
      SIDE / 2.0 + reach * path.sin(),
      SIDE / 2.0 + reach * (path * 1.37 + 1.0).sin(),
    )
  }
}

/// A hash of a cell, for the map's scatter and noise
fn hash(x: i32, y: i32, salt: u32) -> u32 {
  let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
    ^ (y as u32).wrapping_mul(0xD816_3841)
    ^ salt.wrapping_mul(0xCB1A_B31F);
  h ^= h >> 13;
  h = h.wrapping_mul(0x5BD1_E995);
  h ^ (h >> 15)
}

/// Smooth value noise in 0..1, on a lattice one unit apart
fn noise(x: f32, y: f32, salt: u32) -> f32 {
  let (x0, y0) = (x.floor(), y.floor());
  let (fx, fy) = (x - x0, y - y0);
  let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
  let (ix, iy) = (x0 as i32, y0 as i32);
  let at = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, salt) & 0xFFFF) as f32 / 65535.0;
  let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
  let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
  top + (bottom - top) * sy
}

/// The big map: four layers over Tiny Dungeon's sheet (tileset 0) and its
/// Wang terrain (tileset 1), none colliding or casting shadows
fn build(dungeon: Handle<Tileset>, terrain: Handle<Tileset>, set: &WangSet) -> TileMap {
  let mut map = TileMap::new(UVec2::splat(SIZE), UVec2::splat(CELL));
  map.add_tileset(TilesetSource::Asset(dungeon));
  map.add_tileset(TilesetSource::Asset(terrain));
  let layers = ["floor", "walls", "props", "details"].map(|name| map.add_tile_layer(name));
  for &layer in &layers {
    let info = &mut map.layers[layer].info;
    info.collides = false;
    info.occludes = Some(false);
  }

  // Rock where the noise at a cell corner is high: a corner per vertex of
  // the grid, so neighbouring cells agree on the corners they share
  let vertices = SIZE as usize + 1;
  let mut rock = vec![false; vertices * vertices];
  for vy in 0..vertices {
    for vx in 0..vertices {
      let (x, y) = (vx as f32, vy as f32);
      let value = noise(x / 22.0, y / 22.0, 1) * 0.7 + noise(x / 7.0, y / 7.0, 2) * 0.3;
      rock[vy * vertices + vx] = value > 0.6;
    }
  }
  // The set's tile for each mix of corners (a bit per corner, set for
  // rock: top-left, top-right, bottom-right, bottom-left), a few variants
  // each, picked once rather than per cell
  let mut table: Vec<Vec<(u32, TileFlip)>> = vec![Vec::new(); 16];
  let colors = set.color_index("wall").zip(set.color_index("floor"));
  match colors {
    Some((wall, floor)) => {
      for (mix, variants) in table.iter_mut().enumerate().skip(1) {
        let color = |bit: usize| if mix & (1 << bit) != 0 { wall } else { floor };
        let any = WangSet::ANY;
        let wanted = [any, color(1), any, color(2), any, color(3), any, color(0)];
        for k in 0..4 {
          if let Some(pick) = set.best_tile(wanted, IVec2::new(k * 7, k * 13))
            && !variants.contains(&pick)
          {
            variants.push(pick);
          }
        }
      }
    }
    None => game_warn!(
      "the Wang set \"{}\" has no wall or no floor colour: the big map has no walls",
      set.name()
    ),
  }

  if let Some(floor) = map.tiles_mut(layers[0]) {
    floor.fill(
      IVec2::ZERO,
      IVec2::splat(SIZE as i32 - 1),
      Some(Tile::new(0, SAND)),
    );
  }
  for y in 0..SIZE as i32 {
    for x in 0..SIZE as i32 {
      let (vx, vy) = (x as usize, y as usize);
      let corner = |dx: usize, dy: usize| rock[(vy + dy) * vertices + vx + dx];
      let mix = usize::from(corner(0, 0))
        | usize::from(corner(1, 0)) << 1
        | usize::from(corner(1, 1)) << 2
        | usize::from(corner(0, 1)) << 3;
      let cell = IVec2::new(x, y);
      let h = hash(x, y, 3);
      if mix != 0 {
        let variants = &table[mix];
        if !variants.is_empty() {
          let (id, flip) = variants[h as usize % variants.len()];
          if let Some(walls) = map.tiles_mut(layers[1]) {
            walls.set(cell, Some(Tile::new(1, id).with_flip(flip)));
          }
        }
        continue;
      }
      let floor_tile = match h % 64 {
        0..=6 => Some(SPECKS),
        7 => Some(STONES),
        _ => None,
      };
      if let (Some(id), Some(floor)) = (floor_tile, map.tiles_mut(layers[0])) {
        floor.set(cell, Some(Tile::new(0, id)));
      }
      let scatter = hash(x, y, 4);
      if scatter.is_multiple_of(73)
        && let Some(props) = map.tiles_mut(layers[2])
      {
        let id = PROPS[(scatter / 73) as usize % PROPS.len()];
        props.set(cell, Some(Tile::new(0, id)));
      } else if scatter % 157 == 1
        && let Some(details) = map.tiles_mut(layers[3])
      {
        details.set(cell, Some(Tile::new(0, DRAIN)));
      }
    }
  }
  map
}

impl DemoPage for BigMapPage {
  fn name(&self) -> &str {
    "Big map"
  }

  fn description(&self) -> &str {
    "1024 x 1024 cells in four layers, built in code: each frame draws only the chunks in view"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Map", &mut self.shown);
    s.toggle("Fly over", &mut self.auto);
    s.float("Zoom", &mut self.zoom, 0.25..=3.0, 0.25);
    s.float("Speed", &mut self.speed, 100.0..=3000.0, 100.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      crate::shared::wasd("Fly the camera yourself (pad: left stick)"),
      Control::key(KeyCode::Space, "Fly over on or off (pad: A)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (Some(dungeon), Some(terrain)) = (
      load::<Tileset>(ctx, DUNGEON),
      load::<Tileset>(ctx, DUNGEON_TERRAIN),
    ) else {
      return;
    };
    let Some(set) = asset(ctx, &terrain).and_then(|t| t.wang_set("dungeon").cloned()) else {
      return;
    };
    let map = build(dungeon, terrain, &set);
    self.cells = map.layers.len() as u64 * u64::from(SIZE).pow(2);
    let map = ctx.assets().add(map);
    self.map = Some(map.clone());
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map).without_collision()))
        .id(),
    );
    self.at = Self::flown(self.path);
    let bounds = Rect::new(0.0, 0.0, SIDE, SIDE);
    self.view.follow(ctx, self.at, self.zoom, bounds, 1.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    if crate::shared::main_action(ctx.input) {
      self.auto = !self.auto;
    }
    // WASD or the stick takes the camera
    let input = &*ctx.input;
    let key = |k: KeyCode| f32::from(u8::from(input.key_down(k)));
    let (sx, sy) = input.left_stick();
    let manual = Vec2::new(
      key(KeyCode::KeyD) - key(KeyCode::KeyA) + sx,
      key(KeyCode::KeyS) - key(KeyCode::KeyW) + sy,
    );
    if manual.length_squared() > 0.01 {
      self.auto = false;
      self.at += manual.clamp_length_max(1.0) * self.speed * dt;
    } else if self.auto {
      // The path's speed along it is about `speed` px/s
      self.path = (self.path + dt * self.speed / (SIDE * 0.5)).rem_euclid(TAU * 100.0);
      self.at = Self::flown(self.path);
    }
    self.at = self.at.clamp(Vec2::ZERO, Vec2::splat(SIDE));
    let (at, zoom) = (self.at, self.zoom);
    self
      .view
      .follow(ctx, at, zoom, Rect::new(0.0, 0.0, SIDE, SIDE), 1.0);

    if let Some(root) = self.root {
      let disabled = ctx.world.get::<Disabled>(root).is_some();
      if self.shown && disabled {
        ctx.world.entity_mut(root).remove::<Disabled>();
      } else if !self.shown && !disabled {
        ctx.world.entity_mut(root).insert(Disabled);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    // The engine's smoothed cost, read into the slot of the map's state
    // once the average has settled after a change
    if self.shown != self.was_shown {
      self.was_shown = self.shown;
      self.settling = 0;
    }
    self.settling = self.settling.saturating_add(1);
    let live = ctx.resource::<FrameCost>().copied();
    if self.settling >= SETTLE && live.is_some() {
      self.cost[usize::from(self.shown)] = live;
    }
    let settled = self.settling >= SETTLE;
    let view = ctx.camera().visible_bounds();
    let chunk = |v: f32| {
      (v / CHUNK_SIDE)
        .floor()
        .clamp(0.0, (SIZE / CHUNK_SIZE - 1) as f32)
    };
    let columns = chunk(view.right) - chunk(view.left) + 1.0;
    let rows = chunk(view.bottom) - chunk(view.top) + 1.0;
    let per_layer = (SIZE / CHUNK_SIZE).pow(2);
    // The map's state now: what the engine reads now, marked until its
    // average settles; the other state: what it last settled at
    // The map's draws are encoded as the render passes run, so most of its
    // cost is in present, and the total is the number to compare
    let state = |label: &str, slot: usize| {
      let now = usize::from(self.shown) == slot;
      let shown = if now && !settled {
        live
      } else {
        self.cost[slot]
      };
      match shown {
        Some(cost) => vec![
          (
            format!(
              "{label}: total {:.3} ms{}",
              cost.total_ms(),
              if now && !settled { " (settling)" } else { "" }
            ),
            if now { Color::WHITE } else { DIM },
          ),
          (
            format!(
              "  render {:.3}, present {:.3}, update {:.3}",
              cost.render_ms(),
              cost.present_ms(),
              cost.update_ms()
            ),
            DIM,
          ),
        ],
        None => vec![(format!("{label}: not measured yet"), DIM)],
      }
    };
    let mut lines = vec![("CPU a frame (FrameCost, smoothed)".to_string(), HOT)];
    lines.extend(state("Map on", 1));
    lines.extend(state("Map off", 0));
    lines.extend([
      (
        format!(
          "Chunks in view: {:.0} of {per_layer} a layer, 4 layers",
          columns * rows
        ),
        Color::WHITE,
      ),
      (format!("{} cells in all", group(self.cells)), DIM),
      ("F1: the engine's frame timing overlay".to_string(), DIM),
      (format!("Camera at {:.0}, {:.0}", self.at.x, self.at.y), DIM),
    ]);
    hud_beside_panel(ui, ROWS, &lines);
  }
}

/// `n` with commas between its thousands
fn group(n: u64) -> String {
  let digits = n.to_string();
  let mut out = String::new();
  for (i, c) in digits.chars().enumerate() {
    if i > 0 && (digits.len() - i).is_multiple_of(3) {
      out.push(',');
    }
    out.push(c);
  }
  out
}
