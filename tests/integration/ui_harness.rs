use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

fn menu(ui: &mut UiContext<'_>, picked: &mut Option<usize>) {
  ui.column().focus_scope().show(|ui| {
    for (i, label) in ["Play", "Options", "Quit"].into_iter().enumerate() {
      let button = ui.button(label).id(i);
      let button = if i == 0 {
        button.default_focus()
      } else {
        button
      };
      if button.clicked() {
        *picked = Some(i);
      }
    }
  });
}

#[test]
fn a_menu_is_driven_without_a_gpu() {
  let mut ui = UiHarness::new(320.0, 240.0);
  let mut picked = None;
  ui.frame(|u| menu(u, &mut picked));
  ui.frame(|u| menu(u, &mut picked));
  assert!(ui.focused().is_some(), "the menu took focus");
  ui.action("ui.down");
  ui.frame(|u| menu(u, &mut picked));
  ui.action("ui.confirm");
  ui.frame(|u| menu(u, &mut picked));
  assert_eq!(picked, Some(1));
  assert!(ui.command_count() > 0);
}

#[test]
fn an_unknown_action_does_nothing() {
  let mut ui = UiHarness::new(320.0, 240.0);
  let mut picked = None;
  ui.frame(|u| menu(u, &mut picked));
  ui.action("jump");
  ui.frame(|u| menu(u, &mut picked));
  assert_eq!(picked, None);
}

/// Rows before and after the strip of cards in `strip_list`
const BEFORE: usize = 8;
const AFTER: usize = 8;

/// What `strip_list` drew: the rows and the cards (ids and rects), the
/// list's rect and the strip's
struct Strip {
  rows: Vec<UiId>,
  cards: Vec<(UiId, Rect)>,
  list: Rect,
  strip: Rect,
}

/// A 160-unit list of 16 rows with a strip of three cards, wider than the
/// list, after row 7 (the ui demo's Clipping page)
fn strip_list(ui: &mut UiContext<'_>) -> Strip {
  let (mut rows, mut cards, mut strip) = (Vec::new(), Vec::new(), Rect::default());
  let mut list = Rect::default();
  ui.column().focus_scope().show(|ui| {
    list = ui
      .scroll()
      .width(300.0)
      .height(160.0)
      .gap(4.0)
      .show(|ui| {
        for i in 0..BEFORE + AFTER {
          rows.push(ui.button(format!("Row {i}")).id(i).fill_width().show().id);
          if i + 1 == BEFORE {
            strip = ui
              .scroll()
              .horizontal()
              .fill_width()
              .height(50.0)
              .gap(8.0)
              .show(|ui| {
                for c in 0..3 {
                  let card = ui.custom(110.0, 40.0).id(c).focusable(true).show();
                  cards.push((card.id, card.rect));
                }
              })
              .rect;
          }
        }
      })
      .rect;
  });
  Strip {
    rows,
    cards,
    list,
    strip,
  }
}

/// One navigation step, then frames enough for the scroll to settle; what
/// was drawn last
fn step_and_settle(ui: &mut UiHarness, action: &str) -> Strip {
  ui.action(action);
  let mut drawn = ui.frame(strip_list);
  for _ in 0..30 {
    drawn = ui.frame(strip_list);
  }
  drawn
}

/// Whether `inner` lies inside `outer` (to a hair, for rounding)
fn within(inner: Rect, outer: Rect) -> bool {
  inner.x >= outer.x - 0.5
    && inner.y >= outer.y - 0.5
    && inner.right() <= outer.right() + 0.5
    && inner.bottom() <= outer.bottom() + 0.5
}

/// Which card has the focus, if one does
fn card_at(drawn: &Strip, focused: Option<UiId>) -> Option<usize> {
  drawn
    .cards
    .iter()
    .position(|(card, _)| Some(*card) == focused)
}

#[test]
fn the_pad_reaches_a_strip_of_cards_inside_a_list_both_ways() {
  let mut ui = UiHarness::new(400.0, 300.0);
  let mut drawn = ui.frame(strip_list);
  for _ in 0..2 {
    drawn = ui.frame(strip_list);
  }
  assert_eq!(ui.focused(), Some(drawn.rows[0]), "the first row to start");
  // Down from row to row until a card has the focus
  let mut reached = None;
  for _ in 0..BEFORE + 2 {
    drawn = step_and_settle(&mut ui, "ui.down");
    if let Some(k) = card_at(&drawn, ui.focused()) {
      reached = Some(k);
      break;
    }
  }
  let Some(k) = reached else {
    panic!("Down from row to row never reaches the strip of cards");
  };
  // Every area around it scrolled it into view: the list down to the strip
  let card = drawn.cards[k].1;
  assert!(
    within(card, drawn.list),
    "the card {card:?} shows in the list {:?}",
    drawn.list
  );
  // Right to the last card, past the strip's edge: the strip scrolls too
  while card_at(&drawn, ui.focused()) != Some(2) {
    drawn = step_and_settle(&mut ui, "ui.right");
    assert!(
      card_at(&drawn, ui.focused()).is_some(),
      "Right stays in the strip"
    );
  }
  let last = drawn.cards[2].1;
  assert!(
    within(last, drawn.strip) && within(last, drawn.list),
    "the last card {last:?} shows in the strip {:?} and the list {:?}",
    drawn.strip,
    drawn.list
  );
  // Down to the row after the strip, and Up from there reaches a card again
  drawn = step_and_settle(&mut ui, "ui.down");
  assert_eq!(ui.focused(), Some(drawn.rows[BEFORE]), "the row after it");
  drawn = step_and_settle(&mut ui, "ui.up");
  assert!(
    card_at(&drawn, ui.focused()).is_some(),
    "Up from below reaches the strip"
  );
}
