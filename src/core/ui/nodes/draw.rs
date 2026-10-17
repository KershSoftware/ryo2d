//! The walk: a scene's node trees drawn through the builders, keyed by entity
//!
//! Each node is drawn by the builder a closure would call, given `.id(entity)`
//! (text, separators and spacers are drawn inside `ui.scope(entity, ..)`), so
//! its id depends only on its entity and its parent's id. `draw_node`
//! dispatches on the node's kind. A container's builder is configured from
//! the node first; its `.show` then walks the children with the world free.

use glam::Vec2;

use crate::core::ecs::kernel::{
  Children, Entity, EntityWorldMut, Has, Mut, ReleaseStateQueryData, SingleEntityQueryData, World,
};
use crate::core::text::{RichText, TextReveal, TextStyle};
use crate::core::ui::theme::warn_once;
use crate::core::ui::{Anchor, Rect, Response, Theme, UiContext};

use super::components::{
  UiDisabled, UiHidden, UiImage, UiInteraction, UiLabel, UiName, UiNodes, UiRoot, UiTooltip,
};
use super::events::{UiEvent, UiEventKind};
use super::node::{Kind, NodeLayout, TextRole, UiNodeKind};

/// A root's placement, from its `UiRoot`
#[derive(Clone, Copy)]
struct Place {
  anchor: Anchor,
  offset: Vec2,
  at: Option<Rect>,
}

/// The anchor, offset and rect a node is drawn with: a root's placement in
/// place of its own options
fn placement(
  place: Option<Place>,
  layout: &NodeLayout,
) -> (Option<Anchor>, Option<Vec2>, Option<Rect>) {
  match place {
    Some(place) => (Some(place.anchor), Some(place.offset), place.at),
    None => (layout.anchor, layout.offset, layout.at),
  }
}

/// One walk over node trees: what it collects on the way
pub(crate) struct Walk {
  /// The scene the trees belong to
  pub(crate) scene: u64,
  pub(crate) events: Vec<UiEvent>,
  /// The last modal or popup node drawn: Back goes to it
  #[allow(dead_code)] // read once node dialogs answer Back
  pub(crate) back: Option<Entity>,
  /// A focus request waiting for its node to be drawn
  pub(crate) pending_focus: Option<Entity>,
}

impl Walk {
  /// Keep an unused focus request for a later frame; the walk's events
  pub(crate) fn finish(self, world: &mut World, _ui: &mut UiContext<'_>) -> Vec<UiEvent> {
    if let Some(node) = self.pending_focus
      && let Some(mut nodes) = world.get_resource_mut::<UiNodes>()
    {
      nodes.pending_focus = Some(node);
    }
    self.events
  }
}

/// Start a UI pass: embedded trees may be drawn again
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // only the harness calls it yet
pub(crate) fn begin_pass(world: &mut World) {
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
    nodes.embedded_drawn.clear();
  }
}

/// Draw `scene`'s node trees through the builders, in spawn order (a later
/// root over an earlier one), and return their events in walk order
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // only the harness calls it yet
pub(crate) fn draw_nodes(world: &mut World, ui: &mut UiContext<'_>, scene: u64) -> Vec<UiEvent> {
  let Some(pending_focus) = world.try_resource_scope(|world, mut nodes: Mut<UiNodes>| {
    nodes
      .roots
      .retain(|&(_, root)| world.get::<UiRoot>(root).is_some());
    nodes.pending_focus.take()
  }) else {
    return Vec::new();
  };
  let mut walk = Walk {
    scene,
    events: Vec::new(),
    back: None,
    pending_focus,
  };
  let mut i = 0;
  while let Some((owner, root)) = world
    .get_resource::<UiNodes>()
    .and_then(|nodes| nodes.roots.get(i).copied())
  {
    i += 1;
    let Some(placed) = world.get::<UiRoot>(root) else {
      continue;
    };
    if owner != scene || placed.embedded {
      continue;
    }
    let place = Place {
      anchor: placed.anchor,
      offset: placed.offset,
      at: placed.at,
    };
    draw_node(world, ui, root, Some(place), &mut walk);
  }
  walk.finish(world, ui)
}

