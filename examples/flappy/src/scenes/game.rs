//! Game Scene - Main gameplay

use rand::Rng;
use ryo2d::prelude::*;

use super::GameOverScene;
use crate::components::*;
use crate::constants::*;
use crate::save::save_high_score;

pub struct GameScene {
  // Game state
  score: u32,
  high_score: u32,
  game_started: bool,
  death_timer: Option<f32>,

  // Entity tracking
  bird: Option<Entity>,
  pipes: Vec<Entity>,
  ground_entities: Vec<Entity>,

  // Bird sprites for animation
  bird_sprites: BirdSprites,
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
      pipes: Vec::new(),
      ground_entities: Vec::new(),
      bird_sprites: BirdSprites::load(),
    }
  }

  fn spawn_bird(&mut self, ctx: &mut SceneContext) {
    let bird = ctx.world.spawn((
      Transform::new(BIRD_START_X, BIRD_START_Y),
      Velocity::zero(),
      RigidBody::new().with_gravity_scale(0.0),
      // Hitbox is smaller than sprite and centered on bird's body (forgiving for wing animations)
      Collider::circle_collider(BIRD_SIZE * 0.35).with_offset(BIRD_SIZE / 2.0, BIRD_SIZE / 2.0),
      SpriteRenderer::new(self.bird_sprites.flap_down.clone()).with_z_order(10),
      Bird::new(),
    ));
    self.bird = Some(bird);
  }

  fn spawn_pipe_pair(&mut self, ctx: &mut SceneContext, x: f32) {
    let mut rng = rand::thread_rng();
    let gap_center_y = rng.gen_range(PIPE_MIN_HEIGHT..=PIPE_MAX_HEIGHT);

    // Top pipe
    let top_height = gap_center_y - PIPE_GAP / 2.0;
    let pipe_sprite = Sprite::load("assets/pipe.png").expect("Failed to load pipe sprite");
    let sprite_w = pipe_sprite.width() as f32;
    let sprite_h = pipe_sprite.height() as f32;
    let scale_y = top_height / sprite_h;
    let mut transform = Transform::new(x, 0.0);
    transform.scale_y = scale_y;
    let top_pipe = ctx.world.spawn((
      transform,
      Velocity::zero(),
      RigidBody::kinematic(),
      // Use final world-space dimensions for collider (sprite_w, top_height)
      Collider::box_collider(sprite_w, top_height),
      SpriteRenderer::new(pipe_sprite).with_z_order(5),
      Pipe::new_top(),
      Scrolling::no_wrap(PIPE_SCROLL_SPEED),
    ));

    // Bottom pipe
    let bottom_y = gap_center_y + PIPE_GAP / 2.0;
    let bottom_height = GROUND_Y - bottom_y;
    let pipe_sprite = Sprite::load("assets/pipe.png").expect("Failed to load pipe sprite");
    let sprite_w = pipe_sprite.width() as f32;
    let sprite_h = pipe_sprite.height() as f32;
    let scale_y = bottom_height / sprite_h;
    let mut transform = Transform::new(x, bottom_y);
    transform.scale_y = scale_y;
    let bottom_pipe = ctx.world.spawn((
      transform,
      Velocity::zero(),
      RigidBody::kinematic(),
      // Use final world-space dimensions for collider (sprite_w, bottom_height)
      Collider::box_collider(sprite_w, bottom_height),
      SpriteRenderer::new(pipe_sprite).with_z_order(5),
      Pipe::new_bottom(),
      Scrolling::no_wrap(PIPE_SCROLL_SPEED),
    ));

    self.pipes.push(top_pipe);
    self.pipes.push(bottom_pipe);
  }

  fn spawn_ground(&mut self, ctx: &mut SceneContext) {
    // Spawn two ground segments for seamless scrolling
    // tile_width = SCREEN_WIDTH (when tile goes off left edge)
    // wrap_distance = 2*SCREEN_WIDTH (jump to behind the other tile)
    let tile_width = SCREEN_WIDTH as f32;
    let wrap_distance = SCREEN_WIDTH as f32 * 2.0;
    for i in 0..2 {
      let x = i as f32 * SCREEN_WIDTH as f32;
      let ground = ctx.world.spawn((
        Transform::new(x, GROUND_Y),
        SpriteRenderer::new(
          Sprite::load("assets/ground.png").expect("Failed to load ground sprite"),
        )
        .with_z_order(15),
        Ground,
        Scrolling::new(GROUND_SCROLL_SPEED, tile_width, wrap_distance),
      ));
      self.ground_entities.push(ground);
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
      if should_flap {
        bird.flap();
      }

      // Apply gravity
      bird.apply_gravity(dt);

      bird.velocity_y
    };

    // Update velocity
    if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(bird_entity) {
      velocity.dy = velocity_y;
    }

    // Check bounds and clamp position
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(bird_entity) {
      // Calculate bird's bottom edge (transform.y is top-left corner)
      let bird_bottom = transform.y + BIRD_SIZE;

      // Check if hit ceiling or ground
      if transform.y <= CEILING_Y || bird_bottom >= GROUND_Y {
        // Clamp position at the boundary
        if transform.y <= CEILING_Y {
          transform.y = CEILING_Y;
        } else if bird_bottom >= GROUND_Y {
          // Clamp so the bottom of the bird is at GROUND_Y
          transform.y = GROUND_Y - BIRD_SIZE;
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
        // Make the bird kinematic so physics doesn't move it
        if let Some(mut body) = ctx.world.get_mut::<RigidBody>(bird_entity) {
          *body = RigidBody::kinematic();
        }
      }
    }

    // Note: Dead state is checked in main update loop, not here
  }

  fn update_bird_sprite(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    // Get bird state
    let (velocity_y, is_alive, hurt_frame) = {
      let Some(mut bird) = ctx.world.get_mut::<Bird>(bird_entity) else {
        return;
      };

      // Update hurt animation timer if dead
      if !bird.is_alive {
        bird.hurt_timer += dt;
        if bird.hurt_timer >= 0.1 {
          // 10 FPS hurt animation
          bird.hurt_timer = 0.0;
          bird.hurt_frame += 1;
        }
      }

      (bird.velocity_y, bird.is_alive, bird.hurt_frame)
    };

    // Update sprite based on state
    if let Some(mut renderer) = ctx.world.get_mut::<SpriteRenderer>(bird_entity) {
      renderer.sprite = self
        .bird_sprites
        .get_sprite(velocity_y, is_alive, hurt_frame)
        .clone();
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
      for (entity, transform, scrolling) in query.iter() {
        let new_x = scrolling.update(transform.x, dt);
        to_update.push((entity, new_x));
      }
    }

    for (entity, new_x) in to_update {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = new_x;
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
      if let Some(transform) = ctx.world.get::<Transform>(pipe_entity) {
        if transform.x > rightmost_x {
          rightmost_x = transform.x;
        }
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

    // Update scoring - check if bird passed a pipe
    if let Some(bird_entity) = self.bird {
      if let Some(bird_transform) = ctx.world.get::<Transform>(bird_entity) {
        let bird_x = bird_transform.x;

        // Find any pipe pair that hasn't been scored and has been passed
        let mut scored_this_frame = false;
        for &pipe_entity in &self.pipes {
          if let Some(pipe_transform) = ctx.world.get::<Transform>(pipe_entity) {
            let pipe_x = pipe_transform.x;
            // Check if pipe's right edge has passed the bird's position
            if pipe_x + PIPE_WIDTH < bird_x {
              if let Some(mut pipe) = ctx.world.get_mut::<Pipe>(pipe_entity) {
                if !pipe.has_scored {
                  pipe.has_scored = true;
                  // Only count score once per pipe pair (both top and bottom share same x)
                  if !scored_this_frame {
                    scored_this_frame = true;
                    self.score += 1;
                  }
                }
              }
            }
          }
        }
      }
    }

    // Remove pipes that are off screen
    let mut to_remove = Vec::new();
    for &entity in &self.pipes {
      if let Some(transform) = ctx.world.get::<Transform>(entity) {
        if transform.x < -PIPE_WIDTH - 100.0 {
          to_remove.push(entity);
        }
      }
    }
    for entity in to_remove {
      let _ = ctx.world.despawn(entity);
      self.pipes.retain(|&e| e != entity);
    }
  }

  fn check_collisions(&mut self, ctx: &mut SceneContext) {
    let Some(bird_entity) = self.bird else {
      return;
    };

    // Use collisions_started() to only react when objects first touch
    // (not when they separate - that's collisions_stopped())
    for event in ctx.collisions_started() {
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
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Spawn entities
    self.spawn_bird(ctx);
    self.spawn_ground(ctx);

    // Spawn initial pipes
    self.spawn_pipe_pair(ctx, SCREEN_WIDTH as f32 + 100.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Start game on first input
    if !self.game_started {
      if ctx.input.key_pressed(KeyCode::Space) || ctx.input.mouse_pressed(MouseButton::Left) {
        self.game_started = true;
        if let Some(bird_entity) = self.bird {
          if let Some(mut bird) = ctx.world.get_mut::<Bird>(bird_entity) {
            bird.flap();
          }
        }
      }
    }

    // Update game systems
    self.update_bird(ctx, dt);
    self.update_bird_sprite(ctx, dt);
    self.update_scrolling(ctx, dt);
    self.update_pipes(ctx);
    self.check_collisions(ctx);

    // Check if bird died - start death timer
    if let Some(bird_entity) = self.bird {
      if let Some(bird) = ctx.world.get::<Bird>(bird_entity) {
        if !bird.is_alive && self.death_timer.is_none() {
          self.death_timer = Some(0.0);
        }
      }
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

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Draw score
    ui.draw_text(&self.score.to_string(), SCORE_X, SCORE_Y, Color::WHITE);

    // Pre-game instructions
    if !self.game_started {
      let (screen_w, screen_h) = ui.screen_size();
      let text = "Press SPACE or CLICK to start";
      let (text_w, _) = ui.text_size(text);
      ui.draw_text(
        text,
        screen_w as f32 / 2.0 - text_w as f32 / 2.0,
        screen_h as f32 / 2.0,
        Color::WHITE,
      );
    }
  }
}
