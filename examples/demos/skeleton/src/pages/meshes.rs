//! Meshes: an attachment drawn as triangles, where a region is one
//! rectangle on one bone. The hero's cape is a mesh whose vertices are
//! weighted over three bones, so the cloth bends as the bones swing; the
//! banner's flag is a mesh on one bone that stands still, rippled by the
//! deform keys of an animation

use demo_kit::prelude::*;

use crate::hero::{self, GROUND, Patrol, RUN_SPEED, SCALE, WALK_SPEED};

/// The banner's skeleton file: a pole, a flag and the flag's ripple
const BANNER: &str = "banner.skeleton.ron";
/// Where the banner's pole stands, clear of where the hero walks, and its
/// size on screen against its art's
const POLE: Vec2 = Vec2::new(640.0, GROUND);
const BANNER_SCALE: f32 = 0.5;
/// The animations the cape's bones are keyed in, and how fast each carries
/// the hero
const GAITS: [(&str, f32); 3] = [("walk", WALK_SPEED), ("run", RUN_SPEED), ("idle", 0.0)];
/// The skin that has the cape's mesh
const CAPE: &str = "cape";
/// Seconds a hit flash takes to fade
const FLASH: f32 = 0.3;

pub struct MeshesPage {
  hero: Option<Entity>,
  banner: Option<Entity>,
  patrol: Patrol,
  /// What each mesh is, read from its skeleton's data, for the readout
  cape: String,
  flag: String,
  /// Seconds of the hit flash left
  flash: f32,
  /// Settings
  gait: usize,
  caped: bool,
  ripple: f32,
  /// What the hero was last asked to play and to wear
  playing_gait: usize,
  wearing: Option<bool>,
}

impl Default for MeshesPage {
  fn default() -> Self {
    Self {
      hero: None,
      banner: None,
      patrol: Patrol::new(140.0, 460.0),
      cape: String::new(),
      flag: String::new(),
      flash: 0.0,
      gait: 0,
      caped: true,
      ripple: 1.0,
      playing_gait: 0,
      wearing: None,
    }
  }
}

/// The mesh the skin `skin` of `data` has in `slot` under `name`
fn mesh<'a>(
  data: &'a SkeletonData,
  skin: &str,
  slot: &str,
  name: &str,
) -> Option<&'a MeshAttachment> {
  match data.skin(skin)?.attachment(data.slot(slot)?, name)? {
    SkeletonAttachment::Mesh(mesh) => Some(mesh),
    _ => None,
  }
}

/// What holds the vertices of `mesh`: the bones they are weighted over, or
/// the one bone of its slot
fn held(mesh: &MeshAttachment) -> String {
  match &mesh.vertices {
    MeshVertices::Weighted(weights) => {
      let mut bones: Vec<BoneId> = (0..weights.vertex_count())
        .flat_map(|vertex| weights.influences(vertex))
        .map(|influence| influence.bone)
        .collect();
      bones.sort_unstable_by_key(|bone| bone.index());
      bones.dedup();
      format!("weighted over {} bones", bones.len())
    }
    MeshVertices::Unweighted(_) => "on one bone".to_string(),
  }
}

/// How many keys `animation` of `data` has for the shape of the mesh
/// `slot` shows under `name`
fn deform_keys(data: &SkeletonData, animation: &str, slot: &str, name: &str) -> usize {
  let (Some(animation), Some(slot)) = (data.animation(animation), data.slot(slot)) else {
    return 0;
  };
  animation
    .timelines
    .iter()
    .map(|timeline| match timeline {
      SkeletonTimeline::Deform {
        slot: keyed,
        attachment,
        frames,
      } if *keyed == slot && &**attachment == name => frames.len(),
      _ => 0,
    })
    .sum()
}

/// A mesh's size, for the readout
fn size(mesh: &MeshAttachment) -> String {
  format!(
    "{} vertices in {} triangles",
    mesh.vertex_count(),
    mesh.triangles.len() / 3
  )
}