/// Draw `node` and its subtree by its kind; `place` is a root's placement
fn draw_node(
  world: &mut World,
  ui: &mut UiContext<'_>,
  node: Entity,
  place: Option<Place>,
  walk: &mut Walk,
) {
  let Ok(entity) = world.get_entity_mut(node) else {
    return;
  };
  if entity.contains::<UiHidden>() {
    return;
  }
  let Some(kind) = entity.get::<UiNodeKind>() else {
    warn_once("ui-node:not-a-node".to_string(), || {
      "an entity under a UI node is not a node; it is skipped".to_string()
    });
    return;
  };
  let (response, sends) = match kind.kind {
    Kind::Column | Kind::Row | Kind::Grid(_) | Kind::Stack | Kind::Panel => {
      (draw_container(entity, ui, place, walk), Sends::Clicks)
    }
    Kind::Scroll(_) => (draw_scroll(entity, ui, place, walk), Sends::Clicks),
    Kind::Text(_) => {
      draw_text(entity, ui);
      return;
    }
    Kind::Button(_) => (draw_button(entity, ui, place), Sends::Clicks),
    Kind::Image(_) => (draw_image(entity, ui, place), Sends::Clicks),
    Kind::Separator => {
      ui.scope(node, |ui| ui.separator());
      return;
    }
    Kind::Spacer => {
      ui.scope(node, |ui| ui.spacer());
      return;
    }
    // Not drawn yet: value widgets, lists, slots, prompts, rebinds, popups
    _ => return,
  };
  if let Some(response) = response {
    answer(world, ui, node, &response, sends, walk);
  }
}

/// Which of `Clicked` and `Changed` a kind sends
#[derive(Clone, Copy)]
#[allow(dead_code)] // `Changes` is sent once value widgets are drawn
enum Sends {
  Clicks,
  Changes,
}

/// Turn a drawn node's `response` into its `UiInteraction`, a focus request
/// that was waiting for it, and the events it sends
fn answer(
  world: &mut World,
  ui: &mut UiContext<'_>,
  node: Entity,
  response: &Response,
  sends: Sends,
  walk: &mut Walk,
) {
  let now = UiInteraction {
    id: response.id,
    hovered: response.hovered(),
    focused: response.focused(),
  };
  match world.get_mut::<UiInteraction>(node) {
    Some(mut state) => {
      if *state != now {
        *state = now;
      }
    }
    None => {
      if let Ok(mut entity) = world.get_entity_mut(node) {
        entity.insert(now);
      }
    }
  }
  if walk.pending_focus == Some(node) {
    ui.request_focus(response.id);
    walk.pending_focus = None;
  }
  let sent = match sends {
    Sends::Clicks => response.clicked().then_some(UiEventKind::Clicked),
    Sends::Changes => response.changed().then_some(UiEventKind::Changed),
  };
  let kinds = [
    sent,
    response
      .secondary_clicked()
      .then_some(UiEventKind::SecondaryClicked),
    response.lost_focus().then_some(UiEventKind::FocusLost),
    response.gained_focus().then_some(UiEventKind::FocusGained),
  ];
  for kind in kinds.into_iter().flatten() {
    walk.events.push(UiEvent {
      node,
      name: world.get::<UiName>(node).map(|name| name.0.clone()),
      action: world
        .get::<UiNodeKind>(node)
        .and_then(|kind| kind.action.clone()),
      kind,
      scene: walk.scene,
    });
  }
}

/// Draw `parent`'s children in order
fn walk_children(world: &mut World, ui: &mut UiContext<'_>, parent: Entity, walk: &mut Walk) {
  // By index, so the list of children is never copied
  let mut i = 0;
  while let Some(child) = world
    .get::<Children>(parent)
    .and_then(|children| children.get(i).copied())
  {
    i += 1;
    draw_node(world, ui, child, None, walk);
  }
}

/// Warn once that `UiDisabled` on a container does nothing
fn warn_disabled(node: &EntityWorldMut<'_>, kind: &UiNodeKind) {
  if node.contains::<UiDisabled>() {
    let name = kind.name();
    warn_once(format!("ui-node:disabled:{name}"), || {
      format!("UiDisabled does nothing on a {name} node")
    });
  }
}

