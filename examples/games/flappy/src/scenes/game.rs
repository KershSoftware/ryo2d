//! Game Scene - Main gameplay

use ryo2d::prelude::*;

use super::GameOverScene;
use crate::components::*;
use crate::constants::*;
use crate::save::save_high_score;

const FLAP_SOUND: &str = "flap";
const DEATH_SOUND: &str = "death";

pub struct GameScene {
  // Game state
  score: u32,
  high_score: u32,
  game_started: bool,
  death_timer: Option<f32>,

  // Entity tracking
  bird: Option<Entity>,
  /// The bird's sprite: a child of the bird, scaled down from its larger
  /// raster (the bird's own scale would scale its collider too)
  bird_sprite: Option<Entity>,
  pipes: Vec<Entity>,
  ground_entities: Vec<Entity>,
  ground_border_entities: Vec<Entity>,
  background_entities: Vec<Entity>,
}

const DEATH_DELAY: f32 = 1.0; // seconds to show death animation

impl GameScene {
  pub fn new(high_score: u32) -> Self {
    Self {
      score: 0,
      high_score,
      game_started: false,
      death_timer: None,
      bird: None,
      bird_sprite: None,
      pipes: Vec::new(),
      ground_entities: Vec::new(),
      ground_border_entities: Vec::new(),
      background_entities: Vec::new(),
    }
  }

  fn spawn_bird(&mut self, ctx: &mut SceneContext) {
    // Create animation controller with flapping and hurt animations
    let controller = create_bird_animation_controller();

    // Get the first sprite from the controller for initial rendering
    let initial_sprite = controller
      .current_sprite()
      .cloned()
      .expect("Animation controller should have a sprite");

    let bird = ctx
      .world
      .spawn((
        Transform::from_xy(BIRD_START_X, BIRD_START_Y),
        Velocity::zero(),
        // CCD prevents tunneling through pipes
        RigidBody::dynamic()
          .with_ccd()
          .with_gravity_scale(0.0)
          .lock_rotation(),
        // Hitbox is smaller than sprite and centered on bird's body (forgiving for wing animations)
        Collider::circle(BIRD_SIZE * BIRD_COLLIDER_RATIO),
        Interpolated,
        Bird::new(),
      ))
      .id();
    let sprite = ctx.world.spawn_child(
      bird,
      (
        Transform::from_xy(0.0, 0.0).with_uniform_scale(1.0 / BIRD_RASTER_SCALE),
        Renderable::sprite(initial_sprite).with_z_order(10),
        controller,
      ),
    );
    self.bird_sprite = Some(sprite);
    self.bird = Some(bird);
  }

  fn spawn_pipe_pair(&mut self, ctx: &mut SceneContext, x: f32) {
    let mut rng = rand::thread_rng();
    let gap_center_y = rng.gen_range(PIPE_MIN_HEIGHT..=PIPE_MAX_HEIGHT);

    // Load pipe sprites (cached by AssetManager)
    let cap_handle = ctx
      .assets
      .load_sprite("assets/pipe_cap.png")
      .expect("Failed to load pipe cap");
    let body_handle = ctx
      .assets
      .load_sprite("assets/pipe_body.png")
      .expect("Failed to load pipe body");

    let cap_height = cap_handle.height() as f32; // 24px

    // TOP PIPE: hangs from ceiling, opening faces down
    // Spans from y=0 to gap_center_y - PIPE_GAP/2
    let top_pipe_bottom_y = gap_center_y - PIPE_GAP / 2.0;
    let top_pipe_height = top_pipe_bottom_y;
    let top_body_height = (top_pipe_height - cap_height).max(0.0);

    // Build: body tiles downward from origin, cap at bottom (flipped)
    let (top_composite, top_collider) = CompositeTileBuilder::new()
      .body_tiled(
        body_handle.clone(),
        0.0,
        0.0,
        TileDirection::Down,
        top_body_height,
      )
      .cap_flipped_y(cap_handle.clone(), 0.0, top_body_height) // Cap below body, flipped
      .z_order(5)
      .build_with_collider();

    let top_pipe = ctx
      .world
      .spawn((
        Transform::from_xy(x, 0.0), // Start at top of screen
        Velocity::zero(),
        RigidBody::kinematic(),
        top_collider,
        top_composite,
        Pipe::new_top(),
        Scrolling::no_wrap(PIPE_SCROLL_SPEED),
      ))
      .id();

    // Score gate: a trigger in the gap, carried by the top pipe. The bird
    // can only reach it by flying through the gap.
    ctx.world.spawn((
      Transform::from_xy(PIPE_WIDTH / 2.0, gap_center_y),
      Collider::rect(4.0, PIPE_GAP).as_trigger(),
      ScoreGate,
      ChildOf(top_pipe),
    ));

    // BOTTOM PIPE: rises from ground, opening faces up
    // Spans from gap_center_y + PIPE_GAP/2 down to bottom of screen
    let bottom_y = gap_center_y + PIPE_GAP / 2.0;

    // Only spawn bottom pipe if the cap would be above the ground
    if bottom_y < GROUND_Y {
      // Extend to bottom of screen so all pipes align visually
      let bottom_pipe_height = SCREEN_HEIGHT as f32 - bottom_y;
      let bottom_body_height = (bottom_pipe_height - cap_height).max(0.0);

      // Build: cap at top (normal orientation), body tiles downward
      let (bottom_composite, bottom_collider) = CompositeTileBuilder::new()
        .cap(cap_handle, 0.0, 0.0) // Cap at top, facing up
        .body_tiled(
          body_handle,
          0.0,
          cap_height,
          TileDirection::Down,
          bottom_body_height,
        )
        .z_order(5)
        .build_with_collider();

      let bottom_pipe = ctx
        .world
        .spawn((
          Transform::from_xy(x, bottom_y),
          Velocity::zero(),
          RigidBody::kinematic(),
          bottom_collider,
          bottom_composite,
          Pipe::new_bottom(),
          Scrolling::no_wrap(PIPE_SCROLL_SPEED),
        ))
        .id();

      self.pipes.push(bottom_pipe);
    }

    self.pipes.push(top_pipe);
  }

