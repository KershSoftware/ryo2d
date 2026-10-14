//! Knockback Demo - Ryo2D Example
//!
//! This example demonstrates the knockback system for directional force effects:
//! - Different decay rates (fast, normal, slow)
//! - Accumulation behavior (blending vs replacing)
//! - Plain Transform entities, dynamic crates (the push is one change in velocity, so
//!   crates of different mass leave at the same speed) and characters (swept by the
//!   push, so walls stop it)
//! - Messages (KnockbackStarted, KnockbackFinished)
//!
//! Runs on the demo-kit shell; the force is a setting.

use demo_kit::prelude::*;

/// Marker for simple entities (no physics)
#[derive(Component, Debug)]
struct SimpleEntity;

/// Marker for physics entities (crates and characters)
#[derive(Component, Debug)]
struct PhysicsEntity;

/// Gravity the page adds to its characters' velocity, in px/s²
const GRAVITY: f32 = 980.0;

/// Tracks entity flash state for visual feedback
#[derive(Debug, Default)]
struct FlashState {
  timer: f32,
}

/// Main demo scene
struct KnockbackDemoScene {
  /// Simple entities (top row)
  simple_entities: Vec<Entity>,
  /// Physics entities (bottom row): two crates, then two characters
  physics_entities: Vec<Entity>,
  /// The characters among them
  characters: Vec<Entity>,
  /// Current force setting
  force: f32,
  /// Flash states for entities
  flash_states: std::collections::HashMap<Entity, FlashState>,
  /// Rapid fire cooldown
  rapid_fire_timer: f32,
}

impl KnockbackDemoScene {
  fn new() -> Self {
    Self {
      simple_entities: Vec::new(),
      physics_entities: Vec::new(),
      characters: Vec::new(),
      force: 400.0,
      flash_states: std::collections::HashMap::new(),
      rapid_fire_timer: 0.0,
    }
  }

  /// Create a colored square sprite
  fn create_square(size: u32, r: u8, g: u8, b: u8) -> SpriteHandle {
    let sprite = Sprite::solid_rect(size, size, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("square_{}_{}_{}_{}", size, r, g, b))
  }

  /// Spawn entities in a 4x2 grid
  fn spawn_entities(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    let spacing_x = screen_w / 5.0;
    let y_top = screen_h * 0.35;
    let y_bottom = screen_h * 0.65;

    // Column labels (decay rates)
    let columns = ["Fast (8.0)", "Normal (4.0)", "Slow (2.0)", "No Accum"];

    for (col, _label) in columns.iter().enumerate() {
      let x = spacing_x * (col as f32 + 1.0);

      // Top row: Simple entities (Transform only)
      let simple_sprite = Self::create_square(40, 100, 180, 255); // Blue
      let simple_entity = ctx
        .world
        .spawn((
          SimpleEntity,
          Transform::from_xy(x, y_top),
          Renderable::sprite(simple_sprite).with_z_order(10),
        ))
        .id();
      self.simple_entities.push(simple_entity);

      // Bottom row: two dynamic crates of different density, then two
      // characters, each with a pillar just to its right
      let physics_sprite = Self::create_square(40, 255, 140, 100); // Orange
      let character_sprite = Self::create_square(40, 120, 230, 120); // Green
      let physics_entity = if col < 2 {
        let density = if col == 0 { 0.5 } else { 4.0 };
        ctx
          .world
          .spawn((
            PhysicsEntity,
            Transform::from_xy(x, y_bottom),
            Renderable::sprite(physics_sprite).with_z_order(10),
            RigidBody::dynamic()
              .lock_rotation()
              .with_linear_damping(4.0),
            Collider::rect(40.0, 40.0).with_density(density),
          ))
          .id()
      } else {
        Self::spawn_wall(ctx, x + 46.0, y_bottom + 30.0, 12, 60);
        let character = ctx
          .world
          .spawn((
            PhysicsEntity,
            Transform::from_xy(x, y_bottom),
            Renderable::sprite(character_sprite).with_z_order(10),
            CharacterController::platformer(),
            Collider::rect(40.0, 40.0),
          ))
          .id();
        self.characters.push(character);
        character
      };
      self.physics_entities.push(physics_entity);
    }

    // The floor under the bottom row
    Self::spawn_wall(ctx, screen_w / 2.0, y_bottom + 80.0, screen_w as u32, 20);
  }

