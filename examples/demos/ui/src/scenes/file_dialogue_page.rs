//! The Dialogue page built from a UI file: the conversation panel is spawned
//! once, what the player can do next is a template swapped into its choices
//! column, and the leave question is a template too

use demo_kit::prelude::*;

use super::art;
use super::dialogue_data::{Next, Shop, TREE};

/// The conversation's end, as the closure page shows it
const GOODBYE: &str = "The shopkeeper waves goodbye. R starts again.";

/// The page's file, relative to the working directory
const DIALOGUE: &str = "assets/ui/dialogue.ui.ron";

/// The HUD's tree, moved to follow the shell's content area
struct Hud {
  root: Entity,
}

/// The conversation panel's nodes the page writes
struct Conversation {
  speaker: Entity,
  /// The line's text, with its `TextReveal`
  line: Entity,
  /// The focus-scope column holding what the player can do next
  choices: Entity,
}

pub struct FileDialoguePage {
  portrait: SpriteHandle,
  shop: Shop,
  hud: Option<Hud>,
  conversation: Option<Conversation>,
  /// The `(line, revealed)` the choices column was built for
  built: Option<(usize, bool)>,
  /// The leave-the-conversation question, while it is open
  confirm: Option<Entity>,
  /// Where each of the choice list's choices leads
  choices: Vec<Next>,
  /// Why the file did not load, drawn in place of the page
  error: Option<String>,
}

impl FileDialoguePage {
  pub fn new() -> Self {
    Self {
      portrait: art::shopkeeper(),
      shop: Shop::new(),
      hud: None,
      conversation: None,
      built: None,
      confirm: None,
      choices: Vec::new(),
      error: None,
    }
  }

  fn fail(&mut self, error: UiFileError) {
    let text = error.to_string();
    if self.error.as_deref() != Some(text.as_str()) {
      game_error!("{error}");
      self.error = Some(text);
    }
  }

  /// Spawn the HUD on the first call, where the content area is known, then
  /// keep it below the shell's top lines. It has no focus scope, so it is the
  /// mouse's only and takes no keys.
  fn place_hud(&mut self, ui: &UiContext, ctx: &mut SceneContext) {
    let top = content_area(ui).y + 16.0;
    match &self.hud {
      Some(hud) => {
        if let Some(mut root) = ctx.world.get_mut::<UiRoot>(hud.root)
          && root.offset.y != top
        {
          root.offset.y = top;
        }
      }
      None => match ctx.ui_template(DIALOGUE, "Hud", &UiArgs::new()) {
        Ok(tree) => {
          let root = ctx.spawn_ui(tree);
          if let Some(mut node) = ctx.world.get_mut::<UiRoot>(root) {
            node.offset.y = top;
          }
          self.hud = Some(Hud { root });
          self.show_hud(ctx);
        }
        Err(error) => self.fail(error),
      },
    }
  }

  /// Write health and the purse to the HUD's bindings
  fn show_hud(&self, ctx: &mut SceneContext) {
    if let Some(hud) = &self.hud {
      ctx.ui_set(hud.root, "hp", self.shop.health);
      ctx.ui_set(hud.root, "gold", self.shop.gold);
      ctx.ui_set(hud.root, "gems", self.shop.gems);
    }
  }

  /// Spawn the conversation panel, with the portrait in its placeholder and
  /// the current line in its texts
  fn spawn_panel(&mut self, ctx: &mut SceneContext) -> Result<(), UiFileError> {
    let panel = ctx.assets.load_ui(DIALOGUE)?.root(ctx.assets)?;
    let root = ctx.spawn_ui(panel);
    let find = |name| ctx.find_ui_in(root, name);
    let (Some(speaker), Some(line), Some(choices), Some(portrait)) = (
      find("speaker"),
      find("line"),
      find("choices"),
      find("portrait"),
    ) else {
      ctx.world.despawn(root);
      return Err(UiFileError {
        issues: vec![UiFileIssue {
          path: DIALOGUE.to_string(),
          line: 0,
          column: 0,
          message: "the root needs nodes named speaker, line, choices and portrait".to_string(),
        }],
      });
    };
    let image = UiNode::image(self.portrait.clone())
      .image_size(96.0, 96.0)
      .filter(Filter::Nearest);
    ctx.spawn_ui_in(portrait, image);
    if let Some(shown) = self.shop.node.and_then(|n| TREE.get(n)) {
      Self::set_label(ctx, speaker, shown.speaker);
      Self::set_label(ctx, line, shown.text);
    }
    self.conversation = Some(Conversation {
      speaker,
      line,
      choices,
    });
    Ok(())
  }

