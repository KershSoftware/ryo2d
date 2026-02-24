//! GPU Raycasting Demo
//!
//! Demonstrates pseudo-3D rendering using GPU-accelerated raycasting
//! (Wolfenstein/Build engine style).
//!
//! Controls:
//! - W/S: Move forward/backward
//! - A/D: Strafe left/right
//! - Arrow keys: Turn left/right
//! - Mouse: Look around
//! - E: Open nearby doors
//! - Escape: Quit

mod textures;

use std::path::Path;

use image::GenericImageView;
use ryo2d::prelude::*;

struct RaycastDemo {
  player_x: f32,
  player_y: f32,
  player_angle: f32,
  camera_entity: Option<Entity>,
  door_entities: Vec<Entity>,
  frame_count: u32,
  fps_timer: f32,
  fps_display: u32,
}

impl RaycastDemo {
  fn new() -> Self {
    Self {
      player_x: 4.5,
      player_y: 4.5,
      player_angle: 0.0,
      camera_entity: None,
      door_entities: Vec::new(),
      frame_count: 0,
      fps_timer: 0.0,
      fps_display: 0,
    }
  }
}

impl Scene for RaycastDemo {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Build a 32x32 map
    let mut world = RaycastWorld::new(32, 32);

    // Border walls (wall_texture = 1 for bricks)
    for x in 0..32 {
      *world.get_mut(x, 0).unwrap() = MapCell::wall(1);
      *world.get_mut(x, 31).unwrap() = MapCell::wall(1);
    }
    for y in 0..32 {
      *world.get_mut(0, y).unwrap() = MapCell::wall(1);
      *world.get_mut(31, y).unwrap() = MapCell::wall(1);
    }

    // Interior wall dividing the map (full height, with doorways)
    for y in 1..31 {
      *world.get_mut(15, y).unwrap() = MapCell::wall(2); // stone wall
    }
    // Clear doorway openings (empty cells the player walks through freely)
    *world.get_mut(15, 8).unwrap() = MapCell::default();
    *world.get_mut(15, 20).unwrap() = MapCell::default();
    // Door cells (block when closed, open with E key)
    {
      let cell = world.get_mut(15, 12).unwrap();
      cell.wall_texture = 2;
      cell.flags = MapFlags::DOOR;
    }

    // Some pillars
    *world.get_mut(8, 8).unwrap() = MapCell::wall(1);
    *world.get_mut(12, 12).unwrap() = MapCell::wall(2);

    // Thin walls -- semi-transparent dividers in the east room
    for x in 20..24 {
      let cell = world.get_mut(x, 15).unwrap();
      cell.wall_texture = 2;
      cell.flags = MapFlags::THIN_WALL;
    }

    // Set all floor textures to checker (texture 1) and ceiling to stone (texture 2)
    for y in 1..31 {
      for x in 1..31 {
        let cell = world.get_mut(x, y).unwrap();
        if cell.wall_texture == 0 {
          cell.floor_texture = 1; // checker floor
          cell.ceiling_texture = 2; // stone ceiling
        }
      }
    }

    // Graduated height variation -- steps instead of a uniform platform
    for y in 20..25 {
      for x in 3..8 {
        let height = match x {
          3..=4 => 0.1, // first step
          5 => 0.2,     // second step
          6..=7 => 0.3, // third step
          _ => 0.0,
        };
        let cell = world.get_mut(x, y).unwrap();
        cell.floor_height = height;
      }
    }

    // Pit area with negative floor height
    for y in 26..29 {
      for x in 5..9 {
        let cell = world.get_mut(x, y).unwrap();
        cell.floor_height = -0.2;
      }
    }

    // Dark zone with reduced lighting
    for y in 20..28 {
      for x in 20..28 {
        let cell = world.get_mut(x, y).unwrap();
        cell.light_level = 0.2;
      }
    }

    // Set up the raycast renderer
    let rc = ctx.raycast_renderer().unwrap();
    rc.set_world(world);
    rc.set_map_info(MapInfoUniform {
      dimensions: [32, 32],
      max_distance: 32.0,
      fog_density: 0.04,
      fog_color: [0.05, 0.05, 0.1],
      _pad: 0.0,
    });

