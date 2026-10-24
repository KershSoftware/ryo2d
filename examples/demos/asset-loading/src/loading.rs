//! The Loading screen page: `load` asks for every asset on the worker
//! threads and returns at once; the page draws the group's `progress` as a
//! bar and each asset's `LoadState` as the pump settles it

use demo_kit::prelude::*;

use crate::tracked::{self, Held};

const ROW: f32 = 22.0;
const LOADED: Color = Color::rgb(120, 220, 120);
const LOADING: Color = Color::XP_GOLD;
const FAILED: Color = Color::rgb(255, 110, 100);

/// Where the page's load is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
  /// Nothing asked for yet
  Idle,
  /// On the workers
  Loading,
  /// Every asset loaded or failed
  Done,
  /// The last load let go of in frame `since`. The next frame's pump frees
  /// what nothing else holds, so the load after it reads those files again;
  /// one another holder keeps is found loaded at once
  Releasing { since: u64 },
}

pub(crate) struct LoadingPage {
  phase: Phase,
  /// The handles of the load, in the table's order (empty until asked)
  held: Vec<Held>,
  /// The Load button was clicked in this frame's UI pass
  clicked: bool,
}

impl LoadingPage {
  pub(crate) fn new() -> Self {
    Self {
      phase: Phase::Idle,
      held: Vec::new(),
      clicked: false,
    }
  }

  /// Ask for every asset: each `load` returns its handle at once
  fn load(&mut self, ctx: &mut SceneContext) {
    let assets = ctx.assets();
    self.held = tracked::listed()
      .map(|(path, kind)| Held::load(&assets, path, kind))
      .collect();
    self.phase = Phase::Loading;
  }
}

impl DemoPage for LoadingPage {
  fn name(&self) -> &str {
    "Loading screen"
  }

  fn description(&self) -> &str {
    "load() reads each asset on the worker threads and returns at once; the bar is their progress()"
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Load (again)"),
      Control::mouse("Load button"),
    ]
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let asked = std::mem::take(&mut self.clicked) || ctx.input.key_pressed(KeyCode::Space);
    match self.phase {
      Phase::Idle if asked => self.load(ctx),
      Phase::Loading => {
        if ctx.assets().progress(&self.held).is_done() {
          self.phase = Phase::Done;
        }
      }
      Phase::Done if asked => {
        // Dropping the handles frees the assets at the next pump
        self.held.clear();
        self.phase = Phase::Releasing {
          since: ctx.time.frame_count(),
        };
      }
      // A pump has run since: the bar counts the new handles, whatever
      // another holder kept
      Phase::Releasing { since } if ctx.time.frame_count() > since => self.load(ctx),
      Phase::Idle | Phase::Done | Phase::Releasing { .. } => {}
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 20.0;
    let assets = ctx.assets();
    let progress = assets.progress(&self.held);
    let total = tracked::listed().count();

    let status = match self.phase {
      Phase::Idle => format!("Space or Load asks for these {total} assets on the worker threads."),
      Phase::Loading | Phase::Releasing { .. } => "Loading on the worker threads...".to_string(),
      Phase::Done => format!(
        "Done: {} loaded, {} failed. Space loads them again.",
        progress.loaded, progress.failed
      ),
    };
    ui.draw_text(&status, x, area.y + 10.0, Color::LIGHT_GRAY);

    // The bar: the share of the group settled (loaded or failed)
    let bar = Rect::new(x, area.y + 40.0, 440.0, 20.0);
    ui.draw_rect(bar, Color::DARK_GRAY);
    let fraction = if self.held.is_empty() {
      0.0
    } else {
      progress.fraction()
    };
    if fraction > 0.0 {
      let fill = Rect::new(bar.x, bar.y, bar.width * fraction, bar.height);
      ui.draw_rect(fill, LOADED);
    }
    ui.draw_rect_outline(bar, Color::WHITE, 1.0);
    let settled = progress.loaded + progress.failed;
    ui.draw_text(
      &format!("{settled} / {total}"),
      bar.x + bar.width + 12.0,
      bar.y + 2.0,
      Color::WHITE,
    );

    if matches!(self.phase, Phase::Idle | Phase::Done) {
      let label = if self.phase == Phase::Idle {
        "Load"
      } else {
        "Load again"
      };
      let button = Rect::new(area.x + area.width - 180.0, area.y + 34.0, 160.0, 32.0);
      let clicked = &mut self.clicked;
      ui.column().focus_scope().at(button).show(|ui| {
        if ui
          .button(label)
          .width(button.width)
          .height(button.height)
          .default_focus()
          .clicked()
        {
          *clicked = true;
        }
      });
    }

    let (kind_x, state_x) = (x + 270.0, x + 350.0);
    let header = area.y + 80.0;
    ui.draw_text("Asset", x, header, Color::GRAY);
    ui.draw_text("Type", kind_x, header, Color::GRAY);
    ui.draw_text("LoadState", state_x, header, Color::GRAY);
    // A long error wraps in its column, and the rows below make room for it
    let state_width = area.x + area.width - 20.0 - state_x;
    let mut y = header + 24.0;
    for (i, (path, kind)) in tracked::listed().enumerate() {
      ui.draw_text(path, x, y, Color::WHITE);
      ui.draw_text(kind.label(), kind_x, y, Color::GRAY);
      let (state, color) = match self.held.get(i) {
        None => ("not loaded".to_string(), Color::GRAY),
        Some(held) => match held.state(&assets) {
          LoadState::NotLoaded => ("not loaded".to_string(), Color::GRAY),
          LoadState::Loading => ("loading".to_string(), LOADING),
          LoadState::Loaded => {
            let summary = held.summary(&assets);
            let text = if summary.is_empty() {
              "loaded".to_string()
            } else {
              format!("loaded {summary}")
            };
            (text, LOADED)
          }
          LoadState::Failed(error) => (format!("failed: {}", error.report()), FAILED),
        },
      };
      ui.draw_text_wrapped(&state, state_x, y, state_width as u32, color);
      let (_, height) = ui.wrapped_text_size(&state, state_width as u32);
      y += ROW.max(height as f32 + 4.0);
    }
  }
}