  fn spawn_ground(&mut self, ctx: &mut SceneContext) {
    // Spawn ground segments for seamless scrolling (cached by AssetManager)
    let ground_handle = ctx
      .assets
      .load_sprite("assets/bg-ground.png")
      .expect("Failed to load ground sprite");
    let sprite_width = ground_handle.width() as f32;
    let sprite_height = ground_handle.height() as f32;

    // Scale to match desired ground height (same as old ground.png)
    let target_height = 112.0;
    let scale = target_height / sprite_height;
    let scaled_width = sprite_width * scale;

    // Calculate how many tiles needed to cover screen width + buffer
    let tiles_needed = ((SCREEN_WIDTH as f32 / scaled_width).ceil() as i32 + 1) as usize;
    let wrap_distance = scaled_width * tiles_needed as f32;

    // Position ground at bottom of screen using BottomLeft anchor
    let screen_bottom = SCREEN_HEIGHT as f32;

    for i in 0..tiles_needed {
      let x = i as f32 * scaled_width;
      let transform = Transform::from_xy(x, screen_bottom).with_uniform_scale(scale);

      let ground = ctx
        .world
        .spawn((
          transform,
          Renderable::sprite(ground_handle.clone())
            .with_anchor(Anchor::BottomLeft)
            .with_z_order(15),
          Interpolated,
          Ground,
          Scrolling::new(GROUND_SCROLL_SPEED, scaled_width, wrap_distance),
        ))
        .id();
      self.ground_entities.push(ground);
    }

    // Spawn ground border - line at top of ground
    // Use full screen width, z-order 16 puts it on top of ground (15)
    let (r, g, b, a) = GROUND_BORDER_COLOR;
    let border_sprite = Sprite::solid_rect(SCREEN_WIDTH, 2, r, g, b, a);
    let border_handle = SpriteHandle::from_sprite(border_sprite, "ground_border");

    let border = ctx
      .world
      .spawn((
        Transform::from_xy(0.0, GROUND_Y),
        Renderable::sprite(border_handle)
          .with_anchor(Anchor::TopLeft)
          .with_z_order(16),
      ))
      .id();
    self.ground_border_entities.push(border);
  }

  fn spawn_background(&mut self, ctx: &mut SceneContext) {
    // Spawn two background segments for seamless parallax scrolling
    // Background scrolls slower than pipes/ground for depth effect (cached by AssetManager)
    let bg_handle = ctx
      .assets
      .load_sprite("assets/bg.png")
      .expect("Failed to load background sprite");
    let bg_width = bg_handle.width() as f32;
    let bg_height = bg_handle.height() as f32;
    let tile_width = bg_width;
    let wrap_distance = bg_width * 2.0;

    // Scale background to fill screen height while maintaining aspect ratio
    let screen_height = SCREEN_HEIGHT as f32;
    let scale = screen_height / bg_height;

    for i in 0..2 {
      let x = i as f32 * bg_width * scale;
      let transform = Transform::from_xy(x, 0.0).with_uniform_scale(scale);
      let background = ctx
        .world
        .spawn((
          transform,
          Renderable::sprite(bg_handle.clone())
            .with_anchor(Anchor::TopLeft)
            .with_z_order(0), // Render behind everything
          Interpolated,
          Scrolling::new(
            BACKGROUND_SCROLL_SPEED,
            tile_width * scale,
            wrap_distance * scale,
          ),
        ))
        .id();
      self.background_entities.push(background);
    }
  }

