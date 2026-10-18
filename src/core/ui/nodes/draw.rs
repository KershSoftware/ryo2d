//! The walk: a scene's node trees drawn through the builders, keyed by entity
//!
//! Each node is drawn by the builder a closure would call, given `.id(entity)`
//! (text, separators and spacers are drawn inside `ui.scope(entity, ..)`), so
//! its id depends only on its entity and its parent's id. `draw_node` reads a
//! node's kind, flags and tooltip in one lookup and dispatches on it. A widget is drawn from shared reads
//! of its components, and a value the player changed is written back after
//! the builder returns. A container's builder is configured from the node
//! first; its `.show` then walks the children with the world free.

use glam::Vec2;

use crate::core::ecs::kernel::{
  Children, DetectChangesMut, Entity, EntityWorldMut, Has, Mut, ReleaseStateQueryData,
  SingleEntityQueryData, World,
};
use crate::core::render::batch::Filter;
use crate::core::text::{RichText, TextReveal, TextStyle};
use crate::core::ui::theme::warn_once;
use crate::core::ui::{Anchor, Container, Rect, Response, Scroll, Theme, UiContext, UiId};

use super::bindings::{self, UiBind};
use super::components::{
  BackAnswer, UiChecked, UiDisabled, UiHidden, UiImage, UiIndex, UiInteraction, UiLabel, UiName,
  UiNodes, UiOptions, UiRoot, UiSlot, UiTextValue, UiTooltip, UiValue,
};
use super::events::{UiEvent, UiEventKind};
use super::node::{
  ButtonOpts, ImageOpts, Kind, NodeLayout, RebindOpts, ScrollOpts, SliderOpts, TextOpts, TextRole,
  UiNodeKind,
};

/// What every node's draw reads, in one lookup: hidden, its kind, bound,
/// disabled and its tooltip
type NodeRead = (
  Has<UiHidden>,
  Option<&'static UiNodeKind>,
  Has<UiBind>,
  Has<UiDisabled>,
  Option<&'static UiTooltip>,
);

/// The node's shared options, read by `NodeRead`
#[derive(Clone, Copy)]
struct Shared<'w> {
  tooltip: Option<&'w UiTooltip>,
  disabled: bool,
}

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
  pub(crate) back: Option<Entity>,
  /// A focus request waiting for its node to be drawn
  pub(crate) pending_focus: Option<Entity>,
  /// The children of the containers being walked, innermost last: each
  /// container's list is copied here once, not read again per child
  children: Vec<Entity>,
  /// Whether `NodeRead`'s components are registered in the world
  registered: bool,
}

impl Walk {
  fn new(scene: u64, pending_focus: Option<Entity>) -> Walk {
    Walk {
      scene,
      events: Vec::new(),
      back: None,
      pending_focus,
      children: Vec::new(),
      registered: false,
    }
  }

  /// Keep an unused focus request for a later frame; the walk's events
  pub(crate) fn finish(mut self, world: &mut World, ui: &mut UiContext<'_>) -> Vec<UiEvent> {
    if let Some(target) = self.back
      && ui.back()
    {
      let answer = world.get::<BackAnswer>(target).map(|answer| answer.0);
      match answer {
        Some(no) => push_event(world, &mut self, no, UiEventKind::Clicked),
        None => push_event(world, &mut self, target, UiEventKind::Back),
      }
    }
    if let Some(node) = self.pending_focus
      && let Some(mut nodes) = world.get_resource_mut::<UiNodes>()
    {
      nodes.pending_focus = Some(node);
    }
    self.events
  }
}

/// Start a UI pass: embedded trees may be drawn again
pub(crate) fn begin_pass(world: &mut World) {
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
    nodes.embedded_drawn.clear();
  }
}

impl<'a> UiContext<'a> {
  /// Draw an embedded node tree (spawned with `.embedded()`) here, inside the
  /// open container: in its flow, focus scope and clip. Its events reach
  /// `ctx.ui_events()` after this scene's `ui` returns. Drawing one root
  /// twice in a frame draws it once and warns.
  ///
  /// A modal or popup inside an embedded tree takes Back only through the
  /// closure's own `ui.back()`.
  pub fn node(&mut self, root: Entity, world: &mut World) -> Response {
    let embedded = world.get::<UiRoot>(root).is_some_and(|r| r.embedded);
    if !embedded {
      warn_once("ui-node:embed-not-embedded".to_string(), || {
        "ui.node draws a root spawned with .embedded()".to_string()
      });
      return Response::default();
    }
    let Some(mut nodes) = world.get_resource_mut::<UiNodes>() else {
      return Response::default();
    };
    if nodes.embedded_drawn.contains(&root) {
      warn_once("ui-node:embed-twice".to_string(), || {
        "an embedded node tree was drawn twice in one frame; drawn once".to_string()
      });
      return Response::default();
    }
    nodes.embedded_drawn.push(root);
    let pending_focus = nodes.pending_focus.take();
    let mut walk = Walk::new(self.scene().0, pending_focus);
    bindings::sync_root(world, root);
    let response = draw_node(world, self, root, None, &mut walk);
    if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
      if let Some(node) = walk.pending_focus {
        nodes.pending_focus = Some(node);
      }
      nodes.pending.append(&mut walk.events);
    }
    response.unwrap_or_default()
  }
}

/// Deliver the events embedded trees queued during the scene's `ui`, and
/// return a copy
pub(crate) fn deliver_pending(world: &mut World) -> Vec<UiEvent> {
  let events = world
    .get_resource_mut::<UiNodes>()
    .map(|mut nodes| std::mem::take(&mut nodes.pending))
    .unwrap_or_default();
  super::events::deliver(world, events.clone());
  events
}

/// Draw `scene`'s node trees through the builders, in spawn order (a later
/// root over an earlier one), and return their events in walk order
pub(crate) fn draw_nodes(world: &mut World, ui: &mut UiContext<'_>, scene: u64) -> Vec<UiEvent> {
  let Some(pending_focus) = world.try_resource_scope(|world, mut nodes: Mut<UiNodes>| {
    nodes
      .roots
      .retain(|&(_, root)| world.get::<UiRoot>(root).is_some());
    nodes.pending_focus.take()
  }) else {
    return Vec::new();
  };
  let mut walk = Walk::new(scene, pending_focus);
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
    bindings::sync_root(world, root);
    draw_node(world, ui, root, Some(place), &mut walk);
  }
  walk.finish(world, ui)
}

