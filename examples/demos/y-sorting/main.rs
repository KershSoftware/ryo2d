//! Y-Sorting Demo - Zelda-Style Top-Down View
//!
//! This example demonstrates the YSort component with a classic Zelda-style
//! top-down RPG view:
//! - Tiled grass world with objects
//! - Entities automatically sort by Y position (feet)
//! - Walk behind trees, in front of bushes based on Y position
//!
//! Depth scaling (a setting) also scales entities by their Y position.
//! Runs on the demo-kit shell.

use demo_kit::prelude::*;

const TILE_SIZE: u32 = 16;
const WORLD_TILES_X: u32 = 20;
const WORLD_TILES_Y: u32 = 15;
const GAME_WIDTH: u32 = TILE_SIZE * WORLD_TILES_X; // 320
const GAME_HEIGHT: u32 = TILE_SIZE * WORLD_TILES_Y; // 240

/// Window size is the world at this zoom, leaving room for the demo-kit shell
const SCREEN_ZOOM: f32 = 2.0;

// Play area for depth scaling (leave some margin)
const PLAY_AREA_TOP: f32 = 24.0;
const PLAY_AREA_BOTTOM: f32 = (GAME_HEIGHT - 16) as f32;

/// Marker for the player entity
#[derive(Debug)]
struct Player;

/// Marker for NPC entities that wander
#[derive(Debug)]
struct Wanderer {
  speed: f32,
  direction: (f32, f32),
  change_timer: f32,
  /// The shadow entity that follows this NPC
  shadow: Entity,
}

/// Main demo page
struct YSortingPage {
  player: Option<Entity>,
  player_shadow: Option<Entity>,
  wanderers: Vec<Entity>,
  depth_scale_enabled: bool,
  /// The depth scaling last written to the entities' YSort components
  applied_depth_scale: bool,
}

impl YSortingPage {
  fn new() -> Self {
    Self {
      player: None,
      player_shadow: None,
      wanderers: Vec::new(),
      depth_scale_enabled: true,
      applied_depth_scale: true,
    }
  }

  /// Create a grass tile with some variation
  fn create_grass_tile(variant: u8) -> Sprite {
    let mut data = vec![0u8; (TILE_SIZE * TILE_SIZE * 4) as usize];

    for y in 0..TILE_SIZE {
      for x in 0..TILE_SIZE {
        let idx = ((y * TILE_SIZE + x) * 4) as usize;

        // Base grass color with slight variation
        let noise = ((x * 7 + y * 13 + variant as u32 * 17) % 10) as i32 - 5;
        let g = (140 + noise).clamp(0, 255) as u8;

        data[idx] = 76; // R
        data[idx + 1] = g; // G (varied)
        data[idx + 2] = 48; // B
        data[idx + 3] = 255;

        // Add occasional grass blade detail
        if (x + y * 3 + variant as u32).is_multiple_of(8) {
          data[idx + 1] = g + 20;
        }
      }
    }

    Sprite::from_rgba(data, TILE_SIZE, TILE_SIZE)
  }

  /// Create a top-down character sprite (16x16, Zelda-style)
  fn create_character_sprite(r: u8, g: u8, b: u8) -> Sprite {
    let size = 16u32;
    let mut data = vec![0u8; (size * size * 4) as usize];

    // Simple top-down character: head (circle) + body (oval below)
    let cx = size as f32 / 2.0;

    for y in 0..size {
      for x in 0..size {
        let idx = ((y * size + x) * 4) as usize;
        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;

        // Head (top portion, circular)
        let head_cy = 5.0;
        let head_r = 4.0;
        let head_dist = ((fx - cx).powi(2) + (fy - head_cy).powi(2)).sqrt();

        // Body (bottom portion, wider oval)
        let body_cy = 11.0;
        let body_rx = 5.0;
        let body_ry = 4.0;
        let body_dist = ((fx - cx) / body_rx).powi(2) + ((fy - body_cy) / body_ry).powi(2);

        if head_dist < head_r {
          // Head - slightly lighter
          let shade = 1.0 - (head_dist / head_r) * 0.2;
          data[idx] = ((r as f32 * shade * 1.1).min(255.0)) as u8;
          data[idx + 1] = ((g as f32 * shade * 1.1).min(255.0)) as u8;
          data[idx + 2] = ((b as f32 * shade * 1.1).min(255.0)) as u8;
          data[idx + 3] = 255;
        } else if body_dist < 1.0 && fy > 7.0 {
          // Body
          let shade = 1.0 - body_dist * 0.3;
          data[idx] = (r as f32 * shade) as u8;
          data[idx + 1] = (g as f32 * shade) as u8;
          data[idx + 2] = (b as f32 * shade) as u8;
          data[idx + 3] = 255;
        }
      }
    }

    // Add a dark outline
    Self::add_outline(&mut data, size, size);

    Sprite::from_rgba(data, size, size)
  }

