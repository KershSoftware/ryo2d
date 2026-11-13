//! Events: an animation's event keys reach the game as `SkeletonEvent`
//! messages. Each footstep carries a sound's path, which the page plays
//! where the foot landed, with a puff of dust

use std::collections::VecDeque;
use std::sync::Arc;

use demo_kit::prelude::*;

use crate::hero::{self, GROUND, Patrol, RUN_SPEED, SCALE, WALK_SPEED};

/// The gaits, and how fast each carries the hero
const GAITS: [(&str, f32); 2] = [("walk", WALK_SPEED), ("run", RUN_SPEED)];
/// The events the readout keeps
const LOG: usize = 6;

pub struct EventsPage {
  hero: Option<Entity>,
  patrol: Patrol,
  /// The sound each event of the skeleton names, by its path in the file
  sounds: Vec<(Arc<str>, Handle<Sound>)>,
  /// The puff under a footstep, and under a hard landing
  dust: Option<Handle<ParticleEffect>>,
  thud: Option<Handle<ParticleEffect>>,
  /// The latest events, newest last, and how many there have been
  log: VecDeque<String>,
  count: u32,
  /// Settings
  gait: usize,
  sound: bool,
  puffs: bool,
  playing_gait: usize,
  /// The jump's entry, while it plays
  jump: Option<SkeletonTrackEntryId>,
}

impl Default for EventsPage {
  fn default() -> Self {
    Self {
      hero: None,
      patrol: Patrol::new(140.0, 660.0),
      sounds: Vec::new(),
      dust: None,
      thud: None,
      log: VecDeque::new(),
      count: 0,
      gait: 0,
      sound: true,
      puffs: true,
      playing_gait: 0,
      jump: None,
    }
  }
}

/// A linear volume as the decibels a play adds
fn decibels(volume: f32) -> f32 {
  20.0 * volume.max(1e-3).log10()
}

impl EventsPage {
  /// Where the foot an event names touches the ground: the skeleton's
  /// point attachments on its soles, in the world
  fn foot(&self, ctx: &SceneContext, which: &str) -> Option<Vec2> {
    let skeleton = ctx.world.get::<Skeleton>(self.hero?)?;
    let near = skeleton.point("sole_near").map(|(at, _)| at);
    let far = skeleton.point("sole_far").map(|(at, _)| at);
    match which {
      "near" => near,
      "far" => far,
      _ => Some((near? + far?) / 2.0),
    }
  }

  fn on_event(&mut self, ctx: &mut SceneContext, event: &SkeletonEvent) {
    self.count += 1;
    self.log.push_back(format!(
      "{} at {:.2} s: {} ({}, {:.1})",
      event.animation, event.time, event.name, event.string, event.float
    ));
    while self.log.len() > LOG {
      self.log.pop_front();
    }
    let Some(at) = self.foot(ctx, &event.string) else {
      return;
    };
    // The event's audio is data: its path, volume and balance. The game
    // plays it, here from where the foot is
    if self.sound
      && let Some(audio) = &event.audio
      && let Some((_, sound)) = self.sounds.iter().find(|(path, _)| *path == audio.path)
    {
      ctx.audio().play_with(
        sound,
        PlayOptions {
          position: Some(at),
          volume_db: decibels(audio.volume),
          ..Default::default()
        },
      );
    }
    if self.puffs {
      // The event's float says how hard the foot came down
      let puff = if event.float >= 2.0 {
        &self.thud
      } else {
        &self.dust
      };
      if let Some(puff) = puff {
        ctx.particles().play_at(puff, at);
      }
    }
  }
}

/// A low puff of dust of `count` motes, thrown up to `speed` fast: the
/// engine's dust preset, lighter and wider, played once
fn dust(image: Handle<Image>, count: f32, speed: f32) -> ParticleEffect {
  let mut effect = ParticleEffect::dust(image);
  effect.bursts = vec![Burst::at(0.0, count)];
  effect
    // Up, and out to both sides along the ground
    .with_direction(-90.0, 85.0)
    .with_speed((speed * 0.4, speed))
    .with_lifetime((0.45, 0.9))
    .with_size((8.0, 14.0))
    .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 2.4)]))
    .with_color_over_life(Gradient::new([
      (0.0, Color::new(236, 226, 196, 235)),
      (0.5, Color::new(226, 214, 184, 150)),
      (1.0, Color::new(206, 196, 168, 0)),
    ]))
    .with_drag(2.0)
}

impl DemoPage for EventsPage {
  fn name(&self) -> &str {
    "Events"
  }

  fn description(&self) -> &str {
    "Each footstep is an event key in the walk: a SkeletonEvent message with the key's values and its sound's path.\n\
     The page plays that sound where the foot is (a point attachment on the sole) and puffs dust there. The engine plays nothing itself."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Gait", &mut self.gait, &["Walk", "Run"]);
    s.toggle("Sound", &mut self.sound);
    s.toggle("Dust", &mut self.puffs);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Jump (a hard landing)")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    // An event names its sound by path: load each one the skeleton names
    // now, so a footstep never waits for a file
    let paths: Vec<Arc<str>> = ctx
      .assets()
      .get(&data)
      .map(|data| {
        data
          .events()
          .iter()
          .filter_map(|event| event.audio.as_ref().map(|audio| audio.path.clone()))
          .collect()
      })
      .unwrap_or_default();
    for path in paths {
      if let Ok(sound) = ctx.assets().load_now::<Sound>(&path) {
        self.sounds.push((path, sound));
      }
    }
    let mote = ctx
      .assets()
      .add(Image::solid_circle(16, 255, 255, 255, 255));
    self.dust = Some(ctx.assets().add(dust(mote.clone(), 9.0, 80.0)));
    self.thud = Some(ctx.assets().add(dust(mote, 28.0, 260.0)));

    let hero = hero::spawn(ctx, &data, Vec2::new(300.0, GROUND), SCALE);
    self.hero = Some(hero);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.set_animation(0, GAITS[self.gait].0, true);
    }
    self.playing_gait = self.gait;
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(hero) = self.hero else {
      return;
    };
    // What the skeleton's last step fired
    for event in ctx.messages::<SkeletonEvent>() {
      if event.entity == hero {
        self.on_event(ctx, &event);
      }
    }

    let jump = ctx.input.key_pressed(KeyCode::Space);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      if let Some(entry) = self.jump
        && state.entry(entry).is_none_or(|entry| entry.is_complete())
      {
        self.jump = None;
      }
      if jump && self.jump.is_none() {
        // The gait is queued behind the jump, and starts as it ends
        self.jump = state.set_animation(0, "jump", false);
        state.add_animation(0, GAITS[self.gait].0, true, 0.0);
        self.playing_gait = self.gait;
      } else if self.jump.is_none() && self.gait != self.playing_gait {
        state.set_animation(0, GAITS[self.gait].0, true);
        self.playing_gait = self.gait;
      }
    }
    if self.jump.is_none() {
      self.patrol.step(ctx, hero, GAITS[self.playing_gait].1, dt);
    }
    hero::lift_shadow(ctx, hero);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let mut lines = vec![
      format!("{} events", self.count),
      // What the description's second line says, which the shell does not
      // show
      "The page plays each sound from its event's path; the engine plays none itself".to_string(),
    ];
    lines.extend(self.log.iter().cloned());
    hero::readout(ui, &lines);
  }
}
