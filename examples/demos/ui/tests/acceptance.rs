//! The acceptance pages, driven by a gamepad alone

use std::collections::BTreeSet;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const DPAD: [GamepadButton; 4] = [
  GamepadButton::DPadUp,
  GamepadButton::DPadDown,
  GamepadButton::DPadLeft,
  GamepadButton::DPadRight,
];

fn start(page: usize) -> Option<HeadlessEngine> {
  let mut engine = HeadlessEngine::new_or_skip(ui_demo::demo().start_page(page).engine_builder())?;
  engine.step_frames(4).ok()?;
  Some(engine)
}

fn pad(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  engine.step_frames(3).unwrap();
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

fn assert_all_reachable(engine: &mut HeadlessEngine, what: &str) {
  let all: BTreeSet<UiId> = engine
    .ui_focusables()
    .into_iter()
    .map(|(id, _)| id)
    .collect();
  assert!(!all.is_empty(), "{what}: nothing to focus");
  let reached = reachable(engine);
  let missed: Vec<Rect> = engine
    .ui_focusables()
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
  for (page, name) in [(0, "Menus"), (2, "Inventory"), (3, "Dialogue")] {
    let Some(mut engine) = start(page) else {
      return;
    };
    assert_all_reachable(&mut engine, name);
  }
  // Settings, on each tab in turn
  let Some(mut engine) = start(1) else {
    return;
  };
  for tab in ["Audio", "Video", "Controls"] {
    assert_all_reachable(&mut engine, &format!("Settings, {tab}"));
    pad(&mut engine, GamepadButton::RightBumper);
  }
}

#[test]
fn every_menu_dialog_is_reachable_and_closes_on_back() {
  let Some(mut engine) = start(0) else {
    return;
  };
  let ids = |engine: &HeadlessEngine| -> BTreeSet<UiId> {
    engine
      .ui_focusables()
      .into_iter()
      .map(|(id, _)| id)
      .collect()
  };
  let page = ids(&engine);
  let Some(new_game) = engine.ui_focused() else {
    panic!("the menu focused nothing");
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
      ids(&engine).is_disjoint(&page),
      "{name} opened a modal over the menu"
    );
    assert_all_reachable(&mut engine, name);
    pad(&mut engine, GamepadButton::East);
    assert_eq!(ids(&engine), page, "{name} closed on Back");
  }
}

#[test]
fn the_pad_moves_an_item_into_its_equipment_slot() {
  let Some(mut engine) = start(2) else {
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
    panic!("nothing focused");
  };
  for button in route {
    pad(&mut engine, button);
  }
  let Some(weapon) = engine.ui_focused() else {
    panic!("focus left the page");
  };
  let (Some(sword_rect), Some(weapon_rect)) = (rect_of(&engine, sword), rect_of(&engine, weapon))
  else {
    panic!("the slots are not focusable");
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
  assert!(differs(weapon_rect), "the Weapon slot shows the Sword now");
  assert!(differs(sword_rect), "the Sword's bag slot is empty now");
}
