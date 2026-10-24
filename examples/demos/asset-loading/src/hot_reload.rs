//! The Hot reload page: `load_now` loads every asset at once, and each
//! asset's `AssetEvent::Modified` events are counted. With hot reload on (a
//! debug build with a window), a file saved in `assets/` reloads under the
//! same handle, so what the page draws from it changes in place

use demo_kit::prelude::*;

use crate::tracked::{self, Held, explained};

const ROW: f32 = 26.0;
const CHANGED: Color = Color::XP_GOLD;
const FAILED: Color = Color::rgb(255, 110, 100);
/// The previews' box in a table row
const PREVIEW: (f32, f32) = (44.0, 22.0);
/// The errors' column, wrapped at this width
const NOTES_WIDTH: f32 = 250.0;

/// One asset in the table
struct Row {
  path: &'static str,
  /// The asset, or why it did not load
  held: Result<Held, String>,
  /// Its `AssetEvent::Modified` events since the page opened
  modified: u32,
  /// Why its last reload failed (it keeps its last good value), until it
  /// next changes
  reload_error: Option<String>,
}

pub(crate) struct HotReloadPage {
  rows: Vec<Row>,
}

impl HotReloadPage {
  pub(crate) fn new() -> Self {
    Self { rows: Vec::new() }
  }

  /// Count this step's events for `T`'s assets
  fn count<T: Asset>(&mut self, ctx: &mut SceneContext) {
    for event in ctx.messages::<AssetEvent<T>>() {
      let (id, error) = match event {
        AssetEvent::Modified { id } => (id.untyped(), None),
        AssetEvent::ReloadFailed { id, error } => (id.untyped(), Some(explained(&*error))),
        _ => continue,
      };
      let held = |row: &&mut Row| row.held.as_ref().is_ok_and(|held| held.id() == id);
      let Some(row) = self.rows.iter_mut().find(held) else {
        continue;
      };
      match error {
        None => {
          row.modified += 1;
          row.reload_error = None;
        }
        Some(error) => row.reload_error = Some(format!("reload failed: {error}")),
      }
    }
  }
}

impl DemoPage for HotReloadPage {
  fn name(&self) -> &str {
    "Hot reload"
  }

  fn description(&self) -> &str {
    "load_now() loads at once; each file saved in assets/ reloads in place and sends AssetEvent::Modified"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let mut assets = ctx.assets();
    self.rows = tracked::listed()
      .map(|(path, kind)| Row {
        path,
        held: Held::load_now(&mut assets, path, kind).map_err(|error| explained(&*error)),
        modified: 0,
        reload_error: None,
      })
      .collect();
    // The panel the UI file describes: a reload of the file updates it in place
    let file = self.rows.iter().find_map(|row| match &row.held {
      Ok(Held::UiFile(file)) => Some(file.clone()),
      _ => None,
    });
    if let Some(file) = file {
      match file.root(&mut ctx.assets()) {
        Ok(tree) => {
          ctx.spawn_ui(tree);
        }
        Err(error) => {
          if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.path == tracked::UI_FILE)
          {
            row.held = Err(error.to_string());
          }
        }
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.count::<Image>(ctx);
    self.count::<Font>(ctx);
    self.count::<UiFile>(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 20.0;
    ui.draw_text(
      "Save a file in assets/ while the demo runs: it reloads, and its count goes up.",
      x,
      area.y + 8.0,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "Debug builds with a window watch the folder; release and headless builds do not.",
      x,
      area.y + 28.0,
      Color::GRAY,
    );

    let (name_x, count_x) = (x + PREVIEW.0 + 12.0, x + 270.0);
    let header = area.y + 60.0;
    ui.draw_text("Asset", name_x, header, Color::GRAY);
    ui.draw_text("AssetEvent::Modified", count_x, header, Color::GRAY);
    // Errors go in the right column, under the UI file's panel (which the
    // file sets 16 units in from the right); each names its asset
    let notes_x = ui.screen_size().0 as f32 - 16.0 - NOTES_WIDTH;
    ui.draw_text("Errors", notes_x, area.y + 160.0, Color::GRAY);
    let mut note_y = area.y + 186.0;
    let assets = ctx.assets();
    for (i, row) in self.rows.iter().enumerate() {
      let y = header + 26.0 + i as f32 * ROW;
      if let Ok(held) = &row.held {
        preview(
          ui,
          &assets,
          held,
          Rect::new(x, y - 3.0, PREVIEW.0, PREVIEW.1),
        );
      }
      ui.draw_text(row.path, name_x, y, Color::WHITE);
      let color = if row.modified > 0 {
        CHANGED
      } else {
        Color::LIGHT_GRAY
      };
      ui.draw_text(&format!("modified {}", row.modified), count_x, y, color);
      let note = match &row.held {
        Err(error) => Some(format!("failed: {error}")),
        Ok(_) => row.reload_error.clone(),
      };
      if let Some(note) = note {
        let size = ui
          .text(note)
          .at(notes_x, note_y)
          .wrap(NOTES_WIDTH)
          .color(FAILED)
          .draw();
        note_y += size.y + 8.0;
      }
    }
  }
}

/// A small picture of the asset in `cell`: an image fitted in it, a font's
/// "Aa"; nothing for a UI file (its panel is on the page)
fn preview(ui: &mut UiContext, assets: &AssetsView, held: &Held, cell: Rect) {
  match held {
    Held::Image(image) => {
      // A backdrop, for the images that are mostly clear
      ui.draw_rect(cell, Color::new(255, 255, 255, 18));
      let Some((w, h)) = assets
        .get(image)
        .map(|image| (image.width() as f32, image.height() as f32))
      else {
        return;
      };
      let scale = (cell.width / w).min(cell.height / h).min(1.0);
      let (w, h) = (w * scale, h * scale);
      let rect = Rect::new(
        cell.x + (cell.width - w) / 2.0,
        cell.y + (cell.height - h) / 2.0,
        w,
        h,
      );
      ui.draw_sprite_rect(image, rect, Color::WHITE);
    }
    Held::Font(font) => {
      let style = TextStyle::new()
        .font(TextFont::Face(font.clone()))
        .size(16.0)
        .color(Color::WHITE);
      ui.draw_text_styled("Aa", cell.x + 8.0, cell.y + 3.0, &style);
    }
    Held::UiFile(_) => {}
  }
}
