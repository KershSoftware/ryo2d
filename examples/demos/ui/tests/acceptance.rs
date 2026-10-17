//! The acceptance pages, driven by a gamepad alone

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use demo_kit::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};
use ui_demo::scenes::{InventoryPage, Item, ItemKind, NodeInventoryPage, SlotRef};

const DPAD: [GamepadButton; 4] = [
  GamepadButton::DPadUp,
  GamepadButton::DPadDown,
  GamepadButton::DPadLeft,
  GamepadButton::DPadRight,
];

const MENUS: [usize; 2] = [0, 11];
const SETTINGS: [usize; 2] = [1, 12];
const INVENTORY: [usize; 2] = [2, 13];
const DIALOGUE: [usize; 2] = [3, 14];

fn start(page: usize) -> Option<HeadlessEngine> {
  let mut engine = HeadlessEngine::new_or_skip(ui_demo::demo().start_page(page).engine_builder())?;
  engine.step_frames(4).unwrap();
  Some(engine)
}

fn pad(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  engine.step_frames(3).unwrap();
}

/// What the D-pad can land on now
fn ids(engine: &HeadlessEngine) -> BTreeSet<UiId> {
  engine
    .ui_focusables()
    .into_iter()
    .map(|(id, _)| id)
    .collect()
}

/// The focusables reached from the current focus by D-pad presses alone
fn reachable(engine: &mut HeadlessEngine) -> BTreeSet<UiId> {
  let Some(start) = engine.ui_focused() else {
    panic!("the page focused nothing by itself");
  };
  let mut seen = BTreeSet::from([start]);
  let mut queue = vec![start];
  while let Some(from) = queue.pop() {
    for button in DPAD {
      engine.focus_ui(from);
      engine.step_frames(2).unwrap();
      pad(engine, button);
      if let Some(to) = engine.ui_focused()
        && seen.insert(to)
      {
        queue.push(to);
      }
    }
  }
  seen
}

/// Every target the D-pad had before the walk is reached by it
fn assert_all_reachable(engine: &mut HeadlessEngine, what: &str) {
  let all = engine.ui_focusables();
  assert!(!all.is_empty(), "{what}: nothing to focus");
  let reached = reachable(engine);
  let missed: Vec<Rect> = all
    .into_iter()
    .filter(|(id, _)| !reached.contains(id))
    .map(|(_, r)| r)
    .collect();
  assert!(
    missed.is_empty(),
    "{what}: the D-pad never reaches the widgets at {missed:?}"
  );
}

#[test]
fn every_acceptance_page_is_fully_reachable_by_pad() {
  // Menus, Inventory and Dialogue as they open
  let pages = MENUS
    .iter()
    .map(|&p| (p, "Menus"))
    .chain(INVENTORY.iter().map(|&p| (p, "Inventory")))
    .chain(DIALOGUE.iter().map(|&p| (p, "Dialogue")));
  for (page, name) in pages {
    let Some(mut engine) = start(page) else {
      return;
    };
    assert_all_reachable(&mut engine, &format!("{name} (page {page})"));
    if name == "Dialogue" {
      // Skip shows the whole line, then its choices
      pad(&mut engine, GamepadButton::South);
      assert!(
        engine.ui_focusables().len() > 1,
        "Skip showed the choices (page {page})"
      );
      assert_all_reachable(&mut engine, &format!("Dialogue, choices (page {page})"));
    }
  }
  // Settings, on each tab in turn
  for page in SETTINGS {
    let Some(mut engine) = start(page) else {
      return;
    };
    for tab in ["Audio", "Video", "Controls"] {
      assert_all_reachable(&mut engine, &format!("Settings, {tab} (page {page})"));
      if tab == "Video" {
        // The Window dropdown's list: all of it reachable, and Back closes it
        let list = ids(&engine);
        let window = engine.ui_focusables()[3].0;
        engine.focus_ui(window);
        engine.step_frames(2).unwrap();
        pad(&mut engine, GamepadButton::South);
        assert!(
          ids(&engine).is_disjoint(&list),
          "the Window list opened over the page (page {page})"
        );
        assert_all_reachable(
          &mut engine,
          &format!("Settings, Video, the Window list (page {page})"),
        );
        pad(&mut engine, GamepadButton::East);
        assert_eq!(ids(&engine), list, "Back closed the list (page {page})");
      }
      pad(&mut engine, GamepadButton::RightBumper);
    }
  }
}

