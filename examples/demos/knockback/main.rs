//! Knockback Demo - Ryo2D Example
//!
//! This example demonstrates the knockback system for directional force effects:
//! - Different decay rates (fast, normal, slow)
//! - Accumulation behavior (blending vs replacing)
//! - Physics vs non-physics entities
//! - Events (KnockbackStarted, KnockbackFinished)
//!
//! ## Controls
//!
//! - 1: Fast decay knockback (decay_rate: 8.0)
//! - 2: Normal decay knockback (decay_rate: 4.0)
//! - 3: Slow decay knockback (decay_rate: 2.0)
//! - 4: No accumulation knockback (replaces existing)
//! - Space: Radial knockback from center (all entities)
//! - A: Rapid fire knockback (test accumulation)
//! - R: Reset all entities to starting positions
//! - Up/Down: Adjust force (default: 400)
//! - Escape: Exit

use ryo2d::prelude::*;

/// Marker for simple entities (no physics)
#[derive(Debug)]
struct SimpleEntity {
  start_x: f32,
  start_y: f32,
}

/// Marker for physics entities (with Velocity)
#[derive(Debug)]
struct PhysicsEntity {
  start_x: f32,
  start_y: f32,
}

/// Tracks entity flash state for visual feedback
#[derive(Debug, Default)]
struct FlashState {
  timer: f32,
}

/// Main demo scene
struct KnockbackDemoScene {
  /// Simple entities (top row)
  simple_entities: Vec<Entity>,
  /// Physics entities (bottom row)
  physics_entities: Vec<Entity>,
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
      let simple_entity = ctx.world.spawn((
        SimpleEntity {
          start_x: x,
          start_y: y_top,
        },
        Transform::new(x, y_top).with_pivot_center(),
        Renderable::sprite(simple_sprite).with_z_order(10),
      ));
      self.simple_entities.push(simple_entity);