/// A node's gap: its own, else the theme's spacing for a theme gap
fn gap_of(layout: &NodeLayout, ui: &UiContext<'_>) -> Option<f32> {
  layout
    .gap
    .or_else(|| layout.theme_gap.then(|| ui.theme().metrics.spacing))
}

/// Apply the size, padding and margin options a container and a scroll area
/// share
macro_rules! sized {
  ($builder:ident, $layout:expr) => {{
    let layout: &NodeLayout = $layout;
    if let Some(width) = layout.width {
      $builder = $builder.width(width);
    }
    if let Some(height) = layout.height {
      $builder = $builder.height(height);
    }
    if let Some(width) = layout.min_width {
      $builder = $builder.min_width(width);
    }
    if let Some(width) = layout.max_width {
      $builder = $builder.max_width(width);
    }
    if let Some(height) = layout.min_height {
      $builder = $builder.min_height(height);
    }
    if let Some(height) = layout.max_height {
      $builder = $builder.max_height(height);
    }
    if let Some(padding) = layout.padding {
      $builder = $builder.padding(padding);
    }
    if let Some(margin) = layout.margin {
      $builder = $builder.margin(margin);
    }
  }};
}

/// Apply an anchor, offset and rect from `placement`
macro_rules! placed {
  ($builder:ident, $placement:expr) => {{
    let (anchor, offset, at): (Option<Anchor>, Option<Vec2>, Option<Rect>) = $placement;
    if let Some(anchor) = anchor {
      $builder = $builder.anchor(anchor);
    }
    if let Some(offset) = offset {
      $builder = $builder.offset(offset.x, offset.y);
    }
    if let Some(at) = at {
      $builder = $builder.at(at);
    }
  }};
}

/// Draw a column, row, grid, stack or panel node, its children inside
fn draw_container(
  node: EntityWorldMut<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
  walk: &mut Walk,
) -> Option<Response> {
  let entity = node.id();
  let kind = node.get::<UiNodeKind>()?;
  warn_disabled(&node, kind);
  let (layout, flags) = (&kind.layout, kind.flags);
  let gap = gap_of(layout, ui);
  let mut container = match kind.kind {
    Kind::Column => ui.column(),
    Kind::Row => ui.row(),
    Kind::Grid(columns) => ui.grid(columns),
    Kind::Stack => ui.stack(),
    Kind::Panel => ui.panel(),
    _ => return None,
  }
  .id(entity);
  sized!(container, layout);
  if let Some(gap) = gap {
    container = container.gap(gap);
  }
  if let Some(align) = layout.align {
    container = container.align(align);
  }
  if let Some(justify) = layout.justify {
    container = container.justify(justify);
  }
  if let Some(grow) = layout.grow {
    container = container.grow(grow);
  }
  if layout.wrap_items {
    container = container.wrap_items();
  }
  if let Some(ratio) = layout.aspect {
    container = container.aspect(ratio);
  }
  if let Some(tracks) = &layout.tracks {
    container = container.tracks(tracks);
  }
  placed!(container, placement(place, layout));
  if let Some(color) = layout.background {
    container = container.background(color);
  }
  if flags.focus_scope {
    container = container.focus_scope();
  }
  if flags.trap {
    container = container.trap();
  }
  if flags.wrap_focus {
    container = container.wrap_focus();
  }
  if flags.block_pointer {
    container = container.block_pointer();
  }
  let world = node.into_world_mut();
  Some(container.show(|ui| walk_children(world, ui, entity, walk)))
}

/// Draw a scroll node, its children inside: only the options `Scroll` has
fn draw_scroll(
  node: EntityWorldMut<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
  walk: &mut Walk,
) -> Option<Response> {
  let entity = node.id();
  let kind = node.get::<UiNodeKind>()?;
  let Kind::Scroll(opts) = &kind.kind else {
    return None;
  };
  warn_disabled(&node, kind);
  let layout = &kind.layout;
  let gap = gap_of(layout, ui);
  let mut scroll = ui.scroll().id(entity);
  if opts.horizontal {
    scroll = scroll.horizontal();
  }
  if opts.both {
    scroll = scroll.both();
  }
  if opts.no_bar {
    scroll = scroll.no_bar();
  }
  sized!(scroll, layout);
  if let Some(gap) = gap {
    scroll = scroll.gap(gap);
  }
  if let Some(grow) = layout.grow {
    scroll = scroll.grow(grow);
  }
  placed!(scroll, placement(place, layout));
  let world = node.into_world_mut();
  Some(scroll.show(|ui| walk_children(world, ui, entity, walk)))
}

