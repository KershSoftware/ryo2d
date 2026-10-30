//! The Animated tiles page: the platformer's water, waterfall and coins are
//! tiles with frames in their tileset (`<animation>` in `platformer.tsx`).
//! They step on the game clock, so pausing it (or slowing it) stops them;
//! the page shows which frame each is on, worked out from the same clock

use demo_kit::prelude::*;

use crate::shared::{
  DIM, HOT, PLATFORMER, View, asset, clear_to, hud_beside_panel, load, main_action,
};

/// The platformer's tileset, whose tiles carry the frames
const TILESET: &str = "tilesets/platformer.tsx";
/// The animated tiles the page reads out
const SHOWN: [(&str, u32); 3] = [("Waterfall", 34), ("Water", 33), ("Coin", 151)];
/// What the page frames: the pool, its waterfall and the coins over it
const FRAMED: Rect = Rect::new(324.0, 126.0, 252.0, 162.0);

pub(crate) struct AnimatedPage {
  view: View,
  map: Option<Handle<TileMap>>,
  /// Each read-out tile's frames, from its tileset
  frames: Vec<(&'static str, u32, Vec<TileFrame>)>,
  paused: bool,
  speed: f32,
}

impl AnimatedPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      map: None,
      frames: Vec::new(),
      paused: false,
      speed: 1.0,
    }
  }
}

/// The frame of `frames` shown `elapsed` seconds into the animation, and how
/// long a loop takes
fn frame_at(frames: &[TileFrame], elapsed: f32) -> Option<(usize, f32)> {
  let total: f32 = frames.iter().map(|f| f.duration.as_secs_f32()).sum();
  if total <= 0.0 {
    return None;
  }
  let mut t = elapsed.rem_euclid(total);
  for (i, frame) in frames.iter().enumerate() {
    let d = frame.duration.as_secs_f32();
    if t < d {
      return Some((i, total));
    }
    t -= d;
  }
  Some((frames.len() - 1, total))
}

impl DemoPage for AnimatedPage {
  fn name(&self) -> &str {
    "Animated tiles"
  }

  fn description(&self) -> &str {
    "Water, a waterfall and coins whose tiles have frames: they step on the game clock and stop when it pauses"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Paused", &mut self.paused);
    s.float("Game speed", &mut self.speed, 0.25..=2.0, 0.25);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Pause or play (pad: A)")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.map = load::<TileMap>(ctx, PLATFORMER);
    if let Some(tileset) = load::<Tileset>(ctx, TILESET) {
      let assets = ctx.assets();
      for (name, id) in SHOWN {
        let frames = assets
          .get(&tileset)
          .and_then(|t| t.tile(id))
          .map(|data| data.animation.clone())
          .unwrap_or_default();
        self.frames.push((name, id, frames));
      }
    }
    let Some(map) = self.map.clone() else {
      return;
    };
    if let Some(data) = asset(ctx, &map) {
      clear_to(ctx, data.background.unwrap_or(Color::rgb(127, 240, 165)));
    }
    ctx.world.spawn((Transform::IDENTITY, MapRoot::new(map)));
  }

  // The game clock is set here, in the UI pass: while it is paused no fixed
  // step runs, so `update` would never see the key that starts it again
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    if main_action(ctx.input) {
      self.paused = !self.paused;
    }
    if self.paused != ctx.time.game.is_paused() {
      if self.paused {
        ctx.time.game.pause();
      } else {
        ctx.time.game.resume();
      }
    }
    ctx.time.game.set_speed(self.speed);

    self.view.measure(ui);
    let zoom = self.view.fit_zoom(FRAMED);
    let (x, y) = FRAMED.center();
    self.view.look_at(ctx, Vec2::new(x, y), zoom);

    let elapsed = ctx.time.game.elapsed();
    let mut lines = vec![(
      format!(
        "Game time {elapsed:.2} s at {:.2}x{}",
        self.speed,
        if self.paused { ", paused" } else { "" }
      ),
      if self.paused { HOT } else { Color::WHITE },
    )];
    for (name, id, frames) in &self.frames {
      let text = match frame_at(frames, elapsed) {
        Some((frame, total)) => format!(
          "{name} (tile {id}): frame {} of {}, {:.1} s a loop",
          frame + 1,
          frames.len(),
          total
        ),
        None => format!("{name} (tile {id}): still"),
      };
      lines.push((text, DIM));
    }
    hud_beside_panel(ui, 2, &lines);
  }
}