  fn update_bird(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    // Get bird component and check input
    let should_flap =
      ctx.input.key_pressed(KeyCode::Space) || ctx.input.mouse_pressed(MouseButton::Left);
    let velocity_y = {
      let Some(mut bird) = ctx.world.get_mut::<Bird>(bird_entity) else {
        return;
      };

      // Handle flapping
      if should_flap && bird.is_alive {
        bird.flap();
        // Play flap sound
        let _ = ctx
          .audio
          .play_sound_with(FLAP_SOUND, SoundOptions::new().volume(0.6));
      }

      // Apply gravity
      bird.apply_gravity(dt);

      bird.velocity_y
    };

    // Update velocity
    if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(bird_entity) {
      velocity.dy = velocity_y;
    }

    // Update rotation based on velocity (exponential tilt)
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(bird_entity) {
      // Normalize velocity to -1.0 to 1.0 range
      // Flapping gives -300, max fall is 400
      let velocity_normalized = (velocity_y / BIRD_MAX_FALL_SPEED).clamp(-1.0, 1.0);

      // Apply different exponential curves for climbing vs falling
      let tilt_factor = if velocity_normalized < 0.0 {
        // Climbing/upward - use gentler curve (lower power) for quicker rotation response
        velocity_normalized.abs().powf(0.8) * velocity_normalized.signum()
      } else {
        // Falling/downward - use steeper curve (higher power) for delayed rotation
        velocity_normalized.abs().powf(2.2) * velocity_normalized.signum()
      };

      // Map to rotation range (in degrees, then convert to radians)
      let target_rotation_deg = if tilt_factor < 0.0 {
        // Jumping/upward - tilt back (negative rotation)
        tilt_factor * BIRD_TILT_UP_MAX.abs()
      } else {
        // Falling/downward - tilt forward (positive rotation)
        tilt_factor * BIRD_TILT_DOWN_MAX
      };

      transform.rotation = target_rotation_deg.to_radians();
    }

    // Check bounds and clamp position
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(bird_entity) {
      // Calculate collision circle bounds (the circle is centred on the bird)
      let collider_radius = BIRD_SIZE * BIRD_COLLIDER_RATIO;
      let circle_top = transform.translation.y - collider_radius;
      let circle_bottom = transform.translation.y + collider_radius;

      // Check if collision circle hit ceiling or ground
      if circle_top <= CEILING_Y || circle_bottom >= GROUND_Y {
        // Clamp position so circle stays within bounds
        if circle_top <= CEILING_Y {
          transform.translation.y = CEILING_Y + collider_radius;
        } else if circle_bottom >= GROUND_Y {
          transform.translation.y = GROUND_Y - collider_radius;
        }

        // Stop all movement when hitting boundaries
        if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(bird_entity) {
          velocity.dx = 0.0;
          velocity.dy = 0.0;
        }
        if let Some(mut bird) = ctx.world.get_mut::<Bird>(bird_entity) {
          bird.velocity_y = 0.0;
          bird.kill();
        }
        // Take the bird out of the simulation so physics doesn't move it (a
        // collider left without its body would stay in as a fixed collider)
        ctx
          .world
          .entity_mut(bird_entity)
          .remove::<(RigidBody, Collider)>();
      }
    }

    // Note: Dead state is checked in main update loop, not here
  }

  fn update_bird_animation(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    // Check if bird is alive and switch animation state if needed
    let is_alive = {
      let Some(bird) = ctx.world.get::<Bird>(bird_entity) else {
        return;
      };
      bird.is_alive
    };

    // Switch to hurt animation when dead
    if !is_alive
      && let Some(sprite) = self.bird_sprite
      && let Some(mut controller) = ctx.world.get_mut::<AnimationController>(sprite)
      && controller.current_state() != Some("hurt")
    {
      controller.set_state("hurt");
    }
  }

