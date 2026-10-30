//! The Big map page: a 1024 x 1024 map of four layers (4,194,304 cells,
//! 16 px each: a sand floor, rock walls through the Wang set `dungeon`,
//! props and animated drains) built in code, added to the assets and
//! spawned with `MapRoot`. The camera flies over it. Drawing costs what is
//! on screen, not the map's size: each frame draws only the 32 x 32-cell
//! chunks in view, built once and kept. The page shows the frame time the
//! engine's real clock measures (`Time::real`), which counts the wait for
//! the display too; F1 swaps the engine's FPS counter (on in this demo) for
//! its frame timing overlay, which splits the work into update, render and
//! present.
//!
//! The layers neither collide nor cast shadows: nothing on the page touches
//! them, and a colliding map this size costs about 75 ms of colliders in
//! the step after it lands (and memory for thousands of collider parts)
//! for nothing the page shows

use std::f32::consts::TAU;

use demo_kit::prelude::*;

use crate::shared::{DIM, DUNGEON, DUNGEON_TERRAIN, HOT, View, asset, hud_beside_panel, load};

/// Cells a side, and a cell's side in pixels
const SIZE: u32 = 1024;
const CELL: u32 = 16;
/// The map's side in world pixels
const SIDE: f32 = (SIZE * CELL) as f32;
/// A chunk's side in world pixels
const CHUNK_SIDE: f32 = (CHUNK_SIZE * CELL) as f32;
/// The settings rows, for the notes under the panel
const ROWS: usize = 4;

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
  /// The real frame time, smoothed, in milliseconds
  frame_ms: f32,
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
      frame_ms: 0.0,
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
  for (mix, variants) in table.iter_mut().enumerate().skip(1) {
    let color = |bit: usize| if mix & (1 << bit) != 0 { 1 } else { 2 };
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
    let real = ctx.time.real.delta() * 1000.0;
    self.frame_ms = if self.frame_ms == 0.0 {
      real
    } else {
      self.frame_ms + (real - self.frame_ms) * 0.05
    };
    let view = ctx.camera().visible_bounds();
    let chunk = |v: f32| {
      (v / CHUNK_SIDE)
        .floor()
        .clamp(0.0, (SIZE / CHUNK_SIZE - 1) as f32)
    };
    let columns = chunk(view.right) - chunk(view.left) + 1.0;
    let rows = chunk(view.bottom) - chunk(view.top) + 1.0;
    let per_layer = (SIZE / CHUNK_SIZE).pow(2);
    let fps = if self.frame_ms > 0.0 {
      1000.0 / self.frame_ms
    } else {
      0.0
    };
    let lines = vec![
      (
        format!("Frame {:.1} ms ({fps:.0} fps), Time::real", self.frame_ms),
        HOT,
      ),
      (
        format!(
          "Chunks in view: {:.0} of {per_layer} a layer, 4 layers",
          columns * rows
        ),
        Color::WHITE,
      ),
      (format!("{} cells in all", group(self.cells)), DIM),
      ("F1: the engine's frame timing".to_string(), DIM),
      (
        "(update, render, present); Map off to compare".to_string(),
        DIM,
      ),
      (format!("Camera at {:.0}, {:.0}", self.at.x, self.at.y), DIM),
    ];
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
