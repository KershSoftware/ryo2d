//! Constraints: bones placed by a rule after animation. An arm on a stand
//! reaches for the pointer by two-bone IK, and a transform constraint keeps
//! the tray in its hand level; a walker crosses uneven ground on two-part
//! legs whose feet IK puts on targets the page plants and steps ahead.
//!
//! The page moves only target bones, with world-space bone overrides, and
//! sets the arm's constraints' values with `override_constraint`.

use std::f32::consts::PI;

use demo_kit::prelude::*;

use crate::hero::{self, EDGE, GRASS, HAZE, SKY};

/// The two skeleton files
const REACHER: &str = "reacher.skeleton.ron";
const WALKER: &str = "walker.skeleton.ron";

/// Where the arm's stand is, right of the readout so the arm never
/// reaches over it, and its shoulder above the stand's foot (as the rig has
/// it)
const STAND: f32 = 780.0;
const SHOULDER: f32 = 268.0;
/// The arm's full reach: its two parts' lengths in the rig
const REACH: f32 = 250.0;
const TARGET: Color = Color::rgb(255, 214, 102);
/// How far above the wrist the `horizon` bone is kept: the overlay's line
/// from the hand to it is a short tick
const HORIZON: f32 = 24.0;

/// The ground: its mean height on the 1280 x 720 canvas, and the top of the
/// image drawn for it, clear above its highest point
const GROUND: f32 = 622.0;
const GROUND_TOP: f32 = 590.0;

/// The walker: the height of its hip above the ground under it (the rig's
/// legs are 140 long at full stretch), how far behind its hip a foot is
/// let go, and how high a foot lifts as it steps
const HIP: f32 = 112.0;
const STRIDE: f32 = 110.0;
const LIFT: f32 = 18.0;
/// How far the hip bobs up while a foot is in the air
const BOB: f32 = 3.0;
/// Where it walks from and to, off the canvas at each side: past the right
/// edge it starts again at the left
const WALK_FROM: f32 = -140.0;
const WALK_TO: f32 = 1420.0;

/// The ground's height under `x`: gentle hills, the same every run. The
/// walker plants its feet on it and the image of the ground is drawn from
/// it
pub fn ground_height(x: f32) -> f32 {
  GROUND + 15.0 * (x / 130.0 + 0.4).sin() + 6.0 * (x / 53.0 + 1.3).sin()
}

/// The mean height of the ground under the walker's two feet's reach
fn ground_under(x: f32) -> f32 {
  let samples = [-0.5, -0.25, 0.0, 0.25, 0.5];
  samples
    .iter()
    .map(|&at| ground_height(x + at * STRIDE))
    .sum::<f32>()
    / samples.len() as f32
}

/// A smooth start and stop over 0 to 1
fn ease(t: f32) -> f32 {
  t * t * (3.0 - 2.0 * t)
}

/// One of the walker's feet: planted where its target stands, or stepping
/// from one place to the next
#[derive(Clone, Copy)]
enum Foot {
  Planted(Vec2),
  Stepping { from: Vec2, to: Vec2, done: f32 },
}

impl Foot {
  /// Where the foot's target is now
  fn at(self) -> Vec2 {
    match self {
      Foot::Planted(at) => at,
      Foot::Stepping { from, to, done } => {
        let along = ease(done);
        let x = from.x + (to.x - from.x) * along;
        let y = from.y + (to.y - from.y) * along - LIFT * (PI * done).sin();
        // Never into a hill between the two places
        Vec2::new(x, y.min(ground_height(x)))
      }
    }
  }

  fn planted(self) -> bool {
    matches!(self, Foot::Planted(_))
  }

  /// How far through its step the foot is, 0 when planted
  fn swing(self) -> f32 {
    match self {
      Foot::Planted(_) => 0.0,
      Foot::Stepping { done, .. } => (PI * done).sin(),
    }
  }
}

/// The walker's gait: its hip's place along the ground and its two feet
struct Gait {
  x: f32,
  /// The near foot, then the far
  feet: [Foot; 2],
}

