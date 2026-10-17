//! Dialogue page - a conversation with a shopkeeper: a typewriter line,
//! choices picked with the pad or number keys, and a HUD that changes as
//! you buy

use demo_kit::prelude::*;

use super::art;
use super::dialogue_data::{Next, Shop, TREE};

pub struct DialoguePage {
  portrait: SpriteHandle,
  shop: Shop,
  reveal: TextReveal,
  /// The leave-the-conversation question is open
  leaving: bool,
}

impl DialoguePage {
  pub fn new() -> Self {
    Self {
      portrait: art::shopkeeper(),
      shop: Shop::new(),
      reveal: Self::new_reveal(),
      leaving: false,
    }
  }

  fn new_reveal() -> TextReveal {
    TextReveal::new(40.0).with_punctuation_pause(0.15)
  }

  /// Show `next`, its line revealed from the start
  fn go(&mut self, next: Next) {
    if self.shop.go(next) {
      self.reveal = Self::new_reveal();
    }
  }

  /// Health and the purse, top-left: no focus scope, so it is the mouse's
  /// only and takes no keys
  fn hud(&self, ui: &mut UiContext) {
    let area = content_area(ui);
    ui.row()
      .anchor(Anchor::TopLeft)
      .offset(16.0, area.y + 16.0)
      .gap(12.0)
      .align(Align::Center)
      .show(|ui| {
        ui.progress(self.shop.health)
          .label("HP")
          .width(160.0)
          .show();
        ui.panel().padding(6.0).show(|ui| {
          ui.text(format!("Gold {}", self.shop.gold));
        });
        ui.panel().padding(6.0).show(|ui| {
          ui.text(format!("Gems {}", self.shop.gems));
        });
      });
  }

  /// The conversation's panel: the portrait, the speaker, the line and what
  /// the player can do next
  fn conversation(&mut self, ui: &mut UiContext) {
    let heading = ui.theme().text.heading.style();
    let body = ui.theme().text.body.style();
    let mut next = None;
    // The line shown; `None` once the conversation is over
    let node = self.shop.node.and_then(|n| Some((n, TREE.get(n)?)));
    let choices = node
      .map(|(_, line)| self.shop.choices(line))
      .unwrap_or_default();
    let labels: Vec<&str> = choices.iter().map(|(label, _)| *label).collect();
    let (portrait, reveal, leaving) = (&self.portrait, &mut self.reveal, self.leaving);
    ui.panel()
      .focus_scope()
      .anchor(Anchor::BottomCenter)
      .offset(0.0, -48.0)
      .width(760.0)
      .show(|ui| {
        ui.row().gap(16.0).show(|ui| {
          ui.image(portrait)
            .size(96.0, 96.0)
            .filter(Filter::Nearest)
            .show();
          ui.column().gap(10.0).grow(1.0).show(|ui| {
            let Some((n, line)) = node else {
              ui.text("Shopkeeper").style(&heading);
              ui.text("The shopkeeper waves goodbye. R starts again.")
                .style(&body);
              return;
            };
            ui.text(line.speaker).style(&heading);
            ui.column().max_width(600.0).show(|ui| {
              let text = ui.text(RichText::markup(line.text)).style(&body);
              // Under the leave question the line waits
              if leaving {
                text.reveal(reveal.drawn_clusters()).draw();
              } else {
                text.typewriter(reveal).draw();
              }
            });
            // A new scope for each line and once it has shown, so the
            // focus moves to what appears: Skip, then the choices or
            // Continue
            ui.column()
              .focus_scope()
              .id((n, reveal.is_done()))
              .show(|ui| {
                if !reveal.is_done() {
                  if ui.button("Skip").default_focus().clicked() {
                    reveal.finish();
                  }
                } else if !labels.is_empty() {
                  if let Some(i) = ui.choice_list(&labels).show() {
                    next = choices.get(i).map(|(_, next)| *next);
                  }
                } else if ui.button("Continue").default_focus().clicked() {
                  next = Some(Next::To(n + 1));
                }
              });
          });
        });
      });
    if let Some(next) = next {
      self.go(next);
    }
  }
}

impl Default for DialoguePage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for DialoguePage {
  fn name(&self) -> &str {
    "Dialogue"
  }

  fn description(&self) -> &str {
    "A conversation played with the pad: A skips the typewriter and picks a choice, B asks to leave. Number keys pick choices too.\n\
     Buying spends gold for a gem or a potion: the HUD's counters size to their text, so they grow and shrink with it."
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

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    self.hud(ui);
    self.conversation(ui);
    if self.leaving {
      match ui.confirm("Leave the conversation?", "The shopkeeper will remember.") {
        Some(true) => {
          self.leaving = false;
          self.shop.node = None;
        }
        Some(false) => self.leaving = false,
        None => {}
      }
    } else if self.shop.node.is_some() && ui.back() {
      self.leaving = true;
    }
  }
}
