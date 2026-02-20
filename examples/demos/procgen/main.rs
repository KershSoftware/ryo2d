//! Procedural Generation Demo
//!
//! Interactive demonstration of all procgen algorithms.
//!
//! Controls:
//! - 1-6: Switch algorithm (BSP, Cellular, Noise, Walk, Poisson, WFC)
//! - Space: Regenerate with new seed
//! - G: Toggle grid overlay
//! - Arrow keys: Pan camera
//! - Escape: Exit

use ryo2d::prelude::*;

const SCREEN_WIDTH: u32 = 1024;
const SCREEN_HEIGHT: u32 = 768;
const GRID_SIZE: u32 = 64;
const CELL_SIZE: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Algorithm {
  Bsp,
  Cellular,
  Noise,
  RandomWalk,
  Poisson,
  Wfc,
}

impl Algorithm {
  fn name(&self) -> &'static str {
    match self {
      Algorithm::Bsp => "BSP Dungeon",
      Algorithm::Cellular => "Cellular Automata",
      Algorithm::Noise => "Perlin Noise",
      Algorithm::RandomWalk => "Random Walk",
      Algorithm::Poisson => "Poisson Disk",
      Algorithm::Wfc => "Wave Function Collapse",
    }
  }

  fn key_hint(&self) -> &'static str {
    match self {
      Algorithm::Bsp => "1",
      Algorithm::Cellular => "2",
      Algorithm::Noise => "3",
      Algorithm::RandomWalk => "4",
      Algorithm::Poisson => "5",
      Algorithm::Wfc => "6",
    }
  }
}

struct ProcgenScene {
  algorithm: Algorithm,
  grid: Option<GenerationGrid>,
  poisson_points: Vec<(f32, f32)>,
  seed: u64,
  show_grid: bool,
  camera_x: f32,
  camera_y: f32,
  generation_time_ms: u64,
}

impl ProcgenScene {
  fn new() -> Self {
    Self {
      algorithm: Algorithm::Bsp,
      grid: None,
      poisson_points: Vec::new(),
      seed: 12345,
      show_grid: false,
      camera_x: 0.0,
      camera_y: 0.0,
      generation_time_ms: 0,
    }
  }

  fn generate(&mut self) {
    let start = std::time::Instant::now();

    match self.algorithm {
      Algorithm::Bsp => {
        let result = ProcgenBuilder::new(GRID_SIZE, GRID_SIZE)
          .with_seed(self.seed)
          .bsp(BspConfig::dungeon_preset())
          .build();
        self.grid = Some(result.into_grid());
        self.poisson_points.clear();
      }
      Algorithm::Cellular => {
        let result = ProcgenBuilder::new(GRID_SIZE, GRID_SIZE)
          .with_seed(self.seed)
          .cellular(CellularConfig::cave_preset())
          .build();
        self.grid = Some(result.into_grid());
        self.poisson_points.clear();
      }
      Algorithm::Noise => {
        let result = ProcgenBuilder::new(GRID_SIZE, GRID_SIZE)
          .with_seed(self.seed)
          .noise(NoiseConfig::terrain_preset())
          .build();
        self.grid = Some(result.into_grid());
        self.poisson_points.clear();
      }
      Algorithm::RandomWalk => {
        let result = ProcgenBuilder::new(GRID_SIZE, GRID_SIZE)
          .with_seed(self.seed)
          .random_walk(
            RandomWalkConfig::default()
              .with_steps(2000)
              .with_brush_size(2)
              .with_walker_count(3),
          )
          .build();
        self.grid = Some(result.into_grid());
        self.poisson_points.clear();
      }
      Algorithm::Poisson => {
        // Generate a floor grid and overlay with poisson points
        let mut grid = GenerationGrid::filled(GRID_SIZE, GRID_SIZE, TileType::Floor);
        grid.fill_rect(0, 0, GRID_SIZE, 1, TileType::Wall);
        grid.fill_rect(0, GRID_SIZE as i32 - 1, GRID_SIZE, 1, TileType::Wall);
        grid.fill_rect(0, 0, 1, GRID_SIZE, TileType::Wall);
        grid.fill_rect(GRID_SIZE as i32 - 1, 0, 1, GRID_SIZE, TileType::Wall);

        self.grid = Some(grid);

        let points = poisson_points(
          (GRID_SIZE - 2) as f32 * CELL_SIZE as f32,
          (GRID_SIZE - 2) as f32 * CELL_SIZE as f32,
          &PoissonConfig::default().with_min_distance(30.0),
          self.seed,
        );
        self.poisson_points = points
          .iter()
          .map(|&(x, y)| (x + CELL_SIZE as f32, y + CELL_SIZE as f32))
          .collect();
      }
      Algorithm::Wfc => {
        let result = ProcgenBuilder::new(GRID_SIZE, GRID_SIZE)
          .with_seed(self.seed)
          .wfc(WfcConfig::simple_preset())
          .build();
        self.grid = Some(result.into_grid());
        self.poisson_points.clear();
      }
    }

    self.generation_time_ms = start.elapsed().as_millis() as u64;
  }