  /// A fixed wall of `w` x `h` centred at (x, y)
  fn spawn_wall(ctx: &mut SceneContext, x: f32, y: f32, w: u32, h: u32) {
    let sprite = Sprite::solid_rect(w, h, 90, 90, 110, 255);
    let handle = SpriteHandle::from_sprite(sprite, format!("wall_{w}_{h}"));
    ctx.world.spawn((
      Transform::from_xy(x, y),
      Renderable::sprite(handle).with_z_order(5),
      Collider::rect(w as f32, h as f32),
    ));
  }

  /// Apply knockback to a specific column
  fn knockback_column(
    &self,
    ctx: &mut SceneContext,
    column: usize,
    decay_rate: f32,
    accumulate: bool,
  ) {
    // Direction: push right
    let direction = Vec2::new(1.0, 0.0);

    // Apply to simple entity in this column
    if let Some(&entity) = self.simple_entities.get(column) {
      self.apply_knockback_to_entity(ctx, entity, direction, decay_rate, accumulate);
    }

    // Apply to physics entity in this column
    if let Some(&entity) = self.physics_entities.get(column) {
      self.apply_knockback_to_entity(ctx, entity, direction, decay_rate, accumulate);
    }
  }

  /// Helper to apply knockback to a single entity
  fn apply_knockback_to_entity(
    &self,
    ctx: &mut SceneContext,
    entity: Entity,
    direction: Vec2,
    decay_rate: f32,
    accumulate: bool,
  ) {
    if accumulate {
      Knockback::apply_with_decay(ctx.world, entity, direction, self.force, decay_rate);
    } else if let Some(mut kb) = ctx.world.get_mut::<Knockback>(entity) {
      // Non-accumulating: a new hit replaces the active knockback
      kb.accumulate = false;
      kb.push_with_decay(direction, self.force, decay_rate);
    } else {
      ctx.world.entity_mut(entity).insert(
        Knockback::new(direction, self.force)
          .with_decay_rate(decay_rate)
          .with_accumulate(false),
      );
    }
  }

  /// Apply radial knockback from screen center to all entities
  fn knockback_radial(&self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    let center_x = screen_w / 2.0;
    let center_y = screen_h / 2.0;

    // First, collect entity positions to avoid borrow conflicts
    let entity_positions: Vec<(Entity, f32, f32)> = self
      .simple_entities
      .iter()
      .chain(self.physics_entities.iter())
      .filter_map(|&entity| {
        ctx
          .world
          .get::<Transform>(entity)
          .map(|t| (entity, t.translation.x, t.translation.y))
      })
      .collect();

    // Now apply knockback
    for (entity, x, y) in entity_positions {
      let dx = x - center_x;
      let dy = y - center_y;
      let direction = Vec2::new(dx, dy);
      Knockback::apply_with_decay(ctx.world, entity, direction, self.force, 4.0);
    }
  }

  /// Update flash states and apply visual feedback
  fn update_flashes(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Flash entities whose knockback just started
    for event in ctx.messages::<KnockbackStarted>() {
      self
        .flash_states
        .insert(event.entity, FlashState { timer: 0.15 });
    }

    // Update flash timers
    let mut finished = Vec::new();
    for (entity, state) in &mut self.flash_states {
      state.timer -= dt;
      if state.timer <= 0.0 {
        finished.push(*entity);
      }
    }

    // Remove finished flashes
    for entity in finished {
      self.flash_states.remove(&entity);
    }

    // Apply flash effect (tint white when flashing)
    for &entity in self
      .simple_entities
      .iter()
      .chain(self.physics_entities.iter())
    {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
        if self.flash_states.contains_key(&entity) {
          renderable.tint = Color::rgb(255, 255, 200); // Flash yellow-white
        } else {
          renderable.tint = Color::WHITE; // Normal
        }
      }
    }
  }
}

impl DemoPage for KnockbackDemoScene {
  fn name(&self) -> &str {
    "Knockback"
  }