  fn update_scrolling(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Only scroll if game started
    if !self.game_started {
      return;
    }

    // Update all entities with Scrolling component - collect data first to avoid borrow issues
    let mut to_update: Vec<(Entity, f32)> = Vec::new();
    {
      let mut query = ctx.world.query::<(Entity, &Transform, &Scrolling)>();
      for (entity, transform, scrolling) in query.iter(ctx.world) {
        let new_x = scrolling.update(transform.translation.x, dt);
        to_update.push((entity, new_x));
      }
    }

    for (entity, new_x) in to_update {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.translation.x = new_x;
      }
    }
  }

  fn update_pipes(&mut self, ctx: &mut SceneContext) {
    if !self.game_started {
      return;
    }

    // Find the rightmost pipe's current x position
    let mut rightmost_x: f32 = 0.0;
    for &pipe_entity in &self.pipes {
      if let Some(transform) = ctx.world.get::<Transform>(pipe_entity)
        && transform.translation.x > rightmost_x
      {
        rightmost_x = transform.translation.x;
      }
    }

    // Spawn new pipe when the rightmost pipe has scrolled far enough left
    // that we need another one off-screen to the right
    let spawn_threshold = SCREEN_WIDTH as f32 - PIPE_SPAWN_DISTANCE + PIPE_WIDTH;
    if rightmost_x < spawn_threshold {
      // Spawn just off the right edge of the screen
      let spawn_x = SCREEN_WIDTH as f32 + PIPE_WIDTH;
      self.spawn_pipe_pair(ctx, spawn_x);
    }

    // Remove pipes that are off screen
    let mut to_remove = Vec::new();
    for &entity in &self.pipes {
      if let Some(transform) = ctx.world.get::<Transform>(entity)
        && transform.translation.x < -PIPE_WIDTH - 100.0
      {
        to_remove.push(entity);
      }
    }
    for entity in to_remove {
      ctx.world.despawn(entity);
      self.pipes.retain(|&e| e != entity);
    }
  }

  /// The bird entering a pipe pair's gate scores a point
  fn check_score_gates(&mut self, ctx: &mut SceneContext) {
    let Some(bird) = self.bird else {
      return;
    };
    for message in ctx.messages::<TriggerEntered>() {
      if message.actor() == bird && ctx.world.get::<ScoreGate>(message.trigger).is_some() {
        self.score += 1;
      }
    }
  }

  fn check_collisions(&mut self, ctx: &mut SceneContext) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    // Use CollisionStarted events to only react when objects first touch
    // (not when they separate - that's CollisionStopped)
    for event in ctx.messages::<CollisionStarted>() {
      if event.involves(bird_entity) {
        // Bird hit something - game over
        if let Some(mut bird) = ctx.world.get_mut::<Bird>(bird_entity) {
          bird.kill();
        }
      }
    }
  }
}

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera()
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Load sound effects
    let _ = ctx.audio.load_sound(FLAP_SOUND, "assets/flap.wav");
    let _ = ctx.audio.load_sound(DEATH_SOUND, "assets/death.wav");

    // Spawn entities
    self.spawn_background(ctx);
    self.spawn_bird(ctx);
    self.spawn_ground(ctx);

    // Spawn initial pipes
    self.spawn_pipe_pair(ctx, SCREEN_WIDTH as f32 + 100.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Start game on first input
    if !self.game_started
      && (ctx.input.key_pressed(KeyCode::Space) || ctx.input.mouse_pressed(MouseButton::Left))
    {
      self.game_started = true;
      if let Some(bird_entity) = self.bird
        && let Some(mut bird) = ctx.world.get_mut::<Bird>(bird_entity)
      {
        bird.flap();
      }
    }

    // Update game systems
    self.update_bird(ctx, dt);
    self.update_bird_animation(ctx, dt);
    self.update_scrolling(ctx, dt);
    self.update_pipes(ctx);
    self.check_collisions(ctx);
    self.check_score_gates(ctx);

    // Check if bird died - start death timer and play death sound
    if let Some(bird_entity) = self.bird
      && let Some(bird) = ctx.world.get::<Bird>(bird_entity)
      && !bird.is_alive
      && self.death_timer.is_none()
    {
      self.death_timer = Some(0.0);
      // Play death sound
      let _ = ctx
        .audio
        .play_sound_with(DEATH_SOUND, SoundOptions::new().volume(0.8));
    }

    // Handle death timer
    if let Some(timer) = &mut self.death_timer {
      *timer += dt;
      if *timer >= DEATH_DELAY {
        let is_new_high_score = self.score > self.high_score;
        if is_new_high_score {
          save_high_score(self.score);
          self.high_score = self.score;
        }
        return SceneAction::Switch(Box::new(GameOverScene::new(
          self.score,
          self.high_score,
          is_new_high_score,
        )));
      }
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    // Draw score centered at top
    ui.draw_text_anchored(
      &self.score.to_string(),
      Anchor::TopCenter,
      0.0,
      SCORE_Y,
      Color::WHITE,
    );

    // High score (upper right)
    if self.high_score > 0 {
      let high_score_text = format!("Best: {}", self.high_score);
      ui.draw_text_anchored(
        &high_score_text,
        Anchor::TopRight,
        -16.0,
        16.0,
        Color::rgb(80, 80, 80),
      );
    }

    // Pre-game instructions
    if !self.game_started {
      let (_, screen_h) = ui.screen_size();
      ui.draw_text_anchored(
        "SPACE or CLICK to start",
        Anchor::TopCenter,
        0.0,
        screen_h as f32 / 2.0,
        Color::WHITE,
      );
    }
    SceneAction::Continue
  }
}