  fn new_seed(&mut self) {
    // Use SeededRng::from_entropy() to get a random seed
    self.seed = SeededRng::from_entropy().seed();
    self.generate();
  }
}

impl Scene for ProcgenScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    self.generate();
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Algorithm selection
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.algorithm = Algorithm::Bsp;
      self.generate();
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.algorithm = Algorithm::Cellular;
      self.generate();
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.algorithm = Algorithm::Noise;
      self.generate();
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      self.algorithm = Algorithm::RandomWalk;
      self.generate();
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      self.algorithm = Algorithm::Poisson;
      self.generate();
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      self.algorithm = Algorithm::Wfc;
      self.generate();
    }

    // Regeneration
    if ctx.input.key_pressed(KeyCode::Space) {
      self.new_seed();
    }
    // Toggle grid
    if ctx.input.key_pressed(KeyCode::KeyG) {
      self.show_grid = !self.show_grid;
    }

    // Camera panning
    let pan_speed = 200.0 * dt;
    if ctx.input.key_down(KeyCode::ArrowLeft) {
      self.camera_x -= pan_speed;
    }
    if ctx.input.key_down(KeyCode::ArrowRight) {
      self.camera_x += pan_speed;
    }
    if ctx.input.key_down(KeyCode::ArrowUp) {
      self.camera_y -= pan_speed;
    }
    if ctx.input.key_down(KeyCode::ArrowDown) {
      self.camera_y += pan_speed;
    }

    // Exit (Pop with no scene below exits the engine)
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    SceneAction::Continue
  }

  #[allow(deprecated)]
  fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, _camera: &Camera) {
    // Clear to dark gray
    for pixel in frame.chunks_exact_mut(4) {
      pixel[0] = 30;
      pixel[1] = 30;
      pixel[2] = 35;
      pixel[3] = 255;
    }

    // Render grid
    if let Some(ref grid) = self.grid {
      let offset_x =
        ((width as f32 - GRID_SIZE as f32 * CELL_SIZE as f32) / 2.0 - self.camera_x) as i32;
      let offset_y =
        ((height as f32 - GRID_SIZE as f32 * CELL_SIZE as f32) / 2.0 - self.camera_y) as i32;

      for (gx, gy, tile) in grid.iter() {
        let px = offset_x + gx * CELL_SIZE as i32;
        let py = offset_y + gy * CELL_SIZE as i32;

        let color = tile_color(tile);
        draw_rect(frame, width, height, px, py, CELL_SIZE, CELL_SIZE, color);
      }

      // Draw grid overlay
      if self.show_grid {
        for gy in 0..=GRID_SIZE {
          let py = offset_y + gy as i32 * CELL_SIZE as i32;
          draw_line_h(
            frame,
            width,
            height,
            offset_x,
            py,
            GRID_SIZE * CELL_SIZE,
            Color::new(100, 100, 100, 100),
          );
        }
        for gx in 0..=GRID_SIZE {
          let px = offset_x + gx as i32 * CELL_SIZE as i32;
          draw_line_v(
            frame,
            width,
            height,
            px,
            offset_y,
            GRID_SIZE * CELL_SIZE,
            Color::new(100, 100, 100, 100),
          );
        }
      }

      // Draw region centers
      for region in grid.regions() {
        let (cx, cy) = region.center;
        let px = offset_x + cx * CELL_SIZE as i32 + CELL_SIZE as i32 / 2;
        let py = offset_y + cy * CELL_SIZE as i32 + CELL_SIZE as i32 / 2;
        draw_circle(frame, width, height, px, py, 4, Color::hex(0xFF0000));
      }

      // Draw poisson points
      for &(x, y) in &self.poisson_points {
        let px = offset_x + x as i32;
        let py = offset_y + y as i32;
        draw_circle(frame, width, height, px, py, 5, Color::hex(0x00FF00));
      }
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let text_light = Color::rgb(220, 220, 220);
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    // Title
    ui.draw_text(
      &format!("Procedural Generation: {}", self.algorithm.name()),
      10.0,
      10.0,
      text_light,
    );

    // Info
    ui.draw_text(
      &format!("Seed: {} | Time: {}ms", self.seed, self.generation_time_ms),
      10.0,
      26.0,
      text_accent,
    );

    // Grid info
    if let Some(ref grid) = self.grid {
      ui.draw_text(
        &format!(
          "Regions: {} | Floor: {} | Wall: {}",
          grid.regions().len(),
          grid.count_type(TileType::Floor),
          grid.count_type(TileType::Wall)
        ),
        10.0,
        42.0,
        text_muted,
      );
    }

    // Controls
    ui.draw_text(
      "1-6: Algorithm | Space: New Seed | G: Grid | Arrows: Pan",
      10.0,
      58.0,
      text_muted,
    );

    // Algorithm list on the right
    let (screen_w, _) = ui.screen_size();
    let x = screen_w as f32 - 220.0;

    let algorithms = [
      Algorithm::Bsp,
      Algorithm::Cellular,
      Algorithm::Noise,
      Algorithm::RandomWalk,
      Algorithm::Poisson,
      Algorithm::Wfc,
    ];

    for (i, algo) in algorithms.iter().enumerate() {
      let color = if *algo == self.algorithm {
        text_light
      } else {
        text_muted
      };
      ui.draw_text(
        &format!("[{}] {}", algo.key_hint(), algo.name()),
        x,
        10.0 + i as f32 * 16.0,
        color,
      );
    }
    SceneAction::Continue
  }
}