/// What a widget's shared options come from besides its `UiNodeKind`
struct Extras<'w> {
  /// The node, its id key
  entity: Entity,
  /// A root's placement
  place: Option<Place>,
  tooltip: Option<&'w UiTooltip>,
  /// `UiDisabled`
  disabled: bool,
}

/// The options every widget kind shares, applied to `$builder` (the
/// widget's builder call) from `$node` (its `&UiNodeKind`) and `$extras`
/// (its `Extras`): `.id(entity)`, its layout (a root's placement from its
/// `UiRoot`), its focus options, its variant and tooltip, and
/// `enabled(false)` under `UiDisabled`. Evaluates to the builder.
macro_rules! widget_opts {
  ($builder:expr, $node:expr, $extras:expr) => {{
    let node: &UiNodeKind = $node;
    let extras: &Extras<'_> = &$extras;
    let layout = &node.layout;
    let (anchor, offset, at) = placement(extras.place, layout);
    let mut builder = $builder.id(extras.entity);
    if let Some(width) = layout.width {
      builder = builder.width(width);
    }
    if let Some(height) = layout.height {
      builder = builder.height(height);
    }
    if let Some(margin) = layout.margin {
      builder = builder.margin(margin);
    }
    if let Some(anchor) = anchor {
      builder = builder.anchor(anchor);
    }
    if let Some(offset) = offset {
      builder = builder.offset(offset.x, offset.y);
    }
    if let Some(grow) = layout.grow {
      builder = builder.grow(grow);
    }
    if let Some(width) = layout.min_width {
      builder = builder.min_width(width);
    }
    if let Some(width) = layout.max_width {
      builder = builder.max_width(width);
    }
    if let Some(at) = at {
      builder = builder.at(at);
    }
    if node.flags.default_focus {
      builder = builder.default_focus();
    }
    if node.flags.nav_skip {
      builder = builder.nav_skip();
    }
    if let Some(on) = node.flags.focusable {
      builder = builder.focusable(on);
    }
    if let Some(variant) = &node.variant {
      builder = builder.variant(variant);
    }
    if let Some(tooltip) = extras.tooltip {
      builder = builder.tooltip(tooltip.0.as_str());
    }
    if extras.disabled {
      builder = builder.enabled(false);
    }
    builder
  }};
}

/// Read a node's components as one tuple. A component type in it that the
/// world has never registered (an optional one nothing has inserted yet) is
/// registered first, so each tuple registers what it reads. A node whose
/// components still cannot be read is warned once and not drawn.
fn components<'a, Q: ReleaseStateQueryData + SingleEntityQueryData>(
  node: &'a mut EntityWorldMut<'_>,
  kind: &str,
) -> Option<Q::Item<'a, 'static>> {
  if Q::get_state(node.world().components()).is_none() {
    node.world_scope(|world| {
      Q::init_state(world);
    });
  }
  match node.get_components_mut::<Q>() {
    Ok(items) => Some(items),
    Err(err) => {
      warn_once(format!("ui-node:read:{kind}"), || {
        format!("could not read a {kind} node's components ({err}); it is not drawn")
      });
      None
    }
  }
}

/// Warn once that a `kind` node has no `component`; it is not drawn
fn missing(kind: &str, component: &str) -> Option<Response> {
  warn_once(format!("ui-node:missing:{kind}:{component}"), || {
    format!("a {kind} node has no {component}; it is not drawn")
  });
  None
}

/// A label or text as the builders take it: markup parsed when asked
fn rich(text: &str, markup: bool) -> RichText {
  if markup {
    RichText::markup(text)
  } else {
    RichText::from(text)
  }
}