  /// Add a 1px dark outline to non-transparent pixels
  fn add_outline(data: &mut [u8], width: u32, height: u32) {
    let original = data.to_vec();

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;

        // Skip if this pixel is already filled
        if original[idx + 3] > 0 {
          continue;
        }

        // Check neighbors for any filled pixels
        let neighbors = [
          (x.wrapping_sub(1), y),
          (x + 1, y),
          (x, y.wrapping_sub(1)),
          (x, y + 1),
        ];

        for (nx, ny) in neighbors {
          if nx < width && ny < height {
            let nidx = ((ny * width + nx) * 4) as usize;
            if original[nidx + 3] > 0 {
              // Add dark outline pixel
              data[idx] = 32;
              data[idx + 1] = 32;
              data[idx + 2] = 32;
              data[idx + 3] = 255;
              break;
            }
          }
        }
      }
    }
  }

  /// Create a tree sprite (16x24, trunk at bottom for y-sorting)
  fn create_tree_sprite() -> Sprite {
    let width = 16u32;
    let height = 24u32;
    let mut data = vec![0u8; (width * height * 4) as usize];

    let cx = width as f32 / 2.0;

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;

        // Trunk (bottom 6 pixels, narrow)
        if fy >= 18.0 && (fx - cx).abs() < 3.0 {
          let shade = 0.8 + ((x + y) % 2) as f32 * 0.1;
          data[idx] = (101.0 * shade) as u8;
          data[idx + 1] = (67.0 * shade) as u8;
          data[idx + 2] = (33.0 * shade) as u8;
          data[idx + 3] = 255;
          continue;
        }

        // Foliage (layered circles for a bushy look)
        let foliage_layers = [
          (cx, 10.0, 7.0),      // Main center
          (cx - 3.0, 7.0, 5.0), // Top left
          (cx + 3.0, 7.0, 5.0), // Top right
          (cx, 5.0, 4.0),       // Very top
        ];

        for (fcx, fcy, fr) in foliage_layers {
          let dist = ((fx - fcx).powi(2) + (fy - fcy).powi(2)).sqrt();
          if dist < fr {
            let shade = 0.7 + ((x * 3 + y * 7) % 5) as f32 * 0.08;
            data[idx] = (34.0 * shade) as u8;
            data[idx + 1] = (120.0 * shade) as u8;
            data[idx + 2] = (34.0 * shade) as u8;
            data[idx + 3] = 255;
            break;
          }
        }
      }
    }

    Self::add_outline(&mut data, width, height);
    Sprite::from_rgba(data, width, height)
  }

  /// Create a bush sprite (16x12)
  fn create_bush_sprite() -> Sprite {
    let width = 16u32;
    let height = 12u32;
    let mut data = vec![0u8; (width * height * 4) as usize];

    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0 + 1.0;

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;

        // Bush shape (wide ellipse)
        let dx = (fx - cx) / 7.0;
        let dy = (fy - cy) / 5.0;
        let dist = dx * dx + dy * dy;

        if dist < 1.0 {
          let shade = 0.7 + ((x * 5 + y * 3) % 6) as f32 * 0.08;
          data[idx] = (50.0 * shade) as u8;
          data[idx + 1] = (130.0 * shade) as u8;
          data[idx + 2] = (50.0 * shade) as u8;
          data[idx + 3] = 255;
        }
      }
    }

    Self::add_outline(&mut data, width, height);
    Sprite::from_rgba(data, width, height)
  }

  /// Create a rock sprite (16x12)
  fn create_rock_sprite() -> Sprite {
    let width = 16u32;
    let height = 12u32;
    let mut data = vec![0u8; (width * height * 4) as usize];

    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0 + 1.0;

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;

        // Rock shape (irregular ellipse)
        let dx = (fx - cx) / 6.5;
        let dy = (fy - cy) / 4.5;
        let dist = dx * dx + dy * dy;

        if dist < 1.0 {
          // Add some irregular shading
          let shade = 0.6 + ((x * 7 + y * 11) % 8) as f32 * 0.06;
          let highlight = if fy < cy - 1.0 { 1.15 } else { 1.0 };
          data[idx] = (120.0 * shade * highlight).min(255.0) as u8;
          data[idx + 1] = (110.0 * shade * highlight).min(255.0) as u8;
          data[idx + 2] = (100.0 * shade * highlight).min(255.0) as u8;
          data[idx + 3] = 255;
        }
      }
    }

    Self::add_outline(&mut data, width, height);
    Sprite::from_rgba(data, width, height)
  }

  /// Create a shadow sprite
  fn create_shadow_sprite() -> Sprite {
    let width = 12u32;
    let height = 6u32;
    let mut data = vec![0u8; (width * height * 4) as usize];

    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;

        let dx = (fx - cx) / (width as f32 / 2.0);
        let dy = (fy - cy) / (height as f32 / 2.0);
        let dist = dx * dx + dy * dy;

        if dist < 1.0 {
          let alpha = ((1.0 - dist) * 80.0) as u8;
          data[idx] = 0;
          data[idx + 1] = 0;
          data[idx + 2] = 0;
          data[idx + 3] = alpha;
        }
      }
    }

    Sprite::from_rgba(data, width, height)
  }

  fn get_y_sort(&self) -> YSort {
    if self.depth_scale_enabled {
      YSort::new().with_foot_offset(1.0).with_depth_scale(
        0.85,
        1.15,
        PLAY_AREA_TOP,
        PLAY_AREA_BOTTOM,
      )
    } else {
      YSort::new().with_foot_offset(1.0)
    }
  }
}