/// Draw `node` and its subtree by its kind; `place` is a root's placement.
/// The node is looked up once, and its `UiNodeKind` read once.
fn draw_node(
  world: &mut World,
  ui: &mut UiContext<'_>,
  node: Entity,
  place: Option<Place>,
  walk: &mut Walk,
) -> Option<Response> {
  let Ok(mut entity) = world.get_entity_mut(node) else {
    return None;
  };
  if !walk.registered {
    register::<NodeRead>(&mut entity);
    walk.registered = true;
  }
  let Ok((hidden, kind, bound, disabled, tooltip)) = entity.get_components::<NodeRead>() else {
    return None;
  };
  if hidden {
    return None;
  }
  let Some(kind) = kind else {
    warn_once("ui-node:not-a-node".to_string(), || {
      "an entity under a UI node is not a node; it is skipped".to_string()
    });
    return None;
  };
  let shared = Shared { tooltip, disabled };
  let (response, write, sends) = match &kind.kind {
    Kind::Column | Kind::Row | Kind::Grid(_) | Kind::Stack | Kind::Panel | Kind::Popup { .. } => {
      let dialog = kind.flags.modal || matches!(kind.kind, Kind::Popup { .. });
      let container = container_builder(&entity, kind, shared, ui, place)?;
      let world = entity.into_world_mut();
      if dialog {
        walk.back = Some(node);
      }
      let response = container.show(|ui| walk_children(world, ui, node, walk));
      return answer_container(world, ui, node, response, walk);
    }
    Kind::Scroll(opts) => {
      let scroll = scroll_builder(&entity, kind, shared, opts, ui, place);
      let world = entity.into_world_mut();
      let response = scroll.show(|ui| walk_children(world, ui, node, walk));
      return answer_container(world, ui, node, response, walk);
    }
    Kind::Text(opts) => {
      if let Some(advanced) = draw_text(&entity, opts, ui)
        && let Some(mut reveal) = entity.get_mut::<TextReveal>()
      {
        *reveal = advanced;
      }
      return None;
    }
    Kind::Separator => {
      ui.scope(node, |ui| ui.separator());
      return None;
    }
    Kind::Spacer => {
      ui.scope(node, |ui| ui.spacer());
      return None;
    }
    Kind::RadioGroup { horizontal } => {
      if let Some(group) = draw_radio_group(&entity, kind, shared, *horizontal, ui, place) {
        if group.changed {
          Write::Index(group.chosen).apply(&mut entity);
        }
        wrote(&mut entity, node, bound);
        answer_group(&mut entity, &group, walk);
      }
      return None;
    }
    Kind::ChoiceList { default } => {
      let (picked, write) = draw_choice_list(&entity, kind, shared, *default, ui, place);
      if let Some(write) = write {
        write.apply(&mut entity);
      }
      wrote(&mut entity, node, bound);
      if picked.is_some() {
        push_event(entity.world(), walk, node, UiEventKind::Clicked);
      }
      return None;
    }
    Kind::TextInput(_) => (
      draw_text_input(&mut entity, ui, place),
      None,
      Sends::Changes,
    ),
    Kind::Button(opts) => (
      draw_button(&entity, kind, shared, opts, ui, place),
      None,
      Sends::Clicks,
    ),
    Kind::Image(opts) => (
      draw_image(&entity, kind, shared, opts, ui, place),
      None,
      Sends::Clicks,
    ),
    Kind::Checkbox | Kind::Toggle => {
      let (response, write) = draw_check(&entity, kind, shared, ui, place);
      (response, write, Sends::Changes)
    }
    Kind::Slider(opts) => {
      let (response, write) = draw_slider(&entity, kind, shared, opts, ui, place);
      (response, write, Sends::Changes)
    }
    Kind::Progress { label } => (
      draw_progress(&entity, kind, shared, label.as_deref(), ui, place),
      None,
      Sends::Clicks,
    ),
    Kind::Dropdown | Kind::Tabs => {
      let (response, write) = draw_pick(&entity, kind, shared, ui, place);
      (response, write, Sends::Changes)
    }
    Kind::Slot { filter } => (
      draw_slot(&entity, kind, shared, *filter, ui, place),
      None,
      Sends::Clicks,
    ),
    Kind::Prompt { action } => (
      draw_prompt(&entity, kind, shared, action, ui, place),
      None,
      Sends::Clicks,
    ),
    Kind::Rebind(opts) => (
      draw_rebind(&entity, kind, shared, opts, ui, place),
      None,
      Sends::Changes,
    ),
  };
  if let Some(write) = write {
    write.apply(&mut entity);
  }
  wrote(&mut entity, node, bound);
  if let Some(response) = &response {
    answer(&mut entity, ui, response, sends, walk);
  }
  response
}

/// A bound widget has been drawn and may have written: its name follows
fn wrote(entity: &mut EntityWorldMut<'_>, node: Entity, bound: bool) {
  if bound {
    entity.world_scope(|world| bindings::widget_wrote(world, node));
  }
}

/// Which of `Clicked` and `Changed` a kind sends
#[derive(Clone, Copy)]
enum Sends {
  Clicks,
  Changes,
}

/// A value a widget changed, written to the node once its reads are done:
/// only on a change, so `Changed<..>` filters see real changes
enum Write {
  Checked(bool),
  Value(f32),
  Index(usize),
}

impl Write {
  fn apply(self, node: &mut EntityWorldMut<'_>) {
    match self {
      Write::Checked(on) => {
        if let Some(mut checked) = node.get_mut::<UiChecked>() {
          checked.0 = on;
        }
      }
      Write::Value(value) => {
        if let Some(mut slot) = node.get_mut::<UiValue>() {
          slot.0 = value;
        }
      }
      Write::Index(index) => {
        if let Some(mut slot) = node.get_mut::<UiIndex>() {
          slot.0 = index;
        }
      }
    }
  }
}

/// Turn a drawn node's `response` into its `UiInteraction`, a focus request
/// that was waiting for it, and the events it sends
fn answer(
  node: &mut EntityWorldMut<'_>,
  ui: &mut UiContext<'_>,
  response: &Response,
  sends: Sends,
  walk: &mut Walk,
) {
  set_interaction(
    node,
    UiInteraction {
      id: response.id,
      hovered: response.hovered(),
      focused: response.focused(),
    },
  );
  let entity = node.id();
  if walk.pending_focus == Some(entity) {
    ui.request_focus(response.id);
    walk.pending_focus = None;
  }
  for kind in event_kinds(response, sends).into_iter().flatten() {
    push_event(node.world(), walk, entity, kind);
  }
}

/// [`answer`] for a container whose children have been drawn: the walk may
/// have moved the world on, so the node is looked up again
fn answer_container(
  world: &mut World,
  ui: &mut UiContext<'_>,
  node: Entity,
  response: Response,
  walk: &mut Walk,
) -> Option<Response> {
  if let Ok(mut entity) = world.get_entity_mut(node) {
    answer(&mut entity, ui, &response, Sends::Clicks, walk);
  }
  Some(response)
}

/// The events a `response` reports, in a node's order: `Clicked` or
/// `Changed`, `Submitted`, `Dropped`, `SecondaryClicked`, `FocusLost`,
/// `FocusGained`
fn event_kinds(response: &Response, sends: Sends) -> [Option<UiEventKind>; 6] {
  let sent = match sends {
    Sends::Clicks => response.clicked().then_some(UiEventKind::Clicked),
    Sends::Changes => response.changed().then_some(UiEventKind::Changed),
  };
  [
    sent,
    response.submitted().then_some(UiEventKind::Submitted),
    response
      .dropped::<Entity>()
      .map(|source| UiEventKind::Dropped { source }),
    response
      .secondary_clicked()
      .then_some(UiEventKind::SecondaryClicked),
    response.lost_focus().then_some(UiEventKind::FocusLost),
    response.gained_focus().then_some(UiEventKind::FocusGained),
  ]
}