/// Draw a button node
fn draw_button(
  mut node: EntityWorldMut<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let entity = node.id();
  let (kind, label, tooltip, disabled) = components::<(
    &UiNodeKind,
    Option<&UiLabel>,
    Option<&UiTooltip>,
    Has<UiDisabled>,
  )>(&mut node, "button")?;
  let Kind::Button(opts) = &kind.kind else {
    return None;
  };
  let Some(label) = label else {
    return missing("button", "UiLabel");
  };
  let extras = Extras {
    entity,
    place,
    tooltip,
    disabled,
  };
  let label = rich(&label.0, opts.markup);
  let mut button = widget_opts!(ui.button(label), kind, extras);
  if let Some(icon) = &opts.icon {
    button = button.icon(icon);
  }
  Some(button.show())
}

/// Draw an image node
fn draw_image(
  mut node: EntityWorldMut<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let entity = node.id();
  let (kind, image, tooltip, disabled) = components::<(
    &UiNodeKind,
    Option<&UiImage>,
    Option<&UiTooltip>,
    Has<UiDisabled>,
  )>(&mut node, "image")?;
  let Kind::Image(opts) = &kind.kind else {
    return None;
  };
  let Some(image) = image else {
    return missing("image", "UiImage");
  };
  let extras = Extras {
    entity,
    place,
    tooltip,
    disabled,
  };
  let mut builder = widget_opts!(ui.image(&image.0), kind, extras);
  if let Some(size) = opts.size {
    builder = builder.size(size.x, size.y);
  }
  if let Some(tint) = opts.tint {
    builder = builder.tint(tint);
  }
  if let Some(filter) = opts.filter {
    builder = builder.filter(filter);
  }
  if let Some(uv) = opts.uv {
    builder = builder.uv(uv);
  }
  Some(builder.show())
}

/// The style of a theme text role
fn role_style(theme: &Theme, role: TextRole) -> TextStyle {
  let roles = &theme.text;
  match role {
    TextRole::Body => roles.body.style(),
    TextRole::Heading => roles.heading.style(),
    TextRole::Button => roles.button.style(),
    TextRole::Caption => roles.caption.style(),
    TextRole::Mono => roles.mono.style(),
  }
}

