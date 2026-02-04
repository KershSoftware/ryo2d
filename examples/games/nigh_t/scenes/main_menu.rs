//! Main Menu Scene
//!
//! A dark screen with a glowing fairy cursor that follows the mouse.
//! Hold mouse to charge - fairy gets brighter but more unstable.

use ryo2d::prelude::*;

// Default (uncharged) state
const BASE_RADIUS: f32 = 50.0;
const BASE_INTENSITY: f32 = 0.4;

// Charged state (max)
const MAX_RADIUS: f32 = 350.0;
const MAX_INTENSITY: f32 = 2.2;

// Charge/decay rates
const CHARGE_RATE: f32 = 0.25; // Moderate charge buildup
const DECAY_RATE: f32 = 3.0; // Fast snap back when released

/// Main menu scene - atmospheric dark screen with fairy cursor
pub struct MainMenuScene {
  fairy_entity: Option<Entity>,
  /// Secondary glow lights for organic irregular shape
  glow_entities: [Option<Entity>; 3],
  time: f32,
  mouse_x: f32,
  mouse_y: f32,
  /// Charge level from holding mouse (0.0 to 1.0)
  charge: f32,
  /// Random seed for flickering
  flicker_time: f32,
}

impl MainMenuScene {
  pub fn new() -> Self {
    Self {
      fairy_entity: None,
      glow_entities: [None, None, None],
      time: 0.0,
      mouse_x: 0.0,
      mouse_y: 0.0,
      charge: 0.0,
      flicker_time: 0.0,
    }
  }

  /// Simple pseudo-random based on time (returns 0.0 to 1.0)
  fn random(&self, seed: f32) -> f32 {
    let x = (self.flicker_time * seed * 12.9898).sin() * 43_758.547;
    x.fract().abs()
  }

  /// Create a tiny bright core sprite (no transparency to avoid artifacts)
  fn create_core_sprite(assets: &mut AssetManager) -> SpriteHandle {
    let size = 6u32;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;

    for y in 0..size {
      for x in 0..size {
        let idx = ((y * size + x) * 4) as usize;
        let dx = x as f32 - center + 0.5;
        let dy = y as f32 - center + 0.5;
        let dist = (dx * dx + dy * dy).sqrt();

        // Tight bright core - fully opaque, no semi-transparent edges
        let max_dist = center;
        if dist < max_dist {
          let intensity = 1.0 - (dist / max_dist).powi(2);
          // Bright white-cyan core
          let r = (200.0 + 55.0 * intensity) as u8;
          let g = (220.0 + 35.0 * intensity) as u8;
          let b = 255u8;
          // Fully opaque in the center, fade only at very edge
          let a = if dist < max_dist * 0.7 {
            255
          } else {
            (intensity * 255.0) as u8
          };

          data[idx] = r;
          data[idx + 1] = g;
          data[idx + 2] = b;
          data[idx + 3] = a;
        }
      }
    }

    let sprite = Sprite::from_rgba(data, size, size);
    assets.create_sprite(sprite, "fairy_core")
  }
}

