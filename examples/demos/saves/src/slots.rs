//! The Slots page: save, load and delete three slots. Each is listed with
//! what its header says (name, when it was saved, play time, labels) and a
//! mark when the save was damaged and its backup is what shows.

use demo_kit::prelude::*;

use crate::shared::{
  BAD, DIM, GOOD, HOT, HeroV3, Log, SLOTS, clock, finished_line, random_hero, time_of_day,
};

pub(crate) struct SlotsPage {
  slot: usize,
  frame: u32,
  listed: Vec<SaveSlot>,
  refresh: bool,
  log: Log,
}

impl SlotsPage {
  pub(crate) fn new() -> Self {
    Self {
      slot: 0,
      frame: 0,
      listed: Vec::new(),
      refresh: true,
      log: Log::default(),
    }
  }

  fn save(&mut self, ctx: &mut SceneContext) {
    let hero = random_hero();
    let info = SaveInfo::new()
      .name(format!("{} the {}", hero.name, hero.class))
      .play_time(u64::from(self.frame / 60))
      .label("class", hero.class.clone())
      .label("gold", hero.gold.to_string());
    if let Err(error) = ctx.saves().save_with(SLOTS[self.slot], hero, info) {
      self
        .log
        .push(format!("save {}: {error}", SLOTS[self.slot]), BAD);
    }
  }

  fn load(&mut self, ctx: &mut SceneContext) {
    let slot = SLOTS[self.slot];
    match ctx.saves().load::<HeroV3>(slot) {
      Ok(loaded) => {
        let from = if loaded.from_backup {
          " (from backup)"
        } else {
          ""
        };
        self
          .log
          .push(format!("{slot}{from}: {}", loaded.data.summary()), GOOD);
      }
      Err(error) => self.log.push(format!("load {slot}: {error}"), BAD),
    }
  }
}

impl DemoPage for SlotsPage {
  fn name(&self) -> &str {
    "Slots"
  }

  fn description(&self) -> &str {
    "ctx.saves() saves, loads and deletes by slot name; a SaveFinished reports each write"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Slot", &mut self.slot, &SLOTS);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Save a random hero"),
      Control::key(KeyCode::KeyL, "Load"),
      Control::key(KeyCode::KeyX, "Delete"),
    ]
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    for done in ctx.messages::<SaveFinished>() {
      let (line, color) = finished_line(&done);
      self.log.push(line, color);
      self.refresh = true;
    }
    if ctx.input.key_pressed(KeyCode::Space) {
      self.save(ctx);
    }
    if ctx.input.key_pressed(KeyCode::KeyL) {
      self.load(ctx);
    }
    if ctx.input.key_pressed(KeyCode::KeyX)
      && let Err(error) = ctx.saves().delete(SLOTS[self.slot])
    {
      self
        .log
        .push(format!("delete {}: {error}", SLOTS[self.slot]), BAD);
    }
    if self.refresh || self.frame.is_multiple_of(30) {
      self.refresh = false;
      match ctx.saves().list() {
        Ok(list) => self.listed = list,
        Err(error) => self.log.push(format!("list: {error}"), BAD),
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    for (i, slot) in SLOTS.iter().enumerate() {
      let color = if i == self.slot { HOT } else { Color::WHITE };
      let mark = if i == self.slot { ">" } else { " " };
      ui.draw_text(&format!("{mark} {slot}"), x, y, color);
      match self.listed.iter().find(|s| s.slot == *slot) {
        None => ui.draw_text("empty", x + 110.0, y, DIM),
        Some(saved) => {
          let header = &saved.header;
          let name = header.name.as_deref().unwrap_or("unnamed");
          ui.draw_text(name, x + 110.0, y, Color::WHITE);
          let when = format!(
            "saved {}, played {}, version {}",
            time_of_day(header.saved_at),
            clock(header.play_time),
            header.version
          );
          ui.draw_text(&when, x + 110.0, y + 20.0, DIM);
          let labels: Vec<String> = header
            .labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
          ui.draw_text(&labels.join("  "), x + 110.0, y + 40.0, DIM);
          if saved.from_backup {
            ui.draw_text("FROM BACKUP", x + 330.0, y, BAD);
          }
        }
      }
      y += 74.0;
    }
    self.log.draw(ui, x, y + 8.0);
  }
}
