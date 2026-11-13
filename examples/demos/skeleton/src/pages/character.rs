//! Character: one skeleton, played like a game's hero. Keys pick the
//! animation on track 0, and each change crossfades over the mix the
//! skeleton file gives that pair

use demo_kit::prelude::*;

use crate::hero::{self, CANVAS, GROUND, RUN_SPEED, SCALE, WALK_SPEED};

/// How far past an edge of the canvas the hero walks before it comes in
/// at the other
const MARGIN: f32 = 80.0;

/// What the keys ask of the hero
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gait {
  Idle,
  Walk,
  Run,
}

impl Gait {
  fn animation(self) -> &'static str {
    match self {
      Gait::Idle => "idle",
      Gait::Walk => "walk",
      Gait::Run => "run",
    }
  }

  /// How fast it carries the hero, in the hero's units a second
  fn speed(self) -> f32 {
    match self {
      Gait::Idle => 0.0,
      Gait::Walk => WALK_SPEED,
      Gait::Run => RUN_SPEED,
    }
  }
}

pub struct CharacterPage {
  hero: Option<Entity>,
  /// The gait whose animation plays, or will when the jump is over
  gait: Gait,
  /// The jump's entry, while it plays
  jump: Option<SkeletonTrackEntryId>,
  /// Settings: mix as the file says, or cut from one animation to the next
  crossfade: bool,
  /// The skeleton's time scale
  speed: f32,
}

impl Default for CharacterPage {
  fn default() -> Self {
    Self {
      hero: None,
      gait: Gait::Idle,
      jump: None,
      crossfade: true,
      speed: 1.0,
    }
  }
}

impl CharacterPage {
  /// Plays `name` on track 0, mixing from what plays there
  fn play(
    &self,
    ctx: &mut SceneContext,
    name: &str,
    looping: bool,
  ) -> Option<SkeletonTrackEntryId> {
    let mut state = ctx.world.get_mut::<SkeletonAnimation>(self.hero?)?;
    let entry = state.set_animation(0, name, looping)?;
    if !self.crossfade
      && let Some(entry) = state.entry_mut(entry)
    {
      // The file's mix for the pair is the entry's until it is changed
      entry.mix_duration = 0.0;
    }
    Some(entry)
  }
}

impl DemoPage for CharacterPage {
  fn name(&self) -> &str {
    "Character"
  }

  fn description(&self) -> &str {
    "One skeleton with idle, walk, run and jump on track 0; each change crossfades over the mix its file gives the pair.\n\
     The rig is assets/hero.skeleton.ron: 14 bones, parts from one atlas. In a development build's console, debug:skeletons draws its bones."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Crossfade", &mut self.crossfade);
    s.float("Time scale", &mut self.speed, 0.1..=2.0, 0.1);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("A D", &[KeyCode::KeyA, KeyCode::KeyD], "Walk"),
      Control::keys("Shift", &[KeyCode::ShiftLeft, KeyCode::ShiftRight], "Run"),
      Control::key(KeyCode::Space, "Jump"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    let hero = hero::spawn(ctx, &data, Vec2::new(420.0, GROUND), SCALE);
    self.hero = Some(hero);
    self.play(ctx, self.gait.animation(), true);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(hero) = self.hero else {
      return;
    };
    let input = &ctx.input;
    let direction =
      f32::from(input.key_down(KeyCode::KeyD)) - f32::from(input.key_down(KeyCode::KeyA));
    let running = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
    let jump = input.key_pressed(KeyCode::Space);
    let wanted = match (direction != 0.0, running) {
      (false, _) => Gait::Idle,
      (true, false) => Gait::Walk,
      (true, true) => Gait::Run,
    };

    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.time_scale = self.speed;
    }
    // The jump is over once its entry has played to its end
    if let Some(entry) = self.jump {
      let over = ctx
        .world
        .get::<SkeletonAnimation>(hero)
        .and_then(|state| state.entry(entry))
        .is_none_or(|entry| entry.is_complete());
      if over {
        self.jump = None;
        self.play(ctx, wanted.animation(), true);
      }
    } else if jump {
      self.jump = self.play(ctx, "jump", false);
    } else if wanted != self.gait {
      self.play(ctx, wanted.animation(), true);
    }
    self.gait = wanted;

    // The entity walks; the animation only moves the limbs. It turns round
    // with the renderable's flip, and leaves one side to come in at the other
    let step = direction * wanted.speed() * SCALE * self.speed * dt;
    if step != 0.0 {
      let mut wrapped = false;
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(hero) {
        let mut x = transform.translation.x + step;
        if x > CANVAS.x + MARGIN {
          x = -MARGIN;
          wrapped = true;
        } else if x < -MARGIN {
          x = CANVAS.x + MARGIN;
          wrapped = true;
        }
        transform.translation.x = x;
      }
      // Drawn where it now is, not sliding back across the screen
      if wrapped {
        ctx.skip_interpolation(hero);
      }
      hero::face(ctx, hero, direction < 0.0);
    }
    hero::lift_shadow(ctx, hero);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let Some(hero) = self.hero else {
      return;
    };
    let mix = if self.crossfade {
      "crossfading over the file's mixes"
    } else {
      "cutting, with no mix"
    };
    hero::readout(
      ui,
      &[
        format!(
          "Playing {}",
          self.jump.map_or(self.gait.animation(), |_| "jump")
        ),
        hero::track_line(ctx, hero, 0),
        mix.to_string(),
      ],
    );
  }
}