  fn description(&self) -> &str {
    "Knockback on plain entities, crates of any mass, and characters walls stop"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Force", &mut self.force, 100.0..=1000.0, 50.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "1-4",
        &[
          KeyCode::Digit1,
          KeyCode::Digit2,
          KeyCode::Digit3,
          KeyCode::Digit4,
        ],
        "Knock a column",
      ),
      Control::key(KeyCode::Space, "Radial knockback"),
      Control::key(KeyCode::KeyA, "Rapid fire (hold)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.spawn_entities(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Gravity for the characters: their Velocity is theirs to drive
    for &character in &self.characters {
      if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(character) {
        velocity.dy += GRAVITY * dt;
      }
    }

    // Column 1: Fast decay (8.0)
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.knockback_column(ctx, 0, 8.0, true);
      println!("Fast decay knockback applied (decay: 8.0)");
    }

    // Column 2: Normal decay (4.0)
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.knockback_column(ctx, 1, 4.0, true);
      println!("Normal decay knockback applied (decay: 4.0)");
    }

    // Column 3: Slow decay (2.0)
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.knockback_column(ctx, 2, 2.0, true);
      println!("Slow decay knockback applied (decay: 2.0)");
    }

    // Column 4: No accumulation
    if ctx.input.key_pressed(KeyCode::Digit4) {
      self.knockback_column(ctx, 3, 4.0, false);
      println!("Non-accumulating knockback applied");
    }

    // Radial knockback from center
    if ctx.input.key_pressed(KeyCode::Space) {
      self.knockback_radial(ctx);
      println!("Radial knockback from center!");
    }

    // Rapid fire knockback (hold A)
    if ctx.input.key_down(KeyCode::KeyA) {
      self.rapid_fire_timer -= dt;
      if self.rapid_fire_timer <= 0.0 {
        // Apply rapid knockback to all columns
        for col in 0..4 {
          self.knockback_column(ctx, col, 4.0, true);
        }
        self.rapid_fire_timer = 0.08; // Fire every 80ms
      }
    } else {
      self.rapid_fire_timer = 0.0;
    }

    // Update visual flash effects
    self.update_flashes(ctx, dt);

    // Log finished knockbacks
    for event in ctx.messages::<KnockbackFinished>() {
      // Check which type of entity
      if ctx.world.get::<SimpleEntity>(event.entity).is_some() {
        println!("Simple entity knockback finished");
      } else if ctx.world.get::<PhysicsEntity>(event.entity).is_some() {
        println!("Physics entity knockback finished");
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let text_light = Color::rgb(220, 220, 220);
    let text_muted = Color::rgb(160, 160, 160);
    let text_blue = Color::rgb(100, 180, 255);
    let text_orange = Color::rgb(255, 160, 100);

    let (screen_w, _) = ui.screen_size();

    // Column labels
    let spacing_x = screen_w as f32 / 5.0;
    let columns = ["Fast (8.0)", "Normal (4.0)", "Slow (2.0)", "No Accum"];

    for (col, label) in columns.iter().enumerate() {
      let x = spacing_x * (col as f32 + 1.0) - 35.0;
      ui.draw_text(label, x, area.y + 16.0, text_light);
    }

    // Row labels
    let y_top = ui.screen_size().1 as f32 * 0.35;
    let y_bottom = ui.screen_size().1 as f32 * 0.65;

    ui.draw_text("Simple", 10.0, y_top - 5.0, text_blue);
    ui.draw_text("(Transform)", 10.0, y_top + 10.0, text_muted);

    ui.draw_text("Physics", 10.0, y_bottom - 5.0, text_orange);
    ui.draw_text("(crates, characters)", 10.0, y_bottom + 10.0, text_muted);
    for (col, label) in ["Light crate", "Heavy crate", "Character", "Character"]
      .iter()
      .enumerate()
    {
      let x = spacing_x * (col as f32 + 1.0) - 35.0;
      ui.draw_text(label, x, y_bottom - 40.0, text_muted);
    }

    // Colour key, above the shell's legend
    let bottom = area.y + area.height;
    ui.draw_text(
      "Blue = Simple entities (direct Transform movement)",
      10.0,
      bottom - 44.0,
      text_blue,
    );
    ui.draw_text(
      "Orange = crates (one change in velocity), green = characters (swept, stopped by walls)",
      10.0,
      bottom - 24.0,
      text_orange,
    );
  }
}

fn main() {
  Demo::new("Knockback")
    .resolution(800, 600)
    .configure(|b| b.target_fps(60).fixed_hz(60))
    .page(KnockbackDemoScene::new)
    .run();
}
