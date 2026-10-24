//! The Loading screen page: `load` asks for every asset on the worker
//! threads and returns at once; the page draws the group's `progress` as a
//! bar and each asset's `LoadState` as the pump settles it

use demo_kit::prelude::*;

use crate::tracked::{self, Held, explained};

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
  /// The last load let go of, until the pump has freed it: a load of an
  /// asset still in its store would find it loaded and read nothing
  Freeing,
}

pub(crate) struct LoadingPage {
  phase: Phase,
  /// The handles of the load, in the table's order (empty until asked)
  held: Vec<Held>,
  /// The last load's assets, while they are being freed
  freeing: Vec<UntypedAssetId>,
  /// The Load button was clicked in this frame's UI pass
  clicked: bool,
}

impl LoadingPage {
  pub(crate) fn new() -> Self {
    Self {
      phase: Phase::Idle,
      held: Vec::new(),
      freeing: Vec::new(),
      clicked: false,
    }
  }

  fn ids(&self) -> Vec<UntypedAssetId> {
    self.held.iter().map(Held::id).collect()
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
        if ctx.assets().progress(&self.ids()).is_done() {
          self.phase = Phase::Done;
        }
      }
      Phase::Done if asked => {
        // Dropping the handles frees the assets at the next pump
        self.freeing = self.ids();
        self.held.clear();
        self.phase = Phase::Freeing;
      }
      Phase::Freeing => {
        // A freed asset is neither loaded, loading nor failed
        if ctx.assets().progress(&self.freeing) == Progress::default() {
          self.freeing.clear();
          self.load(ctx);
        }
      }
      Phase::Idle | Phase::Done => {}
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 20.0;
    let assets = ctx.assets();
    let progress = assets.progress(&self.ids());
    let total = tracked::listed().count();

    let status = match self.phase {
      Phase::Idle => format!("Space or Load asks for these {total} assets on the worker threads."),
      Phase::Loading | Phase::Freeing => "Loading on the worker threads...".to_string(),
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
    for (i, (path, kind)) in tracked::listed().enumerate() {
      let y = header + 24.0 + i as f32 * ROW;
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
          LoadState::Failed(error) => (format!("failed: {}", explained(&*error)), FAILED),
        },
      };
      ui.draw_text(&state, state_x, y, color);
    }
  }
}