  fn reveal<'w>(&self, ctx: &'w mut SceneContext) -> Option<Mut<'w, TextReveal>> {
    let line = self.conversation.as_ref()?.line;
    ctx.world.get_mut::<TextReveal>(line)
  }

  fn set_label(ctx: &mut SceneContext, node: Entity, text: &str) {
    if let Some(mut label) = ctx.world.get_mut::<UiLabel>(node)
      && label.0 != text
    {
      label.0 = text.to_string();
    }
  }

  /// Show `next`: its line revealed from the start, or the conversation's
  /// end; and the HUD as the shop left it
  fn go(&mut self, ctx: &mut SceneContext, next: Next) {
    let line = if self.shop.go(next) {
      self.shop.node.and_then(|n| TREE.get(n))
    } else {
      None
    };
    match line {
      Some(line) => {
        if let Some(conversation) = &self.conversation {
          Self::set_label(ctx, conversation.speaker, line.speaker);
          Self::set_label(ctx, conversation.line, line.text);
        }
        if let Some(mut reveal) = self.reveal(ctx) {
          reveal.restart();
        }
      }
      None => self.show_end(ctx),
    }
    self.show_hud(ctx);
  }

  /// The conversation is over: the goodbye, shown whole
  fn show_end(&mut self, ctx: &mut SceneContext) {
    self.shop.node = None;
    if let Some(conversation) = &self.conversation {
      Self::set_label(ctx, conversation.speaker, "Shopkeeper");
      Self::set_label(ctx, conversation.line, GOODBYE);
    }
    if let Some(mut reveal) = self.reveal(ctx) {
      reveal.finish();
    }
  }

  /// The tree for what the player can do next: Skip while the line shows,
  /// then its choices or Continue
  fn choices_tree(
    &mut self,
    ctx: &mut SceneContext,
    n: usize,
    done: bool,
  ) -> Result<UiNode, UiFileError> {
    let column = UiNode::column().focus_scope().name("choices");
    let none = UiArgs::new();
    let content = if !done {
      ctx.ui_template(DIALOGUE, "Skip", &none)?
    } else {
      let choices = TREE
        .get(n)
        .map(|line| self.shop.choices(line))
        .unwrap_or_default();
      if choices.is_empty() {
        let tree = ctx.ui_template(DIALOGUE, "Continue", &none)?;
        self.choices = Vec::new();
        tree
      } else {
        let labels: Vec<String> = choices.iter().map(|(label, _)| label.to_string()).collect();
        let tree = ctx.ui_template(DIALOGUE, "Choices", &UiArgs::new().with("options", labels))?;
        self.choices = choices.iter().map(|(_, next)| *next).collect();
        tree
      }
    };
    Ok(column.child(content))
  }

  /// Swap in the choices for what the player can do next; the column is
  /// hidden once the conversation is over. An updated scope is not a newly
  /// opened one, so Skip and Continue are given the focus with `focus_ui`. A
  /// choice list has no `Response`, so `focus_ui` does nothing for it; it
  /// takes the focus by the scope's own rule.
  /// `skipped`: Skip finished the line this frame, so it shows whole at the
  /// next draw, with the choices that follow it.
  fn update_scope(&mut self, ctx: &mut SceneContext, skipped: bool) {
    let Some(choices) = self.conversation.as_ref().map(|c| c.choices) else {
      return;
    };
    let Some(n) = self.shop.node else {
      if self.built.take().is_some() {
        ctx.update_ui(choices, UiNode::column().focus_scope().name("choices"));
        ctx.world.entity_mut(choices).insert(UiHidden);
      }
      return;
    };
    let done = skipped || self.reveal(ctx).is_some_and(|reveal| reveal.is_done());
    let key = (n, done);
    if self.built == Some(key) {
      return;
    }
    let tree = match self.choices_tree(ctx, n, done) {
      Ok(tree) => tree,
      Err(error) => return self.fail(error),
    };
    ctx.update_ui(choices, tree);
    ctx.world.entity_mut(choices).remove::<UiHidden>();
    self.built = Some(key);
    if let Some(pick) = ctx.find_ui_in(choices, "pick") {
      ctx.focus_ui(pick);
    }
  }

  /// Ask whether to leave; the line waits under the question
  fn open_confirm(&mut self, ctx: &mut SceneContext) {
    match ctx.ui_template(DIALOGUE, "LeaveConfirm", &UiArgs::new()) {
      Ok(tree) => self.confirm = Some(ctx.spawn_ui(tree)),
      Err(error) => return self.fail(error),
    }
    if let Some(mut reveal) = self.reveal(ctx) {
      reveal.pause();
    }
  }

  fn close_confirm(&mut self, ctx: &mut SceneContext) {
    if let Some(confirm) = self.confirm.take() {
      ctx.world.despawn(confirm);
    }
    if let Some(mut reveal) = self.reveal(ctx) {
      reveal.resume();
    }
  }
}