#[test]
fn every_menu_dialog_is_reachable_and_closes_on_back() {
  for page in MENUS {
    let Some(mut engine) = start(page) else {
      return;
    };
    let menu = ids(&engine);
    let Some(new_game) = engine.ui_focused() else {
      panic!("the menu focused nothing (page {page})");
    };
    // Continue starts disabled, so New Game is first: Options is one down, Credits two, Quit three
    for (downs, name) in [(1, "Options"), (2, "Credits"), (3, "Quit")] {
      engine.focus_ui(new_game);
      engine.step_frames(2).unwrap();
      for _ in 0..downs {
        pad(&mut engine, GamepadButton::DPadDown);
      }
      pad(&mut engine, GamepadButton::South);
      assert!(
        ids(&engine).is_disjoint(&menu),
        "{name} opened a modal over the menu (page {page})"
      );
      assert_all_reachable(&mut engine, &format!("{name} (page {page})"));
      pad(&mut engine, GamepadButton::East);
      assert_eq!(ids(&engine), menu, "{name} closed on Back (page {page})");
    }
  }
}

#[test]
fn the_quit_confirm_gives_the_focus_back_to_quit_by_pad_alone() {
  for page in MENUS {
    let Some(mut engine) = start(page) else {
      return;
    };
    let menu = ids(&engine);
    // A focus at every step, and never one from outside the walk
    let step = |engine: &mut HeadlessEngine, button: GamepadButton, what: &str| {
      pad(engine, button);
      match engine.ui_focused() {
        Some(id) => id,
        None => panic!("nothing has the focus after {what} (page {page})"),
      }
    };
    let Some(mut at) = engine.ui_focused() else {
      panic!("the menu focused nothing by itself (page {page})");
    };
    // New Game first (Continue is disabled); Options, Credits, then Quit
    for what in ["Down to Options", "Down to Credits", "Down to Quit"] {
      let next = step(&mut engine, GamepadButton::DPadDown, what);
      assert_ne!(next, at, "{what} moved the focus (page {page})");
      at = next;
    }
    let quit = at;
    for (answer, route) in [
      ("Back", &[GamepadButton::East][..]),
      ("No", &[GamepadButton::South][..]),
      ("Yes", &[GamepadButton::DPadLeft, GamepadButton::South][..]),
    ] {
      let open = step(&mut engine, GamepadButton::South, "A on Quit");
      assert!(
        !menu.contains(&open),
        "the confirm has the focus (page {page})"
      );
      for &button in route {
        step(&mut engine, button, answer);
      }
      assert_eq!(
        engine.ui_focused(),
        Some(quit),
        "{answer} gave it back to Quit (page {page})"
      );
      assert_eq!(
        ids(&engine),
        menu,
        "{answer} closed the confirm (page {page})"
      );
    }
  }
}

