//! Layers: a second track plays over the first. Track 0 walks the whole
//! body; track 1 plays an animation that keys only an arm, the head and
//! (the wave) the mouth, at an alpha, so the legs keep walking under a wave

use demo_kit::prelude::*;

use crate::hero::{self, GROUND, Patrol, RUN_SPEED, SCALE, WALK_SPEED};

/// The animations for track 1: each leaves the legs and the body alone
const LAYERS: [&str; 2] = ["wave", "aim"];
/// The animations for track 0, and how fast each carries the hero
const BASES: [(&str, f32); 3] = [("walk", WALK_SPEED), ("run", RUN_SPEED), ("idle", 0.0)];
/// Seconds the layer takes to fade out or in
const FADE: f32 = 0.3;

pub struct LayersPage {
  hero: Option<Entity>,
  patrol: Patrol,
  /// Settings: the layer's animation, its alpha, and what plays under it
  layer: usize,
  alpha: f32,
  base: usize,
  /// Whether the layer is on (Space fades it out and in)
  on: bool,
  /// What the tracks were last asked to play
  playing_layer: Option<usize>,
  playing_base: usize,
  /// The layer's own entry on track 1, which takes the alpha: a handle
  /// finds its entry whether it is queued, playing or fading out, and
  /// never the empty entry a fade plays
  layer_entry: Option<SkeletonTrackEntryId>,
}

impl Default for LayersPage {
  fn default() -> Self {
    Self {
      hero: None,
      patrol: Patrol::new(150.0, 640.0),
      layer: 0,
      alpha: 1.0,
      base: 0,
      on: true,
      playing_layer: None,
      playing_base: 0,
      layer_entry: None,
    }
  }
}

impl DemoPage for LayersPage {
  fn name(&self) -> &str {
    "Layers"
  }

  fn description(&self) -> &str {
    "Track 1 plays over track 0: the wave keys one arm, the head and the mouth, so the legs keep walking under it.\n\
     Alpha blends the layer with what is below. Space fades it out with set_empty, and in again by queueing it after an empty entry."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Layer", &mut self.layer, &["Wave", "Aim"]);
    s.float("Alpha", &mut self.alpha, 0.0..=1.0, 0.1);
    s.choice("Under it", &mut self.base, &["Walk", "Run", "Idle"]);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Fade the layer out or in")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    let hero = hero::spawn(ctx, &data, Vec2::new(330.0, GROUND), SCALE);
    self.hero = Some(hero);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.set_animation(0, BASES[self.base].0, true);
      // The first entry on an empty track shows at once
      self.layer_entry = state.set_animation(1, LAYERS[self.layer], true);
    }
    self.playing_base = self.base;
    self.playing_layer = Some(self.layer);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(hero) = self.hero else {
      return;
    };
    if ctx.input.key_pressed(KeyCode::Space) {
      self.on = !self.on;
    }
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      if self.base != self.playing_base {
        state.set_animation(0, BASES[self.base].0, true);
        self.playing_base = self.base;
      }
      let wanted = self.on.then_some(self.layer);
      if wanted != self.playing_layer {
        match (wanted, self.playing_layer) {
          // Another layer: the track crossfades to it
          (Some(layer), Some(_)) => {
            self.layer_entry = state.set_animation(1, LAYERS[layer], true);
          }
          // In from nothing: an entry queued after an empty one mixes in
          (Some(layer), None) => {
            state.set_empty(1, 0.0);
            self.layer_entry = state.add_animation(1, LAYERS[layer], true, 0.0);
            if let Some(entry) = self.layer_entry
              && let Some(entry) = state.entry_mut(entry)
            {
              entry.mix_duration = FADE;
            }
          }
          // Out to the track below; the track clears when the fade is over
          (None, _) => state.set_empty(1, FADE),
        }
        self.playing_layer = wanted;
      }
      // The alpha goes on the layer's own entry, from the step it is made
      // in (a queued entry has it before it starts) and while it fades out
      if let Some(entry) = self.layer_entry
        && let Some(entry) = state.entry_mut(entry)
      {
        entry.alpha = self.alpha;
      }
    }
    self.patrol.step(ctx, hero, BASES[self.playing_base].1, dt);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let Some(hero) = self.hero else {
      return;
    };
    let layer = if self.on {
      format!("Layer on at alpha {:.1}", self.alpha)
    } else {
      "Layer faded out".to_string()
    };
    hero::readout(
      ui,
      &[
        layer,
        hero::track_line(ctx, hero, 1),
        hero::track_line(ctx, hero, 0),
      ],
    );
  }
}
