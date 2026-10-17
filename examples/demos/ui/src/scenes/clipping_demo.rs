//! Clipping demo page - scroll areas (one nested in another), nested clips
//! for absolute drawing, and modal dialogs

use demo_kit::prelude::*;

const ROWS: usize = 40;
const CARDS: usize = 12;
/// The list's row the card strip follows
const STRIP_AFTER: usize = 9;

/// The dialog open over the page
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialog {
  Modal,
  Ask,
}

pub struct ClippingDemo {
  dialog: Option<Dialog>,
  /// What the last dialog was answered with
  result: Option<(&'static str, bool)>,
  /// The list's row last clicked or confirmed
  selected: Option<usize>,
}

impl ClippingDemo {
  pub fn new() -> Self {
    Self {
      dialog: None,
      result: None,
      selected: None,
    }
  }

  /// The list: 40 rows with a strip of 12 cards in it, each a scroll area;
  /// the wheel scrolls the innermost one under the pointer that can move
  fn list(&mut self, ui: &mut UiContext, rect: Rect) {
    let ring = ui.focus_visible();
    let accent = ui.theme().palette.accent;
    let label = TextStyle::default();
    let note = TextStyle::default().color(Color::LIGHT_GRAY);
    let selected = self.selected;
    let mut clicked = None;
    ui.draw_rect(rect, Color::rgb(30, 30, 40));
    ui.scroll().at(rect).padding(6.0).gap(2.0).show(|ui| {
      for i in 0..ROWS {
        let response = ui
          .custom(0.0, 26.0)
          .id(i)
          .fill_width()
          .sense(Sense::click())
          .focusable(true)
          .paint(|p, response| {
            let size = p.size();
            let fill = if Some(i) == selected {
              Color::rgb(60, 100, 160)
            } else if response.hovered() {
              Color::rgb(55, 55, 75)
            } else if i % 2 == 0 {
              Color::rgb(40, 40, 55)
            } else {
              Color::rgb(35, 35, 48)
            };
            let row = Rect::new(0.0, 0.0, size.x, size.y);
            if response.focused() && ring {
              p.bordered_rect(row, fill, 3.0, 2.0, accent);
            } else {
              p.rect(row, fill);
            }
            p.text(format!("Item {:02}", i + 1), 8.0, 5.0, &label);
          });
        if response.clicked() {
          clicked = Some(i);
        }
        if i == STRIP_AFTER {
          // A strip inside the list: the wheel over it scrolls it while it
          // can, then the list; Shift turns the wheel sideways
          ui.scroll()
            .horizontal()
            .fill_width()
            .height(84.0)
            .gap(8.0)
            .padding(4.0)
            .show(|ui| {
              for card in 0..CARDS {
                ui.custom(110.0, 64.0)
                  .id(card)
                  .focusable(true)
                  .paint(|p, response| {
                    let size = p.size();
                    let hue = card as f32 / CARDS as f32;
                    let fill = Color::rgb(
                      (60.0 + hue * 120.0) as u8,
                      (70.0 + (1.0 - hue) * 80.0) as u8,
                      140,
                    );
                    let border = if response.focused() && ring {
                      accent
                    } else {
                      Color::rgb(100, 100, 120)
                    };
                    let width = if response.focused() && ring { 2.0 } else { 1.0 };
                    p.bordered_rect(
                      Rect::new(0.0, 0.0, size.x, size.y),
                      fill,
                      4.0,
                      width,
                      border,
                    );
                    p.text(format!("Card {}", card + 1), 8.0, 8.0, &label);
                    p.text("Shift + wheel", 8.0, 30.0, &note);
                  });
              }
            });
        }
      }
    });
    if clicked.is_some() {
      self.selected = clicked;
    }
  }

