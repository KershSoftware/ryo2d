//! The Settings page built from a UI file: each tab's rows are a column,
//! hidden unless its tab is picked

use demo_kit::prelude::*;

use super::settings_data::{TABS, bind_actions};

/// The page's file, relative to the working directory
const SETTINGS: &str = "assets/ui/settings.ui.ron";

pub struct FileSettingsPage {
  /// Each tab's column, by tab
  tabs: Vec<Entity>,
  reset_controls: bool,
  /// Why the file did not load, drawn in place of the page
  error: Option<String>,
}

impl FileSettingsPage {
  pub fn new() -> Self {
    Self {
      tabs: Vec::new(),
      reset_controls: false,
      error: None,
    }
  }
}

impl Default for FileSettingsPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for FileSettingsPage {
  fn name(&self) -> &str {
    "File Settings"
  }

  fn description(&self) -> &str {
    "The Settings page built from a UI file: Q and E (the shoulder buttons) switch tabs, the D-pad moves and changes values.\n\
     Each tab's rows are a column the page shows or hides when the tabs change; the sliders' values are bound text."
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

  fn enter(&mut self, ctx: &mut SceneContext) {
    bind_actions(ctx);
    match ctx.spawn_ui_file(SETTINGS) {
      Ok(root) => {
        self.tabs = (0..TABS.len())
          .filter_map(|i| ctx.find_ui_in(root, &format!("tab-{i}")))
          .collect();
      }
      Err(error) => {
        game_error!("{error}");
        self.error = Some(error.to_string());
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if std::mem::take(&mut self.reset_controls) {
      ctx.actions().reset_defaults();
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    if let Some(error) = &self.error {
      ui.text(error.as_str());
    }
    for event in ctx.ui_events() {
      match (event.kind, event.name.as_deref(), event.action.as_deref()) {
        (UiEventKind::Changed, Some("tabs"), _) => {
          let picked = ctx.world.get::<UiIndex>(event.node).map_or(0, |i| i.0);
          for (i, &column) in self.tabs.iter().enumerate() {
            if i == picked {
              ctx.world.entity_mut(column).remove::<UiHidden>();
            } else {
              ctx.world.entity_mut(column).insert(UiHidden);
            }
          }
        }
        (UiEventKind::Clicked, _, Some("reset")) => self.reset_controls = true,
        _ => {}
      }
    }
  }
}
