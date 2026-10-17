//! Settings page - a game's settings screen: tabs switched with the
//! shoulder buttons, sliders, toggles, radios, dropdowns and rebinding rows

use demo_kit::prelude::*;

const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
const OUTPUTS: [&str; 3] = ["Default", "Headphones", "Speakers"];
const WINDOWS: [&str; 3] = ["Windowed", "Borderless", "Fullscreen"];
const RESOLUTIONS: [&str; 4] = ["1280×720", "1600×900", "1920×1080", "2560×1440"];
/// The page's actions and their labels, for the Controls tab
const ACTIONS: [(&str, &str); 7] = [
  ("move_left", "Move left"),
  ("move_right", "Move right"),
  ("jump", "Jump"),
  ("attack", "Attack"),
  ("interact", "Interact"),
  ("inventory", "Inventory"),
  ("pause", "Pause"),
];
/// The widest a row's widget is
const WIDGET_WIDTH: f32 = 220.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiScale {
  Small,
  Medium,
  Large,
}

pub struct SettingsPage {
  tab: usize,
  master: f32,
  music: f32,
  effects: f32,
  voice: f32,
  mute_in_background: bool,
  output: usize,
  window: usize,
  resolution: usize,
  vsync: bool,
  ui_scale: UiScale,
  brightness: f32,
  show_fps: bool,
  /// Reset controls was chosen: the bindings go back to their defaults in
  /// the next update
  reset_controls: bool,
}

impl SettingsPage {
  pub fn new() -> Self {
    Self {
      tab: 0,
      master: 0.8,
      music: 0.6,
      effects: 0.7,
      voice: 1.0,
      mute_in_background: true,
      output: 0,
      window: 0,
      resolution: 2,
      vsync: true,
      ui_scale: UiScale::Medium,
      brightness: 1.0,
      show_fps: false,
      reset_controls: false,
    }
  }

  fn audio(&mut self, ui: &mut UiContext) {
    let sliders = [
      ("Master", &mut self.master),
      ("Music", &mut self.music),
      ("Effects", &mut self.effects),
      ("Voice", &mut self.voice),
    ];
    for (label, value) in sliders {
      row(ui, label, |ui| {
        ui.slider(value, 0.0..=1.0)
          .step(0.05)
          .show_value()
          .width(WIDGET_WIDTH)
          .show();
      });
    }
    ui.checkbox(&mut self.mute_in_background, "Mute in background")
      .show();
    let output = &mut self.output;
    row(ui, "Output", |ui| {
      ui.dropdown(output, &OUTPUTS).width(WIDGET_WIDTH).show();
    });
  }

  fn video(&mut self, ui: &mut UiContext) {
    let (window, resolution) = (&mut self.window, &mut self.resolution);
    row(ui, "Window", |ui| {
      ui.dropdown(window, &WINDOWS).width(WIDGET_WIDTH).show();
    });
    row(ui, "Resolution", |ui| {
      ui.dropdown(resolution, &RESOLUTIONS)
        .width(WIDGET_WIDTH)
        .show();
    });
    ui.toggle(&mut self.vsync, "VSync").show();
    let scale = &mut self.ui_scale;
    row(ui, "UI scale", |ui| {
      ui.row().gap(16.0).show(|ui| {
        ui.radio(&mut *scale, UiScale::Small, "Small").show();
        ui.radio(&mut *scale, UiScale::Medium, "Medium").show();
        ui.radio(&mut *scale, UiScale::Large, "Large").show();
      });
    });
    let brightness = &mut self.brightness;
    row(ui, "Brightness", |ui| {
      ui.slider(brightness, 0.5..=1.5)
        .step(0.05)
        .show_value()
        .width(WIDGET_WIDTH)
        .show();
    });
    ui.checkbox(&mut self.show_fps, "Show FPS").show();
  }

  fn controls(&mut self, ui: &mut UiContext) {
    for (action, label) in ACTIONS {
      ui.rebind(action).id(action).label(label).show();
    }
    ui.row().justify(Justify::End).show(|ui| {
      if ui.button("Reset controls").clicked() {
        self.reset_controls = true;
      }
    });
  }
}

/// A settings row: `label` on the left, the widget `contents` draws on
/// the right
fn row(ui: &mut UiContext, label: &str, contents: impl FnOnce(&mut UiContext)) {
  ui.row()
    .id(label)
    .gap(12.0)
    .align(Align::Center)
    .show(|ui| {
      ui.column().grow(1.0).show(|ui| {
        ui.text(label);
      });
      contents(ui);
    });
}

impl Default for SettingsPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for SettingsPage {
  fn name(&self) -> &str {
    "Settings"
  }

  fn description(&self) -> &str {
    "A settings screen played with the pad: the shoulder buttons (Q and E) switch tabs, the D-pad moves and changes values, A opens a list or rebinds.\n\
     Controls rebinds the page's own actions: pick a row, then press the key or button to use (Esc cancels). \
     A binding another action has swaps between them."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("Q / E", &[KeyCode::KeyQ, KeyCode::KeyE], "Switch tab"),
      Control::keys(
        "Enter / Space",
        &[KeyCode::Enter, KeyCode::Space],
        "Open a list or rebind",
      ),
    ]
  }

  /// The actions the Controls tab rebinds
  fn enter(&mut self, ctx: &mut SceneContext) {
    use Binding::{Key, Pad};
    let stick = |direction| {
      Binding::Stick(StickDirection {
        stick: Stick::Left,
        direction,
      })
    };
    ctx
      .actions()
      .bind(
        "move_left",
        [
          Key(KeyCode::KeyA),
          Key(KeyCode::ArrowLeft),
          stick(Direction::Left),
        ],
      )
      .bind(
        "move_right",
        [
          Key(KeyCode::KeyD),
          Key(KeyCode::ArrowRight),
          stick(Direction::Right),
        ],
      )
      .bind("jump", [Key(KeyCode::Space), Pad(GamepadButton::South)])
      .bind("attack", [Key(KeyCode::KeyJ), Pad(GamepadButton::West)])
      .bind("interact", [Key(KeyCode::KeyE), Pad(GamepadButton::North)])
      .bind(
        "inventory",
        [Key(KeyCode::KeyI), Pad(GamepadButton::Select)],
      )
      .bind("pause", [Key(KeyCode::KeyP), Pad(GamepadButton::Start)]);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if std::mem::take(&mut self.reset_controls) {
      ctx.actions().reset_defaults();
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let heading = ui.theme().text.heading.style();
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(600.0)
      .height(460.0)
      .gap(12.0)
      .show(|ui| {
        ui.text("Settings").style(&heading);
        ui.tabs(&mut self.tab, &TABS).show();
        ui.scroll()
          .fill_width()
          .grow(1.0)
          .gap(10.0)
          .show(|ui| match self.tab {
            0 => self.audio(ui),
            1 => self.video(ui),
            _ => self.controls(ui),
          });
        ui.row().show(|ui| {
          ui.text(RichText::markup(
            "[prompt=ui.tab_prev] [prompt=ui.tab_next] Switch tab    [prompt=ui.back] Back",
          ));
        });
      });
  }
}