#[test]
fn the_clipping_pages_strip_of_cards_is_reached_from_its_rows() {
  // Clipping & Modals: 40 rows of 26 with a strip of 64-tall cards in them
  let Some(mut engine) = start(6) else {
    return;
  };
  let height = |engine: &HeadlessEngine| {
    let focused = engine.ui_focused();
    engine
      .ui_focusables()
      .into_iter()
      .find(|(id, _)| Some(*id) == focused)
      .map(|(_, rect)| rect.height)
  };
  let is_card = |engine: &HeadlessEngine| height(engine).is_some_and(|h| h > 40.0);
  // A press, then time for the list to scroll there
  let settle = |engine: &mut HeadlessEngine, button: GamepadButton| {
    pad(engine, button);
    engine.step_frames(20).unwrap();
  };
  // From the Show Modal Dialog button, Left into the list
  settle(&mut engine, GamepadButton::DPadLeft);
  assert_eq!(height(&engine), Some(26.0), "a row has the focus");
  let mut downs = 0;
  while !is_card(&engine) {
    downs += 1;
    assert!(downs <= 12, "Down from row to row never reaches a card");
    settle(&mut engine, GamepadButton::DPadDown);
  }
  // On to the last row, then back up from row to row
  let mut last = engine.ui_focused();
  loop {
    settle(&mut engine, GamepadButton::DPadDown);
    if engine.ui_focused() == last {
      break;
    }
    last = engine.ui_focused();
  }
  assert_eq!(height(&engine), Some(26.0), "the last row");
  let mut ups = 0;
  while !is_card(&engine) {
    ups += 1;
    assert!(ups <= 40, "Up from row to row never reaches a card");
    settle(&mut engine, GamepadButton::DPadUp);
  }
}

#[test]
fn the_pad_moves_an_item_into_its_equipment_slot() {
  for page in INVENTORY {
    let Some(mut engine) = start(page) else {
      return;
    };
    let rect_of = |engine: &HeadlessEngine, id: UiId| {
      engine
        .ui_focusables()
        .into_iter()
        .find(|(t, _)| *t == id)
        .map(|(_, r)| r)
    };
    let route = [
      GamepadButton::DPadLeft,
      GamepadButton::DPadDown,
      GamepadButton::DPadDown,
    ];
    // The Sword's bag slot has the first focus; Left reaches Head, then Body and Weapon below it
    let Some(sword) = engine.ui_focused() else {
      panic!("nothing focused (page {page})");
    };
    for button in route {
      pad(&mut engine, button);
    }
    let Some(weapon) = engine.ui_focused() else {
      panic!("focus left the page (page {page})");
    };
    let (Some(sword_rect), Some(weapon_rect)) = (rect_of(&engine, sword), rect_of(&engine, weapon))
    else {
      panic!("the slots are not focusable (page {page})");
    };
    let before = engine.frame().unwrap();
    engine.focus_ui(sword);
    engine.step_frames(2).unwrap();
    pad(&mut engine, GamepadButton::South); // pick up the Sword
    for button in route {
      pad(&mut engine, button);
    }
    pad(&mut engine, GamepadButton::South); // put it down on Weapon
    let after = engine.frame().unwrap();
    let differs = |r: Rect| {
      (r.x as u32..r.right() as u32).any(|x| {
        (r.y as u32..r.bottom() as u32).any(|y| before.get_pixel(x, y) != after.get_pixel(x, y))
      })
    };
    assert!(
      differs(weapon_rect),
      "the Weapon slot shows the Sword now (page {page})"
    );
    assert!(
      differs(sword_rect),
      "the Sword's bag slot is empty now (page {page})"
    );
  }
}

/// What the watched Inventory page showed on its last UI frame
#[derive(Debug, Clone, Default)]
struct Shown {
  status: String,
  first_slot: Option<Item>,
}

/// A page whose status and first bag slot the test reads
trait Watchable: DemoPage {
  fn status(&self) -> &str;
  fn first_slot(&self) -> Option<Item>;
}

impl Watchable for InventoryPage {
  fn status(&self) -> &str {
    InventoryPage::status(self)
  }
  fn first_slot(&self) -> Option<Item> {
    self.item(SlotRef::Bag(0))
  }
}

impl Watchable for NodeInventoryPage {
  fn status(&self) -> &str {
    NodeInventoryPage::status(self)
  }
  fn first_slot(&self) -> Option<Item> {
    self.item(SlotRef::Bag(0))
  }
}

/// An Inventory page, with what it shows copied out after each UI frame
struct WatchedInventory<P: Watchable> {
  page: P,
  shown: Arc<Mutex<Shown>>,
}

