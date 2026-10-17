//! A closure screen around an embedded node grid: one focus path runs from
//! the closure's buttons through the grid and back

use demo_kit::prelude::*;

const START_CELLS: u32 = 8;
const MAX_CELLS: u32 = 16;
const KEPT_CELLS: usize = 4;

#[derive(Component)]
struct Cell(u32);

pub struct MixedPage {
  grid: Option<Entity>,
  status: String,
}

impl MixedPage {
  pub fn new() -> Self {
    Self {
      grid: None,
      status: "Add a cell, or pick one.".to_string(),
    }
  }

  fn cell(n: u32) -> UiNode {
    UiNode::button(format!("Cell {n}"))
      .action("cell")
      .width(80.0)
      .insert(Cell(n))
  }

  fn cells(&self, ctx: &SceneContext) -> Vec<Entity> {
    self
      .grid
      .and_then(|grid| ctx.world.get::<Children>(grid))
      .map(|kids| kids.iter().copied().collect())
      .unwrap_or_default()
  }

  fn add(&mut self, ctx: &mut SceneContext) {
    let Some(grid) = self.grid else { return };
    let count = self.cells(ctx).len() as u32;
    if count >= MAX_CELLS {
      self.status = "The grid is full.".to_string();
      return;
    }
    let n = count + 1;
    ctx.spawn_ui_in(grid, Self::cell(n));
    self.status = format!("Added cell {n}.");
  }

  fn clear(&mut self, ctx: &mut SceneContext) {
    for cell in self.cells(ctx).into_iter().skip(KEPT_CELLS) {
      ctx.world.despawn(cell);
    }
    self.status = "Cleared.".to_string();
  }
}

impl Default for MixedPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for MixedPage {
  fn name(&self) -> &str {
    "Mixed"
  }

  fn description(&self) -> &str {
    "A closure screen around an embedded node grid. The D-pad runs from the closure's buttons \
     through the grid and back: one focus path across both.\n\
     Add spawns a cell node into the grid, Clear despawns all but the first four, and picking a cell reports it."
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let cells = (1..=START_CELLS).map(Self::cell);
    self.grid = Some(ctx.spawn_ui(UiNode::grid(4).gap(6.0).embedded().children(cells)));
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    for event in ctx.ui_events() {
      if event.kind == UiEventKind::Clicked
        && event.action.as_deref() == Some("cell")
        && let Some(Cell(n)) = ctx.world.get::<Cell>(event.node)
      {
        self.status = format!("Picked cell {n}.");
      }
    }
    let Some(grid) = self.grid else { return };
    let heading = ui.theme().text.heading.style();
    let muted = ui.theme().palette.muted;
    let mut add = false;
    let mut clear = false;
    let mut done = false;
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(420.0)
      .gap(10.0)
      .show(|ui| {
        ui.text("Mixed").style(&heading);
        ui.row().gap(8.0).show(|ui| {
          add = ui.button("Add").default_focus().clicked();
          clear = ui.button("Clear").clicked();
        });
        ui.node(grid, ctx.world);
        ui.text(self.status.as_str()).color(muted);
        done = ui.button("Done").clicked();
      });
    if add {
      self.add(ctx);
    }
    if clear {
      self.clear(ctx);
    }
    if done {
      self.status = "Done.".to_string();
    }
  }
}