      // Bottom row: Physics entities (with Velocity)
      let physics_sprite = Self::create_square(40, 255, 140, 100); // Orange
      let physics_entity = ctx.world.spawn((
        PhysicsEntity {
          start_x: x,
          start_y: y_bottom,
        },
        Transform::new(x, y_bottom).with_pivot_center(),
        Renderable::sprite(physics_sprite).with_z_order(10),
        Velocity::new(0.0, 0.0),
      ));
      self.physics_entities.push(physics_entity);
    }
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
      ctx.knockback_with_decay(entity, direction, self.force, decay_rate);
    } else {
      // For non-accumulating, check if entity has knockback first
      let has_knockback = ctx.world.has::<Knockback>(entity);
      if has_knockback {
        if let Some(mut kb) = ctx.world.get_mut::<Knockback>(entity) {
          kb.accumulate = false;
          kb.apply_with_decay(direction, self.force, decay_rate);
        }
      } else {
        ctx.world.add_component(
          entity,
          Knockback::new(direction, self.force)
            .with_decay_rate(decay_rate)
            .with_accumulate(false),
        );
      }
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
          .map(|t| (entity, t.x, t.y))
      })
      .collect();

    // Now apply knockback
    for (entity, x, y) in entity_positions {
      let dx = x - center_x;
      let dy = y - center_y;
      let direction = Vec2::new(dx, dy);
      ctx.knockback_with_decay(entity, direction, self.force, 4.0);
    }
  }

  /// Reset all entities to their starting positions
  fn reset_entities(&self, ctx: &mut SceneContext) {
    // Reset simple entities
    for &entity in &self.simple_entities {
      if let (Some(simple), Some(mut transform)) = (
        ctx.world.get::<SimpleEntity>(entity),
        ctx.world.get_mut::<Transform>(entity),
      ) {
        transform.x = simple.start_x;
        transform.y = simple.start_y;
      }
      // Stop any active knockback
      if let Some(mut kb) = ctx.world.get_mut::<Knockback>(entity) {
        kb.stop();
      }
    }

    // Reset physics entities
    for &entity in &self.physics_entities {
      if let (Some(physics), Some(mut transform)) = (
        ctx.world.get::<PhysicsEntity>(entity),
        ctx.world.get_mut::<Transform>(entity),
      ) {
        transform.x = physics.start_x;
        transform.y = physics.start_y;
      }
      // Reset velocity
      if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(entity) {
        velocity.dx = 0.0;
        velocity.dy = 0.0;
      }
      // Stop any active knockback
      if let Some(mut kb) = ctx.world.get_mut::<Knockback>(entity) {
        kb.stop();
      }
    }
  }

  /// Update flash states and apply visual feedback
  fn update_flashes(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Process knockback events
    for event in ctx.events.read::<KnockbackStarted>() {
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

impl Scene for KnockbackDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    println!("Knockback Demo initialized!");
    println!("  1: Fast decay (8.0)");
    println!("  2: Normal decay (4.0)");
    println!("  3: Slow decay (2.0)");
    println!("  4: No accumulation");
    println!("  Space: Radial knockback");
    println!("  A: Rapid fire (hold)");
    println!("  R: Reset positions");
    println!("  Up/Down: Adjust force");
    println!("  ESC: Exit");

    self.spawn_entities(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
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

    // Reset positions
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_entities(ctx);
      println!("Entities reset to starting positions");
    }

    // Adjust force
    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      self.force = (self.force + 50.0).min(1000.0);
      println!("Force: {:.0} px/sec", self.force);
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      self.force = (self.force - 50.0).max(100.0);
      println!("Force: {:.0} px/sec", self.force);
    }

    // Update visual flash effects
    self.update_flashes(ctx, dt);

    // Log knockback finished events
    for event in ctx.events.read::<KnockbackFinished>() {
      // Check which type of entity
      if ctx.world.has::<SimpleEntity>(event.entity) {
        println!("Simple entity knockback finished");
      } else if ctx.world.has::<PhysicsEntity>(event.entity) {
        println!("Physics entity knockback finished");
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let text_light = Color::rgb(220, 220, 220);
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_blue = Color::rgb(100, 180, 255);
    let text_orange = Color::rgb(255, 160, 100);

    let (screen_w, _) = ui.screen_size();

    // Title
    ui.draw_text("Knockback Demo", 10.0, 10.0, text_light);

    // Current force
    let force_text = format!("Force: {:.0} px/sec", self.force);
    ui.draw_text(&force_text, 10.0, 30.0, text_accent);

    // Controls
    ui.draw_text(
      "1-4: Knockback columns | Space: Radial | A: Rapid",
      10.0,
      50.0,
      text_muted,
    );
    ui.draw_text(
      "Up/Down: Force | R: Reset | ESC: Exit",
      10.0,
      70.0,
      text_muted,
    );

    // Column labels
    let spacing_x = screen_w as f32 / 5.0;
    let columns = ["Fast (8.0)", "Normal (4.0)", "Slow (2.0)", "No Accum"];

    for (col, label) in columns.iter().enumerate() {
      let x = spacing_x * (col as f32 + 1.0) - 35.0;
      ui.draw_text(label, x, 100.0, text_light);
    }

    // Row labels
    let y_top = ui.screen_size().1 as f32 * 0.35;
    let y_bottom = ui.screen_size().1 as f32 * 0.65;

    ui.draw_text("Simple", 10.0, y_top - 5.0, text_blue);
    ui.draw_text("(Transform)", 10.0, y_top + 10.0, text_muted);

    ui.draw_text("Physics", 10.0, y_bottom - 5.0, text_orange);
    ui.draw_text("(Velocity)", 10.0, y_bottom + 10.0, text_muted);

    // Instructions at bottom
    ui.draw_text(
      "Blue = Simple entities (direct Transform movement)",
      10.0,
      ui.screen_size().1 as f32 - 40.0,
      text_blue,
    );
    ui.draw_text(
      "Orange = Physics entities (Velocity-based movement)",
      10.0,
      ui.screen_size().1 as f32 - 20.0,
      text_orange,
    );
    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Knockback Demo - Ryo2D")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .target_fps(60)
    .physics_hz(60)
    .start_scene(KnockbackDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