  /// Absolute drawing under two clips: only their intersection shows
  fn nested_clips(&self, ui: &mut UiContext, x: f32, y: f32) {
    ui.draw_text("Nested clips:", x, y - 20.0, Color::WHITE);
    let outer_rect = Rect::new(x, y, 200.0, 150.0);
    ui.draw_panel(
      outer_rect,
      Color::rgb(35, 35, 45),
      Some(Color::rgb(60, 60, 80)),
    );
    ui.push_clip(outer_rect.shrink(10.0));
    ui.draw_text(
      &format!("Outer clip depth: {}", ui.clip_depth()),
      x + 10.0,
      y + 10.0,
      Color::rgb(150, 150, 180),
    );
    let inner_rect = Rect::new(x + 50.0, y + 30.0, 200.0, 150.0);
    ui.draw_panel(
      inner_rect,
      Color::rgb(45, 35, 55),
      Some(Color::rgb(80, 60, 100)),
    );
    ui.push_clip(inner_rect.shrink(10.0));
    ui.draw_text(
      &format!("Inner clip depth: {}", ui.clip_depth()),
      inner_rect.x + 10.0,
      inner_rect.y + 10.0,
      Color::rgb(180, 150, 200),
    );
    for (i, line) in [
      "This text is clipped",
      "to the intersection",
      "of both clip rects",
    ]
    .iter()
    .enumerate()
    {
      ui.draw_text(
        line,
        inner_rect.x + 10.0,
        inner_rect.y + 30.0 + i as f32 * 18.0,
        Color::WHITE,
      );
    }
    ui.draw_rect(
      Rect::new(inner_rect.x + 50.0, inner_rect.y + 90.0, 200.0, 100.0),
      Color::new(100, 150, 255, 100),
    );
    ui.pop_clip();
    ui.pop_clip();
    let dim = Color::rgb(110, 110, 130);
    ui.draw_text("The blue box reaches past both clips:", x, y + 170.0, dim);
    ui.draw_text("only their intersection shows.", x, y + 188.0, dim);
  }

  /// The open dialog; its buttons or Back answer it
  fn dialog(&mut self, ui: &mut UiContext) {
    let Some(dialog) = self.dialog else {
      return;
    };
    let heading = ui.theme().text.heading.style();
    let mut answer = None;
    match dialog {
      Dialog::Modal => {
        ui.modal().width(320.0).gap(12.0).show(|ui| {
          ui.text("Confirm Action").style(&heading);
          ui.text("Are you sure you want to proceed?");
          ui.row().gap(8.0).justify(Justify::End).show(|ui| {
            if ui.button("Confirm").variant("accent").clicked() {
              answer = Some(true);
            }
            if ui.button("Cancel").default_focus().clicked() {
              answer = Some(false);
            }
          });
        });
        if answer.is_none() && ui.back() {
          answer = Some(false);
        }
      }
      Dialog::Ask => {
        answer = ui.confirm("Delete the save?", "This cannot be undone.");
      }
    }
    if let Some(yes) = answer {
      let name = match dialog {
        Dialog::Modal => "Modal",
        Dialog::Ask => "Ask",
      };
      self.result = Some((name, yes));
      self.dialog = None;
    }
  }
}

impl Default for ClippingDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for ClippingDemo {
  fn name(&self) -> &str {
    "Clipping & Modals"
  }

  fn description(&self) -> &str {
    "Scroll areas clip their contents, for hits as well as drawing; the wheel scrolls the innermost one it can.\n\
     Absolute drawing clips with push_clip. The modal and Ask block the page until they are answered; Back cancels them."
  }

  fn controls(&self) -> Vec<Control> {
    let mut controls = vec![
      Control::mouse("Wheel or drag the bar to scroll"),
      Control::mouse("Shift + wheel over the cards"),
    ];
    if self.dialog.is_some() {
      controls.push(Control::key(KeyCode::Escape, "Cancel the dialog"));
    }
    controls
  }

  /// Escape cancels the open dialog instead of quitting
  fn wants_escape(&self) -> bool {
    self.dialog.is_some()
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let top = area.y + 24.0;
    let list = Rect::new(30.0, top, 320.0, 300.0);
    let mut open = None;
    // One focus scope over the list and the buttons, so the arrows walk them
    ui.stack().focus_scope().at(area).show(|ui| {
      self.list(ui, list);
      ui.column()
        .at(Rect::new(420.0, top + 20.0, 220.0, 80.0))
        .gap(8.0)
        .show(|ui| {
          if ui
            .button("Show Modal Dialog")
            .fill_width()
            .default_focus()
            .clicked()
          {
            open = Some(Dialog::Modal);
          }
          if ui.button("Ask").fill_width().clicked() {
            open = Some(Dialog::Ask);
          }
        });
    });
    if open.is_some() {
      self.dialog = open;
    }

    ui.draw_text(
      "A scroll area, with one inside it:",
      list.x,
      list.y - 20.0,
      Color::WHITE,
    );
    ui.draw_text("Modals:", 420.0, top - 20.0, Color::WHITE);
    if let Some((name, yes)) = self.result {
      let (text, color) = if yes {
        (format!("{name}: confirmed"), Color::rgb(80, 200, 80))
      } else {
        (format!("{name}: cancelled"), Color::rgb(200, 80, 80))
      };
      ui.draw_text(&text, 420.0, top + 120.0, color);
    }
    self.nested_clips(ui, 30.0, list.bottom() + 50.0);
    self.dialog(ui);
  }
}