impl Gait {
  fn new(x: f32, speed: f32) -> Self {
    let mut gait = Self {
      x,
      feet: [Foot::Planted(Vec2::ZERO); 2],
    };
    gait.place_feet(speed);
    gait
  }

  /// The seconds a step takes at `speed`: a share of the time a foot stands,
  /// so a foot is down again well before the other must go
  fn step_time(speed: f32) -> f32 {
    (0.45 * STRIDE / speed).min(0.4)
  }

  /// Plants the feet as they are in a steady walk at `speed`: one just put
  /// down ahead of the hip, the other half a cycle behind it
  fn place_feet(&mut self, speed: f32) {
    let ahead = self.x + STRIDE / 2.0;
    let behind = self.x - speed * Self::step_time(speed) / 2.0;
    self.feet = [
      Foot::Planted(Vec2::new(ahead, ground_height(ahead))),
      Foot::Planted(Vec2::new(behind, ground_height(behind))),
    ];
  }

  /// Carries the hip on by `speed` for `dt`. A planted foot stays put until
  /// the hip is half a stride past it, and then, if the other foot is
  /// down, steps to half a stride ahead of where the hip will be when it
  /// lands. True when the walker has gone off the right and starts again
  /// at the left
  fn step(&mut self, speed: f32, dt: f32) -> bool {
    self.x += speed * dt;
    let time = Self::step_time(speed);
    for foot in &mut self.feet {
      if let Foot::Stepping { to, done, .. } = foot {
        *done += dt / time;
        if *done >= 1.0 {
          *foot = Foot::Planted(*to);
        }
      }
    }
    for index in 0..2 {
      let other = self.feet[1 - index];
      if let Foot::Planted(at) = self.feet[index]
        && self.x - at.x > STRIDE / 2.0
        && other.planted()
      {
        let x = self.x + speed * time + STRIDE / 2.0;
        self.feet[index] = Foot::Stepping {
          from: at,
          to: Vec2::new(x, ground_height(x)),
          done: 0.0,
        };
      }
    }
    if self.x > WALK_TO {
      self.x = WALK_FROM;
      self.place_feet(speed);
      return true;
    }
    false
  }

  /// Where the hip is: above the ground under the feet, bobbing up a
  /// little while a foot is in the air
  fn hip(&self) -> Vec2 {
    let lift = self
      .feet
      .iter()
      .map(|foot| foot.swing())
      .fold(0.0, f32::max);
    Vec2::new(self.x, ground_under(self.x) - HIP - BOB * lift)
  }
}

pub struct ConstraintsPage {
  reacher: Option<Entity>,
  walker: Option<Entity>,
  gait: Gait,
  /// Settings: the arm's IK values, the tray's rotate mix, and the
  /// walker's speed
  bend: usize,
  softness: f32,
  stretch: bool,
  mix: f32,
  level: f32,
  speed: f32,
  /// Where the pointer was when the page opened: until it moves, the arm
  /// reaches for a place that circles its shoulder, in and out of reach
  pointer_at_first: Option<Vec2>,
  pointing: bool,
  clock: f32,
  /// Where the arm reaches, and how far that is from its shoulder
  target: Vec2,
  distance: f32,
}

impl Default for ConstraintsPage {
  fn default() -> Self {
    Self {
      reacher: None,
      walker: None,
      gait: Gait::new(560.0, 120.0),
      bend: 0,
      softness: 0.0,
      stretch: false,
      mix: 1.0,
      level: 1.0,
      speed: 120.0,
      pointer_at_first: None,
      pointing: false,
      clock: 0.0,
      target: Vec2::ZERO,
      distance: 0.0,
    }
  }
}

