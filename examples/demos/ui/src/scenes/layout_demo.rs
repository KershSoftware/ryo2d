//! Layout demo page - containers: rows, grids (equal and fixed tracks),
//! stacks with anchored children, grow and spacers, and wrapping rows

use demo_kit::prelude::*;

const ANCHORS: [(Anchor, &str); 9] = [
  (Anchor::TopLeft, "TopLeft"),
  (Anchor::TopCenter, "TopCenter"),
  (Anchor::TopRight, "TopRight"),
  (Anchor::CenterLeft, "CenterLeft"),
  (Anchor::Center, "Center"),
  (Anchor::CenterRight, "CenterRight"),
  (Anchor::BottomLeft, "BottomLeft"),
  (Anchor::BottomCenter, "BottomCenter"),
  (Anchor::BottomRight, "BottomRight"),
];

const CHIPS: [&str; 14] = [
  "Sword", "Shield", "Bow", "Arrows", "Potion", "Elixir", "Map", "Compass", "Rope", "Torch",
  "Lantern", "Bread", "Cheese", "Key",
];

pub struct LayoutDemo {
  /// Room between the children of every section
  gap: f32,
  /// Room inside every section's panel
  padding: f32,
}

impl LayoutDemo {
  pub fn new() -> Self {
    Self {
      gap: 8.0,
      padding: 12.0,
    }
  }
}

impl Default for LayoutDemo {
  fn default() -> Self {
    Self::new()
  }
}

/// A section: its title, then a panel of `contents` with the page's padding
fn section(ui: &mut UiContext, title: &str, padding: f32, contents: impl FnOnce(&mut UiContext)) {
  let heading = ui.theme().text.heading.style().size(16.0);
  ui.column().id(title).gap(6.0).show(|ui| {
    ui.text(title).style(&heading);
    ui.panel().fill_width().padding(padding).show(contents);
  });
}

impl DemoPage for LayoutDemo {
  fn name(&self) -> &str {
    "Layout"
  }

  fn description(&self) -> &str {
    "Containers lay widgets out: rows, grids, stacks, grow and wrapping. Gap and Padding change every section.\n\
     The arrows walk the widgets and the list scrolls to follow; Tab moves between them and the settings panel."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Gap", &mut self.gap, 0.0..=32.0, 2.0);
    s.float("Padding", &mut self.padding, 0.0..=32.0, 2.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Wheel to scroll")]
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let muted = ui.theme().palette.muted;
    let (gap, padding) = (self.gap, self.padding);
    // Left of the settings panel
    let page = Rect::new(area.x, area.y, (area.width * 0.55).floor(), area.height);
    ui.column().focus_scope().at(page).show(|ui| {
      ui.scroll().fill().padding(8.0).gap(18.0).show(|ui| {
        section(ui, "1. A row, gap between", padding, |ui| {
          ui.row().gap(gap).show(|ui| {
            for label in ["A", "B", "C", "D"] {
              ui.button(label).id(label).show();
            }
          });
        });

        section(
          ui,
          "2. Grids: equal columns, then fixed tracks",
          padding,
          |ui| {
            ui.column().gap(gap).show(|ui| {
              ui.grid(3).gap(gap).show(|ui| {
                for i in 1..=9 {
                  ui.button(format!("Tile {i}")).id(i).fill_width().show();
                }
              });
              ui.grid(3).tracks(&[80.0, 160.0, 80.0]).gap(gap).show(|ui| {
                for (i, label) in ["80", "160", "80"].iter().enumerate() {
                  ui.button(*label).id(i).fill_width().show();
                }
              });
            });
          },
        );

        section(ui, "3. A stack: nine anchors in 300 x 160", padding, |ui| {
          ui.stack().width(300.0).height(160.0).show(|ui| {
            for (anchor, name) in ANCHORS {
              ui.text(name).anchor(anchor).color(muted);
            }
          });
        });

        section(
          ui,
          "4. Grow: a spacer pushes the end button over",
          padding,
          |ui| {
            ui.row().fill_width().gap(gap).show(|ui| {
              ui.button("Fixed").show();
              ui.spacer();
              ui.button("End").show();
            });
          },
        );

        section(
          ui,
          "5. Wrapping: chips flow onto new lines",
          padding,
          |ui| {
            ui.row().fill_width().wrap_items().gap(gap).show(|ui| {
              for chip in CHIPS {
                ui.button(chip).id(chip).variant("tab").show();
              }
            });
          },
        );
      });
    });
  }
}
