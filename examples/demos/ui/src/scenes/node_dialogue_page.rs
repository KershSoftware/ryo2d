//! The Dialogue page built from node trees: the conversation panel is spawned
//! once, what the player can do next is a subtree swapped under it, and the
//! leave question is a confirm node

use demo_kit::prelude::*;

use super::art;
use super::dialogue_data::{Next, Shop, TREE};

/// The conversation's end, as the closure page shows it
const GOODBYE: &str = "The shopkeeper waves goodbye. R starts again.";

/// The HUD's nodes: the tree is moved to follow the shell's content area
struct Hud {
  root: Entity,
  hp: Entity,
  gold: Entity,
  gems: Entity,
}

/// The conversation panel's nodes the page writes
struct Conversation {
  speaker: Entity,
  /// The line's text, with its `TextReveal`
  line: Entity,
  /// The column the choice scope sits in
  lines: Entity,
}

pub struct NodeDialoguePage {
  portrait: SpriteHandle,
  shop: Shop,
  hud: Option<Hud>,
  conversation: Option<Conversation>,
  /// The scope of what the player can do next, and the `(line, revealed)`
  /// it was built for
  scope: Option<(Entity, (usize, bool))>,
  /// The leave-the-conversation question, while it is open
  confirm: Option<Entity>,
  /// Where each of the choice list's choices leads
  choices: Vec<Next>,
}

impl NodeDialoguePage {
  pub fn new() -> Self {
    Self {
      portrait: art::shopkeeper(),
      shop: Shop::new(),
      hud: None,
      conversation: None,
      scope: None,
      confirm: None,
      choices: Vec::new(),
    }
  }

  fn panel(&self) -> UiNode {
    let (speaker, text) = self
      .shop
      .node
      .and_then(|n| TREE.get(n))
      .map_or(("", ""), |line| (line.speaker, line.text));
    UiNode::panel()
      .focus_scope()
      .anchor(Anchor::BottomCenter)
      .offset(0.0, -48.0)
      .width(760.0)
      .child(
        UiNode::row()
          .gap(16.0)
          .child(
            UiNode::image(self.portrait.clone())
              .image_size(96.0, 96.0)
              .filter(Filter::Nearest),
          )
          .child(
            UiNode::column()
              .gap(10.0)
              .grow(1.0)
              .name("lines")
              .child(UiNode::text(speaker).heading().name("speaker"))
              .child(
                UiNode::column().max_width(600.0).child(
                  UiNode::text(text)
                    .markup()
                    .role(TextRole::Body)
                    .typewriter(TextReveal::new(40.0).with_punctuation_pause(0.15))
                    .name("line"),
                ),
              ),
          ),
      )
  }

  /// Health and the purse, top-left: no focus scope, so it is the mouse's
  /// only and takes no keys
  fn hud(&self, top: f32) -> UiNode {
    UiNode::row()
      .anchor(Anchor::TopLeft)
      .offset(16.0, top)
      .gap(12.0)
      .align(Align::Center)
      .child(
        UiNode::progress(self.shop.health)
          .label("HP")
          .width(160.0)
          .name("hp"),
      )
      .child(
        UiNode::panel()
          .padding(6.0)
          .child(UiNode::text(format!("Gold {}", self.shop.gold)).name("gold")),
      )
      .child(
        UiNode::panel()
          .padding(6.0)
          .child(UiNode::text(format!("Gems {}", self.shop.gems)).name("gems")),
      )
  }

  /// Spawn the HUD on the first call, where the content area is known, then
  /// keep it below the shell's top lines
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
      None => {
        let root = ctx.spawn_ui(self.hud(top));
        // Kept even if a name were missing, so the HUD is spawned once
        let find = |name| ctx.find_ui_in(root, name).unwrap_or(Entity::PLACEHOLDER);
        self.hud = Some(Hud {
          root,
          hp: find("hp"),
          gold: find("gold"),
          gems: find("gems"),
        });
      }
    }
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
    if let Some(hud) = &self.hud {
      if let Some(mut value) = ctx.world.get_mut::<UiValue>(hud.hp)
        && value.0 != self.shop.health
      {
        value.0 = self.shop.health;
      }
      Self::set_label(ctx, hud.gold, &format!("Gold {}", self.shop.gold));
      Self::set_label(ctx, hud.gems, &format!("Gems {}", self.shop.gems));
    }
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

  /// Swap in the scope for what the player can do next: Skip while the line
  /// shows, then its choices or Continue; nothing once the conversation is
  /// over. A new scope is a new subtree, so the focus moves to it.
  /// `skipped`: Skip finished the line this frame, so it shows whole at the
  /// next draw, with the scope that follows it.
  fn update_scope(&mut self, ctx: &mut SceneContext, skipped: bool) {
    let Some(n) = self.shop.node else {
      if let Some((scope, _)) = self.scope.take() {
        ctx.world.despawn(scope);
      }
      return;
    };
    let done = skipped || self.reveal(ctx).is_some_and(|reveal| reveal.is_done());
    let key = (n, done);
    if self.scope.is_some_and(|(_, built)| built == key) {
      return;
    }
    if let Some((scope, _)) = self.scope.take() {
      ctx.world.despawn(scope);
    }
    let Some(lines) = self.conversation.as_ref().map(|c| c.lines) else {
      return;
    };
    let scope = UiNode::column().focus_scope();
    let scope = if !done {
      scope.child(UiNode::button("Skip").action("skip").default_focus())
    } else {
      let choices = TREE
        .get(n)
        .map(|line| self.shop.choices(line))
        .unwrap_or_default();
      self.choices = choices.iter().map(|(_, next)| *next).collect();
      if choices.is_empty() {
        scope.child(
          UiNode::button("Continue")
            .action("continue")
            .default_focus(),
        )
      } else {
        let labels: Vec<&str> = choices.iter().map(|(label, _)| *label).collect();
        scope.child(UiNode::choice_list(&labels).action("choose"))
      }
    };
    self.scope = ctx.spawn_ui_in(lines, scope).map(|scope| (scope, key));
  }

  /// Ask whether to leave; the line waits under the question
  fn open_confirm(&mut self, ctx: &mut SceneContext) {
    self.confirm = Some(ctx.spawn_ui(UiNode::confirm(
      "Leave the conversation?",
      "The shopkeeper will remember.",
      "Yes",
      "No",
    )));
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

impl Default for NodeDialoguePage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for NodeDialoguePage {
  fn name(&self) -> &str {
    "Node Dialogue"
  }

  fn description(&self) -> &str {
    "The Dialogue page built from node trees: the same conversation, played the same way.\n\
     What the player can do next is a subtree swapped in under the line; the leave question is a confirm node."
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
    let root = ctx.spawn_ui(self.panel());
    let find = |name| ctx.find_ui_in(root, name);
    if let (Some(speaker), Some(line), Some(lines)) = (find("speaker"), find("line"), find("lines"))
    {
      self.conversation = Some(Conversation {
        speaker,
        line,
        lines,
      });
    }
    self.update_scope(ctx, false);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
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
