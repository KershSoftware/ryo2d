//! Focus across dialogs and scene changes, driven by the pad alone: where a
//! player expects it after a dialog closes, the top scene changes, or both

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The ids of a menu's two buttons, as last drawn
#[derive(Default)]
struct Ids {
  a: Option<UiId>,
  b: Option<UiId>,
}

/// A menu of A (the default) and B; Back pushes a confirm dialog scene, or a
/// plain scene with a scope of its own
struct Menu {
  ids: Arc<Mutex<Ids>>,
  with_modal: bool,
}

impl Scene for Menu {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let (mut a, mut b) = (None, None);
    ui.column().focus_scope().anchor(Anchor::Center).show(|ui| {
      a = Some(ui.button("A").default_focus().show().id);
      b = Some(ui.button("B").show().id);
    });
    if let Ok(mut ids) = self.ids.lock() {
      ids.a = a;
      ids.b = b;
    }
    if ui.back() {
      return if self.with_modal {
        SceneAction::Push(Box::new(Confirm))
      } else {
        SceneAction::Push(Box::new(Plain))
      };
    }
    SceneAction::Continue
  }
}

/// A transparent scene showing only a confirm dialog: No (or Back) pops it
struct Confirm;

impl Scene for Confirm {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    match ui.confirm_with("Quit?", "", "YES", "NO") {
      Some(false) => SceneAction::Pop,
      _ => SceneAction::Continue,
    }
  }

  fn is_transparent(&self) -> bool {
    true
  }
}

/// A transparent scene with a scope of its own: Back pops it
struct Plain;

impl Scene for Plain {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.column().focus_scope().show(|ui| {
      ui.button("Close").show();
    });
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true
  }
}

fn tap(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  engine.step_frames(3).unwrap();
}

/// Down to B, Back to push the other scene, Back there to pop it: the focus
/// before, while it shows, and after it popped, with B's id
fn push_and_pop(with_modal: bool) -> Option<[Option<UiId>; 4]> {
  let ids = Arc::new(Mutex::new(Ids::default()));
  let menu = Menu {
    ids: Arc::clone(&ids),
    with_modal,
  };
  let mut engine =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(800, 600).start_scene(menu))?;
  engine.step_frames(3).unwrap();
  tap(&mut engine, GamepadButton::DPadDown);
  let before = engine.ui_focused();
  tap(&mut engine, GamepadButton::East);
  let over = engine.ui_focused();
  tap(&mut engine, GamepadButton::East);
  let after = engine.ui_focused();
  let b = ids.lock().unwrap().b;
  Some([before, over, after, b])
}

#[test]
fn focus_comes_back_to_the_menu_when_a_pushed_scene_pops() {
  let Some([before, over, after, b]) = push_and_pop(false) else {
    return;
  };
  assert_eq!(before, b);
  assert!(
    over.is_some() && over != b,
    "the pushed scene has the focus"
  );
  assert_eq!(after, b, "the menu is back on B");
}

#[test]
fn focus_comes_back_to_the_menu_when_a_pushed_confirm_answers_no() {
  let Some([before, over, after, b]) = push_and_pop(true) else {
    return;
  };
  assert_eq!(before, b);
  assert!(over.is_some() && over != b, "the dialog has the focus");
  assert_eq!(
    after, b,
    "the menu is back on B, where the dialog came from"
  );
}

/// One scene: page one with a dialog over it, opened by its Open button;
/// Yes shows page two, or switches to a menu
struct Pages {
  page: u8,
  dialog: bool,
  two: Arc<Mutex<Option<UiId>>>,
  switch_to_menu: bool,
}

impl Scene for Pages {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if self.page == 1 {
      ui.column().focus_scope().show(|ui| {
        if ui.button("Open").default_focus().clicked() {
          self.dialog = true;
        }
      });
    } else {
      ui.column().focus_scope().show(|ui| {
        let id = ui.button("Two").default_focus().show().id;
        if let Ok(mut two) = self.two.lock() {
          *two = Some(id);
        }
      });
    }
    if self.dialog {
      match ui.confirm_with("Go?", "", "YES", "NO") {
        Some(true) => {
          self.dialog = false;
          if self.switch_to_menu {
            return SceneAction::Switch(Box::new(Menu {
              ids: Arc::new(Mutex::new(Ids::default())),
              with_modal: false,
            }));
          }
          self.page = 2;
        }
        Some(false) => self.dialog = false,
        None => {}
      }
    }
    SceneAction::Continue
  }
}

/// A opens the dialog (on NO), Left reaches YES, A answers it: the focus
/// then, what the D-pad can reach, and page two's button
fn answer_yes(switch_to_menu: bool) -> Option<(Option<UiId>, Vec<UiId>, Option<UiId>)> {
  let two = Arc::new(Mutex::new(None));
  let pages = Pages {
    page: 1,
    dialog: false,
    two: Arc::clone(&two),
    switch_to_menu,
  };
  let mut engine =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(800, 600).start_scene(pages))?;
  engine.step_frames(3).unwrap();
  tap(&mut engine, GamepadButton::South);
  tap(&mut engine, GamepadButton::DPadLeft);
  tap(&mut engine, GamepadButton::South);
  engine.step_frames(1).unwrap();
  let focused = engine.ui_focused();
  let targets = engine.ui_focusables().into_iter().map(|(id, _)| id);
  let two = *two.lock().unwrap();
  Some((focused, targets.collect(), two))
}

#[test]
fn a_page_a_dialog_s_yes_shows_takes_its_default() {
  let Some((focused, _, two)) = answer_yes(false) else {
    return;
  };
  assert!(two.is_some(), "page two is shown");
  assert_eq!(focused, two, "page two's default has the focus");
}

#[test]
fn a_scene_a_dialog_s_yes_switches_to_takes_its_default() {
  let Some((focused, targets, _)) = answer_yes(true) else {
    return;
  };
  assert_eq!(targets.len(), 2, "the menu's A and B");
  assert_eq!(focused, targets.first().copied(), "the menu's default, A");
}