fn tile_color(tile: TileType) -> Color {
  match tile {
    TileType::Empty => Color::hex(0x1a1a1f),
    TileType::Floor => Color::hex(0x4a4a55),
    TileType::Wall => Color::hex(0x2a2a30),
    TileType::Door => Color::hex(0x8B4513),
    TileType::Water => Color::hex(0x2255AA),
    TileType::Custom(1) => Color::hex(0x3366BB), // Shallow water
    TileType::Custom(2) => Color::hex(0xD4B896), // Sand
    TileType::Custom(3) => Color::hex(0x228B22), // Forest
    TileType::Custom(4) => Color::hex(0xFFFFFF), // Snow
    TileType::Custom(_) => Color::hex(0xFF00FF), // Unknown
  }
}

#[allow(clippy::too_many_arguments)]
fn draw_rect(
  frame: &mut [u8],
  width: u32,
  height: u32,
  x: i32,
  y: i32,
  w: u32,
  h: u32,
  color: Color,
) {
  for py in y.max(0)..(y + h as i32).min(height as i32) {
    for px in x.max(0)..(x + w as i32).min(width as i32) {
      let idx = ((py as u32 * width + px as u32) * 4) as usize;
      if idx + 3 < frame.len() {
        frame[idx] = color.r;
        frame[idx + 1] = color.g;
        frame[idx + 2] = color.b;
        frame[idx + 3] = color.a;
      }
    }
  }
}

fn draw_circle(
  frame: &mut [u8],
  width: u32,
  height: u32,
  cx: i32,
  cy: i32,
  radius: i32,
  color: Color,
) {
  for py in (cy - radius).max(0)..(cy + radius).min(height as i32) {
    for px in (cx - radius).max(0)..(cx + radius).min(width as i32) {
      let dx = px - cx;
      let dy = py - cy;
      if dx * dx + dy * dy <= radius * radius {
        let idx = ((py as u32 * width + px as u32) * 4) as usize;
        if idx + 3 < frame.len() {
          frame[idx] = color.r;
          frame[idx + 1] = color.g;
          frame[idx + 2] = color.b;
          frame[idx + 3] = color.a;
        }
      }
    }
  }
}

fn draw_line_h(
  frame: &mut [u8],
  width: u32,
  height: u32,
  x: i32,
  y: i32,
  length: u32,
  color: Color,
) {
  if y < 0 || y >= height as i32 {
    return;
  }
  for px in x.max(0)..(x + length as i32).min(width as i32) {
    let idx = ((y as u32 * width + px as u32) * 4) as usize;
    if idx + 3 < frame.len() {
      // Alpha blend
      let alpha = color.a as f32 / 255.0;
      frame[idx] = (frame[idx] as f32 * (1.0 - alpha) + color.r as f32 * alpha) as u8;
      frame[idx + 1] = (frame[idx + 1] as f32 * (1.0 - alpha) + color.g as f32 * alpha) as u8;
      frame[idx + 2] = (frame[idx + 2] as f32 * (1.0 - alpha) + color.b as f32 * alpha) as u8;
    }
  }
}

fn draw_line_v(
  frame: &mut [u8],
  width: u32,
  height: u32,
  x: i32,
  y: i32,
  length: u32,
  color: Color,
) {
  if x < 0 || x >= width as i32 {
    return;
  }
  for py in y.max(0)..(y + length as i32).min(height as i32) {
    let idx = ((py as u32 * width + x as u32) * 4) as usize;
    if idx + 3 < frame.len() {
      // Alpha blend
      let alpha = color.a as f32 / 255.0;
      frame[idx] = (frame[idx] as f32 * (1.0 - alpha) + color.r as f32 * alpha) as u8;
      frame[idx + 1] = (frame[idx + 1] as f32 * (1.0 - alpha) + color.g as f32 * alpha) as u8;
      frame[idx + 2] = (frame[idx + 2] as f32 * (1.0 - alpha) + color.b as f32 * alpha) as u8;
    }
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Procedural Generation Demo - Ryo2D")
    .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
    .window_mode(WindowMode::ResizableAspectRatio)
    .target_fps(60)
    .with_scene(ProcgenScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
