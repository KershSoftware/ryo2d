//! What the pages share: the hero and how one is added, the ground it stands
//! on, the pointer, and the lines of text a page draws

use demo_kit::prelude::*;

/// The hero's skeleton file: bones, slots, skins, animations and events
const HERO: &str = "hero.skeleton.ron";

/// The hero's size on screen against its art's: the parts are drawn for a
/// figure about 530 pixels tall
pub const SCALE: f32 = 0.45;
/// The ground's height on the 1280 x 720 canvas, where the heroes stand
pub const GROUND: f32 = 600.0;
/// The canvas's size
pub const CANVAS: Vec2 = Vec2::new(1280.0, 720.0);

/// How fast a hero's feet carry it, in its own units a second: a stride's
/// length over its time, so the feet do not slide (the entity's scale takes
/// it to pixels)
pub const WALK_SPEED: f32 = 350.0;
pub const RUN_SPEED: f32 = 900.0;

/// Labels and hints
const DIM: Color = Color::rgb(190, 196, 214);
/// A value that matters now
const HOT: Color = Color::rgb(255, 214, 102);

/// The hero's skeleton, loaded at once so the first frame draws it. `None`
/// if the file is missing or wrong (the registry warns, naming what)
pub fn load(ctx: &mut SceneContext) -> Option<Handle<SkeletonData>> {
  ctx.assets().load_now(HERO).ok()
}

/// A hero of `data` standing at `at`, drawn at `scale` of its art.
///
/// It is a skeleton entity as a character wants one. Its `Skeleton` makes
/// the entity `Interpolated`, so its bones move smoothly between fixed
/// steps with nothing asked for here; and it has a blob shadow under its
/// feet, not a drop shadow (each part of a skeleton casts its own drop
/// shadow, darker where they overlap)
pub fn spawn(ctx: &mut SceneContext, data: &Handle<SkeletonData>, at: Vec2, scale: f32) -> Entity {
  ctx
    .world
    .spawn((
      Transform::from_xy(at.x, at.y).with_uniform_scale(scale),
      Skeleton::new(data.clone()),
      SkeletonAnimation::new(),
      Renderable::skeleton().with_blob_shadow(shadow(scale)),
    ))
    .id()
}

/// The shadow under a hero drawn at `scale`: it shrinks and fades as the
/// hero leaves the ground ([`lift_shadow`])
fn shadow(scale: f32) -> BlobShadow {
  BlobShadow::ellipse(120.0 * scale, 22.0 * scale)
    .color(Color::new(10, 14, 30, 110))
    .with_height_scale(true)
    .max_height(520.0)
}

/// Shrinks a hero's shadow by how far its hips are above where they stand:
/// a jump is in the animation, so the entity stays on the ground and the
/// bone says how high the hero is
pub fn lift_shadow(ctx: &mut SceneContext, hero: Entity) {
  /// The hip bone's height above the feet in the setup pose
  const HIP: f32 = 180.0;
  let Some(skeleton) = ctx.world.get::<Skeleton>(hero) else {
    return;
  };
  let (Some(hip), Some(root)) = (skeleton.bone_world("hip"), skeleton.bone_world("root")) else {
    return;
  };
  // Both in the world: back to the hero's own units by the root's scale
  let scale = root.matrix2.y_axis.length().max(f32::EPSILON);
  let height = ((root.translation.y - hip.translation.y) / scale - HIP).max(0.0);
  // The renderable is written only when the height moves, so a hero
  // standing still changes nothing
  let moved = ctx
    .world
    .get::<Renderable>(hero)
    .and_then(|renderable| renderable.blob_shadow.as_ref())
    .is_some_and(|shadow| (shadow.height - height).abs() > 0.5);
  if moved
    && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(hero)
    && let Some(shadow) = &mut renderable.blob_shadow
  {
    shadow.set_height(height);
  }
}

/// Turns `hero` to face left or right, with its renderable's flip: the
/// skeleton mirrors about where it stands, its sockets with it. Written
/// only when it turns, so the engine sees a change only then
pub fn face(ctx: &mut SceneContext, hero: Entity, left: bool) {
  if ctx
    .world
    .get::<Renderable>(hero)
    .is_some_and(|renderable| renderable.flip_x != left)
    && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(hero)
  {
    renderable.flip_x = left;
  }
}

