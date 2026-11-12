//! Many lights page
//!
//! Sixty-four shadowed torches over a maze built as a tile map in code. The
//! walls are the map's occluder chains; every torch reads only the walls
//! near it. `Q` cycles the player's shadow quality, which stays changed (and
//! is saved with the player's settings) like an options screen's choice.
//! The walls shadow themselves ("Walls stay dark"), so a torch lights a
//! wall's face and not its top; turned off, the setting changes the wall
//! layer's `self_shadow` and the map builds its chains again.

use std::sync::Arc;

use demo_kit::prelude::*;

/// The maze: 40 x 22 cells of 32 px, rooms at odd cells, the map's top at
/// `TOP` so it sits in the 1280 x 720 screen
const COLS: i32 = 40;
const ROWS: i32 = 22;
const CELL: u32 = 32;
const TOP: f32 = 8.0;
const TORCHES: usize = 64;
/// Each torch's source size in pixels: soft shadows at every quality (the
/// soft shadows page is where the size is played with)
const TORCH_SIZE: f32 = 6.0;

/// A torch of the maze
#[derive(Component)]
pub struct Torch;

/// A small deterministic generator: the maze is the same every run
struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }

  fn below(&mut self, n: usize) -> usize {
    (self.next() % n as u64) as usize
  }
}

/// The maze's walls (`true`) by cell, and its rooms: a depth-first maze over
/// the odd cells, then a tenth of the inner walls knocked out for loops
fn maze(rng: &mut Rng) -> (Vec<bool>, Vec<IVec2>) {
  let at = |c: IVec2| (c.y * COLS + c.x) as usize;
  let mut wall = vec![true; (COLS * ROWS) as usize];
  let rooms: Vec<IVec2> = (0..ROWS / 2)
    .flat_map(|y| (0..COLS / 2).map(move |x| IVec2::new(2 * x + 1, 2 * y + 1)))
    .filter(|c| c.x < COLS - 1 && c.y < ROWS - 2)
    .collect();
  let start = rooms[0];
  wall[at(start)] = false;
  let mut stack = vec![start];
  while let Some(&here) = stack.last() {
    let mut next: Vec<IVec2> = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
      .iter()
      .map(|d| here + *d * 2)
      .filter(|c| c.x > 0 && c.x < COLS - 1 && c.y > 0 && c.y < ROWS - 2 && wall[at(*c)])
      .collect();
    if next.is_empty() {
      stack.pop();
      continue;
    }
    let to = next.swap_remove(rng.below(next.len()));
    wall[at((here + to) / 2)] = false;
    wall[at(to)] = false;
    stack.push(to);
  }
  for y in 1..ROWS - 2 {
    for x in 1..COLS - 1 {
      let c = IVec2::new(x, y);
      if (x + y) % 2 == 1 && wall[at(c)] && rng.below(10) == 0 {
        wall[at(c)] = false;
      }
    }
  }
  (wall, rooms)
}

/// The maze's wall layer
const WALLS: &str = "walls";

/// A map of one solid tile whose shape blocks light, laid where `wall`
/// says; the layer blocks light, shadowing itself with `dark`, and does not
/// collide
fn build_map(ctx: &mut SceneContext, wall: &[bool], dark: bool) -> TileMap {
  let side = CELL;
  let image = ctx
    .assets()
    .add(Image::solid_rect(side, side, 48, 44, 60, 255));
  let mut tiles = Tileset::new(
    "walls",
    UVec2::splat(CELL),
    TilesetImage::Atlas {
      image,
      size: UVec2::splat(CELL),
    },
  );
  tiles.columns = 1;
  tiles.tile_count = 1;
  let c = CELL as f32;
  tiles.insert_tile(
    0,
    TileData {
      shapes: vec![TileShape {
        points: vec![
          Vec2::ZERO,
          Vec2::new(c, 0.0),
          Vec2::new(c, c),
          Vec2::new(0.0, c),
        ],
        occluder: true,
        ..TileShape::default()
      }],
      ..TileData::default()
    },
  );
  let mut map = TileMap::new(UVec2::new(COLS as u32, ROWS as u32), UVec2::splat(CELL));
  let set = map.add_tileset(TilesetSource::Embedded(Arc::new(tiles)));
  let layer = map.add_tile_layer(WALLS);
  map.layers[layer].info.collides = false;
  map.layers[layer].info.occludes = Some(true);
  map.layers[layer].info.self_shadow = dark;
  if let Some(grid) = map.tiles_mut(layer) {
    for (i, _) in wall.iter().enumerate().filter(|(_, w)| **w) {
      let cell = IVec2::new(i as i32 % COLS, i as i32 / COLS);
      grid.set(cell, Some(Tile::new(set, 0)));
    }
  }
  map
}

