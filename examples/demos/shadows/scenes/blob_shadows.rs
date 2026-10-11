//! Blob Shadows page
//!
//! Demonstrates simple elliptical blob shadows beneath sprites.
//! Blob shadows are commonly used for characters in platformers and top-down games.

use demo_kit::prelude::*;

/// A jumping character for demonstrating height-based shadow scaling
struct JumpingCharacter {
  entity: Entity,
  base_y: f32,
  velocity_y: f32,
  is_jumping: bool,
}

/// Blob shadow page state
pub struct BlobShadowsPage {
  // Characters with blob shadows
  characters: Vec<JumpingCharacter>,

  // Static objects with blob shadows
  static_objects: Vec<Entity>,

  // Whether height scaling is enabled
  height_scaling_enabled: bool,
}

impl BlobShadowsPage {
  pub fn new() -> Self {
    Self {
      characters: Vec::new(),
      static_objects: Vec::new(),
      height_scaling_enabled: true,
    }
  }

  fn spawn_character(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    color: (u8, u8, u8),
    name: &str,
    height_scaling: bool,
  ) -> JumpingCharacter {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(32, 48, color.0, color.1, color.2, 255),
      name,
    );

    let blob = if height_scaling {
      BlobShadow::ellipse(20.0, 10.0)
        .offset(48.0) // Below the sprite (pivot at top)
        .with_height_scale(true)
        .max_height(150.0)
    } else {
      BlobShadow::ellipse(20.0, 10.0).offset(48.0)
    };

    let entity = ctx.world.spawn((
      Transform::new(x, y),
      Renderable::sprite(sprite)
        .with_z_order(5)
        .with_blob_shadow(blob),
    ));

    JumpingCharacter {
      entity,
      base_y: y,
      velocity_y: 0.0,
      is_jumping: false,
    }
  }

  fn spawn_static_object(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    size: u32,
    color: (u8, u8, u8),
    name: &str,
    shadow_size: (f32, f32),
  ) -> Entity {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(size, size, color.0, color.1, color.2, 255),
      name,
    );

    let blob = BlobShadow::ellipse(shadow_size.0, shadow_size.1).offset(size as f32); // Below the sprite

    ctx.world.spawn((
      Transform::new(x, y),
      Renderable::sprite(sprite)
        .with_z_order(5)
        .with_blob_shadow(blob),
    ))
  }
}

impl DemoPage for BlobShadowsPage {
  fn name(&self) -> &str {
    "Blob"
  }

  fn description(&self) -> &str {
    "Simple ellipses beneath entities via Renderable.with_blob_shadow(); with height scaling they shrink when jumping."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Height scaling", &mut self.height_scaling_enabled);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Make the characters jump")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Bright lighting for this demo
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      lighting.set_ambient(AmbientLight::new(Color::WHITE, 0.95));
    }

    // Create ground
    let ground = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 300, 100, 140, 100, 255), // Grass green
      "ground",
    );
    ctx.world.spawn((
      Transform::new(0.0, 420.0),
      Renderable::sprite(ground).with_z_order(0),
    ));

    // Create sky
    let sky = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 420, 150, 200, 240, 255), // Light blue
      "sky",
    );
    ctx.world.spawn((
      Transform::new(0.0, 0.0),
      Renderable::sprite(sky).with_z_order(-1),
    ));

    // Spawn characters that can jump
    let char_colors = [
      (220, 100, 100), // Red
      (100, 180, 100), // Green
      (100, 100, 220), // Blue
      (220, 180, 100), // Yellow
    ];

    for (i, &color) in char_colors.iter().enumerate() {
      let x = 200.0 + i as f32 * 250.0;
      let y = 370.0; // Above ground
      let char = Self::spawn_character(
        ctx,
        x,
        y,
        color,
        &format!("char_{}", i),
        self.height_scaling_enabled,
      );
      self.characters.push(char);
    }

    // Spawn some static objects with different shadow sizes
    let objects = [
      (150.0, 380.0, 40, (80, 80, 90), "crate1", (25.0, 12.0)),
      (1100.0, 385.0, 30, (120, 80, 60), "barrel", (18.0, 9.0)),
      (640.0, 350.0, 60, (90, 90, 100), "big_box", (35.0, 18.0)),
    ];

    for (x, y, size, color, name, shadow_size) in objects {
      let entity = Self::spawn_static_object(ctx, x, y, size, color, name, shadow_size);
      self.static_objects.push(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Keep every character shadow in step with the setting
    for char in &self.characters {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(char.entity)
        && let Some(ref mut blob) = renderable.blob_shadow
      {
        blob.height_scaling = self.height_scaling_enabled;
      }
    }

    // Jump controls - Space makes all characters jump, or click on individual
    if ctx.input.key_pressed(KeyCode::Space) {
      for char in &mut self.characters {
        if !char.is_jumping {
          char.velocity_y = -400.0; // Jump velocity
          char.is_jumping = true;
        }
      }
    }

    // Update character physics and shadow height
    let gravity = 800.0;
    for char in &mut self.characters {
      if char.is_jumping {
        // Apply gravity
        char.velocity_y += gravity * dt;

        // Update position
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(char.entity) {
          transform.y += char.velocity_y * dt;

          // Check if landed
          if transform.y >= char.base_y {
            transform.y = char.base_y;
            char.velocity_y = 0.0;
            char.is_jumping = false;
          }

          // Update blob shadow height for scaling effect
          if self.height_scaling_enabled {
            let height = (char.base_y - transform.y).max(0.0);
            if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(char.entity)
              && let Some(ref mut blob) = renderable.blob_shadow
            {
              blob.set_height(height);
            }
          }
        }
      }
    }
  }
}