    // Load textures via TextureManager
    // Note: the raycasting shader subtracts 1 from texture indices (0 = no texture),
    // so map index 1 → array layer 0, map index 2 → array layer 1, etc.
    let (wall_array, floor_array, sprite_array) = {
      let mut tex = ctx.textures();

      // Wall textures: map index 1 = exterior bricks, 2 = interior bricks
      let wall_handles = vec![
        tex.load(Path::new("assets/wall_brick_exterior.png")),
        tex.load(Path::new("assets/wall_brick_interior.png")),
      ];
      let wall_array = tex.create_array(&wall_handles);

      // Floor/ceiling textures: map index 1 = wood floor, 2 = metal ceiling
      let floor_handles = vec![
        tex.load(Path::new("assets/floor_wood.png")),
        tex.load(Path::new("assets/ceiling_metal.png")),
      ];
      let floor_array = tex.create_array(&floor_handles);

      // Sprite textures: index 0 = barrel (rotated wood planks), 1 = torch
      let barrel_img = image::open("assets/sprite_barrel.png")
        .expect("Failed to load barrel texture")
        .rotate90();
      let (bw, bh) = barrel_img.dimensions();
      let barrel_data = barrel_img.into_rgba8().into_raw();

      let sprite_handles = vec![
        tex.from_rgba(barrel_data, bw, bh),
        tex.from_rgba(textures::torch([100, 70, 40], [255, 180, 50], bw), bw, bh),
      ];
      let sprite_array = tex.create_array(&sprite_handles);

      (wall_array, floor_array, sprite_array)
    };

    // Assign texture arrays to raycast renderer
    let rc = ctx.raycast_renderer().unwrap();
    rc.set_wall_textures(wall_array);
    rc.set_floor_textures(floor_array);
    rc.set_sprite_textures(sprite_array);

    // Enable raycasting
    ctx.enable_raycast();

    // Lock cursor for FPS-style mouse look
    ctx.set_cursor_locked(true);

    // Spawn camera entity
    let camera_entity = ctx.world.spawn((
      Transform::new(self.player_x, self.player_y),
      RaycastCamera::new(66.0),
    ));
    self.camera_entity = Some(camera_entity);

    // Spawn some sprite entities (barrels - half cell height, grounded on floor)
    let barrel = |x: f32, y: f32| -> (Transform, RaycastSprite) {
      let mut s = RaycastSprite::new(0);
      s.scale = (0.5, 0.5);
      s.v_offset = -0.25; // shift down so bottom sits on floor
      (Transform::new(x, y), s)
    };
    ctx.world.spawn(barrel(10.0, 5.0));
    ctx.world.spawn(barrel(20.0, 5.0));
    ctx.world.spawn(barrel(6.0, 18.0));

    // Spawn lights in the dark zone
    ctx.world.spawn((
      Transform::new(23.0, 23.0),
      RaycastLight::new(5.0, (1.0, 0.78, 0.39), 1.5),
    ));
    ctx.world.spawn((
      Transform::new(25.0, 25.0),
      RaycastLight::new(4.0, (0.39, 0.59, 1.0), 1.0),
    ));

    // Spawn torch sprites near the lights in the dark zone
    ctx
      .world
      .spawn((Transform::new(23.0, 23.0), RaycastSprite::new(1)));
    ctx
      .world
      .spawn((Transform::new(25.0, 25.0), RaycastSprite::new(1)));

    // Spawn door entity (y=12 has a door, y=8 and y=20 are open doorways)
    let door1 = ctx
      .world
      .spawn((RaycastDoor::new(15, 12, DoorSpeed::Normal),));
    self.door_entities = vec![door1];
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Track FPS
    self.frame_count += 1;
    self.fps_timer += dt;
    if self.fps_timer >= 1.0 {
      self.fps_display = self.frame_count;
      self.frame_count = 0;
      self.fps_timer -= 1.0;
    }

    let move_speed = 3.0 * dt;
    let turn_speed = 2.0 * dt;

    // Mouse look
    let (mouse_dx, _) = ctx.input.mouse_delta();
    self.player_angle += mouse_dx * 0.003;

    // Keyboard turning
    if ctx.input.key_down(KeyCode::ArrowLeft) {
      self.player_angle -= turn_speed;
    }
    if ctx.input.key_down(KeyCode::ArrowRight) {
      self.player_angle += turn_speed;
    }

    let dir_x = self.player_angle.cos();
    let dir_y = self.player_angle.sin();

    // Accumulate movement intent
    let mut move_x = 0.0_f32;
    let mut move_y = 0.0_f32;

    // Forward/back
    if ctx.input.key_down(KeyCode::KeyW) {
      move_x += dir_x * move_speed;
      move_y += dir_y * move_speed;
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      move_x -= dir_x * move_speed;
      move_y -= dir_y * move_speed;
    }

    // Strafe
    if ctx.input.key_down(KeyCode::KeyA) {
      move_x += dir_y * move_speed;
      move_y -= dir_x * move_speed;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      move_x -= dir_y * move_speed;
      move_y += dir_x * move_speed;
    }

