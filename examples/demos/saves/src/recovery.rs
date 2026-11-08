//! The Recovery page: a damaged save loads from its backup, and a damaged
//! `system.json` is set aside at the next start. The page damages the files
//! itself, through `std::fs`, in the save folder; the web keeps its files in
//! the browser's storage and shows a note instead.

use std::path::{Path, PathBuf};

use demo_kit::prelude::*;

use crate::shared::{
  BAD, DIM, GOOD, HOT, HeroV3, Log, finished_line, random_hero, save_dir, time_of_day,
};

const SLOT: &str = "slot-a";
const GARBAGE: &[u8] = b"this is not a save file";

pub(crate) struct RecoveryPage {
  dir: Option<PathBuf>,
  /// Saves of slot-a still to make, so the second leaves a backup
  saves_left: u32,
  frame: u32,
  files: Vec<(String, u64)>,
  aside: bool,
  log: Log,
}

impl RecoveryPage {
  pub(crate) fn new() -> Self {
    Self {
      dir: None,
      saves_left: 0,
      frame: 0,
      files: Vec::new(),
      aside: false,
      log: Log::default(),
    }
  }

  fn save(&mut self, ctx: &mut SceneContext) {
    if let Err(error) = ctx.saves().save(SLOT, random_hero()) {
      self.log.push(format!("save {SLOT}: {error}"), BAD);
    }
  }

  fn load(&mut self, ctx: &mut SceneContext) {
    match ctx.saves().load::<HeroV3>(SLOT) {
      Ok(loaded) if loaded.from_backup => {
        let saved = time_of_day(loaded.header.saved_at);
        self
          .log
          .push(format!("{SLOT}: from the backup, saved {saved}"), HOT);
        self.log.push(loaded.data.summary(), GOOD);
      }
      Ok(loaded) => self
        .log
        .push(format!("{SLOT}: whole. {}", loaded.data.summary()), GOOD),
      Err(error) => self.log.push(format!("load {SLOT}: {error}"), BAD),
    }
  }

  fn damage(&mut self, file: &str) -> bool {
    let Some(dir) = &self.dir else {
      return false;
    };
    match std::fs::write(dir.join(file), GARBAGE) {
      Ok(()) => {
        self.log.push(format!("wrote garbage over {file}"), HOT);
        true
      }
      Err(error) => {
        self.log.push(format!("{file}: {error}"), BAD);
        false
      }
    }
  }

  fn look(&mut self) {
    let Some(dir) = &self.dir else {
      return;
    };
    self.aside = Path::new(dir).join("system.json.damaged").exists();
    self.files = std::fs::read_dir(dir)
      .map(|entries| {
        let mut files: Vec<(String, u64)> = entries
          .flatten()
          .filter_map(|entry| {
            let size = entry.metadata().ok()?.len();
            Some((entry.file_name().to_string_lossy().into_owned(), size))
          })
          .collect();
        files.sort();
        files
      })
      .unwrap_or_default();
  }
}

impl DemoPage for RecoveryPage {
  fn name(&self) -> &str {
    "Recovery"
  }

  fn description(&self) -> &str {
    "A damaged save loads from its backup; a damaged system.json is set aside at the next start"
  }

  fn controls(&self) -> Vec<Control> {
    let mut controls = vec![
      Control::key(KeyCode::KeyS, "Save slot-a twice (so it has a backup)"),
      Control::key(KeyCode::KeyL, "Load slot-a"),
    ];
    if cfg!(not(target_arch = "wasm32")) {
      controls.push(Control::key(
        KeyCode::KeyD,
        "Damage slot-a on disk, then load it",
      ));
      controls.push(Control::key(KeyCode::KeyY, "Damage system.json"));
    }
    controls
  }

  fn enter(&mut self, _ctx: &mut SceneContext) {
    self.dir = save_dir();
    self.look();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    for done in ctx.messages::<SaveFinished>() {
      let (line, color) = finished_line(&done);
      self.log.push(line, color);
      if self.saves_left > 0 {
        self.saves_left -= 1;
        self.save(ctx);
      }
      self.look();
    }
    let input = &*ctx.input;
    let (s, l, d, y) = (
      input.key_pressed(KeyCode::KeyS),
      input.key_pressed(KeyCode::KeyL),
      input.key_pressed(KeyCode::KeyD),
      input.key_pressed(KeyCode::KeyY),
    );
    if s {
      self.saves_left = 1;
      self.save(ctx);
    }
    if d && self.damage("slot-a.ryo-save") {
      self.look();
      self.load(ctx);
    }
    if l {
      self.load(ctx);
    }
    if y && self.damage("system.json") {
      self.look();
    }
    if self.frame.is_multiple_of(60) {
      self.look();
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    let Some(dir) = &self.dir else {
      ui.draw_text(
        "The web build keeps saves in the browser's storage,",
        x,
        y,
        HOT,
      );
      ui.draw_text("so there are no files here to damage.", x, y + 22.0, HOT);
      ui.draw_text(
        "Run the native demo to damage slot-a and system.json.",
        x,
        y + 44.0,
        DIM,
      );
      self.log.draw(ui, x, y + 88.0);
      return;
    };
    ui.draw_text(&format!("Save folder: {}", dir.display()), x, y, DIM);
    y += 28.0;
    for (name, size) in self.files.iter().take(6) {
      ui.draw_text(name, x + 16.0, y, Color::WHITE);
      ui.draw_text(&format!("{size} bytes"), x + 300.0, y, DIM);
      y += 20.0;
    }
    y += 12.0;
    let (text, color) = if self.aside {
      (
        "system.json.damaged exists: a damaged file was set aside",
        GOOD,
      )
    } else {
      ("system.json.damaged: not there", DIM)
    };
    ui.draw_text(text, x, y, color);
    y += 22.0;
    ui.draw_text(
      "A damaged system.json is set aside at the next start.",
      x,
      y,
      DIM,
    );
    y += 34.0;
    self.log.draw(ui, x, y);
  }
}
