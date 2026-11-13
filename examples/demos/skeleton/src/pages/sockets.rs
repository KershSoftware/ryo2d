//! Sockets: an entity on a bone. A `BoneSocket` child of the skeleton's
//! entity is placed on the bone it names in the same step, so a sword in
//! the hand moves with the hand, and mirrors with a hero turned round

use demo_kit::prelude::*;

use crate::hero::{self, GROUND, SCALE};

/// The sword's image: 300 x 68, its grip 32 from the left
const SWORD: &str = "sword.png";
/// Seconds between the cuts the page makes itself, and before the first
const EVERY: f32 = 1.7;
const FIRST: f32 = 1.2;
const HIT: Color = Color::rgb(255, 96, 84);

/// A hero with a sword, and the parts of the sword
struct Fighter {
  hero: Entity,
  /// The entity at the blade's tip, which draws the trail
  tip: Entity,
  /// Seconds until it next cuts by itself
  wait: f32,
}

pub struct SocketsPage {
  fighters: Vec<Fighter>,
  /// The hit box's corners in the world, as the skeleton last gave them
  corners: Vec<Vec2>,
  /// How many of the two blades cut this step
  cutting: usize,
  /// Settings
  auto: bool,
  hit_box: usize,
  trail: bool,
  shown_hit_box: usize,
}

impl Default for SocketsPage {
  fn default() -> Self {
    Self {
      fighters: Vec::new(),
      corners: Vec::new(),
      cutting: 0,
      auto: true,
      hit_box: 0,
      trail: true,
      shown_hit_box: 0,
    }
  }
}

/// Puts a sword in `hero`'s hand: a socket on its weapon bone, and under
/// the socket the blade's sprite and an entity at its tip with a trail.
/// Returns the tip
fn arm(ctx: &mut SceneContext, hero: Entity, sword: &Handle<Image>) -> Entity {
  // The socket takes the bone's place, direction and scale each step; its
  // children follow as any child follows its parent
  let socket = ctx.spawn_child(hero, BoneSocket::new("weapon"));
  ctx.spawn_child(
    socket,
    (
      // The image's centre is 118 along the blade from the grip
      Transform::from_xy(118.0, 0.0),
      Renderable::image(sword.clone()).with_z_order(1),
    ),
  );
  ctx.spawn_child(
    socket,
    (
      Transform::from_xy(258.0, 0.0),
      // A streak that lasts as long as the cut does
      Trail2D::swipe(Color::new(255, 250, 225, 150), 26.0)
        .with_length(24)
        .with_lifetime(0.2)
        .with_min_distance(8.0)
        .with_join(LineJoin::Round)
        .with_style(TrailStyle::Soft)
        .with_z_order(1),
    ),
  )
}

impl SocketsPage {
  fn cut(ctx: &mut SceneContext, hero: Entity) {
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.set_animation(0, "slash", false);
      // Back to idle as the cut ends
      state.add_animation(0, "idle", true, 0.0);
    }
  }
}

impl DemoPage for SocketsPage {
  fn name(&self) -> &str {
    "Sockets"
  }

  fn description(&self) -> &str {
    "Each sword is a sprite under a BoneSocket on the hero's weapon bone: it is where the bone is in the same step, and mirrors with the hero.\n\
     The trail is an entity at the blade's tip. The red box is a bounding-box attachment the slash turns on while it cuts."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Cut by itself", &mut self.auto);
    s.choice(
      "Hit box",
      &mut self.hit_box,
      &["While cutting", "Always", "Never"],
    );
    s.toggle("Trail", &mut self.trail);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Cut"),
      Control::key(KeyCode::KeyF, "Turn both round"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    let Ok(sword) = ctx.assets().load_now::<Image>(SWORD) else {
      return;
    };
    // One facing each way: the second is flipped, and its sword with it
    for (x, flip, wait) in [(250.0, false, FIRST), (610.0, true, FIRST + 0.1)] {
      let hero = hero::spawn(ctx, &data, Vec2::new(x, GROUND), SCALE);
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(hero) {
        renderable.flip_x = flip;
      }
      if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
        state.set_animation(0, "idle", true);
      }
      let tip = arm(ctx, hero, &sword);
      self.fighters.push(Fighter { hero, tip, wait });
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let cut = ctx.input.key_pressed(KeyCode::Space);
    let turn = ctx.input.key_pressed(KeyCode::KeyF);
    let hit_box_changed = self.hit_box != self.shown_hit_box;
    self.shown_hit_box = self.hit_box;
    self.cutting = 0;
    for fighter in &mut self.fighters {
      fighter.wait -= dt;
      if cut || (self.auto && fighter.wait <= 0.0) {
        Self::cut(ctx, fighter.hero);
        fighter.wait = EVERY;
      }
      if turn && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(fighter.hero) {
        renderable.flip_x = !renderable.flip_x;
      }
      // The blade lands on the hero's other side when it turns: the trail
      // starts afresh there, and when it is switched on
      if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(fighter.tip)
        && (turn || trail.is_enabled() != self.trail)
      {
        trail.set_enabled(self.trail);
        trail.clear();
      }
      let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(fighter.hero) else {
        continue;
      };
      if hit_box_changed {
        match self.hit_box {
          // The slash's own keys show and hide it
          0 => skeleton.clear_attachment("hit"),
          // The game's attachment beats the animation's, until cleared
          1 => skeleton.set_attachment("hit", Some("hit")),
          _ => skeleton.set_attachment("hit", None),
        }
      }
      // The box in the world, where the weapon bone carries it: empty
      // while its slot shows nothing
      if skeleton.bounding_box("hit", &mut self.corners) {
        self.cutting += 1;
        // Closed, for the outline
        self.corners.extend(self.corners.first().copied());
        ctx.debug_draw().width(2.0).polyline(&self.corners, HIT);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let hit = match self.cutting {
      0 => "Hit box off".to_string(),
      count => format!("Hit box on: {count}"),
    };
    hero::readout(
      ui,
      &[
        "A sword on each weapon bone".to_string(),
        hit,
        "The red box is a bounding-box attachment; the trail is an entity at the blade's tip"
          .to_string(),
        "The right-hand hero is flipped".to_string(),
      ],
    );
  }
}