/// The ground as an image, `CANVAS` wide from `GROUND_TOP` down: grass
/// under a lighter edge that follows the hills, its top pixel blended by
/// how much of it the ground covers
fn ground_image() -> Image {
  let width = hero::CANVAS.x as u32;
  let height = (hero::CANVAS.y - GROUND_TOP) as u32;
  let mut pixels = vec![0u8; (width * height * 4) as usize];
  for column in 0..width {
    let top = ground_height(column as f32 + 0.5) - GROUND_TOP;
    for row in 0..height {
      let y = row as f32;
      // How much of this pixel is under the ground's top, and whether it
      // is in the edge's band
      let cover = (y + 1.0 - top).clamp(0.0, 1.0);
      if cover <= 0.0 {
        continue;
      }
      let color = if y < top + 6.0 { EDGE } else { GRASS };
      let index = ((row * width + column) * 4) as usize;
      pixels[index..index + 4].copy_from_slice(&[
        color.r,
        color.g,
        color.b,
        (cover * 255.0).round() as u8,
      ]);
    }
  }
  Image::from_rgba(pixels, width, height)
}

/// A skeleton entity of `data` standing at `at`, drawn at its art's size,
/// with a small shadow `width` wide where it stands
fn spawn(
  ctx: &mut SceneContext,
  data: Handle<SkeletonData>,
  at: Vec2,
  width: f32,
  z: i32,
) -> Entity {
  ctx
    .world
    .spawn((
      Transform::from_xy(at.x, at.y),
      Skeleton::new(data),
      SkeletonAnimation::new(),
      Renderable::skeleton().with_z_order(z).with_blob_shadow(
        BlobShadow::ellipse(width, width * 0.18).color(Color::new(10, 14, 30, 110)),
      ),
    ))
    .id()
}

impl ConstraintsPage {
  /// The arm's shoulder in the world, as the rig puts it on its stand
  fn shoulder() -> Vec2 {
    Vec2::new(STAND, ground_height(STAND) - SHOULDER)
  }

  /// Moves the arm's target and sets its constraints from the settings
  fn reach(&mut self, ctx: &mut SceneContext, arm: Entity) {
    let pointer = hero::pointer(ctx);
    match self.pointer_at_first {
      None => self.pointer_at_first = Some(pointer),
      Some(first) if first != pointer => self.pointing = true,
      Some(_) => {}
    }
    let shoulder = Self::shoulder();
    self.target = if self.pointing {
      pointer
    } else {
      // Round the shoulder, from well inside the reach to past it
      let turn = -0.6 + self.clock * 0.7;
      let out = 185.0 + 85.0 * (self.clock * 1.3).sin();
      shoulder + Vec2::new(turn.cos(), turn.sin()) * out
    };
    self.distance = self.target.distance(shoulder);

    let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(arm) else {
      return;
    };
    // The level constraint copies only the horizon bone's rotation, which
    // stays 0: it is kept just above the wrist (where the last step left
    // it), so debug:skeletons draws its line as a short tick there
    if let Some(hand) = skeleton.bone_world("hand") {
      skeleton.override_bone(
        "horizon",
        BoneOverride {
          translation: Some(hand.translation - Vec2::new(0.0, HORIZON)),
          space: BoneSpace::World,
          ..Default::default()
        },
      );
    }
    skeleton.override_bone(
      "target",
      BoneOverride {
        translation: Some(self.target),
        space: BoneSpace::World,
        ..Default::default()
      },
    );
    skeleton.override_constraint(
      "arm",
      SkeletonConstraintOverride::Ik(IkConstraintOverride {
        mix: Some(self.mix),
        softness: Some(self.softness),
        bend_positive: Some(self.bend == 1),
        stretch: Some(self.stretch),
        ..Default::default()
      }),
    );
    skeleton.override_constraint(
      "level",
      SkeletonConstraintOverride::Transform(TransformConstraintOverride {
        rotate: Some(self.level),
        ..Default::default()
      }),
    );
    let mut debug = ctx.debug_draw();
    debug.width(2.0).cross(self.target, 16.0, TARGET);
    debug.width(2.0).circle(self.target, 10.0, TARGET);
  }