impl DemoPage for YSortingPage {
  fn name(&self) -> &str {
    "Y-Sort"
  }

  fn description(&self) -> &str {
    "Entities sort by foot Y: walk behind trees, in front of bushes\n\
     Depth scaling also scales entities by their Y position"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Depth scaling", &mut self.depth_scale_enabled);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::keys(
      "WASD",
      &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
      "Move the player",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // The 320x240 world is shown at 2x so the shell has room around it
    ctx.camera.set_zoom(SCREEN_ZOOM);
    ctx
      .camera
      .set_position(GAME_WIDTH as f32 / 2.0, GAME_HEIGHT as f32 / 2.0);

    // Create grass tiles with variation
    let grass_handles: Vec<_> = (0..4)
      .map(|i| {
        let grass = Self::create_grass_tile(i);
        ctx.assets.create_sprite(grass, format!("grass_{}", i))
      })
      .collect();

    // Fill world with grass tiles
    for ty in 0..WORLD_TILES_Y {
      for tx in 0..WORLD_TILES_X {
        let variant = ((tx * 3 + ty * 7) % 4) as usize;
        ctx.world.spawn((
          Transform::new((tx * TILE_SIZE) as f32, (ty * TILE_SIZE) as f32),
          Renderable::sprite(grass_handles[variant].clone()).with_z_order(-100),
        ));
      }
    }

    // Create sprites
    let player_sprite = Self::create_character_sprite(70, 130, 220); // Blue tunic
    let player_handle = ctx.assets.create_sprite(player_sprite, "player");

    let tree_sprite = Self::create_tree_sprite();
    let tree_handle = ctx.assets.create_sprite(tree_sprite, "tree");

    let bush_sprite = Self::create_bush_sprite();
    let bush_handle = ctx.assets.create_sprite(bush_sprite, "bush");

    let rock_sprite = Self::create_rock_sprite();
    let rock_handle = ctx.assets.create_sprite(rock_sprite, "rock");

    let shadow_sprite = Self::create_shadow_sprite();
    let shadow_handle = ctx.assets.create_sprite(shadow_sprite, "shadow");

    // Spawn player at center
    let player_x = GAME_WIDTH as f32 / 2.0;
    let player_y = GAME_HEIGHT as f32 / 2.0;

    // Player shadow (rendered below, no y-sort)
    self.player_shadow = Some(ctx.world.spawn((
      Transform::new(player_x, player_y + 2.0).with_anchor(Anchor::Center),
      Renderable::sprite(shadow_handle.clone()).with_z_order(-1),
    )));

    // Player
    self.player = Some(ctx.world.spawn((
      Player,
      Transform::new(player_x, player_y).with_anchor(Anchor::BottomCenter),
      Renderable::sprite(player_handle.clone()).with_z_order(0),
      self.get_y_sort(),
    )));

    // Place trees around the world
    let tree_positions = [
      (3, 2),
      (7, 3),
      (15, 2),
      (18, 4),
      (2, 7),
      (12, 6),
      (17, 8),
      (4, 11),
      (9, 10),
      (14, 12),
      (18, 11),
    ];

    for (tx, ty) in tree_positions {
      let x = (tx * TILE_SIZE as i32 + TILE_SIZE as i32 / 2) as f32;
      let y = (ty * TILE_SIZE as i32 + TILE_SIZE as i32) as f32;

      ctx.world.spawn((
        Transform::new(x, y).with_anchor(Anchor::BottomCenter),
        Renderable::sprite(tree_handle.clone()).with_z_order(0),
        self.get_y_sort(),
      ));
    }

    // Place bushes
    let bush_positions = [(5, 4), (10, 3), (1, 9), (8, 8), (16, 6), (13, 10)];

    for (tx, ty) in bush_positions {
      let x = (tx * TILE_SIZE as i32 + TILE_SIZE as i32 / 2) as f32;
      let y = (ty * TILE_SIZE as i32 + TILE_SIZE as i32) as f32;

      ctx.world.spawn((
        Transform::new(x, y).with_anchor(Anchor::BottomCenter),
        Renderable::sprite(bush_handle.clone()).with_z_order(0),
        self.get_y_sort(),
      ));
    }

    // Place rocks
    let rock_positions = [(6, 6), (11, 5), (3, 12), (16, 10)];

    for (tx, ty) in rock_positions {
      let x = (tx * TILE_SIZE as i32 + TILE_SIZE as i32 / 2) as f32;
      let y = (ty * TILE_SIZE as i32 + TILE_SIZE as i32) as f32;

      ctx.world.spawn((
        Transform::new(x, y).with_anchor(Anchor::BottomCenter),
        Renderable::sprite(rock_handle.clone()).with_z_order(0),
        self.get_y_sort(),
      ));
    }

    // Create wandering NPCs
    let npc_configs = [
      ((200, 80, 80), (48.0, 80.0)),    // Red NPC
      ((80, 180, 80), (240.0, 100.0)),  // Green NPC
      ((200, 180, 80), (160.0, 180.0)), // Yellow NPC
    ];

    for (i, ((r, g, b), (x, y))) in npc_configs.iter().enumerate() {
      let npc_sprite = Self::create_character_sprite(*r, *g, *b);
      let npc_handle = ctx.assets.create_sprite(npc_sprite, format!("npc_{}", i));

      // NPC shadow
      let shadow = ctx.world.spawn((
        Transform::new(*x, *y + 2.0).with_anchor(Anchor::Center),
        Renderable::sprite(shadow_handle.clone()).with_z_order(-1),
      ));

      // NPC entity
      let entity = ctx.world.spawn((
        Wanderer {
          speed: 25.0 + (i as f32 * 5.0),
          direction: (1.0, 0.0),
          change_timer: 0.0,
          shadow,
        },
        Transform::new(*x, *y).with_anchor(Anchor::BottomCenter),
        Renderable::sprite(npc_handle).with_z_order(0),
        self.get_y_sort(),
      ));

      self.wanderers.push(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Depth scaling changed in the settings panel
    if self.depth_scale_enabled != self.applied_depth_scale {
      self.applied_depth_scale = self.depth_scale_enabled;

      // Update all YSort components
      let new_y_sort = self.get_y_sort();

      // Get all entities with YSort component
      let entities_to_update = ctx.world.entities_with::<YSort>();

      for entity in entities_to_update {
        if let Some(mut ys) = ctx.world.get_mut::<YSort>(entity) {
          *ys = new_y_sort.clone();
        }
      }
    }

    // Player movement
    let speed = 60.0 * dt;
    let mut player_x = 0.0;
    let mut player_y = 0.0;

    if let Some(player) = self.player
      && let Some(transform) = ctx.world.get::<Transform>(player)
    {
      player_x = transform.x;
      player_y = transform.y;
    }

    let mut dx = 0.0;
    let mut dy = 0.0;

    if ctx.input.key_down(KeyCode::KeyA) {
      dx -= speed;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      dx += speed;
    }
    if ctx.input.key_down(KeyCode::KeyW) {
      dy -= speed;
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      dy += speed;
    }

    // Normalize diagonal movement
    if dx != 0.0 && dy != 0.0 {
      let factor = 0.707; // 1/sqrt(2)
      dx *= factor;
      dy *= factor;
    }

    player_x += dx;
    player_y += dy;

    // Clamp player position
    player_x = player_x.clamp(8.0, GAME_WIDTH as f32 - 8.0);
    player_y = player_y.clamp(PLAY_AREA_TOP + 8.0, PLAY_AREA_BOTTOM);

    // Update player position
    if let Some(player) = self.player
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(player)
    {
      transform.x = player_x;
      transform.y = player_y;
    }

    // Update player shadow position
    if let Some(shadow) = self.player_shadow
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(shadow)
    {
      transform.x = player_x;
      transform.y = player_y + 2.0;
    }

    // Update wandering NPCs
    for &wanderer in &self.wanderers {
      // Get current state
      let (x, y, dir, timer, speed, shadow) = {
        let transform = ctx.world.get::<Transform>(wanderer);
        let wander = ctx.world.get::<Wanderer>(wanderer);

        if let (Some(t), Some(w)) = (transform, wander) {
          (t.x, t.y, w.direction, w.change_timer, w.speed, w.shadow)
        } else {
          continue;
        }
      };

      // Update timer and possibly change direction
      let mut new_timer = timer + dt;
      let mut new_dir = dir;

      if new_timer > 2.0 {
        new_timer = 0.0;
        // Pick a new random direction (using simple pseudo-random)
        let rand_val = ((x * 17.0 + y * 31.0 + timer * 100.0) as i32 % 8) as f32;
        new_dir = match rand_val as i32 {
          0 => (1.0, 0.0),
          1 => (-1.0, 0.0),
          2 => (0.0, 1.0),
          3 => (0.0, -1.0),
          4 => (0.707, 0.707),
          5 => (-0.707, 0.707),
          6 => (0.707, -0.707),
          _ => (-0.707, -0.707),
        };
      }

      // Move
      let mut new_x = x + new_dir.0 * speed * dt;
      let mut new_y = y + new_dir.1 * speed * dt;

      // Bounce off edges
      if new_x < 16.0 || new_x > GAME_WIDTH as f32 - 16.0 {
        new_dir.0 = -new_dir.0;
        new_x = new_x.clamp(16.0, GAME_WIDTH as f32 - 16.0);
      }
      if !(PLAY_AREA_TOP + 8.0..=PLAY_AREA_BOTTOM).contains(&new_y) {
        new_dir.1 = -new_dir.1;
        new_y = new_y.clamp(PLAY_AREA_TOP + 8.0, PLAY_AREA_BOTTOM);
      }

      // Update wanderer state
      if let Some(mut w) = ctx.world.get_mut::<Wanderer>(wanderer) {
        w.direction = new_dir;
        w.change_timer = new_timer;
      }

      if let Some(mut t) = ctx.world.get_mut::<Transform>(wanderer) {
        t.x = new_x;
        t.y = new_y;
      }
      if let Some(mut t) = ctx.world.get_mut::<Transform>(shadow) {
        t.x = new_x;
        t.y = new_y + 2.0;
      }
    }
  }
}

fn main() {
  Demo::new("Y-Sorting")
    .resolution(
      GAME_WIDTH * SCREEN_ZOOM as u32,
      GAME_HEIGHT * SCREEN_ZOOM as u32,
    )
    .configure(|b| {
      b.window_mode(WindowMode::ResizableIntegerScale)
        .target_fps(60)
    })
    .page(YSortingPage::new)
    .run();
}