/// Write `now` as the node's `UiInteraction` if it differs. It is read
/// first, which is cheaper than a mutable borrow when nothing changed.
fn set_interaction(node: &mut EntityWorldMut<'_>, now: UiInteraction) {
  if node.get::<UiInteraction>() == Some(&now) {
    return;
  }
  match node.get_mut::<UiInteraction>() {
    Some(mut state) => {
      if *state != now {
        *state = now;
      }
    }
    None => {
      node.insert(now);
    }
  }
}

/// Queue an event of `kind` from `node`
fn push_event(world: &World, walk: &mut Walk, node: Entity, kind: UiEventKind) {
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

/// Draw `parent`'s children in order. Its list is copied to the walk's
/// stack once, so it is not read again for each child.
fn walk_children(world: &mut World, ui: &mut UiContext<'_>, parent: Entity, walk: &mut Walk) {
  let start = walk.children.len();
  if let Some(children) = world.get::<Children>(parent) {
    walk.children.extend_from_slice(children);
  }
  let end = walk.children.len();
  for i in start..end {
    let child = walk.children[i];
    draw_node(world, ui, child, None, walk);
  }
  walk.children.truncate(start);
}

/// Warn once that `UiDisabled` on a container does nothing
fn warn_disabled(shared: Shared<'_>, kind: &UiNodeKind) {
  if shared.disabled {
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

/// A column, row, grid, stack, panel, modal or popup node's builder,
/// configured from the node; `None` for another kind
fn container_builder<'u, 'a>(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  ui: &'u mut UiContext<'a>,
  place: Option<Place>,
) -> Option<Container<'u, 'a>> {
  let entity = node.id();
  warn_disabled(shared, kind);
  let (layout, flags) = (&kind.layout, kind.flags);
  let gap = gap_of(layout, ui);
  let anchor = kind
    .anchor_node
    .map_or(UiId::NONE, |anchor| recorded_id(node.world(), anchor));
  let mut container = match kind.kind {
    _ if flags.modal => ui.modal(),
    Kind::Popup { .. } => ui.popup(anchor),
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
  if flags.drag_source {
    container = container.drag_source(entity);
  }
  if flags.drop_target {
    container = container.drop_target::<Entity>();
  }
  Some(container)
}

/// A scroll node's builder, configured from the node: only the options
/// `Scroll` has
fn scroll_builder<'u, 'a>(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  opts: &ScrollOpts,
  ui: &'u mut UiContext<'a>,
  place: Option<Place>,
) -> Scroll<'u, 'a> {
  warn_disabled(shared, kind);
  let layout = &kind.layout;
  let gap = gap_of(layout, ui);
  let mut scroll = ui.scroll().id(node.id());
  if opts.horizontal {
    scroll = scroll.horizontal();
  }
  if opts.both {
    scroll = scroll.both();
  }
  if opts.no_bar {
    scroll = scroll.no_bar();
  }
  if kind.flags.drag_source {
    scroll = scroll.drag_source(node.id());
  }
  if kind.flags.drop_target {
    scroll = scroll.drop_target::<Entity>();
  }
  sized!(scroll, layout);
  if let Some(gap) = gap {
    scroll = scroll.gap(gap);
  }
  if let Some(grow) = layout.grow {
    scroll = scroll.grow(grow);
  }
  placed!(scroll, placement(place, layout));
  scroll
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
  /// The ids of the nodes up, down, left and right go to
  nav: [UiId; 4],
}

impl<'w> Extras<'w> {
  /// A widget node's shared options, read from the node
  fn of(
    node: &EntityWorldMut<'_>,
    kind: &UiNodeKind,
    shared: Shared<'w>,
    place: Option<Place>,
  ) -> Extras<'w> {
    Extras {
      entity: node.id(),
      place,
      tooltip: shared.tooltip,
      disabled: shared.disabled,
      nav: neighbour_ids(node.world(), kind.nav),
    }
  }
}

/// The recorded id of each named neighbour (`NONE` for none, or one not
/// drawn yet)
fn neighbour_ids(world: &World, nav: [Option<Entity>; 4]) -> [UiId; 4] {
  nav.map(|target| target.map_or(UiId::NONE, |target| recorded_id(world, target)))
}

fn recorded_id(world: &World, node: Entity) -> UiId {
  world
    .get::<UiInteraction>(node)
    .map_or(UiId::NONE, |state| state.id)
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
    if extras.nav[0] != UiId::NONE {
      builder = builder.nav_up(extras.nav[0]);
    }
    if extras.nav[1] != UiId::NONE {
      builder = builder.nav_down(extras.nav[1]);
    }
    if extras.nav[2] != UiId::NONE {
      builder = builder.nav_left(extras.nav[2]);
    }
    if extras.nav[3] != UiId::NONE {
      builder = builder.nav_right(extras.nav[3]);
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
    // A slot is a source only when it holds something: `draw_slot` decides
    if node.flags.drag_source && !matches!(node.kind, Kind::Slot { .. }) {
      builder = builder.drag_source(extras.entity);
    }
    if node.flags.drop_target {
      builder = builder.drop_target::<Entity>();
    }
    builder
  }};
}

/// Register what `Q` reads, if the world has not yet
fn register<Q: ReleaseStateQueryData + SingleEntityQueryData>(node: &mut EntityWorldMut<'_>) {
  if Q::get_state(node.world().components()).is_none() {
    node.world_scope(|world| {
      Q::init_state(world);
    });
  }
}

/// Read a node's components as one tuple, some of them mutably. A component
/// type in it that the world has never registered (an optional one nothing
/// has inserted yet) is registered first, so each tuple registers what it
/// reads. A node whose components still cannot be read is warned once and
/// not drawn.
fn components<'a, Q: ReleaseStateQueryData + SingleEntityQueryData>(
  node: &'a mut EntityWorldMut<'_>,
  kind: &str,
) -> Option<Q::Item<'a, 'static>> {
  register::<Q>(node);
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
fn missing<T>(kind: &str, component: &str) -> Option<T> {
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
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  opts: &ButtonOpts,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let Some(label) = node.get::<UiLabel>() else {
    return missing("button", "UiLabel");
  };
  let extras = Extras::of(node, kind, shared, place);
  let label = rich(&label.0, opts.markup);
  let mut button = widget_opts!(ui.button(label), kind, extras);
  if let Some(icon) = &opts.icon {
    button = button.icon(icon);
  }
  Some(button.show())
}

/// Draw an image node
fn draw_image(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  opts: &ImageOpts,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let Some(image) = node.get::<UiImage>() else {
    return missing("image", "UiImage");
  };
  let extras = Extras::of(node, kind, shared, place);
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

/// Draw a slot node: a drag source only while it holds an icon
fn draw_slot(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  filter: Option<Filter>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let Some(slot) = node.get::<UiSlot>() else {
    return missing("slot", "UiSlot");
  };
  let extras = Extras::of(node, kind, shared, place);
  let mut builder = widget_opts!(ui.slot(), kind, extras)
    .count(slot.count)
    .selected(slot.selected);
  if let Some(icon) = &slot.icon {
    builder = builder.icon(icon);
    if kind.flags.drag_source {
      builder = builder.drag_source(extras.entity);
    }
  }
  if let Some(filter) = filter {
    builder = builder.filter(filter);
  }
  Some(builder.show())
}

/// Draw a prompt node: the glyph for an action's input
fn draw_prompt(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  action: &str,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let extras = Extras::of(node, kind, shared, place);
  Some(widget_opts!(ui.prompt(action), kind, extras).show())
}

/// Draw a rebind node: a row that rebinds an action's input
fn draw_rebind(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  opts: &RebindOpts,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let extras = Extras::of(node, kind, shared, place);
  let mut builder = widget_opts!(ui.rebind(&opts.action), kind, extras);
  if let Some(label) = &opts.label {
    builder = builder.label(label.as_str());
  }
  if let Some(slot) = opts.slot {
    builder = builder.slot(slot);
  }
  Some(builder.show())
}

/// Draw a checkbox or toggle node; its new `UiChecked`, if the player
/// changed it
fn draw_check(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> (Option<Response>, Option<Write>) {
  let name = kind.name();
  let Some(label) = node.get::<UiLabel>() else {
    return (missing(name, "UiLabel"), None);
  };
  let Some(checked) = node.get::<UiChecked>() else {
    return (missing(name, "UiChecked"), None);
  };
  let extras = Extras::of(node, kind, shared, place);
  let mut on = checked.0;
  let response = if matches!(kind.kind, Kind::Toggle) {
    widget_opts!(ui.toggle(&mut on, label.0.as_str()), kind, extras).show()
  } else {
    widget_opts!(ui.checkbox(&mut on, label.0.as_str()), kind, extras).show()
  };
  let write = (on != checked.0).then_some(Write::Checked(on));
  (Some(response), write)
}

/// Draw a slider node; its new `UiValue`, if the player changed it
fn draw_slider(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  opts: &SliderOpts,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> (Option<Response>, Option<Write>) {
  let Some(value) = node.get::<UiValue>() else {
    return (missing("slider", "UiValue"), None);
  };
  let extras = Extras::of(node, kind, shared, place);
  let (lo, hi) = opts.range;
  let start = if value.0.is_finite() { value.0 } else { lo };
  let drawn = if lo.is_finite() && hi.is_finite() {
    start.clamp(lo.min(hi), hi.max(lo))
  } else {
    start
  };
  let mut v = drawn;
  let mut slider = widget_opts!(ui.slider(&mut v, lo..=hi), kind, extras);
  if let Some(step) = opts.step {
    slider = slider.step(step);
  }
  if opts.show_value {
    slider = slider.show_value();
  }
  if let Some(label) = &opts.label {
    slider = slider.label(label.as_str());
  }
  let response = slider.show();
  let write = (v != drawn).then_some(Write::Value(v));
  (Some(response), write)
}

/// Draw a progress node
fn draw_progress(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  label: Option<&str>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let Some(value) = node.get::<UiValue>() else {
    return missing("progress", "UiValue");
  };
  let extras = Extras::of(node, kind, shared, place);
  let fraction = if value.0.is_nan() {
    0.0
  } else {
    value.0.clamp(0.0, 1.0)
  };
  let mut progress = widget_opts!(ui.progress(fraction), kind, extras);
  if let Some(label) = label {
    progress = progress.label(label);
  }
  Some(progress.show())
}

/// Draw a text field node. The field edits its `UiTextValue` in place, so
/// this kind reads its components mutably.
fn draw_text_input(
  node: &mut EntityWorldMut<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<Response> {
  let entity = node.id();
  let nav = node.get::<UiNodeKind>().map_or([UiId::NONE; 4], |kind| {
    neighbour_ids(node.world(), kind.nav)
  });
  let (kind, mut text, tooltip, disabled) = components::<(
    &UiNodeKind,
    &mut UiTextValue,
    Option<&UiTooltip>,
    Has<UiDisabled>,
  )>(node, "text field")?;
  let Kind::TextInput(opts) = &kind.kind else {
    return None;
  };
  let extras = Extras {
    entity,
    place,
    nav,
    tooltip,
    disabled,
  };
  // Typing edits the string in place; the component is marked changed only
  // when the field says it changed
  let response = {
    let mut field = widget_opts!(
      ui.text_input(&mut text.bypass_change_detection().0),
      kind,
      extras
    );
    if let Some(placeholder) = &opts.placeholder {
      field = field.placeholder(placeholder.as_str());
    }
    if let Some(max) = opts.max_len {
      field = field.max_len(max);
    }
    if opts.password {
      field = field.password();
    }
    field.show()
  };
  if response.changed() {
    text.set_changed();
  }
  Some(response)
}

/// What a radio group reports once drawn
struct GroupAnswer {
  interaction: UiInteraction,
  changed: bool,
  /// The option chosen now
  chosen: usize,
}

/// Draw a radio group node: a row or column of radios
fn draw_radio_group(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  horizontal: bool,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> Option<GroupAnswer> {
  let entity = node.id();
  let Some(options) = node.get::<UiOptions>() else {
    return missing("radio_group", "UiOptions");
  };
  let Some(index) = node.get::<UiIndex>() else {
    return missing("radio_group", "UiIndex");
  };
  let disabled = shared.disabled;
  let layout = &kind.layout;
  let gap = gap_of(layout, ui);
  let mut container = if horizontal { ui.row() } else { ui.column() }.id(entity);
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
  placed!(container, placement(place, layout));
  if let Some(color) = layout.background {
    container = container.background(color);
  }
  if kind.flags.drag_source {
    container = container.drag_source(entity);
  }
  if kind.flags.drop_target {
    container = container.drop_target::<Entity>();
  }
  let drawn = index.0.min(options.0.len().saturating_sub(1));
  let mut chosen = drawn;
  let (mut hovered, mut focused) = (false, false);
  let response = container.show(|ui| {
    for (k, option) in options.0.iter().enumerate() {
      let mut radio = ui.radio(&mut chosen, k, option.as_str()).id(k as u64);
      if disabled {
        radio = radio.enabled(false);
      }
      let response = radio.show();
      hovered |= response.hovered();
      focused |= response.focused();
    }
  });
  Some(GroupAnswer {
    interaction: UiInteraction {
      id: response.id,
      hovered,
      focused,
    },
    changed: chosen != drawn,
    chosen,
  })
}

/// Record a radio group's interaction and send its events
fn answer_group(node: &mut EntityWorldMut<'_>, group: &GroupAnswer, walk: &mut Walk) {
  let was_focused = node
    .get::<UiInteraction>()
    .is_some_and(|state| state.focused);
  set_interaction(node, group.interaction);
  let entity = node.id();
  if group.changed {
    push_event(node.world(), walk, entity, UiEventKind::Changed);
  }
  if group.interaction.focused != was_focused {
    let kind = if group.interaction.focused {
      UiEventKind::FocusGained
    } else {
      UiEventKind::FocusLost
    };
    push_event(node.world(), walk, entity, kind);
  }
}

/// The longest option list passed to a builder from the stack
const SHORT_LIST: usize = 16;

/// A node's options as the builders take them, `&[&str]`: on the stack for
/// a short list, so drawing a dropdown, tabs or choices does not allocate
struct OptionList<'s> {
  short: [&'s str; SHORT_LIST],
  /// How many of `short` are the options; a longer list is in `long`
  len: usize,
  long: Vec<&'s str>,
}

impl<'s> OptionList<'s> {
  fn of(options: &'s [String]) -> OptionList<'s> {
    let mut list = OptionList {
      short: [""; SHORT_LIST],
      len: options.len(),
      long: Vec::new(),
    };
    if options.len() <= SHORT_LIST {
      for (slot, option) in list.short.iter_mut().zip(options) {
        *slot = option.as_str();
      }
    } else {
      list.long = options.iter().map(String::as_str).collect();
    }
    list
  }

  fn as_slice(&self) -> &[&'s str] {
    if self.len <= SHORT_LIST {
      self.short.get(..self.len).unwrap_or(&[])
    } else {
      &self.long
    }
  }
}

/// Draw a dropdown or tabs node; its new `UiIndex`, if the player changed it
fn draw_pick(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> (Option<Response>, Option<Write>) {
  let name = kind.name();
  let Some(options) = node.get::<UiOptions>() else {
    return (missing(name, "UiOptions"), None);
  };
  let Some(index) = node.get::<UiIndex>() else {
    return (missing(name, "UiIndex"), None);
  };
  let extras = Extras::of(node, kind, shared, place);
  let list = OptionList::of(&options.0);
  let list = list.as_slice();
  let drawn = index.0.min(list.len().saturating_sub(1));
  let mut chosen = drawn;
  let response = if matches!(kind.kind, Kind::Tabs) {
    widget_opts!(ui.tabs(&mut chosen, list), kind, extras).show()
  } else {
    widget_opts!(ui.dropdown(&mut chosen, list), kind, extras).show()
  };
  let write = (chosen != drawn).then_some(Write::Index(chosen));
  (Some(response), write)
}

/// Draw a choice list node: the choice picked this frame, and its new
/// `UiIndex` if that differs
fn draw_choice_list(
  node: &EntityWorldMut<'_>,
  kind: &UiNodeKind,
  shared: Shared<'_>,
  default: Option<usize>,
  ui: &mut UiContext<'_>,
  place: Option<Place>,
) -> (Option<usize>, Option<Write>) {
  let Some(options) = node.get::<UiOptions>() else {
    return (missing("choice_list", "UiOptions"), None);
  };
  let Some(index) = node.get::<UiIndex>() else {
    return (missing("choice_list", "UiIndex"), None);
  };
  let extras = Extras::of(node, kind, shared, place);
  let list = OptionList::of(&options.0);
  let mut choices = widget_opts!(ui.choice_list(list.as_slice()), kind, extras);
  if let Some(first) = default {
    choices = choices.default(first);
  }
  let Some(picked) = choices.show() else {
    return (None, None);
  };
  let write = (index.0 != picked).then_some(Write::Index(picked));
  (Some(picked), write)
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

/// Draw a text node inside its entity's scope (text has no `.id`); it has no
/// response. Its `TextReveal` advanced, if it has one that moved.
fn draw_text(
  node: &EntityWorldMut<'_>,
  opts: &TextOpts,
  ui: &mut UiContext<'_>,
) -> Option<TextReveal> {
  let Some(label) = node.get::<UiLabel>() else {
    return missing("text", "UiLabel");
  };
  let reveal = node.get::<TextReveal>().copied();
  let mut advanced = reveal;
  ui.scope(node.id(), |ui| {
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
  advanced.filter(|advanced| Some(*advanced) != reveal)
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use std::sync::{Arc, Mutex};

  use crate::core::ecs::kernel::{ChildOf, Children, Entity, On, World};
  use crate::core::testing::UiHarness;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::warnings_containing as warnings;
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

#[cfg(all(test, feature = "default-fonts"))]
mod values {
  use winit::keyboard::KeyCode;

  use crate::core::ecs::kernel::{Children, DetectChanges, Entity, World};
  use crate::core::input::Direction;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn changes(d: &UiDriver) -> Vec<Entity> {
    d.events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Changed)
      .map(|e| e.node)
      .collect()
  }

  /// `node` alone in a focus scope, focused, drawn twice
  fn focused(node: UiNode) -> (UiDriver, Entity) {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(UiNode::column().focus_scope().child(node.default_focus()));
    d.frame(|_| {});
    d.frame(|_| {});
    let node = kids(d.world(), root)[0];
    (d, node)
  }

  #[test]
  fn a_checkbox_writes_its_component_and_sends_changed() {
    let (mut d, node) = focused(UiNode::checkbox("Mute", false));
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiChecked>(node), Some(&UiChecked(true)));
    assert_eq!(changes(&d), [node]);
  }

  #[test]
  fn a_slider_with_a_broken_range_draws_without_panicking() {
    for range in [
      f32::NAN..=f32::NAN,
      0.0..=f32::NAN,
      0.0..=f32::INFINITY,
      f32::NEG_INFINITY..=f32::INFINITY,
    ] {
      let (mut d, _) = focused(UiNode::slider(range, 0.5));
      d.frame(|_| {});
    }
  }

  #[test]
  fn a_slider_reads_what_the_game_wrote() {
    let (mut d, node) = focused(UiNode::slider(0.0..=1.0, 0.5).step(0.05));
    if let Some(mut value) = d.world_mut().get_mut::<UiValue>(node) {
      value.0 = 0.75;
    }
    d.frame(|_| {});
    d.nav(Direction::Right);
    d.frame(|_| {});
    let value = d
      .world()
      .get::<UiValue>(node)
      .map(|v| v.0)
      .unwrap_or_default();
    assert!(
      (value - 0.8).abs() < 1e-5,
      "stepped from the game's 0.75: {value}"
    );
    assert_eq!(changes(&d), [node]);
  }

  #[test]
  fn a_frame_without_input_changes_no_component() {
    let (mut d, node) = focused(UiNode::slider(0.0..=1.0, 0.5));
    d.world_mut().clear_trackers();
    d.frame(|_| {});
    let changed = d
      .world()
      .entity(node)
      .get_ref::<UiValue>()
      .is_some_and(|v| v.is_changed());
    assert!(!changed, "drawing alone marks nothing changed");
    assert!(changes(&d).is_empty());
  }

  #[test]
  fn a_value_outside_the_range_draws_clamped() {
    let (mut d, node) = focused(UiNode::slider(0.0..=1.0, 0.5).step(0.05));
    for wrote in [3.0, f32::NAN] {
      if let Some(mut value) = d.world_mut().get_mut::<UiValue>(node) {
        value.0 = wrote;
      }
      d.frame(|_| {});
      let kept = d
        .world()
        .get::<UiValue>(node)
        .map(|v| v.0)
        .unwrap_or_default();
      assert!(
        kept == wrote || (kept.is_nan() && wrote.is_nan()),
        "the game's value stays until the player moves it"
      );
    }
    d.nav(Direction::Right);
    d.frame(|_| {});
    let value = d
      .world()
      .get::<UiValue>(node)
      .map(|v| v.0)
      .unwrap_or_default();
    assert!(
      (value - 0.05).abs() < 1e-5,
      "NaN drew as the start, and a step moved it: {value}"
    );
  }

  #[test]
  fn a_radio_group_writes_its_index_and_sends_one_change() {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(
      UiNode::column().focus_scope().child(
        UiNode::radio_group(&["Small", "Medium", "Large"], 0)
          .horizontal()
          .gap(16.0),
      ),
    );
    d.frame(|_| {});
    d.frame(|_| {});
    let group = kids(d.world(), root)[0];
    d.nav(Direction::Right);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(group), Some(&UiIndex(1)));
    assert_eq!(changes(&d), [group]);
    assert!(
      d.world()
        .get::<UiInteraction>(group)
        .is_some_and(|i| i.focused())
    );
  }

  #[test]
  fn a_text_field_sends_changed_and_submitted() {
    let (mut d, node) = focused(UiNode::text_input("").placeholder("Name"));
    d.confirm();
    d.frame(|_| {});
    d.type_text("Ryo");
    d.frame(|_| {});
    assert_eq!(
      d.world().get::<UiTextValue>(node),
      Some(&UiTextValue("Ryo".into()))
    );
    assert_eq!(changes(&d), [node]);
    d.key(KeyCode::Enter);
    d.frame(|_| {});
    assert!(
      d.events()
        .iter()
        .any(|e| e.node == node && e.kind == UiEventKind::Submitted)
    );
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod lists {
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::text::TextReveal;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn focused(node: UiNode) -> (UiDriver, Entity) {
    let mut d = UiDriver::new((400.0, 400.0));
    let root = d.spawn_ui(UiNode::column().focus_scope().child(node.default_focus()));
    d.frame(|_| {});
    d.frame(|_| {});
    let node = kids(d.world(), root)[0];
    (d, node)
  }

  #[test]
  fn a_dropdown_picks_and_sends_changed() {
    let (mut d, node) = focused(UiNode::dropdown(
      &["Windowed", "Borderless", "Fullscreen"],
      0,
    ));
    d.confirm();
    d.frame(|_| {});
    d.frame(|_| {});
    d.nav(Direction::Down);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(node), Some(&UiIndex(1)));
    assert!(
      d.events()
        .iter()
        .any(|e| e.node == node && e.kind == UiEventKind::Changed)
    );
  }

  #[test]
  fn an_index_past_the_options_draws_clamped() {
    let (mut d, node) = focused(UiNode::dropdown(&["A", "B"], 0));
    if let Some(mut index) = d.world_mut().get_mut::<UiIndex>(node) {
      index.0 = 5;
    }
    d.frame(|_| {});
    assert_eq!(
      d.world().get::<UiIndex>(node),
      Some(&UiIndex(5)),
      "kept until the player picks"
    );
    if let Some(mut options) = d.world_mut().get_mut::<UiOptions>(node) {
      options.0.clear();
    }
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    d.frame(|_| {});
  }

  #[test]
  fn tabs_switch_on_the_tab_keys_and_send_changed() {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::tabs(&["Audio", "Video", "Controls"], 0))
        .child(UiNode::button("In the tab").default_focus()),
    );
    d.frame(|_| {});
    d.frame(|_| {});
    let tabs = kids(d.world(), root)[0];
    d.tab_next();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(tabs), Some(&UiIndex(1)));
    assert!(
      d.events()
        .iter()
        .any(|e| e.node == tabs && e.kind == UiEventKind::Changed)
    );
  }

  #[test]
  fn a_choice_list_sends_clicked_with_its_index() {
    let (mut d, node) = focused(UiNode::choice_list(&[
      "Buy a gem",
      "Buy a potion",
      "Goodbye",
    ]));
    d.nav(Direction::Down);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(node), Some(&UiIndex(1)));
    let clicked: Vec<_> = d
      .events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Clicked)
      .map(|e| e.node)
      .collect();
    assert_eq!(clicked, [node]);
  }

  #[test]
  fn a_typewriter_reveals_finishes_and_holds() {
    let mut d = UiDriver::new((400.0, 200.0));
    let text = d.spawn_ui(
      UiNode::text("Welcome to the Rusty Lantern, traveller.").typewriter(TextReveal::new(20.0)),
    );
    for _ in 0..15 {
      d.frame(|_| {});
    }
    let reveal = |d: &UiDriver| {
      d.world()
        .get::<TextReveal>(text)
        .copied()
        .unwrap_or(TextReveal::new(1.0))
    };
    let shown = reveal(&d).shown();
    assert!(
      shown > 0.0 && !reveal(&d).is_done(),
      "part of the line: {shown}"
    );
    // The engine reads a rate of 0 as "show everything", not as a hold
    if let Some(mut r) = d.world_mut().get_mut::<TextReveal>(text) {
      r.per_second = 0.0;
    }
    for _ in 0..15 {
      d.frame(|_| {});
    }
    assert!(
      reveal(&d).is_done() && reveal(&d).shown() > shown,
      "shown whole"
    );
    let whole = reveal(&d).shown();
    for _ in 0..15 {
      d.frame(|_| {});
    }
    assert_eq!(reveal(&d).shown(), whole, "held");
    if let Some(mut r) = d.world_mut().get_mut::<TextReveal>(text) {
      r.restart();
      r.per_second = 20.0;
    }
    d.frame(|_| {});
    assert!(!reveal(&d).is_done(), "restarted");
    if let Some(mut r) = d.world_mut().get_mut::<TextReveal>(text) {
      r.finish();
    }
    d.frame(|_| {});
    assert!(reveal(&d).is_done());
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod drag {
  use super::{Sends, event_kinds};
  use crate::core::asset::SpriteHandle;
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::graphics::Sprite;
  use crate::core::input::Direction;
  use crate::core::ui::Response;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn icon() -> SpriteHandle {
    SpriteHandle::from_sprite(
      Sprite::solid_rect(16, 16, 200, 50, 50, 255),
      "node-slot-icon",
    )
  }

  /// A row of three slots: the first holds an item, the second is empty,
  /// the third is empty and refuses drops
  fn bag() -> (UiDriver, [Entity; 3]) {
    let mut d = UiDriver::new((400.0, 200.0));
    let root = d.spawn_ui(
      UiNode::row()
        .focus_scope()
        .gap(6.0)
        .child(
          UiNode::slot()
            .icon(icon())
            .count(3)
            .drag_source()
            .drop_target()
            .default_focus(),
        )
        .child(UiNode::slot().drag_source().drop_target())
        .child(UiNode::slot().drag_source()),
    );
    d.frame(|_| {});
    d.frame(|_| {});
    let k = kids(d.world(), root);
    (d, [k[0], k[1], k[2]])
  }

  fn held(d: &mut UiDriver) -> Option<Entity> {
    d.frame(|ui| ui.held::<Entity>())
  }

  #[test]
  fn carrying_a_slot_to_another_sends_dropped_naming_the_source() {
    let (mut d, [full, empty, _]) = bag();
    d.confirm();
    d.frame(|_| {});
    assert_eq!(held(&mut d), Some(full));
    d.nav(Direction::Right);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    let dropped: Vec<_> = d
      .events()
      .iter()
      .filter_map(|e| match e.kind {
        UiEventKind::Dropped { source } => Some((e.node, source)),
        _ => None,
      })
      .collect();
    assert_eq!(dropped, [(empty, full)]);
    assert_eq!(held(&mut d), None);
  }

  #[test]
  fn an_empty_slot_does_not_drag() {
    let (mut d, [_, empty, _]) = bag();
    d.nav(Direction::Right);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(held(&mut d), None, "nothing to pick up in {empty:?}");
  }

  #[test]
  fn despawning_a_held_slot_cancels_the_drag() {
    let (mut d, [full, _, _]) = bag();
    d.confirm();
    d.frame(|_| {});
    assert_eq!(held(&mut d), Some(full));
    d.world_mut().despawn(full);
    for _ in 0..3 {
      d.frame(|_| {});
    }
    assert_eq!(held(&mut d), None);
    assert_eq!(d.focused(), None, "as for any node that stops being drawn");
    d.nav(Direction::Right);
    d.frame(|_| {});
    assert!(d.focused().is_some(), "navigation lands back in the row");
  }

  #[test]
  fn a_nodes_events_keep_the_documented_order() {
    let source = Entity::from_raw_u32(7).expect("a valid index");
    let response = Response {
      states: Response::CLICKED
        | Response::SUBMITTED
        | Response::SECONDARY
        | Response::LOST_FOCUS
        | Response::GAINED_FOCUS,
      dropped: Some(std::sync::Arc::new(source)),
      ..Response::default()
    };
    let kinds: Vec<_> = event_kinds(&response, Sends::Clicks)
      .into_iter()
      .flatten()
      .collect();
    assert_eq!(
      kinds,
      [
        UiEventKind::Clicked,
        UiEventKind::Submitted,
        UiEventKind::Dropped { source },
        UiEventKind::SecondaryClicked,
        UiEventKind::FocusLost,
        UiEventKind::FocusGained,
      ]
    );
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod dialogs {
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Anchor, Rect};

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn settle(d: &mut UiDriver) {
    for _ in 0..3 {
      d.frame(|_| {});
    }
  }

  fn page() -> UiNode {
    UiNode::column()
      .focus_scope()
      .child(UiNode::button("Page").default_focus())
  }

  fn dialog(action: &str) -> UiNode {
    UiNode::panel()
      .modal()
      .action(action)
      .child(UiNode::button("Inside").default_focus())
      .child(UiNode::button("Also"))
  }

  #[test]
  fn a_modal_node_traps_the_focus() {
    let mut d = UiDriver::new((400.0, 300.0));
    d.spawn_ui(page());
    let modal = d.spawn_ui(dialog("options"));
    settle(&mut d);
    let inside = kids(d.world(), modal);
    let reachable: Vec<_> = nav_targets(&d.memory, Some(d.scene))
      .into_iter()
      .map(|(id, _)| id)
      .collect();
    let ids: Vec<_> = inside.iter().filter_map(|e| d.ui_id(*e)).collect();
    assert_eq!(reachable, ids, "only the dialog's buttons");
  }

  #[test]
  fn back_goes_to_the_innermost_dialog() {
    let mut d = UiDriver::new((400.0, 300.0));
    d.spawn_ui(page());
    d.spawn_ui(dialog("outer"));
    let inner = d.spawn_ui(dialog("inner"));
    settle(&mut d);
    d.back();
    d.frame(|_| {});
    let backs: Vec<_> = d
      .events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Back)
      .map(|e| (e.node, e.action.clone()))
      .collect();
    assert_eq!(backs, [(inner, Some("inner".to_string()))]);
  }

  fn backs(d: &UiDriver) -> Vec<(Entity, Option<String>)> {
    d.events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Back)
      .map(|e| (e.node, e.action.clone()))
      .collect()
  }

  #[test]
  fn back_goes_to_a_popup_inside_a_modal() {
    let mut d = UiDriver::new((400.0, 300.0));
    let modal = d.spawn_ui(
      UiNode::panel()
        .modal()
        .action("dialog")
        .child(UiNode::button("Menu").name("menu").default_focus())
        .child(
          UiNode::popup("menu")
            .action("popup")
            .child(UiNode::button("Item")),
        ),
    );
    settle(&mut d);
    let popup = kids(d.world(), modal)[1];
    d.back();
    d.frame(|_| {});
    assert_eq!(backs(&d), [(popup, Some("popup".to_string()))]);
  }

  #[test]
  fn back_goes_to_a_modal_spawned_inside_a_modal() {
    let mut d = UiDriver::new((400.0, 300.0));
    let outer = d.spawn_ui(dialog("outer"));
    settle(&mut d);
    let inner = crate::core::ui::nodes::spawn_ui_in(d.world_mut(), outer, dialog("inner"))
      .expect("a panel takes children");
    settle(&mut d);
    d.back();
    d.frame(|_| {});
    assert_eq!(backs(&d), [(inner, Some("inner".to_string()))]);
  }

  #[test]
  fn a_confirm_opens_on_no_and_back_answers_no() {
    let mut d = UiDriver::new((400.0, 300.0));
    let confirm = d.spawn_ui(UiNode::confirm(
      "Quit?",
      "Your progress is saved.",
      "Yes",
      "No",
    ));
    settle(&mut d);
    let buttons = kids(d.world(), *kids(d.world(), confirm).last().unwrap());
    let no = buttons[1];
    assert_eq!(d.focused(), d.ui_id(no), "opens on No");
    d.back();
    d.frame(|_| {});
    let answers: Vec<_> = d
      .events()
      .iter()
      .filter(|e| e.kind == UiEventKind::Clicked)
      .map(|e| (e.node, e.action.clone()))
      .collect();
    assert_eq!(answers, [(no, Some("no".to_string()))]);
    d.nav(Direction::Left);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert!(
      d.events()
        .iter()
        .any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some("yes"))
    );
  }

  /// The rects the pad can land on, top to bottom, left to right
  fn target_rects(d: &UiDriver) -> Vec<Rect> {
    let mut rects: Vec<Rect> = nav_targets(&d.memory, Some(d.scene))
      .into_iter()
      .map(|(_, rect)| rect)
      .collect();
    rects.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap());
    rects
  }

  #[test]
  fn a_modal_root_opens_centred_as_ui_modal_does() {
    let mut nodes = UiDriver::new((400.0, 300.0));
    let confirm = nodes.spawn_ui(UiNode::confirm(
      "Quit?",
      "Your progress is saved.",
      "Yes",
      "No",
    ));
    settle(&mut nodes);
    let mut closure = UiDriver::new((400.0, 300.0));
    for _ in 0..3 {
      closure.frame(|ui| ui.confirm("Quit?", "Your progress is saved."));
    }
    let rects = target_rects(&nodes);
    assert_eq!(rects.len(), 2, "Yes and No");
    assert_eq!(rects, target_rects(&closure), "where ui.confirm puts them");
    assert_eq!(
      nodes.world().get::<UiRoot>(confirm).map(|root| root.anchor),
      Some(Anchor::Center),
      "its UiRoot says so"
    );
  }

  #[test]
  fn a_modal_root_keeps_the_anchor_it_was_given() {
    let mut nodes = UiDriver::new((400.0, 300.0));
    let root = nodes.spawn_ui(dialog("options").anchor(Anchor::TopRight));
    settle(&mut nodes);
    let mut closure = UiDriver::new((400.0, 300.0));
    for _ in 0..3 {
      closure.frame(|ui| {
        ui.modal().anchor(Anchor::TopRight).show(|ui| {
          ui.button("Inside").default_focus().show();
          ui.button("Also").show();
        });
      });
    }
    assert_eq!(target_rects(&nodes), target_rects(&closure));
    assert_eq!(
      nodes.world().get::<UiRoot>(root).map(|root| root.anchor),
      Some(Anchor::TopRight)
    );
  }

  #[test]
  fn a_popup_hangs_below_its_named_anchor() {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(
          UiNode::button("More")
            .name("more")
            .at(Rect::new(20.0, 20.0, 80.0, 24.0)),
        )
        .child(
          UiNode::popup("more").child(UiNode::button("Rename").name("rename").default_focus()),
        ),
    );
    settle(&mut d);
    let rename = d.find_ui("rename").unwrap();
    let rect = nav_targets(&d.memory, Some(d.scene))
      .into_iter()
      .find(|(id, _)| Some(*id) == d.ui_id(rename))
      .map(|(_, r)| r);
    let Some(rect) = rect else {
      panic!("the popup's button is not focusable: {root:?}")
    };
    assert!(rect.y >= 44.0, "below the anchor's bottom edge: {rect:?}");
  }

  #[test]
  fn a_named_neighbour_is_where_the_pad_goes() {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(
      UiNode::column()
        .focus_scope()
        .anchor(Anchor::Center)
        .child(UiNode::button("Play").default_focus().nav_down("quit"))
        .child(UiNode::button("Options"))
        .child(UiNode::button("Quit").name("quit")),
    );
    settle(&mut d);
    let quit = kids(d.world(), root)[2];
    d.nav(Direction::Down);
    d.frame(|_| {});
    assert_eq!(d.focused(), d.ui_id(quit), "past Options to Quit");
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod embedding {
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world
      .get::<Children>(of)
      .map(|c| c.to_vec())
      .unwrap_or_default()
  }

  fn grid() -> UiNode {
    UiNode::grid(2)
      .gap(4.0)
      .embedded()
      .children((0..4).map(|i| UiNode::button(format!("Cell {i}")).action("cell")))
  }

  /// Before, the embedded grid, After: one focus scope
  fn screen(ui: &mut UiContext<'_>, world: &mut World, root: Entity) {
    ui.column().focus_scope().gap(8.0).show(|ui| {
      ui.button("Before").default_focus();
      ui.node(root, world);
      ui.button("After");
    });
  }

  fn rects(d: &UiDriver) -> Vec<Rect> {
    nav_targets(&d.memory, Some(d.scene))
      .into_iter()
      .map(|(_, r)| r)
      .collect()
  }

  #[test]
  fn an_embedded_tree_draws_in_the_closures_flow_and_focus_path() {
    let mut d = UiDriver::new((400.0, 400.0));
    let root = d.spawn_ui(grid());
    for _ in 0..3 {
      d.frame_with_world(|ui, world| screen(ui, world, root));
    }
    let r = rects(&d);
    assert_eq!(r.len(), 6, "Before, four cells, After");
    assert!(r[0].y < r[1].y && r[4].y < r[5].y, "in the column's order");
    let first_cell = kids(d.world(), root)[0];
    d.nav(Direction::Down);
    d.frame_with_world(|ui, world| screen(ui, world, root));
    assert_eq!(
      d.focused(),
      d.ui_id(first_cell),
      "down from Before into the grid"
    );
  }

  #[test]
  fn an_embedded_root_is_not_drawn_on_its_own() {
    let mut d = UiDriver::new((400.0, 400.0));
    d.spawn_ui(grid());
    d.frame(|_| {});
    d.frame(|_| {});
    assert_eq!(d.command_count(), 0);
  }

  #[test]
  fn drawing_an_embedded_tree_twice_warns_once_and_draws_it_once() {
    let mut d = UiDriver::new((400.0, 400.0));
    let root = d.spawn_ui(grid());
    for _ in 0..3 {
      d.frame_with_world(|ui, world| {
        ui.column().focus_scope().show(|ui| {
          ui.node(root, world);
          ui.node(root, world);
        });
      });
    }
    assert_eq!(rects(&d).len(), 4, "four cells, not eight");
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|w| w.message.contains("drawn twice in one frame"))
      .count();
    assert_eq!(warned, 1);
  }

  #[test]
  fn the_harness_delivers_walk_events_before_its_closure() {
    let mut d = UiDriver::new((400.0, 400.0));
    let root = d.spawn_ui(UiNode::button("Go").action("go"));
    for _ in 0..3 {
      d.frame(|_| {});
    }
    d.move_to(5.0, 5.0);
    d.frame(|_| {});
    d.press();
    d.frame(|_| {});
    d.release();
    let seen = d.frame_with_world(|_, world| {
      let messages = world.resource::<crate::core::ecs::kernel::Messages<UiEvent>>();
      let mut cursor = messages.get_cursor();
      cursor
        .read(messages)
        .any(|e| e.kind == UiEventKind::Clicked && e.node == root)
    });
    assert!(seen, "the closure sees this frame's walk events");
  }

  #[test]
  fn embedded_events_arrive_after_the_closure() {
    let mut d = UiDriver::new((400.0, 400.0));
    let root = d.spawn_ui(grid());
    for _ in 0..3 {
      d.frame_with_world(|ui, world| screen(ui, world, root));
    }
    d.nav(Direction::Down);
    d.frame_with_world(|ui, world| screen(ui, world, root));
    d.confirm();
    let during = d.frame_with_world(|ui, world| {
      screen(ui, world, root);
      world.resource::<UiNodes>().pending.len()
    });
    assert_eq!(during, 1, "queued while the closure runs");
    assert!(
      d.events()
        .iter()
        .any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some("cell"))
    );
    assert!(
      d.world().resource::<UiNodes>().pending.is_empty(),
      "delivered after it"
    );
  }
}
