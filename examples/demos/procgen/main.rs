//! Procedural Generation Demo
//!
//! Interactive demonstration of all procgen algorithms: BSP dungeons, cellular
//! caves, Perlin terrain, random walks, Poisson disk sampling and wave
//! function collapse. Runs on the demo-kit shell; the algorithm and grid
//! overlay are settings.

use demo_kit::prelude::*;

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
}

const ALGORITHMS: [Algorithm; 6] = [
  Algorithm::Bsp,
  Algorithm::Cellular,
  Algorithm::Noise,
  Algorithm::RandomWalk,
  Algorithm::Poisson,
  Algorithm::Wfc,
];

/// Setting labels, in `ALGORITHMS` order
const ALGORITHM_LABELS: [&str; 6] = ["BSP", "Cellular", "Noise", "Random walk", "Poisson", "WFC"];

struct ProcgenPage {
  /// Index into `ALGORITHMS`, bound to the Algorithm setting
  algorithm_index: usize,
  /// The algorithm the current grid was generated with
  generated_index: usize,
  grid: Option<GenerationGrid>,
  poisson_points: Vec<(f32, f32)>,
  seed: u64,
  show_grid: bool,
  camera_x: f32,
  camera_y: f32,
  generation_time_ms: u64,
}

impl ProcgenPage {
  fn new() -> Self {
    Self {
      algorithm_index: 0,
      generated_index: 0,
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

    self.generated_index = self.algorithm_index;
    match ALGORITHMS[self.algorithm_index] {
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

impl DemoPage for ProcgenPage {
  fn name(&self) -> &str {
    "Generators"
  }

  fn description(&self) -> &str {
    "Procedural generation: pick an algorithm, roll a new seed, pan the map"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Algorithm", &mut self.algorithm_index, &ALGORITHM_LABELS);
    s.toggle("Grid", &mut self.show_grid);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "New seed"),
      Control::keys(
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
        "Pan",
      ),
    ]
  }

  fn enter(&mut self, _ctx: &mut SceneContext) {
    self.generate();
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Algorithm changed in the settings panel
    if self.algorithm_index != self.generated_index {
      self.generate();
    }

    // Regeneration
    if ctx.input.key_pressed(KeyCode::Space) {
      self.new_seed();
    }

    // Camera panning
    let pan_speed = 200.0 * dt;
    if ctx.input.key_down(KeyCode::KeyA) {
      self.camera_x -= pan_speed;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      self.camera_x += pan_speed;
    }
    if ctx.input.key_down(KeyCode::KeyW) {
      self.camera_y -= pan_speed;
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      self.camera_y += pan_speed;
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let w = screen_w as f32;
    let h = screen_h as f32;

    // Background
    ui.draw_rect(Rect::new(0.0, 0.0, w, h), Color::rgb(30, 30, 35));

    let area = content_area(ui);
    // Centre the map in the content area, below the three info lines
    let info_h = 52.0;

    // Render grid tiles
    if let Some(ref grid) = self.grid {
      let cell = CELL_SIZE as f32;
      let ox = (w - GRID_SIZE as f32 * cell) / 2.0 - self.camera_x;
      let oy =
        area.y + info_h + (area.height - info_h - GRID_SIZE as f32 * cell) / 2.0 - self.camera_y;

      for (gx, gy, tile) in grid.iter() {
        let px = ox + gx as f32 * cell;
        let py = oy + gy as f32 * cell;
        ui.draw_rect(Rect::new(px, py, cell, cell), tile_color(tile));
      }

      // Grid overlay
      if self.show_grid {
        let grid_color = Color::new(100, 100, 100, 100);
        let grid_len = GRID_SIZE as f32 * cell;
        for gy in 0..=GRID_SIZE {
          let py = oy + gy as f32 * cell;
          ui.draw_rect(Rect::new(ox, py, grid_len, 1.0), grid_color);
        }
        for gx in 0..=GRID_SIZE {
          let px = ox + gx as f32 * cell;
          ui.draw_rect(Rect::new(px, oy, 1.0, grid_len), grid_color);
        }
      }

      // Region centers (dot markers)
      for region in grid.regions() {
        let (cx, cy) = region.center;
        let px = ox + cx as f32 * cell + cell / 2.0 - 4.0;
        let py = oy + cy as f32 * cell + cell / 2.0 - 4.0;
        ui.draw_rect(Rect::new(px, py, 8.0, 8.0), Color::hex(0xFF0000));
      }

      // Poisson points (dot markers)
      for &(x, y) in &self.poisson_points {
        let px = ox + x - 4.0;
        let py = oy + y - 4.0;
        ui.draw_rect(Rect::new(px, py, 8.0, 8.0), Color::hex(0x00FF00));
      }
    }

    let text_light = Color::rgb(220, 220, 220);
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    let x = area.x + 10.0;
    let mut y = area.y + 4.0;
    ui.draw_text(
      &format!("Algorithm: {}", ALGORITHMS[self.generated_index].name()),
      x,
      y,
      text_light,
    );
    y += 16.0;
    ui.draw_text(
      &format!("Seed: {} | Time: {}ms", self.seed, self.generation_time_ms),
      x,
      y,
      text_accent,
    );
    y += 16.0;
    if let Some(ref grid) = self.grid {
      ui.draw_text(
        &format!(
          "Regions: {} | Floor: {} | Wall: {}",
          grid.regions().len(),
          grid.count_type(TileType::Floor),
          grid.count_type(TileType::Wall)
        ),
        x,
        y,
        text_muted,
      );
    }
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

fn main() {
  Demo::new("Procedural Generation")
    .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableAspectRatio)
        .target_fps(60)
    })
    .page(ProcgenPage::new)
    .run();
}