  /// Walks the walker on and puts its body and its feet's targets where its
  /// gait has them
  fn walk(&mut self, ctx: &mut SceneContext, walker: Entity, dt: f32) {
    let wrapped = self.gait.step(self.speed, dt);
    let hip = self.gait.hip();
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(walker) {
      transform.translation = Vec2::new(hip.x, ground_height(hip.x));
    }
    if wrapped {
      // From the right edge to the left at once, not drawn along the way
      ctx.skip_interpolation(walker);
    }
    let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(walker) else {
      return;
    };
    let mut place = |bone: &str, at: Vec2| {
      skeleton.override_bone(
        bone,
        BoneOverride {
          translation: Some(at),
          space: BoneSpace::World,
          ..Default::default()
        },
      );
    };
    place("body", hip);
    place("foot_near", self.gait.feet[0].at());
    place("foot_far", self.gait.feet[1].at());
  }

  /// What the arm does with its target now, for the readout
  fn reaching(&self) -> String {
    let at = format!("Target {:.0} from the shoulder", self.distance);
    if self.mix <= 0.0 {
      return format!("{at}: at a mix of 0 the arm keeps its setup pose");
    }
    if self.distance <= REACH - self.softness {
      return format!("{at}: in reach ({REACH:.0}), the wrist on it");
    }
    if self.softness > 0.0 {
      return format!("{at}: within the softness of full reach, the wrist slows short of it");
    }
    if self.stretch {
      format!("{at}: out of reach, the upper arm stretched to it")
    } else {
      format!("{at}: out of reach, the arm points at it")
    }
  }
}

impl DemoPage for ConstraintsPage {
  fn name(&self) -> &str {
    "Constraints"
  }

  fn description(&self) -> &str {
    "Constraints place bones by a rule after animation: two-bone IK reaches the wrist for the pointer and puts the walker's feet on its targets.\n\
     A transform constraint keeps the tray level. Bend, Softness, Stretch and Mix are the arm's IK values, set with override_constraint."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Bend", &mut self.bend, &["Negative", "Positive"]);
    s.float("Softness", &mut self.softness, 0.0..=60.0, 5.0);
    s.toggle("Stretch", &mut self.stretch);
    s.float("Mix", &mut self.mix, 0.0..=1.0, 0.1);
    s.float("Level hand", &mut self.level, 0.0..=1.0, 0.1);
    s.float("Speed", &mut self.speed, 40.0..=240.0, 20.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Move the arm's target")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::band(ctx, 0.0, hero::CANVAS.y, SKY, -30);
    // The haze runs down behind the ground, so its valleys show it
    hero::band(
      ctx,
      GROUND - 160.0,
      hero::CANVAS.y - GROUND + 160.0,
      HAZE,
      -29,
    );
    let ground = ctx.assets().add(ground_image());
    ctx.world.spawn((
      Transform::from_xy(0.0, GROUND_TOP),
      Renderable::image(ground)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(-28),
    ));

    let (Ok(arm), Ok(walker)) = (
      ctx.assets().load_now::<SkeletonData>(REACHER),
      ctx.assets().load_now::<SkeletonData>(WALKER),
    ) else {
      return;
    };
    let arm = spawn(ctx, arm, Vec2::new(STAND, ground_height(STAND)), 70.0, -1);
    self.reacher = Some(arm);
    let hip = self.gait.hip();
    let walker = spawn(ctx, walker, Vec2::new(hip.x, ground_height(hip.x)), 90.0, 0);
    self.walker = Some(walker);
    // Posed before the first frame, so it never shows its setup pose
    self.reach(ctx, arm);
    self.walk(ctx, walker, 0.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.clock += dt;
    if let Some(arm) = self.reacher {
      self.reach(ctx, arm);
    }
    if let Some(walker) = self.walker {
      self.walk(ctx, walker, dt);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    if self.reacher.is_none() {
      return;
    }
    let aim = if self.pointing {
      "the pointer"
    } else {
      "a circling target (move the pointer to aim)"
    };
    hero::readout(
      ui,
      &[
        format!("The arm reaches for {aim}"),
        "IK \"arm\": turns the upper arm and forearm so the wrist reaches its target".to_string(),
        format!(
          "Transform \"level\": turns the hand to the horizon bone's rotation, mix {:.1}",
          self.level
        ),
        "IK \"leg_near\" and \"leg_far\": put each foot on a target the page plants".to_string(),
        self.reaching(),
        "In a development build's console, debug:skeletons marks each target".to_string(),
      ],
    );
  }
}