impl Default for FileDialoguePage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for FileDialoguePage {
  fn name(&self) -> &str {
    "File Dialogue"
  }

  fn description(&self) -> &str {
    "The Dialogue page built from a UI file: the same conversation, played the same way.\n\
     What the player can do next is a template swapped into the file's choices column; the leave question is a template too."
  }

  fn controls(&self) -> Vec<Control> {
    let digits = [
      KeyCode::Digit1,
      KeyCode::Digit2,
      KeyCode::Digit3,
      KeyCode::Digit4,
    ];
    let mut controls = vec![
      Control::keys(
        "Enter / Space",
        &[KeyCode::Enter, KeyCode::Space],
        "Skip, pick",
      ),
      Control::keys("1-4", &digits, "Pick a choice"),
    ];
    if self.wants_escape() {
      controls.push(Control::key(KeyCode::Escape, "Leave the conversation"));
    }
    controls
  }

  /// Escape asks to leave, until the conversation is over
  fn wants_escape(&self) -> bool {
    self.shop.node.is_some()
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    if let Err(error) = self.spawn_panel(ctx) {
      self.fail(error);
    }
    self.update_scope(ctx, false);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    if let Some(error) = &self.error {
      ui.text(error.as_str());
    }
    self.place_hud(ui, ctx);
    let mut skipped = false;
    for event in ctx.ui_events() {
      if event.kind != UiEventKind::Clicked {
        continue;
      }
      match event.action.as_deref() {
        Some("skip") => {
          if let Some(mut reveal) = self.reveal(ctx) {
            reveal.finish();
            skipped = true;
          }
        }
        Some("choose") => {
          let picked = ctx.world.get::<UiIndex>(event.node).map(|index| index.0);
          if let Some(next) = picked.and_then(|i| self.choices.get(i).copied()) {
            self.go(ctx, next);
          }
        }
        Some("continue") => {
          if let Some(n) = self.shop.node {
            self.go(ctx, Next::To(n + 1));
          }
        }
        Some(answer @ ("yes" | "no")) => {
          self.close_confirm(ctx);
          if answer == "yes" {
            self.show_end(ctx);
          }
        }
        _ => {}
      }
    }
    if self.confirm.is_none() && self.shop.node.is_some() && ui.back() {
      self.open_confirm(ctx);
    }
    self.update_scope(ctx, skipped);
  }
}
