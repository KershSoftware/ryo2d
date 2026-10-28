//! Menu Scene - Starting screen for the game

use ryo2d::prelude::*;
use std::time::Duration;

use super::{DebugScene, GameScene};
use crate::components::Scrolling;
use crate::constants::*;
use crate::look::use_game_look;
use crate::save::load_high_score;

/// The game's PNGs, loaded by the menu so the game scene finds them loaded
const PRELOADED: [&str; 4] = ["bg.png", "bg-ground.png", "pipe_cap.png", "pipe_body.png"];

pub struct MenuScene {
  high_score: u32,
  debug_enabled: bool,
  /// The bird's two frames, loaded on enter, and their width
  bird_flap: Option<(Handle<Image>, Handle<Image>, f32)>,
  anim_timer: f32,
  anim_frame: bool,
  background_entities: Vec<Entity>,
  /// The game's PNGs, held while the menu is up: the game scene loads the
  /// same paths as the menu goes, and finds them loaded
  preloaded: Vec<Handle<Image>>,
}

impl MenuScene {
  pub fn new() -> Self {
    // Check if debug mode is enabled in Ryo.toml
    let debug_enabled = Workspace::discover()
      .ok()
      .and_then(|ws| ws.config().env_bool("debug_scene"))
      .unwrap_or(false);

    Self {
      high_score: load_high_score(),
      debug_enabled,
      bird_flap: None,
      anim_timer: 0.0,
      anim_frame: false,
      background_entities: Vec::new(),
      preloaded: Vec::new(),
    }
  }
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    use_game_look(ctx);

    ctx
      .camera()
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // The game's PNGs, loaded now so the game scene finds them loaded
    if self.preloaded.is_empty() {
      self.preloaded = PRELOADED
        .iter()
        .map(|path| {
          ctx
            .assets()
            .load_now::<Image>(path)
            .unwrap_or_else(|error| panic!("Failed to load {path}: {error}"))
        })
        .collect();
    }
    // The bird's frames (SVG, fitted to 64 px)
    if self.bird_flap.is_none() {
      let mut frame = |path: &str| {
        let fit = ImageSettings {
          svg: SvgSize::Fit {
            width: 64,
            height: 64,
          },
        };
        ctx
          .assets()
          .load_now_with::<Image, _>(path, fit)
          .expect("Failed to load bird sprite")
      };
      let (up, down) = (frame("bird-flap-up.svg"), frame("bird-flap-down.svg"));
      let width = ctx.assets().get(&up).map_or(0.0, |up| up.width() as f32);
      self.bird_flap = Some((up, down, width));
    }

    // The music: it carries on across scenes, and the track already
    // playing plays on
    let music = ctx.assets().load::<Music>("8bit-bg.music.ron");
    ctx.audio().play_music(&music);

    // Spawn scrolling background (uses cached assets)
    self.spawn_background(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    for &entity in &self.background_entities {
      ctx.world.despawn(entity);
    }
    self.background_entities.clear();
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Update scrolling background
    for &entity in &self.background_entities {
      if let Some(scrolling) = ctx.world.get::<Scrolling>(entity).copied()
        && let Some(mut transform) = ctx.world.get_mut::<Transform>(entity)
      {
        let new_x = scrolling.update(transform.translation.x, dt);
        transform.translation.x = new_x;
      }
    }

    // Animate bird preview
    self.anim_timer += dt;
    if self.anim_timer >= BIRD_FLAP_FRAME_TIME {
      self.anim_timer = 0.0;
      self.anim_frame = !self.anim_frame;
    }

    // Debug mode on D key (only if enabled)
    if self.debug_enabled && ctx.input.key_pressed(KeyCode::KeyD) {
      return SceneAction::SwitchWith(
        Box::new(DebugScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();

    // Title
    ui.draw_text_anchored("FLAPPY", Anchor::TopCenter, 0.0, 100.0, Color::WHITE);

    // High score (upper right)
    if self.high_score > 0 {
      let high_score_text = format!("Best: {}", self.high_score);
      let (text_w, _) = ui.text_size(&high_score_text);
      ui.draw_text(
        &high_score_text,
        screen_w as f32 - text_w as f32 - 16.0,
        16.0,
        Color::rgb(80, 80, 80),
      );
    }

    // Bird preview (centered below title, animated)
    if let Some((flap_up, flap_down, bird_w)) = &self.bird_flap {
      let bird_sprite = if self.anim_frame { flap_up } else { flap_down };
      let bird_w = *bird_w;
      let bird_x = screen_w as f32 / 2.0 - bird_w / 2.0;
      let bird_y = 180.0;
      ui.draw_sprite(bird_sprite, bird_x, bird_y);
    }

    // Start (and debug) buttons below the bird; Confirm (Space, Enter, A)
    // presses the focused one
    let mut start = false;
    let mut debug = false;
    let debug_enabled = self.debug_enabled;
    ui.column()
      .focus_scope()
      .anchor(Anchor::TopCenter)
      .offset(0.0, screen_h as f32 / 2.0)
      .gap(20.0)
      .align(Align::Center)
      .show(|ui| {
        start = ui
          .button("START")
          .width(200.0)
          .height(60.0)
          .default_focus()
          .clicked();
        // Smaller, below start - only if debug mode enabled
        if debug_enabled {
          debug = ui.button("DEBUG").width(120.0).height(40.0).clicked();
        }
      });
    if start {
      return SceneAction::SwitchWith(
        Box::new(GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }
    if debug {
      return SceneAction::SwitchWith(
        Box::new(DebugScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Instructions, one short line each so they fit the narrow screen
    let grey = Color::rgb(100, 100, 100);
    let top = screen_h as f32 - 100.0;
    ui.draw_text_anchored("SPACE or CLICK to flap", Anchor::TopCenter, 0.0, top, grey);
    if self.debug_enabled {
      ui.draw_text_anchored("D for debug", Anchor::TopCenter, 0.0, top + 24.0, grey);
    }
    SceneAction::Continue
  }
}

impl MenuScene {
  fn spawn_background(&mut self, ctx: &mut SceneContext) {
    // Spawn two background segments for seamless parallax scrolling
    // Menu background scrolls faster than game for visual appeal
    let mut assets = ctx.assets();
    let bg_handle = assets
      .load_now::<Image>("bg.png")
      .expect("Failed to load background sprite");
    let (bg_width, bg_height) = assets
      .get(&bg_handle)
      .map_or((0.0, 0.0), |bg| (bg.width() as f32, bg.height() as f32));
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
          Renderable::image(bg_handle.clone())
            .with_anchor(Anchor::TopLeft)
            .with_z_order(0), // Render behind everything
          Scrolling::new(
            MENU_BACKGROUND_SCROLL_SPEED,
            tile_width * scale,
            wrap_distance * scale,
          ),
        ))
        .id();
      self.background_entities.push(background);
    }
  }
}