impl<P: Watchable> DemoPage for WatchedInventory<P> {
  fn name(&self) -> &str {
    self.page.name()
  }

  fn controls(&self) -> Vec<Control> {
    self.page.controls()
  }

  fn wants_escape(&self) -> bool {
    self.page.wants_escape()
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.page.enter(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.page.update(ctx, dt);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.page.ui(ui, ctx);
    if let Ok(mut shown) = self.shown.lock() {
      shown.status = self.page.status().to_string();
      shown.first_slot = self.page.first_slot();
    }
  }
}

/// Escape puts back an item the pad holds over a button, on the page `page` makes
fn escape_puts_back<P: Watchable>(page: impl Fn() -> P + 'static) {
  let shown = Arc::new(Mutex::new(Shown::default()));
  let watched = Arc::clone(&shown);
  let demo = Demo::new("Inventory")
    .resolution(1024, 768)
    .page(move || WatchedInventory {
      page: page(),
      shown: Arc::clone(&watched),
    });
  let Some(mut engine) = HeadlessEngine::new_or_skip(demo.engine_builder()) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let shown = || shown.lock().map(|s| s.clone()).unwrap_or_default();
  let sword = shown().first_slot;
  assert_eq!(sword.map(|item| item.kind), Some(ItemKind::Sword));

  pad(&mut engine, GamepadButton::South); // pick up the Sword
  assert!(shown().status.contains("Holding"), "{:?}", shown().status);
  // Down the bag's rows to the Sort button under them
  for _ in 0..4 {
    pad(&mut engine, GamepadButton::DPadDown);
  }
  let focused = engine.ui_focused();
  let on = engine
    .ui_focusables()
    .into_iter()
    .find(|(id, _)| Some(*id) == focused);
  assert!(
    on.is_some_and(|(_, rect)| rect.height < 40.0),
    "the focus is on Sort, a button, not a slot: {on:?}"
  );

  engine.tap_key(KeyCode::Escape);
  assert_eq!(
    engine.step_frames(3).unwrap(),
    StepOutcome::Running,
    "Escape put the item back instead of quitting the demo"
  );
  assert!(!shown().status.contains("Holding"), "{:?}", shown().status);
  assert_eq!(shown().first_slot, sword, "the Sword is still in its slot");
  // And nothing is held: A on Sort sorts
  pad(&mut engine, GamepadButton::South);
  assert_eq!(shown().status, "Sorted the backpack.");
}

#[test]
fn escape_puts_back_an_item_the_pad_holds_over_a_button() {
  escape_puts_back(InventoryPage::new);
  escape_puts_back(NodeInventoryPage::new);
}

#[test]
fn the_mixed_pages_focus_path_runs_through_the_node_grid() {
  let Some(mut engine) = start(15) else { return };
  let Some(add) = engine.ui_focused() else {
    panic!("nothing focused")
  };
  assert_all_reachable(&mut engine, "Mixed");
  engine.focus_ui(add);
  engine.step_frames(2).unwrap();
  let rect_of = |engine: &HeadlessEngine, id| {
    engine
      .ui_focusables()
      .into_iter()
      .find(|(t, _)| *t == id)
      .map(|(_, r)| r)
  };
  let mut steps = 0;
  let done_y = engine
    .ui_focusables()
    .last()
    .map(|(_, r)| r.y)
    .unwrap_or_default();
  while let Some(at) = engine.ui_focused().and_then(|id| rect_of(&engine, id)) {
    if at.y >= done_y || steps > 10 {
      break;
    }
    pad(&mut engine, GamepadButton::DPadDown);
    steps += 1;
  }
  assert_eq!(steps, 3, "Add, the grid's two rows, then Done");
  pad(&mut engine, GamepadButton::DPadUp);
  let back = engine.ui_focused().and_then(|id| rect_of(&engine, id));
  assert!(
    back.is_some_and(|r| r.y < done_y),
    "Up from Done goes back into the grid"
  );
}
