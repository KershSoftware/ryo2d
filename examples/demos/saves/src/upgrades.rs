//! The Upgrades page: a save an older build wrote (version 1) loads as the
//! current type. The engine has no import call, so the page saves the old
//! shape itself, as the older build did, and the registered steps carry it
//! to version 3 on load.

use demo_kit::prelude::*;

use crate::shared::{BAD, DIM, GOOD, HOT, HeroV1, HeroV3, OLD_SLOT, clock, time_of_day};

pub(crate) struct UpgradesPage {
  loaded: Option<Result<Loaded<HeroV3>, String>>,
}

impl UpgradesPage {
  pub(crate) fn new() -> Self {
    Self { loaded: None }
  }

  fn reload(&mut self, ctx: &mut SceneContext) {
    self.loaded = Some(
      ctx
        .saves()
        .load::<HeroV3>(OLD_SLOT)
        .map_err(|error| error.to_string()),
    );
  }
}

impl DemoPage for UpgradesPage {
  fn name(&self) -> &str {
    "Upgrades"
  }

  fn description(&self) -> &str {
    "A version 1 save loads as version 3 through the steps the game registered"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Load hero-old again")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    if !ctx.saves().exists(OLD_SLOT) {
      let info = SaveInfo::new().name("Rowan (old build)");
      if let Err(error) = ctx.saves().save_with(OLD_SLOT, HeroV1::rowan(), info) {
        self.loaded = Some(Err(error.to_string()));
        return;
      }
    }
    self.reload(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if ctx.input.key_pressed(KeyCode::Space) {
      self.reload(ctx);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    let before = HeroV1::rowan();
    ui.draw_text("Before: HeroV1, as the first build saved it", x, y, DIM);
    y += 24.0;
    let old = format!("{}: hp {}, gold {}", before.name, before.hp, before.gold);
    ui.draw_text(&old, x + 16.0, y, Color::WHITE);
    y += 44.0;
    ui.draw_text("After: load::<HeroV3>(\"hero-old\")", x, y, DIM);
    y += 24.0;
    match &self.loaded {
      None => ui.draw_text("not loaded yet", x + 16.0, y, DIM),
      Some(Err(error)) => ui.draw_text(error, x + 16.0, y, BAD),
      Some(Ok(loaded)) => {
        let header = &loaded.header;
        ui.draw_text(&loaded.data.summary(), x + 16.0, y, GOOD);
        y += 36.0;
        let from = match loaded.upgraded_from {
          Some(version) => format!("upgraded_from: Some({version})"),
          None => "upgraded_from: None".to_string(),
        };
        ui.draw_text(&from, x + 16.0, y, HOT);
        y += 24.0;
        let saved = format!(
          "header: version {}, saved {}, played {}",
          header.version,
          time_of_day(header.saved_at),
          clock(header.play_time)
        );
        ui.draw_text(&saved, x + 16.0, y, DIM);
        y += 40.0;
        ui.draw_text(
          "Steps: v1 renames hp and adds a class; v2 adds stats",
          x,
          y,
          DIM,
        );
      }
    }
  }
}