/// A flat band of `color` across the canvas, `height` tall from `top`
fn band(ctx: &mut SceneContext, top: f32, height: f32, color: Color, z: i32) {
  let image = ctx
    .assets()
    .add(Image::solid_rect(8, 8, color.r, color.g, color.b, 255));
  ctx.world.spawn((
    Transform::from_xy(0.0, top).with_scale(Vec2::new(CANVAS.x / 8.0, height / 8.0)),
    Renderable::image(image)
      .with_anchor(Anchor::TopLeft)
      .with_z_order(z),
  ));
}

const SKY: Color = Color::rgb(46, 64, 104);
const HAZE: Color = Color::rgb(52, 76, 116);
const GRASS: Color = Color::rgb(66, 104, 88);
const EDGE: Color = Color::rgb(92, 138, 104);

/// The sky and the ground the heroes stand on, behind everything
pub fn backdrop(ctx: &mut SceneContext) {
  band(ctx, 0.0, CANVAS.y, SKY, -30);
  band(ctx, GROUND - 150.0, 150.0, HAZE, -29);
  band(ctx, GROUND - 26.0, CANVAS.y - GROUND + 26.0, GRASS, -28);
  band(ctx, GROUND - 26.0, 6.0, EDGE, -27);
}

/// The sky over a field that starts at `top`, for a page whose heroes
/// stand at many depths
pub fn field(ctx: &mut SceneContext, top: f32) {
  band(ctx, 0.0, CANVAS.y, SKY, -30);
  band(ctx, top - 90.0, 90.0, HAZE, -29);
  band(ctx, top, CANVAS.y - top, GRASS, -28);
  band(ctx, top, 6.0, EDGE, -27);
}

/// The pointer in world space
pub fn pointer(ctx: &mut SceneContext) -> Vec2 {
  let (x, y) = ctx.input.mouse_position();
  ctx.camera().screen_to_world(Vec2::new(x, y))
}

/// What plays on `track` of `hero`, for a page's readout: the animation's
/// name and how far into it the entry is
pub fn track_line(ctx: &SceneContext, hero: Entity, track: usize) -> String {
  let entry = ctx
    .world
    .get::<SkeletonAnimation>(hero)
    .and_then(|state| state.entry(state.current(track)?));
  match entry {
    Some(entry) => format!(
      "Track {track}: {} {:.2} s, alpha {:.2}",
      entry.animation(),
      entry.track_time(),
      entry.alpha
    ),
    None => format!("Track {track}: nothing"),
  }
}

/// Lines of text down the left of the content area, on a dim backing so
/// they read over the scene. The first is the heading
pub fn readout(ui: &mut UiContext, lines: &[String]) {
  let area = content_area(ui);
  let (x, mut y) = (area.x + 10.0, area.y + 8.0);
  for (index, line) in lines.iter().enumerate() {
    let (width, height) = ui.text_size(line);
    let (width, height) = (width as f32, height as f32);
    ui.draw_rect(
      Rect::new(x - 4.0, y - 2.0, width + 8.0, height + 4.0),
      Color::new(14, 18, 34, 170),
    );
    ui.draw_text(line, x, y, if index == 0 { HOT } else { DIM });
    y += height + 6.0;
  }
}

/// A hero walking to and fro between two places on the ground, turning
/// round at each with its renderable's flip
pub struct Patrol {
  left: f32,
  right: f32,
  /// 1 walking right, -1 walking left
  heading: f32,
}

impl Patrol {
  pub fn new(left: f32, right: f32) -> Self {
    Self {
      left,
      right,
      heading: 1.0,
    }
  }

  /// Carries `hero` on by `speed` of its own units a second for `dt`
  pub fn step(&mut self, ctx: &mut SceneContext, hero: Entity, speed: f32, dt: f32) {
    let Some(mut transform) = ctx.world.get_mut::<Transform>(hero) else {
      return;
    };
    let scale = transform.scale.x.abs();
    let x = transform.translation.x + self.heading * speed * scale * dt;
    if x >= self.right {
      self.heading = -1.0;
    } else if x <= self.left {
      self.heading = 1.0;
    }
    transform.translation.x = x.clamp(self.left, self.right);
    face(ctx, hero, self.heading < 0.0);
  }
}
