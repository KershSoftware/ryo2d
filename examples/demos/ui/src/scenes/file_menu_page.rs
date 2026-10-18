//! The Menus page built from a UI file: the title screen is spawned once,
//! and each dialog is a template of the file spawned when it opens and
//! despawned when it closes

use demo_kit::prelude::*;

use super::menu_data::{CONTINUES, CREDITS, Dialog, NEW_GAME, NO_SAVE, QUIT_CHOSEN};

pub struct FileMenuPage {
  has_save: bool,
  volume: f32,
  fullscreen: bool,
  subtitles: bool,
  /// The open dialog and its tree
  dialog: Option<(Dialog, Entity)>,
  /// Why the file did not load, drawn in place of the page
  error: Option<String>,
}

/// The page's file, relative to the working directory
const MENU: &str = "assets/ui/menu.ui.ron";

impl FileMenuPage {
  pub fn new() -> Self {
    Self {
      has_save: false,
      volume: 0.8,
      fullscreen: false,
      subtitles: true,
      dialog: None,
      error: None,
    }
  }

  fn fail(&mut self, error: UiFileError) {
    game_error!("{error}");
    self.error = Some(error.to_string());
  }

  /// The dialog's tree, built from the file's template of its name
  fn dialog_tree(&self, ctx: &mut SceneContext, dialog: Dialog) -> Result<UiNode, UiFileError> {
    match dialog {
      Dialog::Options => {
        let args = UiArgs::new()
          .with("volume", self.volume)
          .with("fullscreen", self.fullscreen)
          .with("subtitles", self.subtitles);
        ctx.ui_template(MENU, "Options", &args)
      }
      Dialog::Credits => ctx.ui_template(MENU, "Credits", &UiArgs::new()),
      Dialog::Quit => ctx.ui_template(MENU, "QuitConfirm", &UiArgs::new()),
    }
  }

  /// Spawn the dialog, and fill the credits' placeholder with a line each
  fn spawn_dialog(&self, ctx: &mut SceneContext, dialog: Dialog) -> Result<Entity, UiFileError> {
    let tree = self.dialog_tree(ctx, dialog)?;
    let root = ctx.spawn_ui(tree);
    if dialog == Dialog::Credits
      && let Some(lines) = ctx.find_ui_in(root, "credit-lines")
    {
      for (role, name) in CREDITS {
        let args = UiArgs::new().with("role", role).with("name", name);
        let line = ctx.ui_template(MENU, "CreditLine", &args)?;
        ctx.spawn_ui_in(lines, line);
      }
    }
    Ok(root)
  }

  fn open(&mut self, ctx: &mut SceneContext, dialog: Dialog) {
    if self.dialog.is_some() {
      return;
    }
    match self.spawn_dialog(ctx, dialog) {
      Ok(root) => self.dialog = Some((dialog, root)),
      Err(error) => self.fail(error),
    }
  }

  /// The title screen: its title, the menu, and the status line
  fn spawn_screen(ctx: &mut SceneContext) -> Result<(), UiFileError> {
    let title = ctx.ui_template(MENU, "Title", &UiArgs::new())?;
    ctx.spawn_ui(title);
    ctx.spawn_ui_file(MENU)?;
    let status = ctx.ui_template(MENU, "Status", &UiArgs::new().with("status", NO_SAVE))?;
    ctx.spawn_ui(status);
    Ok(())
  }

  fn close(&mut self, ctx: &mut SceneContext) {
    if let Some((_, root)) = self.dialog.take() {
      ctx.world.despawn(root);
    }
  }

  fn set_status(ctx: &mut SceneContext, status: &str) {
    if let Some(node) = ctx.find_ui("status")
      && let Some(mut label) = ctx.world.get_mut::<UiLabel>(node)
    {
      label.0 = status.to_string();
    }
  }
}

impl Default for FileMenuPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for FileMenuPage {
  fn name(&self) -> &str {
    "File Menus"
  }

  fn description(&self) -> &str {
    "The Menus page built from a UI file: the same title screen, played the same way.\n\
     Each dialog is a template of the file spawned when it opens; the page reacts to its nodes' events."
  }

  fn controls(&self) -> Vec<Control> {
    let mut controls = vec![Control::keys(
      "Enter / Space",
      &[KeyCode::Enter, KeyCode::Space],
      "Select",
    )];
    if self.dialog.is_some() {
      controls.push(Control::key(KeyCode::Escape, "Close the dialog"));
    }
    controls
  }

  fn wants_escape(&self) -> bool {
    self.dialog.is_some()
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    if let Err(error) = Self::spawn_screen(ctx) {
      self.fail(error);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    if let Some(error) = &self.error {
      ui.text(error.as_str());
    }
    for event in ctx.ui_events() {
      match (event.kind, event.action.as_deref()) {
        (UiEventKind::Clicked, Some("continue")) => Self::set_status(ctx, CONTINUES),
        (UiEventKind::Clicked, Some("new")) => {
          self.has_save = true;
          if let Some(button) = ctx.find_ui("continue") {
            ctx.world.entity_mut(button).remove::<UiDisabled>();
          }
          Self::set_status(ctx, NEW_GAME);
        }
        (UiEventKind::Clicked, Some("options")) => self.open(ctx, Dialog::Options),
        (UiEventKind::Clicked, Some("credits")) => self.open(ctx, Dialog::Credits),
        (UiEventKind::Clicked, Some("quit")) => self.open(ctx, Dialog::Quit),
        (UiEventKind::Clicked, Some("yes")) => {
          Self::set_status(ctx, QUIT_CHOSEN);
          self.close(ctx);
        }
        (UiEventKind::Clicked, Some("done" | "close" | "no")) | (UiEventKind::Back, _) => {
          self.close(ctx)
        }
        (UiEventKind::Changed, _) => match event.name.as_deref() {
          Some("volume") => {
            self.volume = ctx
              .world
              .get::<UiValue>(event.node)
              .map_or(self.volume, |v| v.0)
          }
          Some("fullscreen") => {
            self.fullscreen = ctx.world.get::<UiChecked>(event.node).is_some_and(|c| c.0)
          }
          Some("subtitles") => {
            self.subtitles = ctx.world.get::<UiChecked>(event.node).is_some_and(|c| c.0)
          }
          _ => {}
        },
        _ => {}
      }
    }
  }
}
