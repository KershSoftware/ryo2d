//! Inventory page - a backpack and equipment slots: items move by mouse
//! drag or by pad (A picks up, A puts down), and equipment refuses the
//! wrong kind of item

use demo_kit::prelude::*;

use super::inventory_model::{BAG, EquipSlot, GAP, IDLE, Icons, Inventory, Item, SlotRef};

pub struct InventoryPage {
  model: Inventory,
  icons: Icons,
  /// An item was held (by the pad or a mouse drag) on the last frame
  /// (`ui.held`): Escape is the UI's Back then, which puts it back
  holding: bool,
}

impl InventoryPage {
  pub fn new() -> Self {
    Self {
      model: Inventory::new(),
      icons: Icons::new(),
      holding: false,
    }
  }

  /// The status line: what the last pick-up, move or refusal did
  pub fn status(&self) -> &str {
    self.model.status()
  }

  /// What slot `at` holds
  pub fn item(&self, at: SlotRef) -> Option<Item> {
    self.model.item(at)
  }

  /// One slot: a target for any item, the source of the one it holds
  fn slot(&self, ui: &mut UiContext, at: SlotRef, key: usize) -> Response {
    let mut slot = ui
      .slot()
      .id(key)
      .filter(Filter::Nearest)
      .drop_target::<SlotRef>();
    // The bag's first slot has the first focus
    if at == SlotRef::Bag(0) {
      slot = slot.default_focus();
    }
    match self.item(at) {
      Some(item) => {
        slot = slot
          .icon(self.icons.of(item.kind))
          .count(item.count)
          .tooltip(item.kind.name())
          .drag_source(at);
      }
      None => {
        if let SlotRef::Equip(equip) = at {
          slot = slot.tooltip(format!("{} slot", equip.name()));
        }
      }
    }
    slot.show()
  }
}

/// What a frame's slots reported
#[derive(Default)]
struct Seen {
  /// A drop: from, to (`None` for Discard)
  drop: Option<(SlotRef, Option<SlotRef>)>,
  /// The slot under the pointer, and the focused one: the details show the
  /// first
  hovered: Option<SlotRef>,
  focused: Option<SlotRef>,
}

impl Seen {
  fn note(&mut self, at: SlotRef, response: &Response) {
    if let Some(from) = response.dropped::<SlotRef>() {
      self.drop = Some((from, Some(at)));
    }
    if response.hovered() {
      self.hovered = Some(at);
    }
    if response.focused() {
      self.focused = Some(at);
    }
  }
}

impl Default for InventoryPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for InventoryPage {
  fn name(&self) -> &str {
    "Inventory"
  }

  fn description(&self) -> &str {
    "Drag and drop between a backpack and equipment slots, with the mouse or the pad: A picks an item up, A puts it down, B puts it back.\n\
     An equipment slot takes only its kind of item. Sort stacks the backpack by kind; Discard throws away what is dropped on it."
  }

  fn controls(&self) -> Vec<Control> {
    let mut controls = vec![
      Control::mouse("Drag an item to another slot"),
      Control::keys(
        "Enter / Space",
        &[KeyCode::Enter, KeyCode::Space],
        "Pick up, put down",
      ),
    ];
    if self.holding {
      controls.push(Control::key(KeyCode::Escape, "Put the item back"));
    }
    controls
  }

  /// While an item is held, Escape puts it back instead of quitting
  fn wants_escape(&self) -> bool {
    self.holding
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let heading = ui.theme().text.heading.style();
    let body = ui.theme().text.body.style();
    let muted = ui.theme().palette.muted;
    let mut seen = Seen::default();
    let mut sort = false;

    ui.column()
      .focus_scope()
      .anchor(Anchor::Center)
      .gap(12.0)
      .align(Align::Center)
      .show(|ui| {
        ui.row().gap(16.0).show(|ui| {
          ui.panel().gap(10.0).show(|ui| {
            ui.text("Equipment").style(&heading);
            ui.column().gap(GAP).show(|ui| {
              for equip in EquipSlot::ALL {
                ui.row()
                  .id(equip.index())
                  .gap(12.0)
                  .align(Align::Center)
                  .show(|ui| {
                    ui.text(equip.name()).style(&body).color(muted);
                    ui.spacer();
                    let at = SlotRef::Equip(equip);
                    let response = self.slot(ui, at, equip.index());
                    seen.note(at, &response);
                  });
              }
            });
          });
          ui.panel().gap(10.0).show(|ui| {
            ui.text("Backpack").style(&heading);
            ui.grid(6).gap(GAP).show(|ui| {
              for i in 0..BAG {
                let at = SlotRef::Bag(i);
                let response = self.slot(ui, at, i);
                seen.note(at, &response);
              }
            });
          });
        });

        // What the slot under the pointer, or the focused one, holds
        ui.panel().fill_width().height(76.0).gap(4.0).show(|ui| {
          match seen.hovered.or(seen.focused).and_then(|at| self.item(at)) {
            Some(item) => {
              ui.text(item.kind.name()).style(&heading);
              ui.text(item.kind.description()).style(&body);
            }
            None => {
              ui.text("Nothing here").style(&heading).color(muted);
              ui.text("Point at a slot, or move the focus to one, to see what it holds.")
                .style(&body)
                .color(muted);
            }
          }
        });

        ui.row()
          .fill_width()
          .gap(12.0)
          .align(Align::Center)
          .show(|ui| {
            sort = ui.button("Sort").clicked();
            ui.spacer();
            ui.text("Discard").style(&body).color(muted);
            let discard = ui
              .slot()
              .drop_target::<SlotRef>()
              .tooltip("Drop an item here to throw it away")
              .show();
            if let Some(from) = discard.dropped::<SlotRef>() {
              seen.drop = Some((from, None));
            }
          });

        ui.text(self.model.status()).style(&body);
        ui.text(RichText::markup(
          "[prompt=ui.confirm] Pick up / put down    [prompt=ui.back] Cancel",
        ))
        .color(muted);
      });

    let mut told = match seen.drop {
      Some((from, Some(to))) => self.model.move_item(from, to),
      Some((from, None)) => self.model.discard(from),
      None => false,
    };
    if sort {
      self.model.sort();
      told = true;
    }
    // What is held now, after this frame's pick-ups; once it is put back,
    // released over nothing or put down where it was, the status stops
    // saying it is held
    let held = ui.held::<SlotRef>().and_then(|at| self.item(at));
    match held {
      Some(item) => self
        .model
        .set_status(format!("Holding the {}.", item.kind.name())),
      None if self.holding && !told => self.model.set_status(IDLE),
      None => {}
    }
    self.holding = held.is_some();
  }
}
