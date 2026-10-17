//! Menus page - a game's title screen: a menu, an options dialog, scrolling
//! credits and a quit confirmation, all played with the pad alone

use demo_kit::prelude::*;

use super::menu_data::{CONTINUES, CREDITS, Dialog, NEW_GAME, NO_SAVE, QUIT_CHOSEN};

pub struct MenuPage {
  /// A game was started, so Continue works
  has_save: bool,
  status: String,
  dialog: Option<Dialog>,
  volume: f32,
  fullscreen: bool,
  subtitles: bool,
}

impl MenuPage {
  pub fn new() -> Self {
    Self {
      has_save: false,
      status: NO_SAVE.to_string(),
      dialog: None,
      volume: 0.8,
      fullscreen: false,
      subtitles: true,
    }
  }

  /// The title, the menu and the footer
  fn menu(&mut self, ui: &mut UiContext) {
    let title = ui.theme().text.heading.style().size(48.0);
    ui.column()
      .anchor(Anchor::TopCenter)
      .offset(0.0, 80.0)
      .show(|ui| {
        ui.text("RYO QUEST").style(&title);
      });

    let mut open = None;
    let (has_save, status) = (self.has_save, &mut self.status);
    let mut started = false;
    ui.panel()
      .focus_scope()
      .wrap_focus()
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .show(|ui| {
        if ui
          .button("Continue")
          .fill_width()
          .enabled(has_save)
          .clicked()
        {
          *status = CONTINUES.to_string();
        }
        if ui.button("New Game").fill_width().default_focus().clicked() {
          started = true;
          *status = NEW_GAME.to_string();
        }
        for (label, dialog) in [
          ("Options", Dialog::Options),
          ("Credits", Dialog::Credits),
          ("Quit", Dialog::Quit),
        ] {
          if ui.button(label).id(label).fill_width().clicked() {
            open = Some(dialog);
          }
        }
      });
    if started {
      self.has_save = true;
    }
    if open.is_some() {
      self.dialog = open;
    }

    let muted = ui.theme().palette.muted;
    ui.column()
      .anchor(Anchor::BottomCenter)
      .offset(0.0, -40.0)
      .align(Align::Center)
      .gap(8.0)
      .show(|ui| {
        ui.text(self.status.as_str()).color(muted);
        ui.text(RichText::markup(
          "[prompt=ui.confirm] Select    [prompt=ui.back] Back",
        ));
      });
  }

  /// The open dialog; Back or its button closes it
  fn dialog(&mut self, ui: &mut UiContext) {
    let Some(dialog) = self.dialog else {
      return;
    };
    let heading = ui.theme().text.heading.style();
    let mut close = false;
    match dialog {
      Dialog::Options => {
        let (volume, fullscreen, subtitles) =
          (&mut self.volume, &mut self.fullscreen, &mut self.subtitles);
        ui.modal().width(320.0).gap(12.0).show(|ui| {
          ui.text("Options").style(&heading);
          ui.slider(volume, 0.0..=1.0)
            .step(0.05)
            .label("Volume")
            .show_value()
            .show();
          ui.toggle(fullscreen, "Fullscreen").show();
          ui.checkbox(subtitles, "Subtitles").show();
          ui.row().justify(Justify::End).show(|ui| {
            close = ui.button("Done").default_focus().clicked();
          });
        });
        close |= ui.back();
      }
      Dialog::Credits => {
        let body = ui.theme().text.body.style();
        let muted = ui.theme().palette.muted;
        ui.modal().width(360.0).gap(12.0).show(|ui| {
          ui.text("Credits").style(&heading);
          ui.scroll().height(200.0).fill_width().gap(10.0).show(|ui| {
            for (role, name) in CREDITS {
              ui.column().id(role).gap(2.0).show(|ui| {
                ui.text(role).style(&body).color(muted);
                ui.text(name).style(&body);
              });
            }
          });
          ui.text("The right stick scrolls the credits.").color(muted);
          ui.row().justify(Justify::End).show(|ui| {
            close = ui.button("Close").default_focus().clicked();
          });
        });
        close |= ui.back();
      }
      Dialog::Quit => match ui.confirm("Quit to desktop?", "Your progress is saved.") {
        Some(true) => {
          self.status = QUIT_CHOSEN.to_string();
          close = true;
        }
        Some(false) => close = true,
        None => {}
      },
    }
    if close {
      self.dialog = None;
    }
  }
}

impl Default for MenuPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for MenuPage {
  fn name(&self) -> &str {
    "Menus"
  }

  fn description(&self) -> &str {
    "A title screen played with the pad: the D-pad moves, A picks, B closes the dialog on top.\n\
     Options, Credits and Quit open modals that hold the focus until they close, and give it back to the menu. \
     The right stick scrolls the credits."
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

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    self.menu(ui);
    self.dialog(ui);
  }
}
