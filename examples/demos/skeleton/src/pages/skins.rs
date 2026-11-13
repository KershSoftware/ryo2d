//! Skins: one skeleton, many looks. A skin replaces some slots'
//! attachments; several worn together make a look, here a body, an outfit
//! and a hat

use demo_kit::prelude::*;

use crate::hero::{self, GROUND};

/// The bodies and outfits, as the settings name them, with the skin each
/// wears. The first of each is the default skin's own, so it wears none
const BODIES: [(&str, Option<&str>); 3] = [
  ("Brown hair", None),
  ("Blond", Some("blond")),
  ("Dark", Some("dark")),
];
const OUTFITS: [(&str, Option<&str>); 3] = [
  ("Runner", None),
  ("Ranger", Some("ranger")),
  ("Sailor", Some("sailor")),
];
/// Drawn a little larger than on the other pages, to show the parts
const SIZE: f32 = 0.6;

pub struct SkinsPage {
  hero: Option<Entity>,
  /// Settings
  body: usize,
  outfit: usize,
  hat: bool,
  /// The skins last put on, to put them on again only when one changes
  worn: Option<(usize, usize, bool)>,
}

impl Default for SkinsPage {
  fn default() -> Self {
    Self {
      hero: None,
      body: 0,
      outfit: 1,
      hat: true,
      worn: None,
    }
  }
}

impl SkinsPage {
  /// The skins the settings ask for, the later over the earlier
  fn skins(&self) -> Vec<&'static str> {
    let mut skins = Vec::new();
    skins.extend(BODIES[self.body].1);
    skins.extend(OUTFITS[self.outfit].1);
    if self.hat {
      skins.push("hat");
    }
    skins
  }
}

impl DemoPage for SkinsPage {
  fn name(&self) -> &str {
    "Skins"
  }

  fn description(&self) -> &str {
    "A skin replaces the attachments of some slots. set_skins wears several as one, over the default skin: a body, an outfit and a hat.\n\
     The bones and animations are the same for every look; the ranger's short sleeves and shorts show the body's own arms and legs."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Hero", &mut self.body, &BODIES.map(|(name, _)| name));
    s.choice("Outfit", &mut self.outfit, &OUTFITS.map(|(name, _)| name));
    s.toggle("Hat", &mut self.hat);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    let hero = hero::spawn(ctx, &data, Vec2::new(400.0, GROUND), SIZE);
    self.hero = Some(hero);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.set_animation(0, "idle", true);
    }
    // Worn before the first frame, so the hero never shows another look
    self.update(ctx, 0.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let wanted = (self.body, self.outfit, self.hat);
    if self.worn == Some(wanted) {
      return;
    }
    if let Some(hero) = self.hero
      && let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero)
    {
      skeleton.set_skins(&self.skins());
      self.worn = Some(wanted);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let skins = self.skins();
    let worn = if skins.is_empty() {
      "set_skins(&[]): the default skin alone".to_string()
    } else {
      format!("set_skins(&{skins:?})")
    };
    hero::readout(
      ui,
      &[
        format!(
          "{}, {}{}",
          BODIES[self.body].0,
          OUTFITS[self.outfit].0.to_lowercase(),
          if self.hat { ", hat" } else { "" }
        ),
        worn,
        "Each is worn over the default skin".to_string(),
      ],
    );
  }
}
