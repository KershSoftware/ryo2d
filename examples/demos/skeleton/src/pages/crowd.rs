//! Crowd: two hundred heroes at once (or fifty to four hundred), each its
//! own skeleton with its own skins, animation and pace, sorted by their feet

use demo_kit::prelude::*;

use crate::hero::{self, CANVAS, RUN_SPEED, WALK_SPEED};

/// How many heroes the setting offers, with each as the panel shows it
const COUNTS: [(usize, &str); 4] = [(50, "50"), (100, "100"), (200, "200"), (400, "400")];
/// A hero's size in the crowd
const SIZE: f32 = 0.2;
/// The field the crowd stands on: from this height down
const FIELD: f32 = 230.0;
/// How far past an edge a hero walks before it comes in at the other
const MARGIN: f32 = 50.0;

/// A hero that crosses the field: how fast, in pixels a second (negative
/// to the left)
#[derive(Component)]
struct Crossing(f32);

/// A small repeatable generator, so the crowd is the same each time
struct Dice(u32);

impl Dice {
  /// The next number from 0 up to 1
  fn roll(&mut self) -> f32 {
    self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (self.0 >> 8) as f32 / (1u32 << 24) as f32
  }

  fn below(&mut self, count: usize) -> usize {
    ((self.roll() * count as f32) as usize).min(count - 1)
  }
}

pub struct CrowdPage {
  data: Option<Handle<SkeletonData>>,
  heroes: Vec<Entity>,
  /// Setting: which of `COUNTS`
  count: usize,
  spawned: Option<usize>,
}

impl Default for CrowdPage {
  fn default() -> Self {
    Self {
      data: None,
      heroes: Vec::new(),
      count: 2,
      spawned: None,
    }
  }
}

impl CrowdPage {
  /// Replaces the crowd with as many heroes as the setting asks for
  fn fill(&mut self, ctx: &mut SceneContext) {
    let Some(data) = self.data.clone() else {
      return;
    };
    for hero in self.heroes.drain(..) {
      ctx.world.despawn(hero);
    }
    let mut dice = Dice(7);
    for _ in 0..COUNTS[self.count].0 {
      let at = Vec2::new(
        dice.roll() * CANVAS.x,
        FIELD + 70.0 + dice.roll() * (CANVAS.y - FIELD - 100.0),
      );
      let hero = hero::spawn(ctx, &data, at, SIZE);
      // Sorted among the others by where it stands: a skeleton's origin is
      // at its feet
      ctx.world.entity_mut(hero).insert(YSort::new());

      // A look: a body, an outfit, and a hat for one in five
      let mut skins = Vec::new();
      skins.extend([None, Some("blond"), Some("dark")][dice.below(3)]);
      skins.extend([None, Some("ranger"), Some("sailor")][dice.below(3)]);
      if dice.below(5) == 0 {
        skins.push("hat");
      }
      if let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero) {
        skeleton.set_skins(&skins);
      }

      // Something to do, at a pace of its own
      let (animation, speed) = match dice.below(10) {
        0..=3 => ("walk", WALK_SPEED),
        4..=5 => ("run", RUN_SPEED),
        _ => ("idle", 0.0),
      };
      let pace = 0.8 + dice.roll() * 0.4;
      let left = dice.below(2) == 0;
      let waves = speed == 0.0 && dice.below(3) == 0;
      if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
        if let Some(entry) = state.set_animation(0, animation, true)
          && let Some(entry) = state.entry_mut(entry)
        {
          entry.speed = pace;
        }
        if waves {
          state.set_animation(1, "wave", true);
        }
      }
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(hero) {
        renderable.flip_x = left;
      }
      if speed > 0.0 {
        let heading = if left { -1.0 } else { 1.0 };
        ctx
          .world
          .entity_mut(hero)
          .insert(Crossing(heading * speed * pace * SIZE));
      }
      self.heroes.push(hero);
    }
    self.spawned = Some(self.count);
  }
}

impl DemoPage for CrowdPage {
  fn name(&self) -> &str {
    "Crowd"
  }

  fn description(&self) -> &str {
    "A crowd of skeletons, 14 bones and 24 parts each, every one with its own skins and animation; some wave on a second track.\n\
     Each is drawn as one run of quads from the atlas, y-sorted by its feet. The frame cost is the CPU's, in this build."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Heroes", &mut self.count, &COUNTS.map(|(_, name)| name));
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::field(ctx, FIELD);
    self.data = hero::load(ctx);
    self.fill(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    if self.spawned != Some(self.count) {
      self.fill(ctx);
    }
    // Those crossing the field walk on, and come in again at the far side
    let mut wrapped = Vec::new();
    let mut crossing = ctx.world.query::<(Entity, &mut Transform, &Crossing)>();
    for (hero, mut transform, crossing) in crossing.iter_mut(ctx.world) {
      let mut x = transform.translation.x + crossing.0 * dt;
      if x > CANVAS.x + MARGIN {
        x = -MARGIN;
        wrapped.push(hero);
      } else if x < -MARGIN {
        x = CANVAS.x + MARGIN;
        wrapped.push(hero);
      }
      transform.translation.x = x;
    }
    // A hero that went round is drawn where it is, not sliding back across
    for hero in wrapped {
      ctx.skip_interpolation(hero);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let frame = ctx.resource::<FrameCost>().copied().map_or_else(
      || "Frame: measuring".to_string(),
      |cost| {
        format!(
          "Frame: {:.2} ms on the CPU (update {:.2}, render {:.2})",
          cost.total_ms(),
          cost.update_ms(),
          cost.render_ms()
        )
      },
    );
    hero::readout(
      ui,
      &[
        format!("{} skeletons", self.heroes.len()),
        frame,
        if cfg!(debug_assertions) {
          "Measured in a development build".to_string()
        } else {
          "Measured in a release build".to_string()
        },
      ],
    );
  }
}
