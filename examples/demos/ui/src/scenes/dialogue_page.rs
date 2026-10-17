//! Dialogue page - a conversation with a shopkeeper: a typewriter line,
//! choices picked with the pad or number keys, and a HUD that changes as
//! you buy

use demo_kit::prelude::*;

use super::art;

/// Where a choice leads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
  To(usize),
  End,
}

/// One node of the conversation. A line with no choices continues to the
/// next node.
struct Line {
  speaker: &'static str,
  text: &'static str,
  choices: &'static [(&'static str, Next)],
}

const GEM_PRICE: u32 = 30;
const POTION_PRICE: u32 = 20;
/// The nodes buying leads to: they take the gold as they open
const GEM_NODE: usize = 1;
const POTION_NODE: usize = 2;

const TREE: [Line; 5] = [
  Line {
    speaker: "Shopkeeper",
    text: "Welcome to the [color=#ffd166]Rusty Lantern[/color], traveller! Potions, gems and the finest goods this side of the river.",
    choices: &[
      ("Buy a gem (30 gold)", Next::To(GEM_NODE)),
      ("Buy a potion (20 gold)", Next::To(POTION_NODE)),
      ("What's that smell?", Next::To(3)),
      ("Goodbye", Next::End),
    ],
  },
  Line {
    speaker: "Shopkeeper",
    text: "One [color=#7fdbff]sapphire gem[/color], polished this very morning. Anything else?",
    choices: &[("Show me the rest", Next::To(0)), ("That's all", Next::End)],
  },
  Line {
    speaker: "Shopkeeper",
    text: "A [color=#ff6b6b]healing potion[/color]. Drink it slowly, or don't. I'm not your mother.",
    choices: &[("Show me the rest", Next::To(0)), ("That's all", Next::End)],
  },
  Line {
    speaker: "Shopkeeper",
    text: "That would be the [color=#b8e986]troll cheese[/color].[pause=0.4] Nobody buys the troll cheese.",
    choices: &[],
  },
  Line {
    speaker: "Shopkeeper",
    text: "Nobody.[pause=0.3] Now, are you buying, or just breathing?",
    choices: &[("Show me the rest", Next::To(0)), ("Goodbye", Next::End)],
  },
];

pub struct DialoguePage {
  portrait: SpriteHandle,
  /// The line shown; `None` once the conversation is over
  node: Option<usize>,
  reveal: TextReveal,
  /// The leave-the-conversation question is open
  leaving: bool,
  health: f32,
  gold: u32,
  gems: u32,
}

impl DialoguePage {
  pub fn new() -> Self {
    Self {
      portrait: art::shopkeeper(),
      node: Some(0),
      reveal: Self::new_reveal(),
      leaving: false,
      health: 0.45,
      gold: 100,
      gems: 0,
    }
  }

  fn new_reveal() -> TextReveal {
    TextReveal::new(40.0).with_punctuation_pause(0.15)
  }

  /// Show `next`: a node takes the price of what it sells as it opens
  fn go(&mut self, next: Next) {
    let Next::To(node) = next else {
      self.node = None;
      return;
    };
    match node {
      GEM_NODE => {
        self.gold = self.gold.saturating_sub(GEM_PRICE);
        self.gems += 1;
      }
      POTION_NODE => {
        self.gold = self.gold.saturating_sub(POTION_PRICE);
        self.health = (self.health + 0.35).min(1.0);
      }
      _ => {}
    }
    self.node = Some(node);
    self.reveal = Self::new_reveal();
  }

  /// The choices the player can afford
  fn choices(&self, line: &Line) -> Vec<(&'static str, Next)> {
    line
      .choices
      .iter()
      .copied()
      .filter(|(_, next)| match next {
        Next::To(GEM_NODE) => self.gold >= GEM_PRICE,
        Next::To(POTION_NODE) => self.gold >= POTION_PRICE,
        _ => true,
      })
      .collect()
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
        ui.progress(self.health).label("HP").width(160.0).show();
        ui.panel().padding(6.0).show(|ui| {
          ui.text(format!("Gold {}", self.gold));
        });
        ui.panel().padding(6.0).show(|ui| {
          ui.text(format!("Gems {}", self.gems));
        });
      });
  }

  /// The conversation's panel: the portrait, the speaker, the line and what
  /// the player can do next
  fn conversation(&mut self, ui: &mut UiContext) {
    let heading = ui.theme().text.heading.style();
    let body = ui.theme().text.body.style();
    let mut next = None;
    let node = self.node;
    let choices = node.map(|n| self.choices(&TREE[n])).unwrap_or_default();
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
            let Some(n) = node else {
              ui.text("Shopkeeper").style(&heading);
              ui.text("The shopkeeper waves goodbye. R starts again.")
                .style(&body);
              return;
            };
            let line = &TREE[n];
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
    self.node.is_some()
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    self.hud(ui);
    self.conversation(ui);
    if self.leaving {
      match ui.confirm("Leave the conversation?", "The shopkeeper will remember.") {
        Some(true) => {
          self.leaving = false;
          self.node = None;
        }
        Some(false) => self.leaving = false,
        None => {}
      }
    } else if self.node.is_some() && ui.back() {
      self.leaving = true;
    }
  }
}