impl MeshesPage {
  /// Puts the cape's skin on the hero, or takes it off: without it the
  /// cape's slot names an attachment no skin worn has, and shows nothing
  fn wear(&mut self, ctx: &mut SceneContext) {
    if self.wearing == Some(self.caped) {
      return;
    }
    if let Some(hero) = self.hero
      && let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero)
    {
      skeleton.set_skins(if self.caped { &[CAPE] } else { &[] });
      self.wearing = Some(self.caped);
    }
  }

  /// Shows the hit flash as far as it has faded. The flash is the entity's
  /// sprite effect: every part of the skeleton takes it, the cape's mesh
  /// as the regions do
  fn show_flash(&self, ctx: &mut SceneContext, hero: Entity) {
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(hero) {
      renderable.shader =
        (self.flash > 0.0).then(|| SpriteShader::flash(Color::WHITE, self.flash / FLASH));
    }
  }
}

impl DemoPage for MeshesPage {
  fn name(&self) -> &str {
    "Meshes"
  }

  fn description(&self) -> &str {
    "A mesh attachment is drawn as triangles. The cape's vertices are weighted over three bones and bend as they swing; deform keys ripple the banner's.\n\
     The cape is a skin, worn on this page alone. In a development build's console, debug:skeletons draws each mesh's hull and its triangles."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Gait", &mut self.gait, &["Walk", "Run", "Idle"]);
    s.toggle("Cape", &mut self.caped);
    s.float("Ripple speed", &mut self.ripple, 0.0..=2.0, 0.1);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Flash the hero")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    let Ok(banner_data) = ctx.assets().load_now::<SkeletonData>(BANNER) else {
      return;
    };
    // What the readout says of each mesh comes from the data itself
    if let Some(data) = ctx.assets().get(&data)
      && let Some(cape) = mesh(data, CAPE, "cape", "cape")
    {
      self.cape = format!("Cape: {}, which bend it. {}", held(cape), size(cape));
    }
    if let Some(data) = ctx.assets().get(&banner_data)
      && let Some(flag) = mesh(data, "default", "flag", "flag")
    {
      self.flag = format!(
        "Banner: {}, deformed by its ripple's {} keys. {}",
        held(flag),
        deform_keys(data, "ripple", "flag", "flag"),
        size(flag)
      );
    }

    // The banner is a skeleton of its own, behind the hero: a pole and a
    // flag, with a small shadow at the pole's foot
    let banner = ctx
      .world
      .spawn((
        Transform::from_xy(POLE.x, POLE.y).with_uniform_scale(BANNER_SCALE),
        Skeleton::new(banner_data),
        SkeletonAnimation::new(),
        Renderable::skeleton()
          .with_z_order(-1)
          .with_blob_shadow(BlobShadow::ellipse(22.0, 6.0).color(Color::new(10, 14, 30, 110))),
      ))
      .id();
    self.banner = Some(banner);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(banner) {
      state.set_animation(0, "ripple", true);
    }

    let hero = hero::spawn(ctx, &data, Vec2::new(300.0, GROUND), SCALE);
    self.hero = Some(hero);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.set_animation(0, GAITS[self.gait].0, true);
    }
    self.playing_gait = self.gait;
    // Worn before the first frame, so the hero never shows without it
    self.wear(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(hero) = self.hero else {
      return;
    };
    self.wear(ctx);
    if self.gait != self.playing_gait
      && let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero)
    {
      // The cape's bones are keyed in each gait, so they crossfade with
      // the rest of the body
      state.set_animation(0, GAITS[self.gait].0, true);
      self.playing_gait = self.gait;
    }
    // The ripple is the banner's whole animation: its time scale is the
    // deform's speed
    if let Some(banner) = self.banner
      && let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(banner)
      && state.time_scale != self.ripple
    {
      state.time_scale = self.ripple;
    }

    if ctx.input.key_pressed(KeyCode::Space) {
      self.flash = FLASH;
      self.show_flash(ctx, hero);
    } else if self.flash > 0.0 {
      self.flash = (self.flash - dt).max(0.0);
      self.show_flash(ctx, hero);
    }

    self.patrol.step(ctx, hero, GAITS[self.playing_gait].1, dt);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    if self.hero.is_none() {
      return;
    }
    let worn = if self.caped {
      "The cape is a skin: set_skins(&[\"cape\"])"
    } else {
      "The cape's skin is off: its slot shows nothing"
    };
    hero::readout(
      ui,
      &[
        "Two meshes: a cape and a banner".to_string(),
        self.cape.clone(),
        self.flag.clone(),
        worn.to_string(),
        "In a development build's console, debug:skeletons draws each mesh's hull and its triangles"
          .to_string(),
      ],
    );
  }
}
