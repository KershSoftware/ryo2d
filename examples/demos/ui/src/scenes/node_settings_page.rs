//! The Settings page built from node trees: each tab's rows are a column,
//! hidden unless its tab is picked

use demo_kit::prelude::*;

use super::settings_data::{
  ACTIONS, OUTPUTS, RESOLUTIONS, TABS, WIDGET_WIDTH, WINDOWS, bind_actions,
};

pub struct NodeSettingsPage {
  /// Each tab's column, by tab
  tabs: Vec<Entity>,
  reset_controls: bool,
}

/// A labelled row: the label takes the room the widget leaves
fn row(label: &str, widget: UiNode) -> UiNode {
  UiNode::row()
    .gap(12.0)
    .align(Align::Center)
    .child(UiNode::column().grow(1.0).child(UiNode::text(label)))
    .child(widget)
}

fn slider(label: &str, value: f32, range: std::ops::RangeInclusive<f32>) -> UiNode {
  row(
    label,
    UiNode::slider(range, value)
      .step(0.05)
      .show_value()
      .width(WIDGET_WIDTH),
  )
}

fn dropdown(label: &str, options: &[&str], selected: usize) -> UiNode {
  row(
    label,
    UiNode::dropdown(options, selected).width(WIDGET_WIDTH),
  )
}

fn tab(index: usize, rows: Vec<UiNode>) -> UiNode {
  let column = UiNode::column()
    .fill_width()
    .gap(10.0)
    .name(format!("tab-{index}"))
    .children(rows);
  if index == 0 { column } else { column.hidden() }
}

impl NodeSettingsPage {
  pub fn new() -> Self {
    Self {
      tabs: Vec::new(),
      reset_controls: false,
    }
  }

  fn tree() -> UiNode {
    let audio = tab(
      0,
      vec![
        slider("Master", 0.8, 0.0..=1.0),
        slider("Music", 0.6, 0.0..=1.0),
        slider("Effects", 0.7, 0.0..=1.0),
        slider("Voice", 1.0, 0.0..=1.0),
        UiNode::checkbox("Mute in background", true),
        dropdown("Output", &OUTPUTS, 0),
      ],
    );
    let video = tab(
      1,
      vec![
        dropdown("Window", &WINDOWS, 0),
        dropdown("Resolution", &RESOLUTIONS, 13),
        UiNode::toggle("VSync", true),
        row(
          "UI scale",
          UiNode::radio_group(&["Small", "Medium", "Large"], 1)
            .horizontal()
            .gap(16.0),
        ),
        slider("Brightness", 1.0, 0.5..=1.5),
        UiNode::checkbox("Show FPS", false),
      ],
    );
    let mut controls: Vec<UiNode> = ACTIONS
      .iter()
      .map(|(action, label)| UiNode::rebind(action).label(*label))
      .collect();
    controls.push(
      UiNode::row()
        .justify(Justify::End)
        .child(UiNode::button("Reset controls").action("reset")),
    );
    let controls = tab(2, controls);
    UiNode::panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(600.0)
      .height(460.0)
      .gap(12.0)
      .child(UiNode::text("Settings").heading())
      .child(UiNode::tabs(&TABS, 0).name("tabs"))
      .child(
        UiNode::scroll()
          .fill_width()
          .grow(1.0)
          .gap(10.0)
          .child(audio)
          .child(video)
          .child(controls),
      )
      .child(
        UiNode::row().child(
          UiNode::text(
            "[prompt=ui.tab_prev] [prompt=ui.tab_next] Switch tab    [prompt=ui.back] Back",
          )
          .markup(),
        ),
      )
  }
}

impl Default for NodeSettingsPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for NodeSettingsPage {
  fn name(&self) -> &str {
    "Node Settings"
  }

  fn description(&self) -> &str {
    "The Settings page built from node trees: Q and E (the shoulder buttons) switch tabs, the D-pad moves and changes values.\n\
     Each tab's rows are a column the page shows or hides when the tabs change."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("Q / E", &[KeyCode::KeyQ, KeyCode::KeyE], "Switch tab"),
      Control::keys(
        "Enter / Space",
        &[KeyCode::Enter, KeyCode::Space],
        "Open a list or rebind",
      ),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    bind_actions(ctx);
    let root = ctx.spawn_ui(Self::tree());
    self.tabs = (0..TABS.len())
      .filter_map(|i| ctx.find_ui_in(root, &format!("tab-{i}")))
      .collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if std::mem::take(&mut self.reset_controls) {
      ctx.actions().reset_defaults();
    }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) {
    for event in ctx.ui_events() {
      match (event.kind, event.name.as_deref(), event.action.as_deref()) {
        (UiEventKind::Changed, Some("tabs"), _) => {
          let picked = ctx.world.get::<UiIndex>(event.node).map_or(0, |i| i.0);
          for (i, &column) in self.tabs.iter().enumerate() {
            if i == picked {
              ctx.world.entity_mut(column).remove::<UiHidden>();
            } else {
              ctx.world.entity_mut(column).insert(UiHidden);
            }
          }
        }
        (UiEventKind::Clicked, _, Some("reset")) => self.reset_controls = true,
        _ => {}
      }
    }
  }
}