impl Default for MainMenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MainMenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Set ambient light to zero for pitch black darkness
    if let Some(lighting) = ctx.lighting {
      lighting.ambient_mut().set_intensity(0.0);
    }

    // Enable CRT post-processing effect
    ctx.effects.enable_crt();

    // Hide the system cursor
    ctx.set_cursor_visible(false);

    // Get screen center for initial position
    let (width, height) = ctx.screen_size();
    let center_x = width / 2.0;
    let center_y = height / 2.0;
    self.mouse_x = center_x;
    self.mouse_y = center_y;

    // Spawn the fairy - tiny bright core sprite with light
    // Use softer falloff (1.5) for more natural look
    let core_sprite = Self::create_core_sprite(ctx.assets);
    let light_color = Color::rgb(180, 220, 255);
    self.fairy_entity = Some(
      ctx.world.spawn((
        Transform::new(center_x, center_y).with_pivot_center(),
        Renderable::sprite(core_sprite).with_z_order(100),
        Light2D::point(light_color, BASE_RADIUS)
          .with_intensity(BASE_INTENSITY)
          .with_falloff(1.5),
      )),
    );

    // Spawn secondary glow lights that orbit around for organic irregular shape
    let glow_colors = [
      Color::rgb(150, 200, 255), // Slightly different blue
      Color::rgb(200, 230, 255), // Whiter
      Color::rgb(170, 210, 240), // Another variation
    ];
    for (i, color) in glow_colors.iter().enumerate() {
      self.glow_entities[i] = Some(
        ctx.world.spawn((
          Transform::new(center_x, center_y),
          Light2D::point(*color, BASE_RADIUS * 0.6)
            .with_intensity(BASE_INTENSITY * 0.4)
            .with_falloff(1.2),
        )),
      );
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.time += dt;
    self.flicker_time += dt * 60.0; // Fast flicker timer

    // Get mouse position
    let (mx, my) = ctx.input.mouse_position();
    self.mouse_x = mx;
    self.mouse_y = my;

    // Handle charging - mouse held increases charge (with some resistance)
    if ctx.input.mouse_down(MouseButton::Left) {
      // Base charge rate with slight diminishing returns at high charge
      let resistance = 1.0 - self.charge * 0.3;
      let charge_amount = CHARGE_RATE * resistance * dt;

      // Occasional stutter/resistance - feels like pushing through
      let fight_back = self.random(5.0);
      if fight_back < 0.03 && self.charge > 0.2 {
        // Rare small pushback at higher charges
        self.charge = (self.charge - 0.02).max(0.0);
      } else if fight_back < 0.10 {
        // Stutter - reduced charge this frame
        self.charge = (self.charge + charge_amount * 0.3).min(1.0);
      } else {
        self.charge = (self.charge + charge_amount).min(1.0);
      }
    } else {
      // Fast snap back when released
      self.charge = (self.charge - DECAY_RATE * dt).max(0.0);
    }

    // Update fairy
    if let Some(entity) = self.fairy_entity {
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        // Base gentle bob (reduces as charge increases - fairy getting unstable)
        let bob_stability = 1.0 - self.charge * 0.8;
        let bob_x = (self.time * 4.0).sin() * 2.0 * bob_stability;
        let bob_y = (self.time * 5.0).cos() * 1.5 * bob_stability;

        // Erratic movement increases with charge - starts subtle, gets wild
        let charge_sq = self.charge * self.charge;
        let charge_cube = charge_sq * self.charge;

        // Low frequency wobble (gets stronger with charge)
        let wobble_x = (self.time * 8.0 + self.random(1.5) * 3.0).sin() * charge_sq * 8.0;
        let wobble_y = (self.time * 7.0 + self.random(2.5) * 3.0).cos() * charge_sq * 6.0;

        // High frequency twitch (really kicks in at high charge)
        let twitch_x = (self.random(1.0) - 0.5) * charge_cube * 25.0;
        let twitch_y = (self.random(2.0) - 0.5) * charge_cube * 25.0;

        // Occasional violent jerk at high charge
        let jerk_chance = self.random(6.0);
        let jerk_x = if jerk_chance < 0.02 * charge_sq {
          (self.random(7.0) - 0.5) * 40.0
        } else {
          0.0
        };
        let jerk_y = if jerk_chance < 0.02 * charge_sq {
          (self.random(8.0) - 0.5) * 40.0
        } else {
          0.0
        };

        transform.x = self.mouse_x + bob_x + wobble_x + twitch_x + jerk_x;
        transform.y = self.mouse_y + bob_y + wobble_y + twitch_y + jerk_y;

        // Rotation gets unstable at high charge
        transform.rotation =
          (self.time * 3.0).sin() * charge_sq * 0.3 + (self.random(9.0) - 0.5) * charge_cube * 0.5;
      }

      // Update light intensity and radius
      if let Some(mut light) = ctx.world.get_component_mut::<Light2D>(entity) {
        // Erratic flickering - more intense when uncharged, smoother when charged
        let flicker_base = self.random(3.0);
        let flicker_strength = 0.3 * (1.0 - self.charge * 0.7); // Less flicker when charged
        let flicker = 1.0 + (flicker_base - 0.5) * flicker_strength;

        // Occasionally blink almost off (only when not charging much)
        let blink_chance = self.random(4.0);
        let blink = if blink_chance < 0.02 * (1.0 - self.charge) {
          0.3 // Dim blink
        } else {
          1.0
        };

        // Calculate intensity based on charge
        let target_intensity = BASE_INTENSITY + (MAX_INTENSITY - BASE_INTENSITY) * self.charge;
        let final_intensity = target_intensity * flicker * blink;
        light.set_intensity(final_intensity);

        // Calculate radius based on charge with some organic variation
        let radius_wobble = (self.time * 2.3).sin() * 0.1 + (self.time * 5.7).cos() * 0.05;
        let target_radius = BASE_RADIUS + (MAX_RADIUS - BASE_RADIUS) * self.charge;
        light.set_radius(target_radius * (1.0 + radius_wobble));
      }
    }

    // Update secondary glow lights - orbit around fairy for organic shape
    // Orbit visibility scales with charge - subtle at rest, pronounced when charging
    let fairy_x = self.mouse_x;
    let fairy_y = self.mouse_y;
    let orbit_speeds = [1.7, 2.3, 3.1]; // Different speeds
    let base_orbit_distances = [3.0, 4.0, 3.5]; // Tiny at rest
    let max_orbit_distances = [15.0, 22.0, 18.0]; // Full orbit when charged

    for (i, glow_entity) in self.glow_entities.iter().enumerate() {
      if let Some(entity) = glow_entity {
        let speed = orbit_speeds[i];

        // Orbit distance scales with charge - barely visible at rest
        let min_dist = base_orbit_distances[i];
        let max_dist = max_orbit_distances[i];
        let dist = min_dist + (max_dist - min_dist) * self.charge + self.charge * 25.0;

        // Wobble also scales with charge
        let angle = self.time * speed + (i as f32) * 2.1;
        let wobble_strength = self.charge * 8.0; // No wobble at rest
        let wobble = (self.time * (speed * 1.7) + i as f32).sin() * wobble_strength;

        let offset_x = angle.cos() * (dist + wobble);
        let offset_y = angle.sin() * (dist + wobble * 0.7);

        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(*entity) {
          transform.x = fairy_x + offset_x;
          transform.y = fairy_y + offset_y;
        }

        if let Some(mut light) = ctx.world.get_component_mut::<Light2D>(*entity) {
          // Secondary lights also scale with charge
          let base_intensity = BASE_INTENSITY * 0.4;
          let max_intensity = MAX_INTENSITY * 0.5;
          let glow_intensity = base_intensity + (max_intensity - base_intensity) * self.charge;

          // Individual flicker for each glow
          let glow_flicker = 0.8 + self.random(10.0 + i as f32) * 0.4;
          light.set_intensity(glow_intensity * glow_flicker);

          let base_radius = BASE_RADIUS * 0.6;
          let max_radius = MAX_RADIUS * 0.5;
          light.set_radius(base_radius + (max_radius - base_radius) * self.charge);
        }
      }
    }

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Title text
    ui.label(
      "NIGH_T",
      10.0,
      10.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 180)),
    );

    ui.label("Hold mouse to charge light", 10.0, 40.0, Color::GRAY, None);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    // Restore cursor visibility
    ctx.set_cursor_visible(true);

    // Clean up fairy entity
    if let Some(entity) = self.fairy_entity.take() {
      let _ = ctx.world.despawn(entity);
    }

    // Clean up secondary glow entities
    for glow_entity in &mut self.glow_entities {
      if let Some(entity) = glow_entity.take() {
        let _ = ctx.world.despawn(entity);
      }
    }
  }
}