/// Draw a text node inside its entity's scope (text has no `.id`), advancing
/// its `TextReveal` if it has one; it has no response
fn draw_text(mut node: EntityWorldMut<'_>, ui: &mut UiContext<'_>) {
  let entity = node.id();
  let Some((kind, label, reveal)) =
    components::<(&UiNodeKind, Option<&UiLabel>, Option<&mut TextReveal>)>(&mut node, "text")
  else {
    return;
  };
  let Kind::Text(opts) = &kind.kind else {
    return;
  };
  let Some(label) = label else {
    missing("text", "UiLabel");
    return;
  };
  let mut advanced = reveal.as_deref().copied();
  ui.scope(entity, |ui| {
    // The text builder borrows the context, so the theme is read first
    let role = opts
      .role
      .filter(|_| opts.style.is_none())
      .map(|role| role_style(ui.theme(), role));
    let muted = opts.muted.then(|| ui.theme().palette.muted);
    let mut text = ui.text(rich(&label.0, opts.markup));
    if let Some(style) = opts.style.as_ref().or(role.as_ref()) {
      text = text.style(style);
    }
    if let Some(size) = opts.size {
      text = text.size(size);
    }
    if let Some(muted) = muted {
      text = text.color(muted);
    }
    if let Some(color) = opts.color {
      text = text.color(color);
    }
    if let Some(align) = opts.align {
      text = text.align(align);
    }
    if let Some(width) = opts.wrap {
      text = text.wrap(width);
    }
    if let Some(lines) = opts.max_lines {
      text = text.max_lines(lines);
    }
    if opts.ellipsis {
      text = text.ellipsis();
    }
    if let Some(reveal) = advanced.as_mut() {
      text = text.typewriter(reveal);
    }
    text.draw();
  });
  if let (Some(mut reveal), Some(advanced)) = (reveal, advanced)
    && *reveal != advanced
  {
    *reveal = advanced;
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use std::sync::{Arc, Mutex};

  use crate::core::ecs::kernel::{ChildOf, Children, Entity, On, World};
  use crate::core::testing::UiHarness;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::{Align, Anchor, Rect, UiContext};

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn targets(h: &UiHarness) -> Vec<Rect> {
    nav_targets(&h.memory, Some(h.scene))
      .into_iter()
      .map(|(_, r)| r)
      .collect()
  }

  fn warnings(needle: &str) -> usize {
    crate::core::dev_log::get_entries()
      .iter()
      .filter(|w| w.message.contains(needle))
      .count()
  }

  /// A screen of every container and the simple widgets, as a closure
  fn closure_screen(ui: &mut UiContext<'_>) {
    let heading = ui.theme().text.heading.style();
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .show(|ui| {
        ui.text("Title").style(&heading);
        ui.row().gap(8.0).align(Align::Center).show(|ui| {
          ui.button("A").default_focus();
          ui.button("B").enabled(false);
        });
        ui.grid(3).gap(4.0).show(|ui| {
          for i in 0..6 {
            ui.button(format!("{i}")).id(i);
          }
        });
        ui.separator();
        ui.scroll().height(60.0).fill_width().show(|ui| {
          for i in 0..10 {
            ui.button(format!("Row {i}")).id(i).fill_width();
          }
        });
      });
  }

  /// The same screen as nodes
  fn node_screen() -> UiNode {
    UiNode::panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .child(UiNode::text("Title").heading())
      .child(
        UiNode::row()
          .gap(8.0)
          .align(Align::Center)
          .child(UiNode::button("A").default_focus())
          .child(UiNode::button("B").enabled(false)),
      )
      .child(
        UiNode::grid(3)
          .gap(4.0)
          .children((0..6).map(|i| UiNode::button(format!("{i}")))),
      )
      .child(UiNode::separator())
      .child(
        UiNode::scroll()
          .height(60.0)
          .fill_width()
          .children((0..10).map(|i| UiNode::button(format!("Row {i}")).fill_width())),
      )
  }

  #[test]
  fn a_node_screen_records_what_the_closure_screen_records() {
    let mut closure = UiHarness::new(400.0, 400.0);
    let mut nodes = UiHarness::new(400.0, 400.0);
    nodes.spawn_ui(node_screen());
    for _ in 0..3 {
      closure.frame(closure_screen);
      nodes.frame(|_| {});
    }
    assert!(!targets(&closure).is_empty());
    assert_eq!(
      targets(&nodes),
      targets(&closure),
      "the same focusables at the same rects"
    );
    assert_eq!(
      nodes.command_count(),
      closure.command_count(),
      "the same draws"
    );
    assert!(
      nodes.focused().is_some(),
      "the scope took its default focus"
    );
  }

  #[test]
  fn a_hidden_node_records_nothing_and_leaves_the_focus_chain() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A"))
        .child(UiNode::column().child(UiNode::button("B"))),
    );
    for _ in 0..2 {
      h.frame(|_| {});
    }
    assert_eq!(targets(&h).len(), 2);
    let inner = kids(h.world(), root)[1];
    h.world_mut().entity_mut(inner).insert(UiHidden);
    h.frame(|_| {});
    assert_eq!(targets(&h).len(), 1, "B went with its hidden column");
    h.world_mut().entity_mut(inner).remove::<UiHidden>();
    h.frame(|_| {});
    assert_eq!(targets(&h).len(), 2);
  }

  #[test]
  fn a_disabled_button_is_drawn_and_not_focusable() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A"))
        .child(UiNode::button("B")),
    );
    h.frame(|_| {});
    let draws = h.command_count();
    let b = kids(h.world(), root)[1];
    h.world_mut().entity_mut(b).insert(UiDisabled);
    h.frame(|_| {});
    assert_eq!(targets(&h).len(), 1);
    assert_eq!(h.command_count(), draws, "still drawn");
  }

  #[test]
  fn only_the_scenes_own_roots_draw() {
    let mut h = UiHarness::new(300.0, 300.0);
    h.scene = 2;
    h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("Elsewhere")),
    );
    h.scene = 1;
    h.frame(|_| {});
    h.frame(|_| {});
    assert!(targets(&h).is_empty());
    assert_eq!(h.command_count(), 0);
  }

  #[test]
  fn a_root_is_placed_by_its_ui_root() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A").width(100.0)),
    );
    h.frame(|_| {});
    let before = targets(&h)[0];
    assert_eq!((before.x, before.y), (0.0, 0.0), "top left by default");
    if let Some(mut placed) = h.world_mut().get_mut::<UiRoot>(root) {
      placed.offset = glam::Vec2::new(30.0, 40.0);
    }
    h.frame(|_| {});
    let after = targets(&h)[0];
    assert_eq!((after.x, after.y), (30.0, 40.0));
  }

  #[test]
  fn a_node_missing_its_state_is_skipped_and_the_rest_draws() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A"))
        .child(UiNode::button("B")),
    );
    let a = kids(h.world(), root)[0];
    h.world_mut().entity_mut(a).remove::<UiLabel>();
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(targets(&h).len(), 1);
    assert_eq!(warnings("a button node has no UiLabel"), 1);
  }

  #[test]
  fn a_node_draws_though_its_optional_components_were_never_registered() {
    use crate::core::text::TextReveal;
    let mut h = UiHarness::new(300.0, 300.0);
    h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("A")));
    let mut text = UiHarness::new(300.0, 300.0);
    text.spawn_ui(UiNode::text("Title"));
    for world in [h.world(), text.world()] {
      let components = world.components();
      assert!(
        components.component_id::<UiTooltip>().is_none()
          && components.component_id::<UiDisabled>().is_none()
          && components.component_id::<TextReveal>().is_none(),
        "a fresh world has never seen them"
      );
    }
    h.frame(|_| {});
    text.frame(|_| {});
    assert_eq!(targets(&h).len(), 1, "the button drew");
    assert_eq!(text.command_count(), 1, "the text drew");
  }

  #[test]
  fn a_child_that_is_not_a_node_is_skipped() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("A")));
    h.world_mut().spawn(ChildOf(root));
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(targets(&h).len(), 1);
  }

  #[test]
  fn focus_follows_the_entity_when_a_sibling_goes() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("A").default_focus())
        .child(UiNode::button("B"))
        .child(UiNode::button("C")),
    );
    for _ in 0..2 {
      h.frame(|_| {});
    }
    for _ in 0..2 {
      h.action("ui.down");
      h.frame(|_| {});
    }
    let on_c = h.focused();
    let a = kids(h.world(), root)[0];
    h.world_mut().despawn(a);
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(h.focused(), on_c, "C kept the focus and its id");
  }

  /// A column of Play (the default) and Quit, named and with actions
  fn menu() -> UiNode {
    UiNode::column()
      .focus_scope()
      .child(
        UiNode::button("Play")
          .action("play")
          .name("play")
          .default_focus()
          .width(100.0),
      )
      .child(
        UiNode::button("Quit")
          .action("quit")
          .name("quit")
          .width(100.0),
      )
  }

  #[test]
  fn a_click_sends_clicked_with_the_nodes_action_and_name() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(menu());
    h.frame(|_| {});
    let quit = kids(h.world(), root)[1];
    let rect = targets(&h)[1];
    h.move_to(rect.x + 5.0, rect.y + 5.0);
    h.press();
    h.frame(|_| {});
    h.release();
    h.frame(|_| {});
    let clicks: Vec<_> = h
      .events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Clicked)
      .cloned()
      .collect();
    assert_eq!(clicks.len(), 1);
    assert_eq!(
      (
        clicks[0].node,
        clicks[0].action.as_deref(),
        clicks[0].name.as_deref()
      ),
      (quit, Some("quit"), Some("quit"))
    );
    h.frame(|_| {});
    assert!(
      h.events().iter().all(|e| e.kind != UiEventKind::Clicked),
      "once"
    );
  }

  #[test]
  fn moving_the_focus_sends_lost_then_gained_in_walk_order() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(menu());
    h.frame(|_| {});
    h.frame(|_| {});
    let [play, quit] = [kids(h.world(), root)[0], kids(h.world(), root)[1]];
    h.action("ui.down");
    h.frame(|_| {});
    let focus: Vec<_> = h.events().iter().map(|e| (e.node, e.kind)).collect();
    assert_eq!(
      focus,
      [
        (play, UiEventKind::FocusLost),
        (quit, UiEventKind::FocusGained)
      ]
    );
    assert!(h.world().get::<UiInteraction>(quit).unwrap().focused());
    assert!(!h.world().get::<UiInteraction>(play).unwrap().focused());
  }

  #[test]
  fn an_observer_on_a_node_hears_its_events() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(menu());
    let play = kids(h.world(), root)[0];
    let heard = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&heard);
    h.world_mut()
      .entity_mut(play)
      .observe(move |event: On<UiEvent>| {
        if let Ok(mut log) = log.lock() {
          log.push(event.kind);
        }
      });
    h.frame(|_| {});
    h.frame(|_| {});
    h.action("ui.confirm");
    h.frame(|_| {});
    assert!(heard.lock().unwrap().contains(&UiEventKind::Clicked));
  }

  #[test]
  fn ui_id_is_known_once_drawn_and_focus_ui_moves_the_focus() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(menu());
    let quit = kids(h.world(), root)[1];
    assert_eq!(h.ui_id(quit), None, "not drawn yet");
    h.frame(|_| {});
    h.frame(|_| {});
    let id = h.ui_id(quit);
    assert!(id.is_some());
    assert_ne!(h.focused(), id);
    h.world_mut().resource_mut::<UiNodes>().pending_focus = Some(quit);
    for _ in 0..2 {
      h.frame(|_| {});
    }
    assert_eq!(h.focused(), id);
  }

  #[test]
  fn a_later_root_is_drawn_over_an_earlier_one() {
    let mut h = UiHarness::new(300.0, 300.0);
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    h.spawn_ui(UiNode::button("Under").at(at).action("under"));
    h.spawn_ui(UiNode::button("Over").at(at).action("over"));
    h.frame(|_| {});
    h.move_to(20.0, 20.0);
    h.press();
    h.frame(|_| {});
    h.release();
    h.frame(|_| {});
    let clicked: Vec<_> = h
      .events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Clicked)
      .filter_map(|e| e.action.clone())
      .collect();
    assert_eq!(clicked, ["over"]);
  }

  #[test]
  fn despawning_the_focused_node_leaves_no_dangling_focus() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(menu());
    h.frame(|_| {});
    h.frame(|_| {});
    let play_id = h.focused();
    h.action("ui.down");
    h.frame(|_| {});
    let quit = kids(h.world(), root)[1];
    let quit_id = h.ui_id(quit);
    assert_ne!(quit_id, play_id);
    assert_eq!(h.focused(), quit_id);
    h.world_mut().despawn(quit);
    for _ in 0..3 {
      h.frame(|_| {});
    }
    assert_eq!(h.focused(), None, "as for a widget a closure stops drawing");
    h.action("ui.down");
    h.frame(|_| {});
    assert_eq!(h.focused(), play_id, "navigation lands on what is left");
  }

  #[test]
  fn a_new_node_in_a_despawned_ones_place_starts_fresh() {
    let mut h = UiHarness::new(300.0, 300.0);
    let rows = || {
      UiNode::scroll()
        .height(60.0)
        .width(120.0)
        .children((0..20).map(|i| UiNode::button(format!("{i}")).fill_width()))
    };
    let first = h.spawn_ui(rows());
    h.frame(|_| {});
    h.move_to(20.0, 20.0);
    h.wheel(0.0, -3.0);
    for _ in 0..30 {
      h.frame(|_| {});
    }
    let id = h.ui_id(first).unwrap();
    assert!(
      h.frame(|ui| ui.scroll_offset(id)).y > 0.0,
      "the wheel scrolled it"
    );
    h.world_mut().despawn(first);
    let second = h.spawn_ui(rows());
    h.frame(|_| {});
    h.frame(|_| {});
    let id = h.ui_id(second).unwrap();
    assert_eq!(h.frame(|ui| ui.scroll_offset(id)).y, 0.0);
  }
}
