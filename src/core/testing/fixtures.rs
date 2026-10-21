//! UI trees shared by the engine's tests and benchmarks

use crate::core::ui::{Align, Anchor, UiNode};

const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
const OPTIONS: [&str; 3] = ["Low", "Medium", "High"];
/// Settings rows: each a row, a label, a control and a value text
const ROWS: usize = 50;

/// The 200-widget settings screen as a node tree: a panel 600 by 460,
/// centred, a focus scope that wraps, holding tabs and a scroll area of 50
/// rows ("Setting {i}"), each a label, a slider (at 0.5), checkbox (on),
/// dropdown (on "Medium") or toggle (on), cycling, and a value text. The
/// scroll area fills the rest of the panel (407 units), so its rows (about
/// 1,406 units) scroll
#[doc(hidden)]
pub fn settings_tree() -> UiNode {
  let rows = (0..ROWS).map(|i| {
    let (widget, value) = match i % 4 {
      0 => (
        UiNode::slider(0.0..=1.0, 0.5).width(200.0),
        "50%".to_string(),
      ),
      1 => (UiNode::checkbox("Enabled", true), "On".to_string()),
      2 => (
        UiNode::dropdown(&OPTIONS, 1).width(200.0),
        OPTIONS[1].to_string(),
      ),
      _ => (UiNode::toggle("Enabled", true), "On".to_string()),
    };
    UiNode::row()
      .gap(12.0)
      .align(Align::Center)
      .child(UiNode::text(format!("Setting {i}")))
      .child(widget)
      .child(UiNode::text(value))
  });
  UiNode::panel()
    .focus_scope()
    .wrap_focus()
    .anchor(Anchor::Center)
    .width(600.0)
    .height(460.0)
    .gap(8.0)
    .child(UiNode::tabs(&TABS, 0))
    .child(
      UiNode::scroll()
        .fill_width()
        .fill_height()
        .gap(6.0)
        .children(rows),
    )
}
