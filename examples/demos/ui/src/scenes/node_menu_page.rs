//! The Menus page built from node trees: the title screen is spawned once,
//! and each dialog is a tree spawned when it opens and despawned when it
//! closes

use demo_kit::prelude::*;

use super::menu_data::{CONTINUES, CREDITS, Dialog, NEW_GAME, NO_SAVE, QUIT_CHOSEN};

pub struct NodeMenuPage {
  has_save: bool,
  volume: f32,
  fullscreen: bool,
  subtitles: bool,
  /// The open dialog and its tree
  dialog: Option<(Dialog, Entity)>,
}

impl NodeMenuPage {
  pub fn new() -> Self {
    Self {
      has_save: false,
      volume: 0.8,
      fullscreen: false,
      subtitles: true,
      dialog: None,
    }
  }

  fn menu() -> UiNode {
    let button = |label: &str, action: &str| UiNode::button(label).action(action).fill_width();
    UiNode::panel()
      .focus_scope()
      .wrap_focus()
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .child(
        button("Continue", "continue")
          .name("continue")
          .enabled(false),
      )
      .child(button("New Game", "new").default_focus())
      .child(button("Options", "options"))
      .child(button("Credits", "credits"))
      .child(button("Quit", "quit"))
  }

  fn options(&self) -> UiNode {
    UiNode::panel()
      .modal()
      .action("options")
      .width(320.0)
      .gap(12.0)
      .child(UiNode::text("Options").heading())
      .child(
        UiNode::slider(0.0..=1.0, self.volume)
          .step(0.05)
          .label("Volume")
          .show_value()
          .name("volume"),
      )
      .child(UiNode::toggle("Fullscreen", self.fullscreen).name("fullscreen"))
      .child(UiNode::checkbox("Subtitles", self.subtitles).name("subtitles"))
      .child(
        UiNode::row()
          .justify(Justify::End)
          .child(UiNode::button("Done").action("done").default_focus()),
      )
  }

  fn credits() -> UiNode {
    let entries = CREDITS.iter().map(|(role, name)| {
      UiNode::column()
        .gap(2.0)
        .child(UiNode::text(*role).role(TextRole::Body).muted())
        .child(UiNode::text(*name).role(TextRole::Body))
    });
    UiNode::panel()
      .modal()
      .action("credits")
      .width(360.0)
      .gap(12.0)
      .child(UiNode::text("Credits").heading())
      .child(
        UiNode::scroll()
          .height(200.0)
          .fill_width()
          .gap(10.0)
          .children(entries),
      )
      .child(UiNode::text("The right stick scrolls the credits.").muted())
      .child(
        UiNode::row()
          .justify(Justify::End)
          .child(UiNode::button("Close").action("close").default_focus()),
      )
  }

  fn open(&mut self, ctx: &mut SceneContext, dialog: Dialog) {
    if self.dialog.is_some() {
      return;
    }
    let tree = match dialog {
      Dialog::Options => self.options(),
      Dialog::Credits => Self::credits(),
      Dialog::Quit => UiNode::confirm("Quit to desktop?", "Your progress is saved.", "Yes", "No"),
    };
    self.dialog = Some((dialog, ctx.spawn_ui(tree)));
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

impl Default for NodeMenuPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for NodeMenuPage {
  fn name(&self) -> &str {
    "Node Menus"
  }

  fn description(&self) -> &str {
    "The Menus page built from node trees: the same title screen, played the same way.\n\
     Each dialog is a tree spawned when it opens; the page reacts to its nodes' events."
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
    ctx.spawn_ui(
      UiNode::column()
        .anchor(Anchor::TopCenter)
        .offset(0.0, 80.0)
        .child(UiNode::text("RYO QUEST").heading().size(48.0)),
    );
    ctx.spawn_ui(Self::menu());
    ctx.spawn_ui(
      UiNode::column()
        .anchor(Anchor::BottomCenter)
        .offset(0.0, -40.0)
        .align(Align::Center)
        .gap(8.0)
        .child(UiNode::text(NO_SAVE).muted().name("status"))
        .child(UiNode::text("[prompt=ui.confirm] Select    [prompt=ui.back] Back").markup()),
    );
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) {
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