/// The name a quality is shown by, and the next one round
fn quality_name(quality: ShadowQuality) -> &'static str {
  match quality {
    ShadowQuality::Low => "Low (256 texels)",
    ShadowQuality::Medium => "Medium (512 texels)",
    ShadowQuality::High => "High (1,024 texels)",
    ShadowQuality::Ultra => "Ultra (2,048 texels)",
  }
}

fn next_quality(quality: ShadowQuality) -> ShadowQuality {
  match quality {
    ShadowQuality::Low => ShadowQuality::Medium,
    ShadowQuality::Medium => ShadowQuality::High,
    ShadowQuality::High => ShadowQuality::Ultra,
    ShadowQuality::Ultra => ShadowQuality::Low,
  }
}

/// Many lights page state
pub struct ManyLightsPage {
  radius: f32,
  quality: ShadowQuality,
  /// The radius the torches were last given
  applied: Option<f32>,
  /// Whether the walls shadow themselves, and what the wall layer was
  /// last given
  walls_dark: bool,
  walls_applied: bool,
  torches: usize,
}

impl ManyLightsPage {
  pub(crate) fn new() -> Self {
    Self {
      radius: 130.0,
      quality: ShadowQuality::default(),
      applied: None,
      walls_dark: true,
      walls_applied: true,
      torches: 0,
    }
  }

  fn torch_light(&self, n: usize) -> Light2D {
    // Warm shades, in sRGB
    let colour = [
      Color::new(255, 200, 120, 255),
      Color::new(255, 170, 90, 255),
      Color::new(255, 225, 160, 255),
    ][n % 3];
    Light2D::point(colour, self.radius)
      .with_intensity(1.2)
      .with_soft_shadows(TORCH_SIZE)
  }
}

impl DemoPage for ManyLightsPage {
  fn name(&self) -> &str {
    "Many lights"
  }

  fn description(&self) -> &str {
    "Sixty-four torches each cast their own shadows off the maze's walls; Q changes the shadow quality.\n\
Lower qualities draw coarser shadow maps and cost less; walls that stay dark shadow themselves."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Torch radius", &mut self.radius, 60.0..=260.0, 10.0);
    s.toggle("Walls stay dark", &mut self.walls_dark);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::KeyQ, "Shadow quality")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.quality = ctx.settings().shadow_quality();
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::new(79, 75, 96, 255), 0.03));

    let floor = ctx
      .assets()
      .add(Image::solid_rect(1280, 720, 100, 92, 112, 255));
    ctx.world.spawn((
      Transform::from_xy(640.0, 360.0),
      Renderable::image(floor).with_z_order(0),
    ));

    let (wall, mut rooms) = maze(&mut Rng(0x9E37_79B9_7F4A_7C15));
    let map = build_map(ctx, &wall, self.walls_dark);
    self.walls_applied = self.walls_dark;
    let handle = ctx.assets().add(map);
    ctx
      .world
      .spawn((MapRoot::new(handle).with_z(5), Transform::from_xy(0.0, TOP)));

    // Torches in sixty-four of the maze's rooms
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let flame = ctx.assets().add(Image::solid_circle(6, 255, 235, 170, 255));
    for n in 0..TORCHES.min(rooms.len()) {
      let room = rooms.swap_remove(rng.below(rooms.len()));
      let at = Vec2::new(
        (room.x as f32 + 0.5) * CELL as f32,
        (room.y as f32 + 0.5) * CELL as f32 + TOP,
      );
      ctx.world.spawn((
        Torch,
        Transform::from_translation(at),
        self.torch_light(n),
        Renderable::image(flame.clone()).with_z_order(10),
      ));
      self.torches += 1;
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.quality = next_quality(self.quality);
      ctx.settings().set_shadow_quality(self.quality);
    }
    if self.applied != Some(self.radius) {
      self.applied = Some(self.radius);
      let mut torches = ctx.world.query_filtered::<&mut Light2D, With<Torch>>();
      for (n, mut light) in torches.iter_mut(ctx.world).enumerate() {
        *light = self.torch_light(n);
      }
    }
    if self.walls_applied != self.walls_dark {
      // The landed wall layer's flag: the map builds its chains again with
      // it. Until the map lands there is no layer, and this waits for it
      let mut layers = ctx.world.query::<&mut LayerInfo>();
      for mut info in layers.iter_mut(ctx.world) {
        if info.name == WALLS {
          info.self_shadow = self.walls_dark;
          self.walls_applied = self.walls_dark;
        }
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.label(
      &format!(
        "{} shadowed torches | shadow quality: {} (Q, saved with your settings)",
        self.torches,
        quality_name(self.quality)
      ),
      area.x + 10.0,
      area.y + 10.0,
      Color::new(180, 180, 180, 255),
      Some(Color::new(0, 0, 0, 180)),
    );
  }
}