    // Bounding-box collision detection with wall sliding
    let margin = 0.25;
    let new_x = self.player_x + move_x;
    let new_y = self.player_y + move_y;
    let mut can_move_x = true;
    let mut can_move_y = true;

    if let Some(rc) = ctx.raycast_renderer()
      && let Some(world) = rc.world()
    {
      let is_blocked = |cx: f32, cy: f32| -> bool {
        if cx < 0.0 || cy < 0.0 {
          return true;
        }
        match world.get(cx as usize, cy as usize) {
          None => true, // out of bounds = solid
          Some(cell) => {
            cell.wall_texture != 0
              && !cell.flags.contains(MapFlags::THIN_WALL)
              && !(cell.flags.contains(MapFlags::DOOR) && cell.door_state > 0.8)
          }
        }
      };

      // Check X movement (new_x, old_y)
      let corners_x = [
        (new_x - margin, self.player_y - margin),
        (new_x + margin, self.player_y - margin),
        (new_x - margin, self.player_y + margin),
        (new_x + margin, self.player_y + margin),
      ];
      for &(cx, cy) in &corners_x {
        if is_blocked(cx, cy) {
          can_move_x = false;
          break;
        }
      }
      // Check Y movement (old_x, new_y)
      let corners_y = [
        (self.player_x - margin, new_y - margin),
        (self.player_x + margin, new_y - margin),
        (self.player_x - margin, new_y + margin),
        (self.player_x + margin, new_y + margin),
      ];
      for &(cx, cy) in &corners_y {
        if is_blocked(cx, cy) {
          can_move_y = false;
          break;
        }
      }
    }

    if can_move_x {
      self.player_x = new_x;
    }
    if can_move_y {
      self.player_y = new_y;
    }

    // E key: activate nearby doors
    if ctx.input.key_pressed(KeyCode::KeyE) {
      let interact_range = 2.0_f32;
      for &door_entity in &self.door_entities {
        if let Some(mut door) = ctx.world.get_mut::<RaycastDoor>(door_entity) {
          let dx = self.player_x - (door.map_x as f32 + 0.5);
          let dy = self.player_y - (door.map_y as f32 + 0.5);
          if dx * dx + dy * dy < interact_range * interact_range {
            door.activate();
          }
        }
      }
    }

    // Tick door animations and sync to world map
    {
      let mut doors_data: Vec<(RaycastDoor,)> = Vec::new();
      for &door_entity in &self.door_entities {
        if let Some(door) = ctx.world.get_mut::<RaycastDoor>(door_entity) {
          doors_data.push((door.clone(),));
        }
      }

      if let Some(rc) = ctx.raycast_renderer()
        && let Some(world) = rc.world_mut()
      {
        door_system(&mut doors_data, world, dt);
      }

      // Write updated door state back to ECS
      for (i, &door_entity) in self.door_entities.iter().enumerate() {
        if let Some(mut door) = ctx.world.get_mut::<RaycastDoor>(door_entity) {
          *door = doors_data[i].0.clone();
        }
      }
    }

    // Update camera entity transform and angle
    if let Some(entity) = self.camera_entity {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = self.player_x;
        transform.y = self.player_y;
      }
      if let Some(mut camera) = ctx.world.get_mut::<RaycastCamera>(entity) {
        camera.set_angle(self.player_angle);
      }
    }

    // Escape to quit (release cursor first)
    if ctx.input.key_pressed(KeyCode::Escape) {
      ctx.set_cursor_locked(false);
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (w, h) = ui.screen_size();
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;

    // Crosshair
    let size = 8.0;
    let thickness = 1.0;
    let crosshair_color = Color::new(200, 200, 200, 180);
    // Horizontal line
    ui.draw_rect(
      Rect::new(cx - size, cy - thickness / 2.0, size * 2.0, thickness),
      crosshair_color,
    );
    // Vertical line
    ui.draw_rect(
      Rect::new(cx - thickness / 2.0, cy - size, thickness, size * 2.0),
      crosshair_color,
    );

    // FPS and position display
    let text_color = Color::new(255, 255, 255, 220);
    ui.draw_text(&format!("FPS: {}", self.fps_display), 8.0, 8.0, text_color);
    ui.draw_text(
      &format!(
        "Pos: ({:.1}, {:.1})  Angle: {:.1}°",
        self.player_x,
        self.player_y,
        self.player_angle.to_degrees()
      ),
      8.0,
      22.0,
      text_color,
    );

    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Raycasting Demo")
    .resolution(1920, 1080)
    .target_fps(60)
    .start_scene(RaycastDemo::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
