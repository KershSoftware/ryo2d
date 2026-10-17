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
