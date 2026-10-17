//! What the Dialogue page and its node version share: the conversation and
//! the shop it spends in

/// Where a choice leads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Next {
  To(usize),
  End,
}

/// One node of the conversation. A line with no choices continues to the
/// next node.
pub(super) struct Line {
  pub(super) speaker: &'static str,
  pub(super) text: &'static str,
  pub(super) choices: &'static [(&'static str, Next)],
}

const GEM_PRICE: u32 = 30;
const POTION_PRICE: u32 = 20;
/// The nodes buying leads to: they take the gold as they open
const GEM_NODE: usize = 1;
const POTION_NODE: usize = 2;

pub(super) const TREE: [Line; 5] = [
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

/// Where the conversation is, and what the player has
pub(super) struct Shop {
  /// The line shown; `None` once the conversation is over
  pub(super) node: Option<usize>,
  pub(super) health: f32,
  pub(super) gold: u32,
  pub(super) gems: u32,
}

impl Shop {
  pub(super) fn new() -> Self {
    Self {
      node: Some(0),
      health: 0.45,
      gold: 100,
      gems: 0,
    }
  }

  /// Show `next`: a node takes the price of what it sells as it opens. A
  /// node past the tree's end ends the conversation. Whether a line opened
  /// (false once the conversation is over)
  pub(super) fn go(&mut self, next: Next) -> bool {
    let node = match next {
      Next::To(node) if node < TREE.len() => node,
      _ => {
        self.node = None;
        return false;
      }
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
    true
  }

  /// The choices the player can afford
  pub(super) fn choices(&self, line: &Line) -> Vec<(&'static str, Next)> {
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
}
