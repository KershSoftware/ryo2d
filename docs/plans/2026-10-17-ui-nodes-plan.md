# Retained UI Nodes (4c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retained UI: a tree of nodes described once in the closure API's vocabulary, spawned as entities, drawn each frame through 4b's builders, with its state in public components and its events out as `UiEvent` messages and entity events. The four acceptance pages, a mixed page and breakout's menus are built on it.

**Architecture:** `ctx.spawn_ui(UiNode)` spawns one entity per node under `ChildOf`, with the node's kind and options in `UiNodeKind` and its data in public state components (`UiLabel`, `UiChecked`, `UiValue`, ...). In each scene's UI pass, between `begin_scene` and the scene's `ui`, `ui::nodes::draw_nodes` walks the scene's roots in spawn order and calls the same builders a closure calls, keyed by entity. It writes a widget's changes back into its components, and collects `UiEvent`s that are written as messages and triggered on the node. The engine despawns a scene's roots when the scene exits. Nothing in 4b's widgets changes except one container option (`.background`).

**Tech Stack:** Rust 2024; the existing `ryo2d::ecs` (bevy_ecs 0.20: `ChildOf`/`Children`, `Message`, `EntityEvent`, observers), 4b's UI (`UiContext`, builders, `UiMemory`, `UiHarness`), criterion (the guard bench).

**Spec:** `docs/specs/2026-10-17-ui-nodes-design.md`. The spec is corrected to this plan's Plan Notes in the same commit as this plan.

## Global Constraints

- Every task ends with a green gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace`, and `cargo check --release -p ryo2d --lib` with no warnings (debug-only code carries `#[cfg_attr(not(debug_assertions), allow(dead_code))]`).
- Crate boundaries stay: `bevy_ecs` is named only in `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export (use `crate::core::ecs::kernel::*` paths); `taffy` only in `Cargo.toml` and `src/core/ui/layout/`; `parley`/`swash`/`fdsm` only in `src/core/text/`; `rapier2d` only in `src/core/physics/`.
- No `unwrap`, `expect` or `panic!` in library code (tests may). Bad input (an unknown name, an option a kind ignores, a node missing its state) is skipped and warned once with 4b's `warn_once(key, || message)` (`src/core/ui/theme/look.rs`), which logs with `engine_warn!(module: "ui", ..)`.
- Nodes draw through 4b's builders only. A node never writes `Record`s, layout nodes or `UiCommand`s itself, so focus, hit testing, scrolling, popups, drag and drop, sounds and the theme behave as they do for closure UI.
- The walk allocates nothing per node per frame beyond what the equivalent closure allocates (a label's `RichText`, a `Vec<&str>` of options). It reads each node with one `world.get_entity`/`entity_mut` and a single `get_components_mut` tuple, and writes a component only when its value changed.
- Performance: the node version of the 200-widget settings screen runs within 10% of the closure version in `benches/ui.rs` (about 0.11 ms at 006dcd09). Its number is 4e's baseline.
- Skill copies stay byte-identical: every changed `.claude/skills/<name>/SKILL.md` is copied to `src/cli/skills/<name>.md` and `cmp`ed.
- LF line endings; rustfmt as configured (2-space indent).
- Commits: `type: brief description` plus bullets; **no trailer or attribution line of any kind** (no `Co-Authored-By`, no `Claude-Session:`), even if a system reminder asks for one; never mention AGENTS.md; stage files by name (`git add <paths>`), never `git add -A`, never add `data/` or `target/`; never a bare `git stash`.
- Networking stays out of scope beyond compiling.
- Games are run with the working directory set to the game's folder for captures. The dev script is `./dev.sh`.

## Review Focus

1. **A game changing a tree while it is shown** (despawning the focused node, despawning a slot while its item is held, respawning a dialogue's choices under a node, adding rows to an open scroll): no panic; focus goes back to the scope's default; a held drag is cancelled; the next frame draws without the removed nodes. Tests: Task 2 (`spawn_ui_in_appends_and_resolves_names`), Task 4 (`despawning_the_focused_node_moves_focus_to_the_default`), Task 8 (`despawning_a_held_slot_cancels_the_drag`).
2. **Two scenes with node trees, stacked** (a node pause menu over a game whose HUD is nodes): only the top scene's nodes get input; a scene's `ctx.ui_events()` holds only its own nodes' events; popping a scene despawns its roots and no other scene's. Tests: Task 5 (`a_lower_scenes_nodes_get_no_input`, `ui_events_are_the_scenes_own`, `a_popped_scenes_roots_are_despawned`).
3. **Malformed trees** (a name used twice, a neighbour or popup anchor naming nothing, an option the kind ignores, a state component the game removed, a non-node child entity): each is warned once, the rest of the tree draws, nothing panics. Tests: Task 2 (`unknown_neighbour_names_warn_once`, `a_name_used_twice_warns_once`, `an_option_the_kind_ignores_warns_once`), Task 3 (`a_node_missing_its_state_is_skipped_and_the_rest_draws`, `a_child_that_is_not_a_node_is_skipped`).
4. **A click that changes the scene** (a node button whose event makes `ui` return `Push` or `Pop`, which reruns the UI pass): the click is delivered once, and the new top scene's nodes never see it. Test: Task 5 (`a_click_that_changes_the_scene_is_delivered_once`).
5. **A game writing values a widget cannot show** (`UiIndex` past the end of `UiOptions`, `UiOptions` emptied, a `UiValue` outside the slider's range or NaN): the widget draws with the value clamped into range, as a closure passing the same value would, and the component keeps the game's value until the player changes it. Tests: Task 6 (`a_value_outside_the_range_draws_clamped`), Task 7 (`an_index_past_the_options_draws_clamped`).

## Plan Notes (decisions the plan makes against the spec)

The spec is corrected to match these in the same commit as this plan.

- **Ids.** A node's id is made by its builder keyed by its entity (`.id(entity)`), under its parent's id, the way any keyed closure widget's id is. Text, separators and spacers have no `.id`, so they are drawn inside `ui.scope(entity, ..)`. The id is recorded in the node's `UiInteraction` each time it is drawn: `ctx.ui_id(node)` is `None` until the node has been drawn once, and re-parenting a node changes its id. The spec's "a hash of the scene scope and the entity" is corrected; the guarantees are the same (state follows the entity through reorders, a new entity in the same slot starts fresh).
- **`UiInteraction` (new component).** Every node gets one. The walk keeps it current: `hovered()`, `focused()` and `id()`. It is how the inventory's details panel follows the slot under the pointer or with the focus, read in the same frame after the walk.
- **`UiTooltip` (new component).** A tooltip changes with the game's data (an inventory slot names its item), so it is a state component. `.tooltip(text)` spawns it.
- **`ctx.spawn_ui_in(parent, tree)` (new).** It spawns a subtree as the last child of an existing node, resolving its names within the parent's root. A dialogue swaps its choices this way. A new subtree is a new focus scope, so focus moves to its default, as in the closure version's `.id((n, done))` scope.
- **`ctx.ui_events()` (new).** Messages are global, so a scene reading `ctx.messages::<UiEvent>()` also sees the events of other scenes' nodes. `ctx.ui_events()` returns only the events of the scene's own nodes. It shares its message cursor with `ctx.messages::<UiEvent>()`, so a scene uses one or the other. `UiEvent` carries a crate-private `scene` field for this; patterns use `..`.
- **Containers gain `.background(Color)`, in both styles.** A scene's nodes draw before its closure, so a closure's full-screen `draw_rect` would cover them. Breakout's menu backdrop and colour blocks become nodes with backgrounds. A container's background is a plain fill of its rect behind its children (no border, no radius).
- **`.modal()` works on any container node**, not only roots, as `ui.modal()` works in any closure container. The node is drawn through `ui.modal()` in place of its kind's container: a column panel over the dim, trapping, opening on its `default_focus`. Popup nodes likewise draw wherever they sit in the tree.
- **Back goes to the innermost node dialog.** After a scene's trees are walked, the last modal or popup node drawn gets Back, if a widget did not use it (`ui.back()`). It sends `Back` with that node's action. For a confirm, it sends `Clicked` from the No button instead. Node dialogs therefore answer Back before the scene's closure calls `ui.back()`.
- **Embedding takes the world.** `ui.node(root, world: &mut World) -> Response` (the spec had `ctx`), called as `ui.node(root, ctx.world)`, so it also works in `UiHarness`. Embedded trees' events are delivered after the scene's `ui` returns: `update` reads them in the same frame, and `ui` from the next frame.
- **`UiHarness` gains a world:** `spawn_ui`, `world`, `world_mut`, `find_ui`, `ui_id`, `events` (the last frame's) and `frame_with_world(|ui, world| ..)`. `frame` draws the harness scene's node trees before its closure. `UiDriver` gets them through `Deref`.
- **Option names.** `UiNode` is one type, so a few closure names that mean two things are split:
  - text: `.text_align(TextAlign)`, since `.align` is a container's cross-axis alignment;
  - image: `.image_size(w, h)`, since `.size` is a text's size;
  - rebind: `.binding_slot(n)`, since `slot` is a constructor;
  - choice list: `.default_choice(i)`.
  Text is plain unless `.markup()`, as a closure passes `RichText::markup(..)`. Theme text roles are read at draw time: `.role(TextRole)`, `.heading()`, and `.muted()` (the palette's muted colour).
- **Which kinds send what.** Value widgets (checkbox, toggle, radio group, slider, text field, dropdown, tabs) send `Changed`, not `Clicked`. Buttons, images, slots and containers send `Clicked`. A choice list sends `Clicked` with the choice in `UiIndex`. A text field also sends `Submitted`. Any focusable node sends `FocusGained`/`FocusLost`, any node `SecondaryClicked`, and a drop target `Dropped`. Within one node the order is: `Clicked` or `Changed`, `Submitted`, `Dropped`, `SecondaryClicked`, `FocusLost`, `FocusGained`.
- **Disabled is for widgets.** `UiDisabled` on a widget draws it with `enabled(false)`. On a container it is ignored and warned once (4b containers have no `enabled`).
- **An empty slot does not drag.** A `drag_source()` slot with no icon in its `UiSlot` is empty, as in 4b, and is not a drag source until it has one.
- **Options a kind ignores warn once at spawn**, naming the option and the kind (`UiNode::gap does nothing on a button node`), as do children of a widget, `.embedded()` on a child and `.modal()` on a widget.
- **Root placement lives on `UiRoot`:** public `anchor`, `offset` and `at`, taken from the root node's own options when spawned (`Anchor::TopLeft` and no offset by default, as closure roots). A game moves a tree by writing them. The dialogue HUD follows the demo shell's content area this way.
- **Neighbours and popup anchors** name nodes, resolved to entities at spawn within the root's tree. They are drawn with the named node's last recorded id, so a neighbour later in walk order applies from the tree's second frame (navigation resolves against last frame's records anyway).
- **`ctx.focus_ui(node)`** asks for the focus when the node is next drawn (`ui.request_focus`, which lands the frame after), and is kept until then. A node that is hidden keeps the request until it is shown.
- **Change detection.** The walk writes a state component only when the widget changed it (or for `TextReveal`, when it advanced), so `Changed<UiValue>` and the like mean a real change.
- **Out-of-range values** are clamped for drawing only. The component keeps the game's value until the player changes it (Review Focus 5).
- **Demo pages.** The node pages are appended to the `ui` demo after its eleven pages, at indices 11 (Node Menus), 12 (Node Settings), 13 (Node Inventory), 14 (Node Dialogue) and 15 (Mixed), so the existing tests' page indices stand. The inventory, dialogue, menu and settings data and logic move into shared modules used by both versions.

---

## File Structure

| Path | Responsibility | Task |
|---|---|---|
| `src/core/ui/nodes/mod.rs` | Module docs and re-exports | 1 |
| `src/core/ui/nodes/node.rs` | `UiNode` (the tree value and its builders), `UiNodeKind`, `TextRole`, the crate's `Kind`, `NodeLayout`, `NodeFlags`, `NodeState`; which options each kind takes | 1 |
| `src/core/ui/nodes/components.rs` | `UiRoot`, the state components, the markers, `UiTooltip`, `UiInteraction`; crate `BackAnswer` and the `UiNodes` resource | 1 |
| `src/core/ui/nodes/events.rs` | `UiEvent`, `UiEventKind`, `deliver` | 1, 4 |
| `src/core/ui/nodes/spawn.rs` | `spawn_ui`, `spawn_ui_in`, name resolution, `find_ui`, `find_ui_in`, `despawn_scene_roots`, warnings | 2 |
| `src/core/ui/nodes/draw.rs` | `begin_pass`, `draw_nodes`, the walk, `deliver_pending`, `UiContext::node` | 3, 4, 6-10 |
| `src/core/ui/containers.rs` | `Container::background(Color)` (`Backing::Fill`) | 3 |
| `src/core/ui/mod.rs`, `src/lib.rs` | `pub mod nodes` re-exports; prelude | 1 |
| `src/core/messages/mod.rs` | `UiEvent` among the engine messages | 1 |
| `src/core/scene/context.rs` | `spawn_ui`, `spawn_ui_in`, `find_ui`, `find_ui_in`, `ui_id`, `focus_ui`, `ui_events` | 2, 4, 5 |
| `src/core/scene/manager.rs` | `ui_pass` draws each scene's nodes, delivers their events, then runs `Scene::ui`; embedded trees' events after it | 5, 10 |
| `src/core/engine/frame.rs` | `draw_ui` despawns exited scenes' roots | 5 |
| `src/core/testing/ui.rs` | `UiHarness`'s world | 2-4, 10 |
| `tests/e2e/ui_nodes.rs`, `tests/e2e.rs` | Engine tests over `HeadlessEngine` | 5, 9 |
| `examples/demos/ui/src/scenes/{menu_data, settings_data, inventory_model, dialogue_data}.rs` | The data and logic both page versions share | 11, 12 |
| `examples/demos/ui/src/scenes/{node_menu_page, node_settings_page, node_inventory_page, node_dialogue_page, mixed_page}.rs` | The node pages | 11-13 |
| `examples/demos/ui/tests/acceptance.rs` | The walks over both versions; the Mixed path | 11-13 |
| `examples/games/breakout/src/scenes/{menu, pause, quit_confirm}.rs` | Breakout's menus on nodes | 14 |
| `benches/ui.rs` | The node settings screen beside the closure one | 15 |
| `.claude/skills/ryo-ui/SKILL.md`, `.claude/skills/ryo-api/SKILL.md` (+ CLI copies) | Nodes section; API | 15 |
| `src/core/ui/AGENTS.md`, `docs/architecture/overview.md`, `docs/backlog/ui-retained-nodes.md` | Nodes docs; the backlog note points at 4d and 4e | 15 |

## Task Order

1. The tree value, components and events
2. Spawning, finding and the harness's world
3. The walk: containers, text, buttons, images, separators, focus options, hidden and disabled, backgrounds
4. Events out, interaction state, `ui_id` and `focus_ui`
5. Engine integration: the UI pass, scene ownership, `ctx.ui_events()`
6. Value widgets: checkbox, toggle, radio group, slider, progress, text field
7. Lists and text: dropdown, tabs, choice list, typewriter
8. Slots, drag and drop, prompts and rebinding
9. Modals, confirms, popups, Back and named neighbours
10. Embedding node trees in closure UI
11. The `ui` demo: Node Menus and Node Settings
12. The `ui` demo: Node Inventory and Node Dialogue
13. The `ui` demo: Mixed
14. Breakout's menus on nodes
15. The guard bench, docs and skills

---

### Task 1: The tree value, components and events

**Files:**
- Create: `src/core/ui/nodes/mod.rs`, `src/core/ui/nodes/node.rs`, `src/core/ui/nodes/components.rs`, `src/core/ui/nodes/events.rs`
- Modify:
  - `src/core/ui/mod.rs`: `pub mod nodes;` and re-exports.
  - `src/lib.rs`: the prelude's UI block.
  - `src/core/messages/mod.rs`: `register_message::<UiEvent>(world)` in `register_engine_messages`.
- Test: `src/core/ui/nodes/node.rs`, `src/core/ui/nodes/events.rs` (tests)

**Interfaces:**
- Consumes: 4b's `Length`, `Edges`, `Align`, `Justify`, `Anchor`, `Rect`, `Color` (`crate::core::ui`), `TextStyle`, `TextAlign`, `TextReveal` (`crate::core::text`), `SpriteHandle` (`crate::core::asset`), `Filter` (`crate::core::render::batch`), `Bundle`, `Component`, `Entity`, `EntityWorldMut`, `Message`, `EntityEvent`, `Resource` (`crate::core::ecs::kernel`).
- Produces (every later task uses these names):

```rust
// node.rs
pub struct UiNode { /* pub(crate) fields below */ }
pub enum TextRole { Body, Heading, Button, Caption, Mono }   // Debug, Clone, Copy, PartialEq, Eq
#[derive(Component, Debug, Clone)]
pub struct UiNodeKind { /* pub(crate) kind, layout, flags, variant, action, nav, anchor_node */ }
impl UiNodeKind {
  pub fn name(&self) -> &'static str;      // "column", "button", ...: the constructor's name
  pub fn action(&self) -> Option<&str>;
}
pub(crate) enum Kind { .. }                // below
pub(crate) struct NodeLayout { .. }        // below
pub(crate) struct NodeFlags { .. }         // below
pub(crate) struct NodeState { .. }         // below
pub(crate) type Insert = Box<dyn FnOnce(&mut EntityWorldMut<'_>) + Send + Sync>;
impl Kind { pub(crate) fn name(&self) -> &'static str; pub(crate) fn takes(&self, option: &str) -> bool;
            pub(crate) fn is_container(&self) -> bool; }

// components.rs
#[derive(Component, Debug, Clone)]
pub struct UiRoot { pub anchor: Anchor, pub offset: Vec2, pub at: Option<Rect>,
                    pub(crate) scene: u64, pub(crate) seq: u64, pub(crate) embedded: bool }
pub struct UiName(pub String); pub struct UiLabel(pub String); pub struct UiImage(pub SpriteHandle);
pub struct UiChecked(pub bool); pub struct UiValue(pub f32); pub struct UiIndex(pub usize);
pub struct UiOptions(pub Vec<String>); pub struct UiTextValue(pub String);
pub struct UiSlot { pub icon: Option<SpriteHandle>, pub count: u32, pub selected: bool }
pub struct UiTooltip(pub String); pub struct UiHidden; pub struct UiDisabled;
pub struct UiInteraction { pub(crate) id: UiId, pub(crate) hovered: bool, pub(crate) focused: bool }
impl UiInteraction { pub fn hovered(&self) -> bool; pub fn focused(&self) -> bool; pub fn id(&self) -> Option<UiId>; }
#[derive(Component)] pub(crate) struct BackAnswer(pub(crate) Entity);   // a confirm's No button
#[derive(Resource, Default)] pub(crate) struct UiNodes {
  pub(crate) roots: Vec<(u64, Entity)>,   // (scene scope, root), in spawn order
  pub(crate) seq: u64,
  pub(crate) pending_focus: Option<Entity>,
  pub(crate) pending: Vec<UiEvent>,       // embedded trees' events, delivered after the scene's ui
  pub(crate) embedded_drawn: Vec<Entity>, // this pass
}

// events.rs
#[derive(Message, EntityEvent, Debug, Clone, PartialEq)]
pub struct UiEvent { #[event_target] pub node: Entity, pub name: Option<String>,
                     pub action: Option<String>, pub kind: UiEventKind, pub(crate) scene: u64 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiEventKind { Clicked, SecondaryClicked, Changed, Submitted, Dropped { source: Entity },
                       Back, FocusGained, FocusLost }
```

**The tree value (`node.rs`).** Write it in full:

```rust
//! The node tree value: a UI described once, spawned with `ctx.spawn_ui`

use std::fmt;
use std::ops::RangeInclusive;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::{Bundle, Component, Entity, EntityWorldMut};
use crate::core::render::batch::Filter;
use crate::core::text::{TextAlign, TextReveal, TextStyle};
use crate::core::ui::{Align, Anchor, Color, Edges, Justify, Length, Rect};

use super::components::UiSlot;

/// A theme text role, read when the text is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
  Body,
  Heading,
  Button,
  Caption,
  Mono,
}

#[derive(Debug, Clone)]
pub(crate) enum Kind {
  Column,
  Row,
  Grid(u16),
  Stack,
  Panel,
  Scroll(ScrollOpts),
  Popup { anchor: String },
  Text(TextOpts),
  Button(ButtonOpts),
  Image(ImageOpts),
  Checkbox,
  Toggle,
  RadioGroup { horizontal: bool },
  Slider(SliderOpts),
  Progress { label: Option<String> },
  TextInput(TextInputOpts),
  Dropdown,
  Tabs,
  ChoiceList { default: Option<usize> },
  Slot { filter: Option<Filter> },
  Prompt { action: String },
  Rebind(RebindOpts),
  Separator,
  Spacer,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScrollOpts {
  pub(crate) horizontal: bool,
  pub(crate) both: bool,
  pub(crate) no_bar: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TextOpts {
  pub(crate) markup: bool,
  pub(crate) style: Option<TextStyle>,
  pub(crate) role: Option<TextRole>,
  pub(crate) size: Option<f32>,
  pub(crate) color: Option<Color>,
  pub(crate) muted: bool,
  pub(crate) align: Option<TextAlign>,
  pub(crate) wrap: Option<f32>,
  pub(crate) max_lines: Option<u32>,
  pub(crate) ellipsis: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ButtonOpts {
  pub(crate) markup: bool,
  pub(crate) icon: Option<SpriteHandle>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ImageOpts {
  pub(crate) size: Option<Vec2>,
  pub(crate) tint: Option<Color>,
  pub(crate) filter: Option<Filter>,
  pub(crate) uv: Option<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub(crate) struct SliderOpts {
  pub(crate) range: (f32, f32),
  pub(crate) step: Option<f32>,
  pub(crate) show_value: bool,
  pub(crate) label: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TextInputOpts {
  pub(crate) placeholder: Option<String>,
  pub(crate) max_len: Option<usize>,
  pub(crate) password: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RebindOpts {
  pub(crate) action: String,
  pub(crate) label: Option<String>,
  pub(crate) slot: Option<usize>,
}

/// The layout options a node was given; `None` leaves the builder's default
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeLayout {
  pub(crate) width: Option<Length>,
  pub(crate) height: Option<Length>,
  pub(crate) min_width: Option<f32>,
  pub(crate) max_width: Option<f32>,
  pub(crate) min_height: Option<f32>,
  pub(crate) max_height: Option<f32>,
  pub(crate) padding: Option<Edges>,
  pub(crate) margin: Option<Edges>,
  pub(crate) gap: Option<f32>,
  /// The theme's `metrics.spacing` as the gap, read when drawn (a confirm)
  pub(crate) theme_gap: bool,
  pub(crate) align: Option<Align>,
  pub(crate) justify: Option<Justify>,
  pub(crate) grow: Option<f32>,
  pub(crate) wrap_items: bool,
  pub(crate) aspect: Option<f32>,
  pub(crate) tracks: Option<Vec<f32>>,
  pub(crate) anchor: Option<Anchor>,
  pub(crate) offset: Option<Vec2>,
  pub(crate) at: Option<Rect>,
  pub(crate) background: Option<Color>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NodeFlags {
  pub(crate) focus_scope: bool,
  pub(crate) trap: bool,
  pub(crate) wrap_focus: bool,
  pub(crate) block_pointer: bool,
  pub(crate) default_focus: bool,
  pub(crate) nav_skip: bool,
  pub(crate) focusable: Option<bool>,
  pub(crate) drag_source: bool,
  pub(crate) drop_target: bool,
  pub(crate) modal: bool,
  /// A confirm's No: Back answers as if it were clicked
  pub(crate) answers_back: bool,
}

/// The state components a node spawns with
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeState {
  pub(crate) label: Option<String>,
  pub(crate) image: Option<SpriteHandle>,
  pub(crate) checked: Option<bool>,
  pub(crate) value: Option<f32>,
  pub(crate) index: Option<usize>,
  pub(crate) options: Option<Vec<String>>,
  pub(crate) text: Option<String>,
  pub(crate) slot: Option<UiSlot>,
  pub(crate) reveal: Option<TextReveal>,
}

pub(crate) type Insert = Box<dyn FnOnce(&mut EntityWorldMut<'_>) + Send + Sync>;

/// A UI node and its children, described in the closure API's names.
///
/// Spawn a tree with `ctx.spawn_ui(tree)`: each node becomes an entity, its
/// data the state components it spawns with (`UiLabel`, `UiChecked`,
/// `UiValue`, ...), and its widget reports `UiEvent`s. A reusable component
/// is a function that returns a `UiNode`.
///
/// ```
/// use ryo2d::prelude::*;
///
/// let pause = UiNode::panel()
///   .focus_scope()
///   .anchor(Anchor::Center)
///   .width(260.0)
///   .gap(8.0)
///   .child(UiNode::button("Resume").action("resume").fill_width().default_focus())
///   .child(UiNode::button("Quit").action("quit").fill_width());
/// # let _ = pause;
/// ```
#[must_use = "spawn it with ctx.spawn_ui(..) or add it to a tree with .child(..)"]
pub struct UiNode {
  pub(crate) kind: Kind,
  pub(crate) layout: NodeLayout,
  pub(crate) flags: NodeFlags,
  pub(crate) variant: Option<String>,
  pub(crate) action: Option<String>,
  pub(crate) name: Option<String>,
  pub(crate) tooltip: Option<String>,
  pub(crate) hidden: bool,
  pub(crate) disabled: bool,
  pub(crate) embedded: bool,
  /// Neighbours by node name: up, down, left, right
  pub(crate) nav: [Option<String>; 4],
  pub(crate) state: NodeState,
  pub(crate) children: Vec<UiNode>,
  pub(crate) inserts: Vec<Insert>,
  /// Every option given, by method name, checked against the kind at spawn
  pub(crate) given: Vec<&'static str>,
}

impl fmt::Debug for UiNode {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("UiNode")
      .field("kind", &self.kind.name())
      .field("name", &self.name)
      .field("action", &self.action)
      .field("children", &self.children)
      .field("inserts", &self.inserts.len())
      .finish_non_exhaustive()
  }
}
```

Constructors. Each builds `UiNode::of(kind)` (a crate-private `fn of(kind: Kind) -> UiNode` with every other field at its default and empty lists) and sets its `state`:

| Constructor | `Kind` | `state` |
|---|---|---|
| `column()`, `row()`, `grid(columns: u16)`, `stack()`, `panel()` | `Column` ... `Panel` | none |
| `scroll()` | `Scroll(ScrollOpts::default())` | none |
| `popup(anchor: impl Into<String>)` | `Popup { anchor }` | none |
| `text(text: impl Into<String>)` | `Text(TextOpts::default())` | `label` |
| `button(label: impl Into<String>)` | `Button(ButtonOpts::default())` | `label` |
| `image(sprite: SpriteHandle)` | `Image(ImageOpts::default())` | `image` |
| `checkbox(label: impl Into<String>, checked: bool)`, `toggle(label, on: bool)` | `Checkbox`, `Toggle` | `label`, `checked` |
| `radio_group(options: &[&str], selected: usize)` | `RadioGroup { horizontal: false }` | `options`, `index` |
| `slider(range: RangeInclusive<f32>, value: f32)` | `Slider(SliderOpts { range: (*range.start(), *range.end()), step: None, show_value: false, label: None })` | `value` |
| `progress(fraction: f32)` | `Progress { label: None }` | `value` |
| `text_input(text: impl Into<String>)` | `TextInput(TextInputOpts::default())` | `text` |
| `dropdown(options: &[&str], selected: usize)`, `tabs(options, selected)` | `Dropdown`, `Tabs` | `options`, `index` |
| `choice_list(options: &[&str])` | `ChoiceList { default: None }` | `options`, `index: Some(0)` |
| `slot()` | `Slot { filter: None }` | `slot: Some(UiSlot::default())` |
| `prompt(action: &str)` | `Prompt { action }` | none |
| `rebind(action: &str)` | `Rebind(RebindOpts { action, label: None, slot: None })` | none |
| `separator()`, `spacer()` | `Separator`, `Spacer` | none |
| `confirm(title: &str, message: &str, yes: &str, no: &str)` | a tree, below | |

`confirm` builds what `ui.confirm_with` draws (`src/core/ui/widgets/modal.rs:89`), reading the theme at draw time:

```rust
  /// A yes/no dialog, as `ui.confirm_with` draws it: a modal panel with
  /// `title`, `message` (left out when empty) and buttons whose actions are
  /// `"yes"` and `"no"`. It opens on No, and Back answers No: a `Clicked`
  /// from the No button.
  pub fn confirm(title: &str, message: &str, yes: &str, no: &str) -> UiNode {
    let mut buttons = UiNode::row().justify(Justify::End).theme_gap();
    buttons = buttons.child(UiNode::button(yes).action("yes"));
    let mut no = UiNode::button(no).action("no").default_focus();
    no.flags.answers_back = true;
    buttons = buttons.child(no);
    let mut dialog = UiNode::panel().modal().theme_gap().child(UiNode::text(title).heading());
    if !message.is_empty() {
      dialog = dialog.child(UiNode::text(message).role(TextRole::Body).wrap(CONFIRM_MESSAGE_WIDTH));
    }
    dialog.child(buttons)
  }
```

`theme_gap()` is a crate-private method that sets `NodeLayout::theme_gap`; the walk then uses `ui.theme().metrics.spacing` as the gap. `CONFIRM_MESSAGE_WIDTH` is `modal.rs`'s `MESSAGE_WIDTH`, made `pub(crate)` and reused (not copied).

Option methods. Every method pushes its own name onto `given` (`self.given.push("gap")`) and then applies:

- **Shared layout** (into `layout`): `width(impl Into<Length>)`, `height(impl Into<Length>)`, `fill_width()`, `fill_height()`, `fill()`, `min_width(f32)`, `max_width(f32)`, `min_height(f32)`, `max_height(f32)`, `padding(impl Into<Edges>)`, `margin(impl Into<Edges>)`, `gap(f32)`, `align(Align)`, `justify(Justify)`, `center()` (align and justify `Center`), `grow(f32)`, `wrap_items()`, `aspect(f32)`, `tracks(&[f32])`, `anchor(Anchor)`, `offset(x: f32, y: f32)`, `at(Rect)`, `background(Color)`.
- **Focus and drag** (into `flags`): `focus_scope()`, `trap()` (scope and trap), `wrap_focus()` (scope and wrap), `block_pointer()`, `default_focus()`, `nav_skip()`, `focusable(bool)`, `drag_source()`, `drop_target()`, `modal()`; `nav_up(name)`, `nav_down(name)`, `nav_left(name)`, `nav_right(name)` (into `nav`, `impl Into<String>`).
- **Other shared:** `tooltip(impl Into<String>)`, `enabled(bool)` (`disabled = !enabled`), `variant(&str)`.
- **Node-only** (not pushed onto `given` unless noted): `action(impl Into<String>)`, `name(impl Into<String>)`, `hidden()`, `embedded()` (pushed), `insert(impl Bundle)`, `child(UiNode)` (pushes `"child"`), `children(impl IntoIterator<Item = UiNode>)` (pushes `"child"` when non-empty).
- **Kind options.** Each applies when `self.kind` is a kind that takes it, and otherwise does nothing (spawn warns, from `given`):
  - text: `markup()` (also button), `style(TextStyle)`, `role(TextRole)`, `heading()` (`role(Heading)`), `size(f32)`, `color(Color)`, `muted()`, `text_align(TextAlign)`, `wrap(f32)`, `max_lines(u32)`, `ellipsis()`, `typewriter(TextReveal)` (into `state.reveal`);
  - button: `icon(SpriteHandle)` (on a slot it sets `UiSlot::icon`);
  - image: `image_size(w: f32, h: f32)`, `tint(Color)`, `filter(Filter)` (also slot), `uv([f32; 4])`;
  - slider: `step(f32)`, `show_value()`, `label(impl Into<String>)` (also progress, rebind);
  - text field: `placeholder(impl Into<String>)`, `max_len(usize)`, `password()`;
  - radio group and scroll: `horizontal()`; scroll: `both()`, `no_bar()`;
  - choice list: `default_choice(usize)`;
  - slot: `count(u32)`, `selected(bool)`;
  - rebind: `binding_slot(usize)`.

`insert` boxes the bundle: `self.inserts.push(Box::new(move |entity: &mut EntityWorldMut<'_>| { entity.insert(bundle); }));`.

`Kind::takes(option)` is a `match` of the option name against the kind's class:

| Class | Kinds | Options it takes |
|---|---|---|
| Container | column, row, grid, stack, panel, popup | `width height fill_width fill_height fill min_width max_width min_height max_height padding margin gap align justify center grow wrap_items aspect anchor offset at background focus_scope trap wrap_focus block_pointer drag_source drop_target modal child embedded`, and `tracks` on a grid |
| Scroll | scroll | `width height fill_width fill_height fill min_width max_width min_height max_height padding margin gap grow anchor offset at drag_source drop_target horizontal both no_bar child embedded` |
| Radio group | radio_group | `width height fill_width min_width max_width margin gap align justify grow anchor offset at enabled horizontal embedded` |
| Widget | button, image, checkbox, toggle, slider, progress, text_input, dropdown, tabs, choice_list, slot, prompt, rebind | `width height fill_width fill_height fill margin anchor offset grow min_width max_width at default_focus nav_up nav_down nav_left nav_right nav_skip focusable variant tooltip enabled drag_source drop_target embedded`, plus its kind options above |
| Text | text | `embedded` and the text options |
| Bare | separator, spacer | `embedded` |

(`embedded` is checked again at spawn: on a child node it warns.)

**Components (`components.rs`).** Write them with these derives and docs:
- `UiRoot`: `Component, Debug, Clone`; the docs say the root's placement is the game's to change.
- `UiName`: `Component, Debug, Clone, PartialEq, Eq, Hash`.
- `UiLabel`, `UiTooltip`, `UiTextValue`, `UiOptions`: `Component, Debug, Clone, PartialEq, Eq, Default`.
- `UiChecked`, `UiIndex`: `Component, Debug, Clone, Copy, PartialEq, Eq, Default`.
- `UiValue`: `Component, Debug, Clone, Copy, PartialEq, Default`.
- `UiImage`: `Component, Debug, Clone`.
- `UiSlot`: `Component, Debug, Clone, Default`.
- `UiHidden`, `UiDisabled`: `Component, Debug, Clone, Copy, Default`.
- `UiInteraction`: `Component, Debug, Clone, Copy, Default, PartialEq, Eq`; `id()` is `(self.id != UiId::NONE).then_some(self.id)`.

**Events (`events.rs`).** `UiEvent` derives both `Message` and `EntityEvent`, with `#[event_target]` on `node`. `UiEventKind` as in Interfaces.

**Exports.** `src/core/ui/mod.rs`:

```rust
pub mod nodes;
pub use nodes::{
  TextRole, UiChecked, UiDisabled, UiEvent, UiEventKind, UiHidden, UiImage, UiIndex, UiInteraction,
  UiLabel, UiName, UiNode, UiNodeKind, UiOptions, UiRoot, UiSlot, UiTextValue, UiTooltip, UiValue,
};
```

and the same names in the prelude's `crate::core::ui::{..}` list in `src/lib.rs`. Inside the crate, `nodes/mod.rs` also has `pub(crate) use components::{BackAnswer, UiNodes};` (later tasks add their functions the same way), so tests reach everything with `use crate::core::ui::nodes::*;`.

- [ ] **Step 1: Write the failing tests**

In `src/core/ui/nodes/node.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_tree_keeps_its_children_in_order_and_its_options() {
    let tree = UiNode::column()
      .gap(8.0)
      .focus_scope()
      .child(UiNode::button("Resume").action("resume").default_focus())
      .child(UiNode::button("Quit").action("quit").name("quit"));
    assert_eq!(tree.kind.name(), "column");
    assert_eq!(tree.layout.gap, Some(8.0));
    assert!(tree.flags.focus_scope);
    let labels: Vec<_> = tree.children.iter().map(|c| c.state.label.clone()).collect();
    assert_eq!(labels, [Some("Resume".to_string()), Some("Quit".to_string())]);
    assert_eq!(tree.children[0].action.as_deref(), Some("resume"));
    assert!(tree.children[0].flags.default_focus);
    assert_eq!(tree.children[1].name.as_deref(), Some("quit"));
  }

  #[test]
  fn a_kind_option_on_another_kind_is_recorded_and_not_applied() {
    let node = UiNode::button("A").step(0.1).gap(4.0);
    assert!(node.given.contains(&"step"));
    assert!(!node.kind.takes("step"), "a button takes no step");
    assert!(!node.kind.takes("gap"), "nor a gap");
    let slider = UiNode::slider(0.0..=1.0, 0.5).step(0.1);
    assert!(matches!(slider.kind, Kind::Slider(SliderOpts { step: Some(s), .. }) if (s - 0.1).abs() < 1e-6));
  }

  #[test]
  fn constructors_spawn_their_state() {
    assert_eq!(UiNode::checkbox("Mute", true).state.checked, Some(true));
    assert_eq!(UiNode::slider(0.0..=2.0, 1.5).state.value, Some(1.5));
    let dropdown = UiNode::dropdown(&["A", "B"], 1);
    assert_eq!(dropdown.state.options, Some(vec!["A".to_string(), "B".to_string()]));
    assert_eq!(dropdown.state.index, Some(1));
    assert_eq!(UiNode::choice_list(&["X"]).state.index, Some(0));
    assert!(UiNode::slot().state.slot.is_some());
    assert_eq!(UiNode::text_input("hi").state.text.as_deref(), Some("hi"));
  }

  #[test]
  fn a_confirm_is_a_modal_panel_that_opens_on_no() {
    let confirm = UiNode::confirm("Quit?", "", "Yes", "No");
    assert!(confirm.flags.modal);
    assert_eq!(confirm.children.len(), 2, "no message, no message line");
    let buttons = &confirm.children[1].children;
    assert_eq!(buttons[0].action.as_deref(), Some("yes"));
    assert_eq!(buttons[1].action.as_deref(), Some("no"));
    assert!(buttons[1].flags.default_focus && buttons[1].flags.answers_back);
  }
}
```

In `src/core/ui/nodes/events.rs`:

```rust
#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use super::*;
  use crate::core::ecs::kernel::{On, World};
  use crate::core::messages::register_engine_messages;

  fn clicked(node: Entity) -> UiEvent {
    UiEvent { node, name: None, action: Some("play".into()), kind: UiEventKind::Clicked, scene: 1 }
  }

  #[test]
  fn ui_events_are_engine_messages() {
    let mut world = World::new();
    register_engine_messages(&mut world);
    assert!(world.contains_resource::<crate::core::ecs::kernel::Messages<UiEvent>>());
  }

  #[test]
  fn a_ui_event_reaches_an_observer_on_its_node() {
    let mut world = World::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let node = world.spawn_empty().id();
    let other = world.spawn_empty().id();
    let log = Arc::clone(&seen);
    world.entity_mut(node).observe(move |event: On<UiEvent>| {
      if let Ok(mut log) = log.lock() {
        log.push(event.action.clone());
      }
    });
    world.trigger(clicked(other));
    world.trigger(clicked(node));
    assert_eq!(*seen.lock().unwrap(), [Some("play".to_string())], "only its own node's");
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes`
Expected: compile errors (the module does not exist).

- [ ] **Step 3: Implement** `node.rs`, `components.rs`, `events.rs` and `mod.rs` as above; add the exports and the `register_engine_messages` line.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::nodes` and `cargo test --workspace`
Expected: pass (the doc test on `UiNode` included).

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/mod.rs src/core/ui/nodes/node.rs src/core/ui/nodes/components.rs src/core/ui/nodes/events.rs src/core/ui/mod.rs src/core/ui/widgets/modal.rs src/lib.rs src/core/messages/mod.rs
git commit -m "feat: the UI node tree value, its components and events" -m "- UiNode builds a tree in the closure API's names; UiNode::confirm builds a yes/no dialog
- public state components, markers, UiInteraction and UiTooltip
- UiEvent is an engine message and an entity event"
```

### Task 2: Spawning, finding and the harness's world

**Files:**
- Create: `src/core/ui/nodes/spawn.rs`
- Modify:
  - `src/core/ui/nodes/mod.rs`: `pub(crate) use spawn::*`.
  - `src/core/scene/context.rs`: `spawn_ui`, `spawn_ui_in`, `find_ui`, `find_ui_in`.
  - `src/core/testing/ui.rs`: the harness's world.
- Test: `src/core/ui/nodes/spawn.rs` (tests)

**Interfaces:**
- Consumes: Task 1's `UiNode` (and its `pub(crate)` fields), `Kind::{takes, name, is_container}`, the components, `UiNodes`, `BackAnswer`.
- Produces:

```rust
// spawn.rs
pub(crate) fn spawn_ui(world: &mut World, scene: u64, tree: UiNode) -> Entity;
pub(crate) fn spawn_ui_in(world: &mut World, parent: Entity, tree: UiNode) -> Option<Entity>;  // None: parent is not a node
pub(crate) fn find_ui(world: &World, scene: u64, name: &str) -> Option<Entity>;
pub(crate) fn find_ui_in(world: &World, root: Entity, name: &str) -> Option<Entity>;
pub(crate) fn root_of(world: &World, node: Entity) -> Option<Entity>;   // up ChildOf to the UiRoot
pub(crate) fn despawn_scene_roots(world: &mut World, scene: u64);
// SceneContext
pub fn spawn_ui(&mut self, tree: UiNode) -> Entity;
pub fn spawn_ui_in(&mut self, parent: Entity, tree: UiNode) -> Option<Entity>;
pub fn find_ui(&self, name: &str) -> Option<Entity>;
pub fn find_ui_in(&self, root: Entity, name: &str) -> Option<Entity>;
// UiHarness
pub fn spawn_ui(&mut self, tree: UiNode) -> Entity;    // owned by the harness's scene
pub fn world(&self) -> &World;
pub fn world_mut(&mut self) -> &mut World;
pub fn find_ui(&self, name: &str) -> Option<Entity>;
```

**The rules:**
- **Spawning a tree.** Depth first, parent before children, children in the order given:
  1. Spawn the node with `UiNodeKind` (from the node's kind, layout, flags, variant and action; `nav: [None; 4]`, `anchor_node: None`), `UiInteraction::default()`, its state components, `UiName` if named, `UiTooltip` if given, `UiHidden` if `hidden`, `UiDisabled` if `!enabled`, `TextReveal` if `typewriter`, and `ChildOf(parent)` below the root.
  2. Run its `inserts` on its `EntityWorldMut`, after the engine's components (a game component may replace a state component on purpose).
  3. Check `given`: each option the kind does not take warns once, keyed `"node-option:{kind}:{option}"`, message ``UiNode::{option} does nothing on a {kind} node``. `embedded` on a node that is not the tree's root warns ``UiNode::embedded does nothing on a child node``.
- **The root.** `spawn_ui` inserts `UiRoot { anchor: layout.anchor.unwrap_or(Anchor::TopLeft), offset: layout.offset.unwrap_or(Vec2::ZERO), at: layout.at, scene, seq, embedded }` on the tree's root and pushes `(scene, root)` onto `UiNodes::roots` (inserting the resource with `get_resource_or_insert_with(UiNodes::default)`). `seq` is `UiNodes::seq`, then incremented. `UiRoot` has crate-private fields, so `spawn_ui` is the only way to make a root.
- **`spawn_ui_in(parent, tree)`** spawns the tree under `parent`, appended after its last child, with no `UiRoot` on its top node. It returns `None` and warns once if `parent` has no `UiNodeKind` or is not a container.
- **Names.** After a tree is spawned, its neighbour names (`UiNode::nav`) and popup anchors are resolved with `find_ui_in(root_of(world, top), name)`: within the whole root tree, so a subtree added later can name nodes already there. The results go into `UiNodeKind::nav` and `UiNodeKind::anchor_node`. A name that finds nothing warns once (``no UI node named "{name}" for nav_down`` / ``for popup``) and stays `None`. A name used by two nodes of one tree warns once (``two UI nodes are named "{name}"; find_ui finds the first``).
- **Confirms.** For each node with `flags.modal`, the first descendant with `flags.answers_back` is found after spawning, and `BackAnswer(that)` is inserted on the modal node.
- **Finding.** `find_ui_in(root, name)` searches depth first from `root` (included) through `Children`, in child order, and returns the first node whose `UiName` matches. `find_ui(world, scene, name)` searches each of the scene's roots in spawn order, embedded ones included.
- **Despawning.** `despawn_scene_roots(world, scene)` removes the scene's entries from `UiNodes::roots` and despawns each root that still exists (`world.despawn`, recursive through `Children`). If a root was despawned already, its entry is dropped.
- **The harness.** `UiHarness` gains `world: World`, created in `new` with `register_message::<UiEvent>(&mut world)`. `spawn_ui(tree)` is `nodes::spawn_ui(&mut self.world, self.scene, tree)`. `find_ui(name)` is `nodes::find_ui(&self.world, self.scene, name)`. (Drawing comes in Task 3.)
- **`SceneContext`** forwards to the same functions with `self.world` and its `scope_id`.

- [ ] **Step 1: Write the failing tests**

In `src/core/ui/nodes/spawn.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{ChildOf, Children, Component};
  use crate::core::testing::UiHarness;
  use crate::core::ui::{Anchor, UiNode};
  use crate::core::ui::nodes::*;

  #[derive(Component, Debug, PartialEq)]
  struct SlotIndex(usize);

  fn children(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn warnings(needle: &str) -> usize {
    crate::core::dev_log::get_entries().iter().filter(|w| w.message.contains(needle)).count()
  }

  #[test]
  fn a_tree_spawns_an_entity_per_node_in_order() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("A"))
        .child(UiNode::row().child(UiNode::text("B")))
        .child(UiNode::checkbox("C", true)),
    );
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(kids.len(), 3);
    assert_eq!(w.get::<UiLabel>(kids[0]), Some(&UiLabel("A".into())));
    assert_eq!(w.get::<ChildOf>(kids[0]).map(|c| c.parent()), Some(root));
    assert_eq!(w.get::<UiLabel>(children(w, kids[1])[0]), Some(&UiLabel("B".into())));
    assert_eq!(w.get::<UiChecked>(kids[2]), Some(&UiChecked(true)));
    for node in [root, kids[0], kids[1], kids[2]] {
      assert!(w.get::<UiNodeKind>(node).is_some() && w.get::<UiInteraction>(node).is_some());
    }
    let ui_root = w.get::<UiRoot>(root).unwrap();
    assert_eq!((ui_root.anchor, ui_root.scene), (Anchor::TopLeft, 1));
    assert!(w.get::<UiRoot>(kids[0]).is_none());
  }

  #[test]
  fn markers_tooltips_and_inserts_land_on_their_node() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::grid(2)
        .child(UiNode::slot().tooltip("Sword").insert(SlotIndex(3)).name("first"))
        .child(UiNode::button("Hidden").hidden())
        .child(UiNode::button("Off").enabled(false)),
    );
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(w.get::<SlotIndex>(kids[0]), Some(&SlotIndex(3)));
    assert_eq!(w.get::<UiTooltip>(kids[0]), Some(&UiTooltip("Sword".into())));
    assert_eq!(w.get::<UiName>(kids[0]), Some(&UiName("first".into())));
    assert!(w.get::<UiHidden>(kids[1]).is_some());
    assert!(w.get::<UiDisabled>(kids[2]).is_some());
    assert_eq!(h.find_ui("first"), Some(kids[0]));
    assert_eq!(h.find_ui("nobody"), None);
  }

  #[test]
  fn find_ui_in_searches_one_tree() {
    let mut h = UiHarness::new(320.0, 240.0);
    let a = h.spawn_ui(UiNode::column().child(UiNode::button("A").name("ok")));
    let b = h.spawn_ui(UiNode::column().child(UiNode::button("B").name("ok")));
    let w = h.world();
    assert_eq!(find_ui_in(w, b, "ok"), Some(children(w, b)[0]));
    assert_eq!(h.find_ui("ok"), Some(children(w, a)[0]), "the first root first");
  }

  #[test]
  fn neighbour_names_resolve_within_the_root() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::button("Play").nav_up("quit"))
        .child(UiNode::button("Quit").name("quit")),
    );
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(w.get::<UiNodeKind>(kids[0]).unwrap().nav[0], Some(kids[1]));
  }

  #[test]
  fn unknown_neighbour_names_warn_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    for _ in 0..2 {
      h.spawn_ui(UiNode::column().child(UiNode::button("Play").nav_down("nobody-4c-test")));
    }
    assert_eq!(warnings("nobody-4c-test"), 1);
  }

  #[test]
  fn a_name_used_twice_warns_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(UiNode::column().child(UiNode::button("A").name("twice-4c")).child(UiNode::button("B").name("twice-4c")));
    assert_eq!(warnings("two UI nodes are named \"twice-4c\""), 1);
    assert_eq!(h.find_ui("twice-4c"), Some(children(h.world(), root)[0]), "the first");
  }

  #[test]
  fn an_option_the_kind_ignores_warns_once() {
    let mut h = UiHarness::new(320.0, 240.0);
    for _ in 0..2 {
      h.spawn_ui(UiNode::progress(0.5).password());
    }
    assert_eq!(warnings("UiNode::password does nothing on a progress node"), 1);
  }

  #[test]
  fn spawn_ui_in_appends_and_resolves_names() {
    let mut h = UiHarness::new(320.0, 240.0);
    let root = h.spawn_ui(UiNode::column().child(UiNode::button("Top").name("top")));
    let choices = UiNode::column().child(UiNode::button("Back up").nav_up("top"));
    let added = spawn_ui_in(h.world_mut(), root, choices).expect("a column takes children");
    let w = h.world();
    let kids = children(w, root);
    assert_eq!(kids.last(), Some(&added), "appended last");
    let button = children(w, added)[0];
    assert_eq!(w.get::<UiNodeKind>(button).unwrap().nav[0], Some(kids[0]));
    assert!(w.get::<UiRoot>(added).is_none());
    let leaf = kids[0];
    assert!(spawn_ui_in(h.world_mut(), leaf, UiNode::text("no")).is_none(), "a button holds no children");
  }

  #[test]
  fn roots_keep_spawn_order_and_leave_with_their_scene() {
    let mut h = UiHarness::new(320.0, 240.0);
    let a = h.spawn_ui(UiNode::column());
    let b = h.spawn_ui(UiNode::column().child(UiNode::text("x")));
    let w = h.world();
    assert!(w.get::<UiRoot>(a).unwrap().seq < w.get::<UiRoot>(b).unwrap().seq);
    let kid = children(w, b)[0];
    despawn_scene_roots(h.world_mut(), 1);
    let w = h.world();
    assert!(w.get_entity(a).is_err() && w.get_entity(b).is_err() && w.get_entity(kid).is_err());
    assert!(w.resource::<UiNodes>().roots.is_empty());
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::spawn`
Expected: compile errors.

- [ ] **Step 3: Implement** `spawn.rs`, the `SceneContext` methods and the harness's world by the rules above. Doc every public method (one line plus an example on `SceneContext::spawn_ui`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::nodes` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/spawn.rs src/core/ui/nodes/mod.rs src/core/scene/context.rs src/core/testing/ui.rs
git commit -m "feat: spawn and find UI node trees" -m "- ctx.spawn_ui and spawn_ui_in spawn an entity per node, in order, with their state
- neighbour and popup names resolve within the tree; ignored options warn once
- find_ui and find_ui_in by name; UiHarness gains a world"
```

### Task 3: The walk: containers, text, buttons, images, separators, focus options, hidden and disabled, backgrounds

**Files:**
- Create: `src/core/ui/nodes/draw.rs`
- Modify:
  - `src/core/ui/containers.rs`: `Container::background(Color)`.
  - `src/core/ui/nodes/mod.rs`: `pub(crate) use draw::{begin_pass, draw_nodes};`.
  - `src/core/testing/ui.rs`: `frame` draws the harness scene's trees; `frame_with_world`.
- Test: `src/core/ui/nodes/draw.rs`, `src/core/ui/containers.rs` (tests)

**Interfaces:**
- Consumes: Tasks 1-2. 4b's builders: `ui.column()`, `row()`, `grid(n)`, `stack()`, `panel()`, `scroll()` and their options; `ui.text(..)` (`UiText`); `ui.button(..)`, `ui.image(&sprite)`, `ui.separator()`, `ui.spacer()`; `widget_builders!` options; `ui.scope(key, ..)`; `ui.theme()`.
- Produces:

```rust
// draw.rs
pub(crate) struct Walk {
  pub(crate) scene: u64,
  pub(crate) events: Vec<UiEvent>,
  pub(crate) back: Option<Entity>,          // Task 9: the last modal or popup node drawn
  pub(crate) pending_focus: Option<Entity>, // Task 4
}
impl Walk {
  /// Back (Task 9), the unused focus request back into `UiNodes` (Task 4); the events
  pub(crate) fn finish(self, world: &mut World, ui: &mut UiContext<'_>) -> Vec<UiEvent>;
}
pub(crate) fn begin_pass(world: &mut World);
pub(crate) fn draw_nodes(world: &mut World, ui: &mut UiContext<'_>, scene: u64) -> Vec<UiEvent>;
fn draw_node(world: &mut World, ui: &mut UiContext<'_>, node: Entity, place: Option<Place>, walk: &mut Walk);
fn walk_children(world: &mut World, ui: &mut UiContext<'_>, parent: Entity, walk: &mut Walk);
#[derive(Clone, Copy)] struct Place { anchor: Anchor, offset: Vec2, at: Option<Rect> }   // a root's, from UiRoot
// containers.rs (public, both styles)
impl Container<'_, '_> { pub fn background(self, color: Color) -> Self; }
// UiHarness
pub fn frame_with_world<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>, &mut World) -> R) -> R;
```

**The walk** (write `draw_nodes` as below, then the per-kind drawing by the rules):

```rust
/// Start a UI pass: embedded trees may be drawn again
pub(crate) fn begin_pass(world: &mut World) {
  if let Some(mut nodes) = world.get_resource_mut::<UiNodes>() {
    nodes.embedded_drawn.clear();
  }
}

/// Draw `scene`'s node trees through the builders, in spawn order (a later
/// root over an earlier one), and return their events in walk order
pub(crate) fn draw_nodes(world: &mut World, ui: &mut UiContext<'_>, scene: u64) -> Vec<UiEvent> {
  if !world.contains_resource::<UiNodes>() {
    return Vec::new();
  }
  let pending_focus = world.resource_scope(|world, mut nodes: Mut<UiNodes>| {
    nodes.roots.retain(|&(_, root)| world.get::<UiRoot>(root).is_some());
    nodes.pending_focus.take()
  });
  let mut walk = Walk { scene, events: Vec::new(), back: None, pending_focus };
  let mut i = 0;
  while let Some(&(owner, root)) = world.resource::<UiNodes>().roots.get(i) {
    i += 1;
    let Some(placed) = world.get::<UiRoot>(root) else { continue };
    if owner != scene || placed.embedded {
      continue;
    }
    let place = Place { anchor: placed.anchor, offset: placed.offset, at: placed.at };
    draw_node(world, ui, root, Some(place), &mut walk);
  }
  walk.finish(world, ui)
}
```

`Walk::finish` writes `pending_focus` back into `UiNodes` (Task 4 fills in the rest) and returns the events.

**The rules:**
- **A node.** `draw_node` looks the entity up once (`world.get_entity`):
  - gone: return;
  - `UiHidden`: return, recording nothing for the node or its subtree;
  - no `UiNodeKind`: warn once (``an entity under a UI node is not a node; it is skipped``) and return;
  - otherwise draw it by its kind.
- **Ids.** Every builder that has `.id` gets `.id(entity)` (the `Entity` is the key). Text, separators and spacers are drawn inside `ui.scope(entity, |ui| ..)`. Both are called from one line each in `draw.rs`, so ids depend only on the entity and its parent's id.
- **Containers** (column, row, grid, stack, panel, scroll; popups and modals in Task 9). Read `UiNodeKind` and build the configured builder while holding only shared borrows of the world. Then call `.show(|ui| walk_children(world, ui, entity, walk))`, so the world is free again while the children are drawn.
  - Apply each `Some` in `NodeLayout` with the builder method of the same name, in this order: `width`, `height`, `min_width`, `max_width`, `min_height`, `max_height`, `padding`, `margin`, `gap` (or the theme's spacing for `theme_gap`), `align`, `justify`, `grow`, `wrap_items`, `aspect`, `tracks`, `anchor`, `offset`, `at`, `background`. Then apply the flags: `focus_scope`, `trap`, `wrap_focus`, `block_pointer`.
  - A root's `Place` replaces the layout's `anchor`, `offset` and `at`.
  - `scroll()` applies `horizontal()`, `both()` and `no_bar()` from its `ScrollOpts`, and only the options `Scroll` has.
  - A container with `UiDisabled` warns once (``UiDisabled does nothing on a {kind} node``) and draws normally.
- **Widgets** (button, image here; the rest in Tasks 6-8). Take what the widget needs from one `get_components_mut` tuple on `world.entity_mut(entity)`, for example `(&UiNodeKind, &UiLabel, Option<&UiTooltip>, Has<UiDisabled>)` for a button. Call the builder, then apply the shared widget options with one macro, `widget_opts!(builder, node, extras)`, used by every widget kind:
  - `width`, `height`, `margin`, `anchor`, `offset`, `grow`, `min_width`, `max_width`, `at` (from `NodeLayout`, or the root's `Place`);
  - `default_focus`, `nav_skip`, `focusable(on)`;
  - `variant(name)`, `tooltip(&tooltip.0)`, and `enabled(false)` under `UiDisabled`;
  - the explicit neighbours, which come in Task 9.
  Then `.show()` and keep the `Response` (Task 4 reads it). If a required state component is missing (a button without `UiLabel`, an image without `UiImage`), warn once (``a {kind} node has no {component}; it is not drawn``) and skip the node.
- **Buttons.** `ui.button(label)`, where the label is `RichText::markup(&label.0)` under `markup()`, else `label.0.as_str()`. Then `.icon(&sprite)` when the kind has an icon.
- **Images.** `ui.image(&image.0)`, then `.size(w, h)`, `.tint(c)`, `.filter(f)` and `.uv(uv)` from `ImageOpts`.
- **Text.** Copy the node's `TextReveal` out if it has one. Inside `ui.scope(entity, ..)`, compute the role's style and the muted colour from `ui.theme()` before calling `ui.text(..)`, since the text builder borrows the context. Then apply, in this order:
  1. `.style(&style)` (an explicit `style` wins over a `role`);
  2. `.size(s)`;
  3. `.color(muted)` under `muted()`;
  4. `.color(c)`;
  5. `.align(a)`, `.wrap(w)`, `.max_lines(n)`, `.ellipsis()`;
  6. `.typewriter(&mut reveal)`;
  7. `.draw()`.
  Write `reveal` back only if it differs from the component. The role's style is `theme.text.{body, heading, button, caption, mono}.style()`. A text node has no response.
- **Separators and spacers.** `ui.scope(entity, |ui| ui.separator())`, and likewise `ui.spacer()`.
- **The background option** (`containers.rs`, public). `Container::background(color)` keeps its colour in a new field. When the container is recorded it draws `Look { fill: color, ..Look::default() }` with `push_look_box(node, &look)` behind its children, after a panel's own look if it has one. Doc: "Fill the container's rect with `color` behind its children."
- **The harness.** `frame_with_world(f)` does what `frame` did, and in place of `ui(&mut context)`: `begin_pass(&mut self.world)`, `draw_nodes(&mut self.world, &mut context, self.scene)` (Task 4 delivers the events), then `f(&mut context, &mut self.world)`. `frame(f)` is `self.frame_with_world(|ui, _| f(ui))`. Document both: the harness scene's node trees draw before the closure, as a scene's do before its `ui`.

- [ ] **Step 1: Write the failing tests**

In `src/core/ui/containers.rs`'s tests:

```rust
  #[test]
  fn a_background_fills_the_container_behind_its_children() {
    use crate::core::ui::immediate::UiCommand;
    let mut d = UiDriver::new((200.0, 200.0));
    let rect = Rect::new(10.0, 20.0, 50.0, 40.0);
    let color = Color::rgb(1, 2, 3);
    d.frame(|ui| ui.stack().at(rect).background(color).show(|ui| { ui.text("on top"); }));
    let fills: Vec<_> = d
      .commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Rect { rect, fill, .. } => Some((*rect, *fill)),
        _ => None,
      })
      .collect();
    assert_eq!(fills.first(), Some(&(rect, color)), "the fill comes first, under the text");
  }
```

In `src/core/ui/nodes/draw.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ecs::kernel::{ChildOf, Children, Entity, World};
  use crate::core::testing::UiHarness;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::{Align, Anchor, Rect, UiContext};

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn targets(h: &UiHarness) -> Vec<Rect> {
    nav_targets(&h.memory, Some(h.scene)).into_iter().map(|(_, r)| r).collect()
  }

  fn warnings(needle: &str) -> usize {
    crate::core::dev_log::get_entries().iter().filter(|w| w.message.contains(needle)).count()
  }

  /// A screen of every container and the simple widgets, as a closure
  fn closure_screen(ui: &mut UiContext<'_>) {
    let heading = ui.theme().text.heading.style();
    ui.panel().focus_scope().anchor(Anchor::Center).width(260.0).gap(8.0).show(|ui| {
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
      .child(UiNode::grid(3).gap(4.0).children((0..6).map(|i| UiNode::button(format!("{i}")))))
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
    assert_eq!(targets(&nodes), targets(&closure), "the same focusables at the same rects");
    assert_eq!(nodes.command_count(), closure.command_count(), "the same draws");
    assert!(nodes.focused().is_some(), "the scope took its default focus");
  }

  #[test]
  fn a_hidden_node_records_nothing_and_leaves_the_focus_chain() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column().focus_scope().child(UiNode::button("A")).child(UiNode::column().child(UiNode::button("B"))),
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
    let root = h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("A")).child(UiNode::button("B")));
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
    h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("Elsewhere")));
    h.scene = 1;
    h.frame(|_| {});
    h.frame(|_| {});
    assert!(targets(&h).is_empty());
    assert_eq!(h.command_count(), 0);
  }

  #[test]
  fn a_root_is_placed_by_its_ui_root() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("A").width(100.0)));
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
    let root = h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("A")).child(UiNode::button("B")));
    let a = kids(h.world(), root)[0];
    h.world_mut().entity_mut(a).remove::<UiLabel>();
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(targets(&h).len(), 1);
    assert_eq!(warnings("a button node has no UiLabel"), 1);
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
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw ui::containers`
Expected: compile errors (`draw_nodes`, `background` missing).

- [ ] **Step 3: Implement** `Container::background`, `draw.rs` and the harness changes by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass. If `a_node_screen_records_what_the_closure_screen_records` differs, the walk is calling an option the closure does not, or in another order. Compare `nav_targets` one by one, and fix the walk, not the test.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs src/core/ui/nodes/mod.rs src/core/ui/containers.rs src/core/testing/ui.rs
git commit -m "feat: draw UI node trees through the builders" -m "- containers, text, buttons, images, separators and spacers, keyed by entity
- hidden nodes record nothing; disabled widgets are drawn and not focusable
- containers can fill their rect with a background colour"
```

### Task 4: Events out, interaction state, `ui_id` and `focus_ui`

**Files:**
- Modify:
  - `src/core/ui/nodes/draw.rs`: responses into `UiInteraction` and events; pending focus.
  - `src/core/ui/nodes/events.rs`: `deliver`.
  - `src/core/scene/context.rs`: `ui_id`, `focus_ui`.
  - `src/core/testing/ui.rs`: `events`, `ui_id`; delivery in `frame_with_world`.
- Test: `src/core/ui/nodes/draw.rs` (tests)

**Interfaces:**
- Consumes: Task 3's walk, `Response::{id, hovered, focused, clicked, secondary_clicked, gained_focus, lost_focus}`, `UiContext::request_focus`.
- Produces:

```rust
// draw.rs
fn answer(world: &mut World, ui: &mut UiContext<'_>, node: Entity, response: &Response, sends: Sends, walk: &mut Walk);
#[derive(Clone, Copy)] enum Sends { Clicks, Changes }   // which of Clicked / Changed the kind sends
// events.rs
pub(crate) fn deliver(world: &mut World, events: Vec<UiEvent>);
// SceneContext
pub fn ui_id(&self, node: Entity) -> Option<UiId>;
pub fn focus_ui(&mut self, node: Entity);
// UiHarness
pub fn events(&self) -> &[UiEvent];          // the last frame's, in walk order
pub fn ui_id(&self, node: Entity) -> Option<UiId>;
```

**The rules:**
- **`answer`** runs after every widget's and container's `show()` (not for text, separators or spacers):
  1. Set the node's `UiInteraction` to `{ id: response.id, hovered: response.hovered(), focused: response.focused() }`, writing only if it differs.
  2. If `walk.pending_focus == Some(node)`, call `ui.request_focus(response.id)` and clear it.
  3. Push events in this order, each built with the node's `UiName` and action, cloned only when an event is pushed, and `scene: walk.scene`:
     - `Clicked` if `sends` is `Clicks` and `response.clicked()`; `Changed` if `sends` is `Changes` and `response.changed()`;
     - `Submitted` (Task 6) and `Dropped` (Task 8);
     - `SecondaryClicked` if `response.secondary_clicked()`;
     - `FocusLost` if `response.lost_focus()`, then `FocusGained` if `response.gained_focus()`.
  Buttons, images and containers send `Clicks`.
- **`Walk::finish`** puts any unused `pending_focus` back into `UiNodes::pending_focus` (it waits for its node to be drawn), then returns the events.
- **`deliver(world, events)`** does nothing for no events. Otherwise it calls `register_message::<UiEvent>(world)`, then for each event in order writes it to `Messages<UiEvent>` and, if its node still exists, `world.trigger(event)`. An observer that despawns a later event's node therefore skips that event's trigger, but not its message.
- **`ctx.ui_id(node)`** is `world.get::<UiInteraction>(node).and_then(UiInteraction::id)`. **`ctx.focus_ui(node)`** sets `UiNodes::pending_focus = Some(node)`, inserting the resource if needed.
- **The harness.** `frame_with_world` keeps `last_events: Vec<UiEvent>`: a clone of the events `draw_nodes` returned (plus the embedded trees' events, from Task 10), set before `deliver(&mut self.world, events)`. At the start of each frame it calls `update()` on `Messages<UiEvent>` if registered, so the world's buffer does not grow. `events()` returns `&self.last_events`.

- [ ] **Step 1: Write the failing tests**

Add these inside Task 3's `tests` module in `draw.rs` (they share its helpers):

```rust
  use std::sync::{Arc, Mutex};
  use crate::core::ecs::kernel::On;

  /// A column of Play (the default) and Quit, named and with actions
  fn menu() -> UiNode {
    UiNode::column()
      .focus_scope()
      .child(UiNode::button("Play").action("play").name("play").default_focus().width(100.0))
      .child(UiNode::button("Quit").action("quit").name("quit").width(100.0))
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
    let clicks: Vec<_> = h.events().iter().filter(|e| e.kind == UiEventKind::Clicked).cloned().collect();
    assert_eq!(clicks.len(), 1);
    assert_eq!((clicks[0].node, clicks[0].action.as_deref(), clicks[0].name.as_deref()), (quit, Some("quit"), Some("quit")));
    h.frame(|_| {});
    assert!(h.events().iter().all(|e| e.kind != UiEventKind::Clicked), "once");
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
    assert_eq!(focus, [(play, UiEventKind::FocusLost), (quit, UiEventKind::FocusGained)]);
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
    h.world_mut().entity_mut(play).observe(move |event: On<UiEvent>| {
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
    let clicked: Vec<_> = h.events().iter().filter(|e| e.kind == UiEventKind::Clicked).filter_map(|e| e.action.clone()).collect();
    assert_eq!(clicked, ["over"]);
  }

  #[test]
  fn despawning_the_focused_node_moves_focus_to_the_default() {
    let mut h = UiHarness::new(300.0, 300.0);
    let root = h.spawn_ui(menu());
    h.frame(|_| {});
    h.frame(|_| {});
    let play_id = h.focused();
    h.action("ui.down");
    h.frame(|_| {});
    let quit = kids(h.world(), root)[1];
    h.world_mut().despawn(quit);
    for _ in 0..2 {
      h.frame(|_| {});
    }
    assert_eq!(h.focused(), play_id);
  }

  #[test]
  fn a_new_node_in_a_despawned_ones_place_starts_fresh() {
    let mut h = UiHarness::new(300.0, 300.0);
    let rows = || UiNode::scroll().height(60.0).width(120.0).children((0..20).map(|i| UiNode::button(format!("{i}")).fill_width()));
    let first = h.spawn_ui(rows());
    h.frame(|_| {});
    h.move_to(20.0, 20.0);
    h.wheel(0.0, -3.0);
    for _ in 0..30 {
      h.frame(|_| {});
    }
    let id = h.ui_id(first).unwrap();
    assert!(h.frame(|ui| ui.scroll_offset(id)).y > 0.0, "the wheel scrolled it");
    h.world_mut().despawn(first);
    let second = h.spawn_ui(rows());
    h.frame(|_| {});
    h.frame(|_| {});
    let id = h.ui_id(second).unwrap();
    assert_eq!(h.frame(|ui| ui.scroll_offset(id)).y, 0.0);
  }
```

(If the wheel's sign scrolls the other way in 4b's harness, flip `-3.0`; the test is about the fresh offset.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw`
Expected: compile errors (`events`, `ui_id`).

- [ ] **Step 3: Implement** by the rules above, then add `ui_id` and `focus_ui` to `SceneContext` with docs.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs src/core/ui/nodes/events.rs src/core/scene/context.rs src/core/testing/ui.rs
git commit -m "feat: UI node events, interaction state and focus" -m "- clicks and focus changes send UiEvents, written as messages and triggered on the node
- UiInteraction holds a node's id, hover and focus; ctx.ui_id and ctx.focus_ui"
```

### Task 5: Engine integration: the UI pass, scene ownership, `ctx.ui_events()`

**Files:**
- Modify:
  - `src/core/scene/manager.rs`: `ui_pass`.
  - `src/core/engine/frame.rs`: `draw_ui`'s exit drain.
  - `src/core/scene/context.rs`: `ui_events`.
- Create: `tests/e2e/ui_nodes.rs`; add `#[path = "e2e/ui_nodes.rs"] mod ui_nodes;` to `tests/e2e.rs`.
- Test: `tests/e2e/ui_nodes.rs`

**Interfaces:**
- Consumes: `begin_pass`, `draw_nodes`, `deliver`, `despawn_scene_roots`; `SceneManager::ui_pass` (`src/core/scene/manager.rs:504`); `Engine::draw_ui` (`src/core/engine/frame.rs:535`).
- Produces:

```rust
// SceneContext
/// The UI events of this scene's own nodes since it last read them (shares
/// `messages::<UiEvent>()`'s cursor: read one or the other)
pub fn ui_events(&mut self) -> Vec<UiEvent>;   // self.messages::<UiEvent>() filtered by event.scene == self.scope_id
```

**The rules:**
- **`ui_pass`.** At its start (after the empty check), `crate::core::ui::nodes::begin_pass(world)`. For each drawn scene:

```rust
      ui.begin_scene(slot.scope_id, i == top);
      // The scene's node trees draw first, so its `ui` draws above them and
      // reads their events this frame
      let events = crate::core::ui::nodes::draw_nodes(world, ui, slot.scope_id);
      crate::core::ui::nodes::deliver(world, events);
      let result = slot.call(world, audio, |scene, ctx| scene.ui(ui, ctx));
      ui.end_scene();
```

  (Task 10 adds `deliver_pending` after `slot.call`.) `draw_nodes` runs with the whole world, before `slot.call` lends `Time`, `Input` and `AssetManager` to the scene, so observers see those resources.
- **Exit.** In `draw_ui`, the loop over `take_exited()` also calls `crate::core::ui::nodes::despawn_scene_roots(&mut self.world, scope)` for each scope.
- **The rerun pass.** No extra code. A lower scene's widgets answer nothing and a popped scene is gone, so a click that changes the top scene is delivered by the first pass only. Review Focus 4 pins this with a test.

- [ ] **Step 1: Write the failing tests**

`tests/e2e/ui_nodes.rs`:

```rust
//! Node trees in the engine: scene ownership, input only for the top scene,
//! events read in the same frame, observers

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[derive(Default)]
struct Log {
  /// In order: "frame" at each of Game's `ui` calls, "observed play" from
  /// the observer on Play, "update X" / "ui X" when Game reads a click on X
  trace: Vec<String>,
  game_root: Option<Entity>,
  paused_root: Option<Entity>,
}

fn note(log: &Arc<Mutex<Log>>, what: impl Into<String>) {
  if let Ok(mut log) = log.lock() {
    log.trace.push(what.into());
  }
}

fn reads(log: &Arc<Mutex<Log>>, prefix: &str) -> Vec<String> {
  log.lock().map(|l| l.trace.iter().filter(|t| t.starts_with(prefix)).cloned().collect()).unwrap_or_default()
}

/// The actions of this scene's own clicks since it last read
fn clicks(ctx: &mut SceneContext) -> Vec<String> {
  ctx.ui_events().into_iter().filter(|e| e.kind == UiEventKind::Clicked).filter_map(|e| e.action).collect()
}

/// Play (the default) and Pause, spawned on enter, with an observer on Play.
/// It reads its clicks in `update` or in `ui`, and Pause pushes `Paused`.
struct Game {
  log: Arc<Mutex<Log>>,
  read_in_update: bool,
}

impl Game {
  fn react(&mut self, ctx: &mut SceneContext, place: &str) -> SceneAction {
    let read = clicks(ctx);
    for action in &read {
      note(&self.log, format!("{place} {action}"));
    }
    if read.iter().any(|a| a == "pause") {
      return SceneAction::Push(Box::new(Paused { log: Arc::clone(&self.log) }));
    }
    SceneAction::Continue
  }
}

impl Scene for Game {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(
      UiNode::column()
        .focus_scope()
        .anchor(Anchor::Center)
        .child(UiNode::button("Play").action("play").name("play").default_focus())
        .child(UiNode::button("Pause").action("pause")),
    );
    if let Some(play) = ctx.find_ui("play") {
      let log = Arc::clone(&self.log);
      ctx.world.entity_mut(play).observe(move |event: On<UiEvent>| {
        if event.kind == UiEventKind::Clicked {
          note(&log, "observed play");
        }
      });
    }
    if let Ok(mut log) = self.log.lock() {
      log.game_root = Some(root);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.read_in_update { self.react(ctx, "update") } else { SceneAction::Continue }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    note(&self.log, "frame");
    if self.read_in_update { SceneAction::Continue } else { self.react(ctx, "ui") }
  }
}

/// A transparent pause menu of Resume (the default), which pops it
struct Paused {
  log: Arc<Mutex<Log>>,
}

impl Scene for Paused {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(
      UiNode::column()
        .focus_scope()
        .anchor(Anchor::Center)
        .child(UiNode::button("Resume").action("resume").default_focus()),
    );
    if let Ok(mut log) = self.log.lock() {
      log.paused_root = Some(root);
    }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if clicks(ctx).iter().any(|a| a == "resume") {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true
  }
}

/// A fixed step every frame (240 Hz), so `update` runs in each frame
fn start(read_in_update: bool) -> Option<(HeadlessEngine, Arc<Mutex<Log>>)> {
  let log = Arc::new(Mutex::new(Log::default()));
  let game = Game { log: Arc::clone(&log), read_in_update };
  let builder = EngineBuilder::new().resolution(320, 240).fixed_hz(240).start_scene(game);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(3).unwrap();
  Some((engine, log))
}

fn press(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  engine.step_frames(3).unwrap();
}

#[test]
fn update_reads_the_frames_events() {
  let Some((mut engine, log)) = start(true) else { return };
  press(&mut engine, GamepadButton::South);
  let trace = log.lock().unwrap().trace.clone();
  let Some(at) = trace.iter().position(|t| t == "observed play") else { panic!("no click: {trace:?}") };
  assert_eq!(
    trace.get(at + 1..at + 3),
    Some(&["frame".to_string(), "update play".to_string()][..]),
    "the observer, then the frame's ui, then update in the same frame: {trace:?}"
  );
  assert_eq!(reads(&log, "update"), ["update play"], "once");
}

#[test]
fn a_click_that_changes_the_scene_is_delivered_once() {
  let Some((mut engine, log)) = start(false) else { return };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  assert_eq!(reads(&log, "ui"), ["ui pause"], "the rerun pass did not repeat the click");
}

#[test]
fn a_lower_scenes_nodes_get_no_input() {
  let Some((mut engine, log)) = start(false) else { return };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  // Paused is on top: South presses its Resume, never Game's focused Pause
  press(&mut engine, GamepadButton::South);
  assert_eq!(reads(&log, "ui"), ["ui pause"]);
}

#[test]
fn ui_events_are_the_scenes_own() {
  let Some((mut engine, log)) = start(true) else { return };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  press(&mut engine, GamepadButton::South); // Resume, which Game's update must not read
  engine.step_frames(3).unwrap();
  assert_eq!(reads(&log, "update"), ["update pause"]);
}

#[test]
fn a_popped_scenes_roots_are_despawned() {
  let Some((mut engine, log)) = start(false) else { return };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  press(&mut engine, GamepadButton::South); // Resume pops Paused
  let (game, paused) = {
    let log = log.lock().unwrap();
    (log.game_root, log.paused_root)
  };
  let (Some(game), Some(paused)) = (game, paused) else { panic!("no roots") };
  assert!(engine.world().get_entity(paused).is_err(), "Paused's tree left with it");
  assert!(engine.world().get_entity(game).is_ok(), "Game's stayed");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test e2e ui_nodes`
Expected: compile errors (`ui_events`), then failures (nodes are not drawn by the engine yet).

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test e2e ui_nodes` and `cargo test --workspace`
Expected: pass (they skip without a GPU; run them where there is one).

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/scene/manager.rs src/core/engine/frame.rs src/core/scene/context.rs tests/e2e/ui_nodes.rs tests/e2e.rs
git commit -m "feat: scenes draw their UI node trees" -m "- each scene's trees draw before its ui, which reads their events in the same frame
- a scene's trees are despawned when it exits; ctx.ui_events reads only its own"
```

### Task 6: Value widgets: checkbox, toggle, radio group, slider, progress, text field

**Files:**
- Modify: `src/core/ui/nodes/draw.rs`
- Test: `src/core/ui/nodes/draw.rs` (a `values` test module)

**Interfaces:**
- Consumes: Tasks 3-4 (`widget_opts!`, `answer`, `Sends::Changes`); `ui.checkbox(&mut bool, label)`, `ui.toggle`, `ui.radio(&mut T, value, label)`, `ui.slider(&mut f32, range)` with `step`/`show_value`/`label`, `ui.progress(f32)` with `label`, `ui.text_input(&mut String)` with `placeholder`/`max_len`/`password`; `Mut::bypass_change_detection`, `DetectChangesMut::set_changed` (`crate::core::ecs::kernel`).
- Produces: the node kinds `Checkbox`, `Toggle`, `RadioGroup`, `Slider`, `Progress`, `TextInput` drawn. `answer` also pushes `Submitted` when `response.submitted()`.

**The rules:**
- **The write-back.** Copy a `Copy` value out (`let mut on = checked.0;`), pass `&mut on` to the builder, and assign back only if it differs (`if on != checked.0 { checked.0 = on; }`), so the component is marked changed only when the player changed it. A text field passes `&mut text.bypass_change_detection().0` and calls `text.set_changed()` when `response.changed()`. Each sends `Changed` through `answer(.., Sends::Changes, ..)`.
- **Checkbox and toggle:** `(&UiNodeKind, &UiLabel, &mut UiChecked, Option<&UiTooltip>, Has<UiDisabled>)`, then `ui.checkbox(&mut on, label.0.as_str())` or `ui.toggle(..)`.
- **Slider.** `(lo, hi)` is the kind's range. Draw `v`, which is the component's value, or `lo` if that is not finite, clamped into `lo.min(hi)..=hi.max(lo)`. Remember the drawn value, call `ui.slider(&mut v, lo..=hi)` with `step`, `show_value` and `label`, and write `v` back only if it differs from the drawn value. The component keeps a value the game set outside the range until the player moves the slider.
- **Progress:** `ui.progress(fraction)` with the fraction clamped to `0.0..=1.0` (NaN as 0), and its `label`; `Sends::Clicks`.
- **Radio group.** A `ui.row()` (if `horizontal`) or `ui.column()`, keyed `.id(entity)`, with the layout options the class takes. Inside, one radio per option: `ui.radio(&mut index, k, option.as_str()).id(k)`, `enabled(false)` under `UiDisabled`. Draw `index` clamped to `options.len().saturating_sub(1)`, and compare against that drawn index. After the radios:
  - write `UiIndex` if `index` changed, and send one `Changed` from the group;
  - set its `UiInteraction` with the row's `id`, `hovered` if any radio is hovered and `focused` if any radio is focused;
  - send `FocusGained` or `FocusLost` when `focused` differs from its previous `UiInteraction`.
- **Text field:** `ui.text_input(..)` with `placeholder`, `max_len` and `password`. It sends `Changed` on an edit and `Submitted` on Enter.

- [ ] **Step 1: Write the failing tests**

Append to `draw.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod values {
  use winit::keyboard::KeyCode;

  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn changes(d: &UiDriver) -> Vec<Entity> {
    d.events().iter().filter(|e| e.kind == UiEventKind::Changed).map(|e| e.node).collect()
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
  fn a_slider_reads_what_the_game_wrote() {
    let (mut d, node) = focused(UiNode::slider(0.0..=1.0, 0.5).step(0.05));
    if let Some(mut value) = d.world_mut().get_mut::<UiValue>(node) {
      value.0 = 0.75;
    }
    d.frame(|_| {});
    d.nav(Direction::Right);
    d.frame(|_| {});
    let value = d.world().get::<UiValue>(node).map(|v| v.0).unwrap_or_default();
    assert!((value - 0.8).abs() < 1e-5, "stepped from the game's 0.75: {value}");
    assert_eq!(changes(&d), [node]);
  }

  #[test]
  fn a_frame_without_input_changes_no_component() {
    let (mut d, node) = focused(UiNode::slider(0.0..=1.0, 0.5));
    d.world_mut().clear_trackers();
    d.frame(|_| {});
    let changed = d.world().entity(node).get_ref::<UiValue>().is_some_and(|v| v.is_changed());
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
      let kept = d.world().get::<UiValue>(node).map(|v| v.0).unwrap_or_default();
      assert!(kept == wrote || (kept.is_nan() && wrote.is_nan()), "the game's value stays until the player moves it");
    }
    d.nav(Direction::Right);
    d.frame(|_| {});
    let value = d.world().get::<UiValue>(node).map(|v| v.0).unwrap_or_default();
    assert!((value - 0.05).abs() < 1e-5, "NaN drew as the start, and a step moved it: {value}");
  }

  #[test]
  fn a_radio_group_writes_its_index_and_sends_one_change() {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(UiNode::column().focus_scope().child(UiNode::radio_group(&["Small", "Medium", "Large"], 0).horizontal().gap(16.0)));
    d.frame(|_| {});
    d.frame(|_| {});
    let group = kids(d.world(), root)[0];
    d.nav(Direction::Right);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(group), Some(&UiIndex(1)));
    assert_eq!(changes(&d), [group]);
    assert!(d.world().get::<UiInteraction>(group).is_some_and(|i| i.focused()));
  }

  #[test]
  fn a_text_field_sends_changed_and_submitted() {
    let (mut d, node) = focused(UiNode::text_input("").placeholder("Name"));
    d.confirm();
    d.frame(|_| {});
    d.type_text("Ryo");
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiTextValue>(node), Some(&UiTextValue("Ryo".into())));
    assert_eq!(changes(&d), [node]);
    d.key(KeyCode::Enter);
    d.frame(|_| {});
    assert!(d.events().iter().any(|e| e.node == node && e.kind == UiEventKind::Submitted));
  }
}
```

(If 4b's text field takes typing on focus without Confirm, drop the `d.confirm()` step. Write the test against how `text_input.rs`'s own tests focus a field.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw::values`
Expected: FAIL. The value kinds are not drawn yet, so the walk warns and nothing changes.

- [ ] **Step 3: Implement** the six kinds by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs
git commit -m "feat: value widget nodes" -m "- checkbox, toggle, radio group, slider, progress and text field nodes
- a player's change is written to the node's component and sent as Changed; drawing alone changes nothing
- values outside a widget's range draw clamped"
```

### Task 7: Lists and text: dropdown, tabs, choice list, typewriter

**Files:**
- Modify: `src/core/ui/nodes/draw.rs`
- Test: `src/core/ui/nodes/draw.rs` (a `lists` test module)

**Interfaces:**
- Consumes: Task 6's write-back rule; `ui.dropdown(&mut usize, &[&str])`, `ui.tabs(&mut usize, &[&str])`, `ui.choice_list(&[&str])` with `.default(i)` (`show() -> Option<usize>`); Task 3's text drawing with `typewriter`.
- Produces: the kinds `Dropdown`, `Tabs`, `ChoiceList` drawn; typewriter text verified.

**The rules:**
- **Options.** Build `let options: Vec<&str> = list.0.iter().map(String::as_str).collect();` per draw (the closure passes a static array; this is the one allocation a list node adds).
- **Dropdown and tabs.** Draw `index` clamped to `options.len().saturating_sub(1)`. Write `UiIndex` back only if the widget changed it from the drawn index, and send `Changed`. With no options, draw `index` 0 over the empty list, as a closure passing `&[]` would.
- **Choice list.** `ui.choice_list(&options).id(entity)`, with `.default(i)` from `default_choice` and the shared widget options. On `Some(i)`, write `UiIndex(i)` (if it differs) and push `Clicked` from the list, built as `answer` builds events. A choice list returns no `Response`, so its `UiInteraction` is not updated and `ctx.ui_id` stays `None` for it. Document this on `UiNode::choice_list`.
- **Typewriter.** Already drawn by Task 3's text rules. This task pins it: the game's `finish()` shows the whole line, and `per_second = 0.0` holds the reveal where it is.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod lists {
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::text::TextReveal;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
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
    let (mut d, node) = focused(UiNode::dropdown(&["Windowed", "Borderless", "Fullscreen"], 0));
    d.confirm();
    d.frame(|_| {});
    d.frame(|_| {});
    d.nav(Direction::Down);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(node), Some(&UiIndex(1)));
    assert!(d.events().iter().any(|e| e.node == node && e.kind == UiEventKind::Changed));
  }

  #[test]
  fn an_index_past_the_options_draws_clamped() {
    let (mut d, node) = focused(UiNode::dropdown(&["A", "B"], 0));
    if let Some(mut index) = d.world_mut().get_mut::<UiIndex>(node) {
      index.0 = 5;
    }
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(node), Some(&UiIndex(5)), "kept until the player picks");
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
    let root = d.spawn_ui(UiNode::column().focus_scope().child(UiNode::tabs(&["Audio", "Video", "Controls"], 0)).child(UiNode::button("In the tab").default_focus()));
    d.frame(|_| {});
    d.frame(|_| {});
    let tabs = kids(d.world(), root)[0];
    d.tab_next();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(tabs), Some(&UiIndex(1)));
    assert!(d.events().iter().any(|e| e.node == tabs && e.kind == UiEventKind::Changed));
  }

  #[test]
  fn a_choice_list_sends_clicked_with_its_index() {
    let (mut d, node) = focused(UiNode::choice_list(&["Buy a gem", "Buy a potion", "Goodbye"]));
    d.nav(Direction::Down);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert_eq!(d.world().get::<UiIndex>(node), Some(&UiIndex(1)));
    let clicked: Vec<_> = d.events().iter().filter(|e| e.kind == UiEventKind::Clicked).map(|e| e.node).collect();
    assert_eq!(clicked, [node]);
  }

  #[test]
  fn a_typewriter_reveals_finishes_and_holds() {
    let mut d = UiDriver::new((400.0, 200.0));
    let text = d.spawn_ui(UiNode::text("Welcome to the Rusty Lantern, traveller.").typewriter(TextReveal::new(20.0)));
    for _ in 0..15 {
      d.frame(|_| {});
    }
    let reveal = |d: &UiDriver| d.world().get::<TextReveal>(text).copied().unwrap_or(TextReveal::new(1.0));
    let shown = reveal(&d).shown();
    assert!(shown > 0.0 && !reveal(&d).is_done(), "part of the line: {shown}");
    if let Some(mut r) = d.world_mut().get_mut::<TextReveal>(text) {
      r.per_second = 0.0;
    }
    for _ in 0..15 {
      d.frame(|_| {});
    }
    assert_eq!(reveal(&d).shown(), shown, "held");
    if let Some(mut r) = d.world_mut().get_mut::<TextReveal>(text) {
      r.finish();
    }
    d.frame(|_| {});
    assert!(reveal(&d).is_done());
  }
}
```

(Use 4b's dropdown tests' frame counts for opening a list: an open popup draws a frame later. Copy their pattern if this one opens a frame late or early.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw::lists`
Expected: FAIL (the list kinds are not drawn; the typewriter test may already pass, which is fine: it pins Task 3).

- [ ] **Step 3: Implement** the three kinds by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs
git commit -m "feat: dropdown, tabs and choice list nodes" -m "- a pick is written to UiIndex and sent as Changed, or Clicked for a choice list
- an index past the options draws clamped; typewriter text finishes and holds on the game's word"
```

### Task 8: Slots, drag and drop, prompts and rebinding

**Files:**
- Modify: `src/core/ui/nodes/draw.rs`
- Test: `src/core/ui/nodes/draw.rs` (a `drag` test module)

**Interfaces:**
- Consumes: `ui.slot()` with `icon`/`count`/`selected`/`filter`; `drag_source<P>(payload)`, `drop_target::<P>()` on widgets and containers; `Response::dropped::<Entity>()`; `ui.held::<Entity>()`; `ui.prompt(&str)`, `ui.rebind(&str)` with `label`/`slot`.
- Produces: the kinds `Slot`, `Prompt`, `Rebind` drawn. Drag and drop on every widget and container. `answer` pushes `Dropped { source }`.

**The rules:**
- **Drag options** (in `widget_opts!` and the container options): `drag_source` adds `.drag_source(entity)`, so the payload is the node's own entity; `drop_target` adds `.drop_target::<Entity>()`. A node dragged onto a node that is not a drop target is refused by 4b, as any payload of the wrong type is.
- **Slots.** `(&UiNodeKind, &UiSlot, Option<&UiTooltip>, Has<UiDisabled>)`. Call `ui.slot().id(entity)`, then `.icon(&icon)` when `UiSlot::icon` is `Some`, `.count(n)`, `.selected(b)` and `.filter(f)`. A slot is a drag source only when it has `drag_source` and an icon; an empty slot does not drag. It sends `Sends::Clicks`.
- **`Dropped`.** In `answer`, `if let Some(source) = response.dropped::<Entity>()`, push `Dropped { source }` on the target, after `Clicked`/`Changed` and `Submitted`.
- **Prompts** draw `ui.prompt(&action)`. **Rebind rows** draw `ui.rebind(&action)` with `.label(..)` and `.slot(n)` (from `binding_slot`), and send `Sends::Changes`, as their response reports.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod drag {
  use crate::core::asset::SpriteHandle;
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::graphics::Sprite;
  use crate::core::input::Direction;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn icon() -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 200, 50, 50, 255), "node-slot-icon")
  }

  /// A row of three slots: the first holds an item, the second is empty,
  /// the third is empty and refuses drops
  fn bag() -> (UiDriver, [Entity; 3]) {
    let mut d = UiDriver::new((400.0, 200.0));
    let root = d.spawn_ui(
      UiNode::row()
        .focus_scope()
        .gap(6.0)
        .child(UiNode::slot().icon(icon()).count(3).drag_source().drop_target().default_focus())
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
    let dropped: Vec<_> = d.events().iter().filter_map(|e| match e.kind {
      UiEventKind::Dropped { source } => Some((e.node, source)),
      _ => None,
    }).collect();
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
    assert!(d.focused().is_some(), "the focus went back into the row");
  }
}
```

(If 4b keeps a payload held after its source stops being drawn, `despawning_a_held_slot_cancels_the_drag` fails. Then the fix belongs in 4b's drag state: cancel a pick-up whose source id was not recorded last frame. That is the same rule for closure UI, so add it to `drag.rs` with a closure test beside this one.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw::drag`
Expected: FAIL (slots are not drawn).

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs
git commit -m "feat: slot, prompt and rebind nodes with drag and drop" -m "- a node's drag payload is its entity; a drop target sends Dropped naming the source node
- an empty slot does not drag; a held node that is despawned is let go"
```

(Add `src/core/ui/widgets/drag.rs` to the `git add` if the note above applied.)

### Task 9: Modals, confirms, popups, Back and named neighbours

**Files:**
- Modify: `src/core/ui/nodes/draw.rs`
- Test: `src/core/ui/nodes/draw.rs` (a `dialogs` test module), `tests/e2e/ui_nodes.rs`

**Interfaces:**
- Consumes: `ui.modal()` (a `Container`), `ui.popup(anchor: UiId)`, `ui.back()`; `UiNodeKind::{nav, anchor_node}` and `BackAnswer` from Task 2; `widget_opts!`'s neighbour slot.
- Produces: `Walk::back` used; `fn neighbour_ids(world: &World, node: Entity) -> [UiId; 4]` and `fn anchor_id(world: &World, node: Entity) -> UiId`, both read before a node's own components are borrowed.

**The rules:**
- **Modal nodes.** A container node with `flags.modal` is drawn with `ui.modal()` in place of its kind's constructor. Its layout options, its flags and `.id(entity)` apply as for any container. After `.show(..)`, set `walk.back = Some(entity)`.
- **Popup nodes.** `ui.popup(anchor_id(world, entity))`, where `anchor_id` is the anchor node's recorded `UiInteraction::id`, or `UiId::NONE` if it has none yet (4b then centres the popup). Then the container options, `.id(entity)`, `.show(..)`, and `walk.back = Some(entity)`.
- **Back** in `Walk::finish`, before the events are returned: if `walk.back` is `Some(target)` and `ui.back()` is true:
  - if `target` has `BackAnswer(no)`, push `Clicked` from `no`, with its name and action;
  - otherwise push `Back` from `target`, with its name and action.
- **Neighbours.** `neighbour_ids` maps each `Some(entity)` in `UiNodeKind::nav` to that node's recorded id (`UiId::NONE` if none). `widget_opts!` applies `nav_up(id)` and the others for each id that is not `NONE`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod dialogs {
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Anchor, Rect};

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn settle(d: &mut UiDriver) {
    for _ in 0..3 {
      d.frame(|_| {});
    }
  }

  fn page() -> UiNode {
    UiNode::column().focus_scope().child(UiNode::button("Page").default_focus())
  }

  fn dialog(action: &str) -> UiNode {
    UiNode::panel().modal().action(action).child(UiNode::button("Inside").default_focus()).child(UiNode::button("Also"))
  }

  #[test]
  fn a_modal_node_traps_the_focus() {
    let mut d = UiDriver::new((400.0, 300.0));
    d.spawn_ui(page());
    let modal = d.spawn_ui(dialog("options"));
    settle(&mut d);
    let inside = kids(d.world(), modal);
    let reachable: Vec<_> = nav_targets(&d.memory, Some(d.scene)).into_iter().map(|(id, _)| id).collect();
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
    let backs: Vec<_> = d.events().iter().filter(|e| e.kind == UiEventKind::Back).map(|e| (e.node, e.action.clone())).collect();
    assert_eq!(backs, [(inner, Some("inner".to_string()))]);
  }

  #[test]
  fn a_confirm_opens_on_no_and_back_answers_no() {
    let mut d = UiDriver::new((400.0, 300.0));
    let confirm = d.spawn_ui(UiNode::confirm("Quit?", "Your progress is saved.", "Yes", "No"));
    settle(&mut d);
    let buttons = kids(d.world(), *kids(d.world(), confirm).last().unwrap());
    let no = buttons[1];
    assert_eq!(d.focused(), d.ui_id(no), "opens on No");
    d.back();
    d.frame(|_| {});
    let answers: Vec<_> = d.events().iter().filter(|e| e.kind == UiEventKind::Clicked).map(|e| (e.node, e.action.clone())).collect();
    assert_eq!(answers, [(no, Some("no".to_string()))]);
    d.nav(Direction::Left);
    d.frame(|_| {});
    d.confirm();
    d.frame(|_| {});
    assert!(d.events().iter().any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some("yes")));
  }

  #[test]
  fn a_popup_hangs_below_its_named_anchor() {
    let mut d = UiDriver::new((400.0, 300.0));
    let root = d.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::button("More").name("more").at(Rect::new(20.0, 20.0, 80.0, 24.0)))
        .child(UiNode::popup("more").child(UiNode::button("Rename").name("rename").default_focus())),
    );
    settle(&mut d);
    let rename = d.find_ui("rename").unwrap();
    let rect = nav_targets(&d.memory, Some(d.scene)).into_iter().find(|(id, _)| Some(*id) == d.ui_id(rename)).map(|(_, r)| r);
    let Some(rect) = rect else { panic!("the popup's button is not focusable: {root:?}") };
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
```

And in `tests/e2e/ui_nodes.rs`:

```rust
/// A scene showing one modal root, which closes (despawns) on Back
struct Dialog {
  backs: Arc<Mutex<Vec<Option<String>>>>,
  root: Option<Entity>,
}

impl Scene for Dialog {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.root = Some(ctx.spawn_ui(
      UiNode::panel().modal().action("options").child(UiNode::button("Done").action("done").default_focus()),
    ));
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind == UiEventKind::Back {
        if let Ok(mut backs) = self.backs.lock() {
          backs.push(event.action.clone());
        }
        if let Some(root) = self.root.take() {
          ctx.world.despawn(root);
        }
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn a_modal_root_traps_focus_and_back_sends_back() {
  let backs = Arc::new(Mutex::new(Vec::new()));
  let scene = Dialog { backs: Arc::clone(&backs), root: None };
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 240).start_scene(scene)) else { return };
  engine.step_frames(3).unwrap();
  assert_eq!(engine.ui_focusables().len(), 1, "only Done");
  press(&mut engine, GamepadButton::East);
  assert_eq!(*backs.lock().unwrap(), [Some("options".to_string())]);
  assert!(engine.ui_focusables().is_empty(), "the dialog closed");
  assert_eq!(engine.step_frames(2).unwrap(), ryo2d::testing::StepOutcome::Running, "Back did not quit");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw::dialogs` and `cargo test --test e2e ui_nodes`
Expected: FAIL (modal flags and popups are not drawn as dialogs; neighbours are not applied).

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::`, `cargo test --test e2e ui_nodes` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs tests/e2e/ui_nodes.rs
git commit -m "feat: modal, confirm and popup nodes, Back and named neighbours" -m "- a modal node traps focus over the dim; the innermost dialog gets Back
- a confirm opens on No, and Back answers No
- popups hang below a named node; nav_up and friends name nodes"
```

### Task 10: Embedding node trees in closure UI

**Files:**
- Modify:
  - `src/core/ui/nodes/draw.rs`: `UiContext::node`, `deliver_pending`; `draw_node` returns the node's `Response`.
  - `src/core/scene/manager.rs`: `deliver_pending` after `Scene::ui`.
  - `src/core/testing/ui.rs`: `deliver_pending` after the closure.
- Test: `src/core/ui/nodes/draw.rs` (an `embedding` test module)

**Interfaces:**
- Consumes: Tasks 3-9; `UiNodes::{embedded_drawn, pending, pending_focus}`.
- Produces:

```rust
impl<'a> UiContext<'a> {
  /// Draw an embedded node tree (spawned with `.embedded()`) here, inside the
  /// open container: in its flow, focus scope and clip. Its events reach
  /// `ctx.ui_events()` after this scene's `ui` returns. Drawing one root
  /// twice in a frame draws it once and warns.
  pub fn node(&mut self, root: Entity, world: &mut World) -> Response;
}
pub(crate) fn deliver_pending(world: &mut World) -> Vec<UiEvent>;   // delivers and returns a copy (the harness keeps it)
fn draw_node(..) -> Option<Response>;   // None for text, separators, spacers, choice lists, hidden or skipped nodes
```

**The rules:**
- **`ui.node(root, world)`.** If `root` has no `UiRoot` or is not `embedded`, warn once (``ui.node draws a root spawned with .embedded()``) and return `Response::default()`. If it is in `embedded_drawn`, warn once (``an embedded node tree was drawn twice in one frame; drawn once``) and return `Response::default()`. Otherwise:
  1. push it onto `embedded_drawn`;
  2. take `pending_focus`;
  3. walk it with `place: None`, so its own layout options apply in the closure's flow;
  4. put the unused `pending_focus` back and append the walk's events to `UiNodes::pending`. Their `scene` is the scene being drawn (`ui.scene().0`).
  The walk's `back` is not used. A modal or popup inside an embedded tree takes Back only through the closure's own `ui.back()`; document this on `node`.
- **`deliver_pending(world)`** takes `UiNodes::pending`, clones it for the return value, and `deliver`s it. `ui_pass` calls it right after `slot.call(..)` returns. The harness calls it after the closure and appends the copy to `last_events`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod embedding {
  use crate::core::ecs::kernel::{Children, Entity, World};
  use crate::core::input::Direction;
  use crate::core::ui::nav::nav_targets;
  use crate::core::ui::nodes::*;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  fn kids(world: &World, of: Entity) -> Vec<Entity> {
    world.get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn grid() -> UiNode {
    UiNode::grid(2).gap(4.0).embedded().children((0..4).map(|i| UiNode::button(format!("Cell {i}")).action("cell")))
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
    nav_targets(&d.memory, Some(d.scene)).into_iter().map(|(_, r)| r).collect()
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
    assert_eq!(d.focused(), d.ui_id(first_cell), "down from Before into the grid");
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
    let warned = crate::core::dev_log::get_entries().iter().filter(|w| w.message.contains("drawn twice in one frame")).count();
    assert_eq!(warned, 1);
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
    assert!(d.events().iter().any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some("cell")));
    assert!(d.world().resource::<UiNodes>().pending.is_empty(), "delivered after it");
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::draw::embedding`
Expected: compile errors (`UiContext::node` missing).

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/nodes/draw.rs src/core/scene/manager.rs src/core/testing/ui.rs
git commit -m "feat: embed node trees in closure UI" -m "- ui.node draws an embedded tree in the closure's container, flow and focus path
- its events are delivered after the scene's ui; a second draw in a frame warns once"
```

### Task 11: The `ui` demo: Node Menus and Node Settings

**Files:**
- Create:
  - `examples/demos/ui/src/scenes/menu_data.rs`: `CREDITS`, `Dialog` and the status lines, moved from `menu_page.rs`.
  - `examples/demos/ui/src/scenes/settings_data.rs`: `TABS`, `OUTPUTS`, `WINDOWS`, `RESOLUTIONS`, `ACTIONS`, `WIDGET_WIDTH`, and `bind_actions(ctx)` (the body of `SettingsPage::enter`), moved from `settings_page.rs`.
  - `examples/demos/ui/src/scenes/node_menu_page.rs`, `examples/demos/ui/src/scenes/node_settings_page.rs`.
- Modify:
  - `menu_page.rs`, `settings_page.rs`: use the moved items (the code is moved, not changed).
  - `scenes/mod.rs`: the new modules and `pub use node_menu_page::NodeMenuPage; pub use node_settings_page::NodeSettingsPage;`.
  - `src/lib.rs` (the demo's): `.page(NodeMenuPage::new).page(NodeSettingsPage::new)` after `.page(DropdownDemo::new)`.
  - `examples/demos/ui/tests/acceptance.rs`: the walks over both versions.
- Test: `examples/demos/ui/tests/acceptance.rs`

**Interfaces:**
- Consumes: everything from Tasks 1-10 through `demo_kit::prelude::*` (which re-exports `ryo2d::prelude::*`): `UiNode`, `UiEvent`, `UiEventKind`, `UiLabel`, `UiValue`, `UiChecked`, `UiIndex`, `UiHidden`, `UiDisabled`, `ctx.spawn_ui`, `ctx.find_ui`, `ctx.ui_events()`.
- Produces: `NodeMenuPage` at page index 11 and `NodeSettingsPage` at 12; `pub(super)` data in `menu_data.rs` and `settings_data.rs` used by both versions.

**The node menu page** (`node_menu_page.rs`), in full:

```rust
//! The Menus page built from node trees: the title screen is spawned once,
//! and each dialog is a tree spawned when it opens and despawned when it
//! closes

use demo_kit::prelude::*;

use super::menu_data::{CONTINUES, CREDITS, Dialog, NEW_GAME, NO_SAVE, QUIT_CHOSEN};

pub struct NodeMenuPage {
  has_save: bool,
  volume: f32,
  fullscreen: bool,
  subtitles: bool,
  /// The open dialog and its tree
  dialog: Option<(Dialog, Entity)>,
}

impl NodeMenuPage {
  pub fn new() -> Self {
    Self { has_save: false, volume: 0.8, fullscreen: false, subtitles: true, dialog: None }
  }

  fn menu() -> UiNode {
    let button = |label: &str, action: &str| UiNode::button(label).action(action).fill_width();
    UiNode::panel()
      .focus_scope()
      .wrap_focus()
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .child(button("Continue", "continue").name("continue").enabled(false))
      .child(button("New Game", "new").default_focus())
      .child(button("Options", "options"))
      .child(button("Credits", "credits"))
      .child(button("Quit", "quit"))
  }

  fn options(&self) -> UiNode {
    UiNode::panel()
      .modal()
      .action("options")
      .width(320.0)
      .gap(12.0)
      .child(UiNode::text("Options").heading())
      .child(UiNode::slider(0.0..=1.0, self.volume).step(0.05).label("Volume").show_value().name("volume"))
      .child(UiNode::toggle("Fullscreen", self.fullscreen).name("fullscreen"))
      .child(UiNode::checkbox("Subtitles", self.subtitles).name("subtitles"))
      .child(UiNode::row().justify(Justify::End).child(UiNode::button("Done").action("done").default_focus()))
  }

  fn credits() -> UiNode {
    let entries = CREDITS.iter().map(|(role, name)| {
      UiNode::column()
        .gap(2.0)
        .child(UiNode::text(*role).role(TextRole::Body).muted())
        .child(UiNode::text(*name).role(TextRole::Body))
    });
    UiNode::panel()
      .modal()
      .action("credits")
      .width(360.0)
      .gap(12.0)
      .child(UiNode::text("Credits").heading())
      .child(UiNode::scroll().height(200.0).fill_width().gap(10.0).children(entries))
      .child(UiNode::text("The right stick scrolls the credits.").muted())
      .child(UiNode::row().justify(Justify::End).child(UiNode::button("Close").action("close").default_focus()))
  }

  fn open(&mut self, ctx: &mut SceneContext, dialog: Dialog) {
    if self.dialog.is_some() {
      return;
    }
    let tree = match dialog {
      Dialog::Options => self.options(),
      Dialog::Credits => Self::credits(),
      Dialog::Quit => UiNode::confirm("Quit to desktop?", "Your progress is saved.", "Yes", "No"),
    };
    self.dialog = Some((dialog, ctx.spawn_ui(tree)));
  }

  fn close(&mut self, ctx: &mut SceneContext) {
    if let Some((_, root)) = self.dialog.take() {
      ctx.world.despawn(root);
    }
  }

  fn set_status(ctx: &mut SceneContext, status: &str) {
    if let Some(node) = ctx.find_ui("status")
      && let Some(mut label) = ctx.world.get_mut::<UiLabel>(node)
    {
      label.0 = status.to_string();
    }
  }
}

impl Default for NodeMenuPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for NodeMenuPage {
  fn name(&self) -> &str {
    "Node Menus"
  }

  fn description(&self) -> &str {
    "The Menus page built from node trees: the same title screen, played the same way.\n\
     Each dialog is a tree spawned when it opens; the page reacts to its nodes' events."
  }

  fn controls(&self) -> Vec<Control> {
    let mut controls = vec![Control::keys("Enter / Space", &[KeyCode::Enter, KeyCode::Space], "Select")];
    if self.dialog.is_some() {
      controls.push(Control::key(KeyCode::Escape, "Close the dialog"));
    }
    controls
  }

  fn wants_escape(&self) -> bool {
    self.dialog.is_some()
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(
      UiNode::column()
        .anchor(Anchor::TopCenter)
        .offset(0.0, 80.0)
        .child(UiNode::text("RYO QUEST").heading().size(48.0)),
    );
    ctx.spawn_ui(Self::menu());
    ctx.spawn_ui(
      UiNode::column()
        .anchor(Anchor::BottomCenter)
        .offset(0.0, -40.0)
        .align(Align::Center)
        .gap(8.0)
        .child(UiNode::text(NO_SAVE).muted().name("status"))
        .child(UiNode::text("[prompt=ui.confirm] Select    [prompt=ui.back] Back").markup()),
    );
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) {
    for event in ctx.ui_events() {
      match (event.kind, event.action.as_deref()) {
        (UiEventKind::Clicked, Some("continue")) => Self::set_status(ctx, CONTINUES),
        (UiEventKind::Clicked, Some("new")) => {
          self.has_save = true;
          if let Some(button) = ctx.find_ui("continue") {
            ctx.world.entity_mut(button).remove::<UiDisabled>();
          }
          Self::set_status(ctx, NEW_GAME);
        }
        (UiEventKind::Clicked, Some("options")) => self.open(ctx, Dialog::Options),
        (UiEventKind::Clicked, Some("credits")) => self.open(ctx, Dialog::Credits),
        (UiEventKind::Clicked, Some("quit")) => self.open(ctx, Dialog::Quit),
        (UiEventKind::Clicked, Some("yes")) => {
          Self::set_status(ctx, QUIT_CHOSEN);
          self.close(ctx);
        }
        (UiEventKind::Clicked, Some("done" | "close" | "no")) | (UiEventKind::Back, _) => self.close(ctx),
        (UiEventKind::Changed, _) => match event.name.as_deref() {
          Some("volume") => self.volume = ctx.world.get::<UiValue>(event.node).map_or(self.volume, |v| v.0),
          Some("fullscreen") => self.fullscreen = ctx.world.get::<UiChecked>(event.node).is_some_and(|c| c.0),
          Some("subtitles") => self.subtitles = ctx.world.get::<UiChecked>(event.node).is_some_and(|c| c.0),
          _ => {}
        },
        _ => {}
      }
    }
  }
}
```

`menu_data.rs` holds `pub(super) enum Dialog { Options, Credits, Quit }` (`Debug, Clone, Copy, PartialEq, Eq`), `pub(super) const CREDITS`, and the status lines, which `menu_page.rs` now uses too:

```rust
pub(super) const NO_SAVE: &str = "No saved game yet: start a New Game.";
pub(super) const CONTINUES: &str = "Your journey continues.";
pub(super) const NEW_GAME: &str = "A new game begins.";
pub(super) const QUIT_CHOSEN: &str = "Quit chosen (the demo stays open).";
```

**The node settings page** (`node_settings_page.rs`):

```rust
//! The Settings page built from node trees: each tab's rows are a column,
//! hidden unless its tab is picked

use demo_kit::prelude::*;

use super::settings_data::{ACTIONS, OUTPUTS, RESOLUTIONS, TABS, WIDGET_WIDTH, WINDOWS, bind_actions};

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
  row(label, UiNode::slider(range, value).step(0.05).show_value().width(WIDGET_WIDTH))
}

fn dropdown(label: &str, options: &[&str], selected: usize) -> UiNode {
  row(label, UiNode::dropdown(options, selected).width(WIDGET_WIDTH))
}

fn tab(index: usize, rows: Vec<UiNode>) -> UiNode {
  let column = UiNode::column().fill_width().gap(10.0).name(format!("tab-{index}")).children(rows);
  if index == 0 { column } else { column.hidden() }
}

impl NodeSettingsPage {
  pub fn new() -> Self {
    Self { tabs: Vec::new(), reset_controls: false }
  }

  fn tree() -> UiNode {
    let audio = tab(0, vec![
      slider("Master", 0.8, 0.0..=1.0),
      slider("Music", 0.6, 0.0..=1.0),
      slider("Effects", 0.7, 0.0..=1.0),
      slider("Voice", 1.0, 0.0..=1.0),
      UiNode::checkbox("Mute in background", true),
      dropdown("Output", &OUTPUTS, 0),
    ]);
    let video = tab(1, vec![
      dropdown("Window", &WINDOWS, 0),
      dropdown("Resolution", &RESOLUTIONS, 13),
      UiNode::toggle("VSync", true),
      row("UI scale", UiNode::radio_group(&["Small", "Medium", "Large"], 1).horizontal().gap(16.0)),
      slider("Brightness", 1.0, 0.5..=1.5),
      UiNode::checkbox("Show FPS", false),
    ]);
    let mut controls: Vec<UiNode> = ACTIONS.iter().map(|(action, label)| UiNode::rebind(action).label(*label)).collect();
    controls.push(UiNode::row().justify(Justify::End).child(UiNode::button("Reset controls").action("reset")));
    let controls = tab(2, controls);
    UiNode::panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(600.0)
      .height(460.0)
      .gap(12.0)
      .child(UiNode::text("Settings").heading())
      .child(UiNode::tabs(&TABS, 0).name("tabs"))
      .child(UiNode::scroll().fill_width().grow(1.0).gap(10.0).child(audio).child(video).child(controls))
      .child(UiNode::row().child(UiNode::text("[prompt=ui.tab_prev] [prompt=ui.tab_next] Switch tab    [prompt=ui.back] Back").markup()))
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
      Control::keys("Enter / Space", &[KeyCode::Enter, KeyCode::Space], "Open a list or rebind"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    bind_actions(ctx);
    let root = ctx.spawn_ui(Self::tree());
    self.tabs = (0..TABS.len()).filter_map(|i| ctx.find_ui_in(root, &format!("tab-{i}"))).collect();
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
```

The closure page draws its rows straight into the scroll area. Each node tab is a column inside it with the same gap and full width, so its rows sit and stretch the same way, and the focus order is the same (`ui_focusables()[3]` is the Window dropdown on the Video tab in both).

**The acceptance walks.** In `tests/acceptance.rs`:
- add `const MENUS: [usize; 2] = [0, 11];`, `const SETTINGS: [usize; 2] = [1, 12];`, `const INVENTORY: [usize; 1] = [2];` and `const DIALOGUE: [usize; 1] = [3];` (Task 12 widens the last two);
- wrap each test's body in `for page in MENUS` (or the page's array), with `start(page)` in place of the literal index, and `{what} (page {page})` in each message.

`every_acceptance_page_is_fully_reachable_by_pad` loops over `MENUS`, `INVENTORY` and `DIALOGUE` for its first part and over `SETTINGS` for the tabs. `every_menu_dialog_is_reachable_and_closes_on_back` and `the_quit_confirm_gives_the_focus_back_to_quit_by_pad_alone` loop over `MENUS`.

- [ ] **Step 1: Write the failing tests**

Change `tests/acceptance.rs` as above. Pages 11 and 12 do not exist yet, so `start(11)` fails (`start_page` past the end panics in `engine_builder`).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ui --test acceptance`
Expected: FAIL on page 11 (no such page) where a GPU is present; on a machine without one the tests return early. Run this task's tests on the Mac.

- [ ] **Step 3: Implement** the data moves, the two pages and their registration.

- [ ] **Step 4: Run the tests and look at the pages**

Run: `cargo test -p ui --test acceptance` and `cargo test --workspace`
Expected: pass.

Capture each node page beside its closure page and compare them by eye. They should match apart from the name and description lines. Run from `examples/demos/ui`, once for each page in 0, 11, 1 and 12:

```bash
RYO_DEMO_PAGE=11 RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=20 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=/tmp/ui-nodes/11 cargo run -p ui
```

- [ ] **Step 5: Gate and commit**

```bash
git add examples/demos/ui/src/scenes/menu_data.rs examples/demos/ui/src/scenes/settings_data.rs examples/demos/ui/src/scenes/node_menu_page.rs examples/demos/ui/src/scenes/node_settings_page.rs examples/demos/ui/src/scenes/menu_page.rs examples/demos/ui/src/scenes/settings_page.rs examples/demos/ui/src/scenes/mod.rs examples/demos/ui/src/lib.rs examples/demos/ui/tests/acceptance.rs
git commit -m "feat: node versions of the Menus and Settings pages" -m "- the ui demo's Node Menus and Node Settings pages, built from node trees
- the acceptance walks run over both versions of each page"
```

### Task 12: The `ui` demo: Node Inventory and Node Dialogue

**Files:**
- Create:
  - `examples/demos/ui/src/scenes/inventory_model.rs`: `EquipSlot`, `ItemKind`, `Item`, `SlotRef`, `Icons`, `GAP`, `BAG`, `IDLE`, and `Inventory` (the bag, the equipment and the status line, with `new`, `item`, `move_item`, `discard`, `sort`, `refusal`, `status`), moved from `inventory_page.rs`.
  - `examples/demos/ui/src/scenes/dialogue_data.rs`: `Next`, `Line`, `TREE`, the prices and node numbers, and `Shop` (`node`, `health`, `gold`, `gems`, with `go(next)` returning whether the line changed, and `choices(line)`), moved from `dialogue_page.rs`.
  - `examples/demos/ui/src/scenes/node_inventory_page.rs`, `examples/demos/ui/src/scenes/node_dialogue_page.rs`.
- Modify:
  - `inventory_page.rs` and `dialogue_page.rs`: use the moved items; their behaviour does not change.
  - `scenes/mod.rs`: re-export `EquipSlot`, `Item`, `ItemKind`, `SlotRef` from `inventory_model`; add `NodeInventoryPage` and `NodeDialoguePage`.
  - `src/lib.rs`: `.page(NodeInventoryPage::new).page(NodeDialoguePage::new)` after Task 11's pages.
  - `tests/acceptance.rs`: `INVENTORY = [2, 13]`, `DIALOGUE = [3, 14]`; the watched-inventory test over both versions.
- Test: `examples/demos/ui/tests/acceptance.rs`

**Interfaces:**
- Consumes: Tasks 1-11; `ui.held::<Entity>()`; `UiInteraction::{hovered, focused}`; `ctx.spawn_ui_in`; `TextReveal::{finish, is_done, restart, per_second}`; `UiRoot::offset`; `content_area(ui)`.
- Produces: `NodeInventoryPage` (index 13) with `pub fn status(&self) -> &str` and `pub fn item(&self, at: SlotRef) -> Option<Item>`, as `InventoryPage` has; `NodeDialoguePage` (index 14); `inventory_model::Inventory`, `dialogue_data::Shop`.

**The node inventory page.** Its tree is the closure page's, node for node. Each slot is built by:

```rust
/// Which bag or equipment place a slot node shows
#[derive(Component, Debug, Clone, Copy)]
struct SlotAt(SlotRef);

fn slot(at: SlotRef) -> UiNode {
  let slot = UiNode::slot().filter(Filter::Nearest).drag_source().drop_target().insert(SlotAt(at));
  if at == SlotRef::Bag(0) { slot.default_focus() } else { slot }
}
```

The tree, spawned in `enter`:
- the outer `column().focus_scope().anchor(Center).gap(12).align(Center)`;
- the Equipment panel: rows of `text(name).role(Body).muted()`, `spacer()`, `slot(Equip(..))`;
- the Backpack panel: `grid(6).gap(GAP)` of the 24 bag slots;
- the details panel (`fill_width().height(76.0).gap(4.0)`), holding two columns (`gap(4.0)`) and showing one at a time:
  - `item`, hidden at first: `text(..).heading().name("item-name")` and `text(..).role(Body).name("item-text")`;
  - `empty`: "Nothing here" `heading().muted()` and the hint `role(Body).muted()`;
- the row of `button("Sort").action("sort")`, `spacer()`, "Discard" `role(Body).muted()`, and `slot().drop_target().tooltip("Drop an item here to throw it away").name("discard")`;
- the status `text(IDLE).role(Body).name("status")`;
- the markup prompt line, `muted()`.

After spawning, the page keeps `slots: Vec<(SlotRef, Entity)>` (found by querying `SlotAt` under the root), the discard slot, and the details' entities. It then calls `refresh(ctx)`.

- **`refresh(ctx)`** writes every slot's `UiSlot` (the item's icon and count, or empty) and its tooltip, as the closure page's `slot()` decides them:
  - an item: its name;
  - an empty equipment slot: `"{Head} slot"`;
  - an empty bag slot: none (remove `UiTooltip`).
  It also writes the status label. It runs after `enter` and after every change to the model.
- **`ui`:**
  - Reads `ctx.ui_events()`:
    - `Dropped { source }` onto a slot: `model.move_item(from, to)` with both `SlotAt`s;
    - `Dropped` onto the discard slot: `model.discard(from)`;
    - `Clicked` on "sort": `model.sort()`.
    Then `refresh`. Remember `told` as the closure page does.
  - Sets the status from `ui.held::<Entity>()`, mapped through `SlotAt` to the model's item, exactly as the closure page's last lines do.
  - Shows the details for the first slot whose `UiInteraction` is `hovered()`, else the one that is `focused()`. It writes the two labels and swaps which column is hidden, only when the shown item changed.
- `controls()` and `wants_escape()` are the closure page's, with `holding` from the last `ui`.

**The node dialogue page.** Its state is a `Shop`, `leaving: bool`, and the entities it updates:
- `hud`, the HUD root, spawned in the first `ui` call, where `content_area(ui)` is known;
- the `hp` progress and the `gold` and `gems` labels;
- `speaker`, and `line` (with its `TextReveal`);
- `lines`, the column the choice scope sits in;
- `scope: Option<(Entity, (usize, bool))>`: the choice scope and the `(line, revealed)` it was built for;
- `confirm: Option<Entity>`, and `speed: f32` (the reveal's speed while the confirm holds it);
- `choices: Vec<Next>`, what the current choice list leads to.

- **`enter`** spawns the conversation panel: `panel().focus_scope().anchor(BottomCenter).offset(0.0, -48.0).width(760.0)`, holding a `row().gap(16.0)` of:
  - `image(portrait).image_size(96.0, 96.0).filter(Filter::Nearest)`;
  - `column().gap(10.0).grow(1.0).name("lines")`, holding `text(speaker).heading().name("speaker")` and `column().max_width(600.0)` around `text(line).markup().role(Body).typewriter(TextReveal::new(40.0).with_punctuation_pause(0.15)).name("line")`.
- **The HUD** is `row().anchor(TopLeft).offset(16.0, area.y + 16.0).gap(12.0).align(Center)`, holding:
  - `progress(health).label("HP").width(160.0).name("hp")`;
  - two `panel().padding(6.0)`s around `text("Gold N").name("gold")` and `text("Gems N").name("gems")`.
  Every `ui` call sets its `UiRoot::offset.y` to `content_area(ui).y + 16.0` when that changed.
- **The choice scope.** Each `ui` call works out `key = (line, reveal.is_done())` from the `line` node's `TextReveal`. When the line is over (no `Shop::node`), it despawns the scope. When `key` differs from the scope's, it despawns the old scope and spawns the new one with `ctx.spawn_ui_in(lines, ..)`:
  - not revealed: `column().focus_scope().child(button("Skip").action("skip").default_focus())`;
  - revealed with choices: `column().focus_scope().child(choice_list(&labels).action("choose"))`, keeping `choices`;
  - revealed with none: `column().focus_scope().child(button("Continue").action("continue").default_focus())`.
  A new scope is a new entity, so focus moves to what appears, as the closure page's `.id((n, done))` scope does.
- **Events:**
  - `Clicked` "skip": `finish()` the line's reveal;
  - `Clicked` "choose": read the list's `UiIndex` and `go(choices[i])`;
  - `Clicked` "continue": `go(Next::To(n + 1))`;
  - `Clicked` "yes" or "no" from the confirm: despawn it and give the reveal back its `speed`; "yes" also ends the conversation (`Shop::node = None`).
- **`go`** calls `shop.go(next)`. Then it writes `speaker`, `line` (`restart()` the reveal), `hp`'s `UiValue`, and the gold and gems labels. When the conversation is over, it shows the closure page's end lines with the reveal finished.
- **Leaving:** when no confirm is open, the conversation is on and `ui.back()` is true:
  1. spawn `UiNode::confirm("Leave the conversation?", "The shopkeeper will remember.", "Yes", "No")`;
  2. keep the reveal's `per_second` in `speed` and set it to 0, so the line waits under the question as the closure page's `reveal(drawn_clusters())` makes it wait.
- `controls()`, `wants_escape()` (the conversation is on) and the number keys (inside `choice_list`) are the closure page's.

**The walks.** Widen `INVENTORY` and `DIALOGUE`. Make the watched-inventory test generic:

```rust
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

struct WatchedInventory<P: Watchable> {
  page: P,
  shown: Arc<Mutex<Shown>>,
}
```

`impl<P: Watchable> DemoPage for WatchedInventory<P>` forwards `name`, `controls`, `wants_escape`, `enter`, `update` and `ui` to the page. After `ui` it records `status()` and `first_slot()`. `escape_puts_back_an_item_the_pad_holds_over_a_button` becomes a helper `fn escape_puts_back<P: Watchable>(page: impl Fn() -> P + 'static)`, called for `InventoryPage::new` and `NodeInventoryPage::new`.

`enter` must be forwarded now. The old wrapper did not need it, but the node page spawns its tree there.

- [ ] **Step 1: Write the failing tests**

Change `tests/acceptance.rs` as above.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ui --test acceptance`
Expected: FAIL on pages 13 and 14 (no such pages) and on the missing `NodeInventoryPage`.

- [ ] **Step 3: Implement** the moves and the two pages by the rules above.

- [ ] **Step 4: Run the tests and look at the pages**

Run: `cargo test -p ui --test acceptance` and `cargo test --workspace`
Expected: pass. Capture pages 2, 13, 3 and 14 as in Task 11 (`RYO_DEMO_PAGE=13 RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=20 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=/tmp/ui-nodes/13 cargo run -p ui`, and so on) and compare each node page with its closure page. On the Mac, play both node pages by mouse, keyboard and pad.

- [ ] **Step 5: Gate and commit**

```bash
git add examples/demos/ui/src/scenes/inventory_model.rs examples/demos/ui/src/scenes/dialogue_data.rs examples/demos/ui/src/scenes/node_inventory_page.rs examples/demos/ui/src/scenes/node_dialogue_page.rs examples/demos/ui/src/scenes/inventory_page.rs examples/demos/ui/src/scenes/dialogue_page.rs examples/demos/ui/src/scenes/mod.rs examples/demos/ui/src/lib.rs examples/demos/ui/tests/acceptance.rs
git commit -m "feat: node versions of the Inventory and Dialogue pages" -m "- slots are nodes whose drag payload is the node; the details follow the hovered or focused slot
- the dialogue swaps its choices as a new subtree, and its leave question is a confirm node
- the acceptance walks run over both versions"
```

### Task 13: The `ui` demo: Mixed

**Files:**
- Create: `examples/demos/ui/src/scenes/mixed_page.rs`
- Modify: `scenes/mod.rs`, `src/lib.rs` (`.page(MixedPage::new)` last, index 15), `tests/acceptance.rs`
- Test: `examples/demos/ui/tests/acceptance.rs`

**Interfaces:**
- Consumes: `ui.node(root, ctx.world)` (Task 10), `ctx.spawn_ui_in`, `ctx.ui_events()`.
- Produces: `MixedPage` at index 15.

**The page.** A closure panel holds an embedded node grid. It is `ui.panel().focus_scope().anchor(Anchor::Center).width(420.0).gap(10.0)` with:
1. a heading "Mixed";
2. a closure row of `button("Add")` (the default focus) and `button("Clear")`;
3. `ui.node(self.grid, ctx.world)`;
4. the status text;
5. a closure `button("Done")`.

The grid is spawned in `enter`: `UiNode::grid(4).gap(6.0).embedded()` with eight `button(format!("Cell {n}")).action("cell").width(80.0).insert(Cell(n))`.
- Add spawns the next cell with `ctx.spawn_ui_in(grid, ..)`, up to 16.
- Clear despawns all but the first four.
- A `Clicked` "cell", read with `ctx.ui_events()`, sets the status to `"Picked cell {n}."` (from `Cell`).
- Done sets `"Done."`.

The description says the D-pad runs from the closure's buttons through the node grid and back.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_mixed_pages_focus_path_runs_through_the_node_grid() {
  let Some(mut engine) = start(15) else { return };
  assert_all_reachable(&mut engine, "Mixed");
  let Some(add) = engine.ui_focused() else { panic!("nothing focused") };
  engine.focus_ui(add);
  engine.step_frames(2).unwrap();
  let rect_of = |engine: &HeadlessEngine, id| engine.ui_focusables().into_iter().find(|(t, _)| *t == id).map(|(_, r)| r);
  let mut steps = 0;
  let done_y = engine.ui_focusables().last().map(|(_, r)| r.y).unwrap_or_default();
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
  assert!(back.is_some_and(|r| r.y < done_y), "Up from Done goes back into the grid");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p ui --test acceptance the_mixed`
Expected: FAIL (no page 15).

- [ ] **Step 3: Implement** the page.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ui --test acceptance` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add examples/demos/ui/src/scenes/mixed_page.rs examples/demos/ui/src/scenes/mod.rs examples/demos/ui/src/lib.rs examples/demos/ui/tests/acceptance.rs
git commit -m "feat: the ui demo's Mixed page" -m "- a closure screen around an embedded node grid, with one focus path through both"
```

### Task 14: Breakout's menus on nodes

**Files:**
- Modify: `examples/games/breakout/src/scenes/{menu, pause, quit_confirm}.rs`
- Test: `examples/games/breakout/tests/menus.rs` (unchanged; it must keep passing)

**Interfaces:**
- Consumes: Tasks 1-9; breakout's `look::{text_style, TEXT_SIZE, use_game_look}`, `save_high_score`, `SystemState`, `GameScene::new`.
- Produces: the three scenes on node trees. Their public constructors (`MenuScene::new()`, `PauseScene::new(score, high_score)`, `QuitConfirmScene::new()`) and behaviour do not change.

**The rules:**
- **MenuScene.** `on_enter` keeps its look, camera and music code. Then it spawns, as separate roots:
  - the backdrop: `UiNode::stack().fill().background(Color::rgb(20, 20, 35))`;
  - one `column().anchor(TopCenter).offset(0.0, y)` per text line, holding `UiNode::text(t).style(text_style(color))`: "BREAKOUT" (white, 100), "A Ryo2D Engine Demo" (gray, 130), the high score (yellow, 180), and "Press ENTER, SPACE, or A to Start" (light gray, 450);
  - the six blocks: `row().anchor(TopCenter).offset(0.0, 250.0).gap(5.0)` of `stack().width(60.0).height(25.0).background(color)`, centred where the closure version's started 12.5 units right of centre;
  - the start button: `column().focus_scope().anchor(TopCenter).offset(0.0, 350.0)` around `button("START GAME").action("start").width(200.0).height(50.0).default_focus()`;
  - "ESC or B to Quit": a `column().anchor(BottomCenter).offset(0.0, -30.0)` text (dark gray).

  `ui` becomes:

```rust
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let start = ctx.ui_events().iter().any(|e| e.kind == UiEventKind::Clicked && e.action.as_deref() == Some("start"));
    if start {
      return self.start_game(ctx);
    }
    // Back (Escape, B) asks before quitting
    if ui.back() {
      return SceneAction::Push(Box::new(QuitConfirmScene::new()));
    }
    SceneAction::Continue
  }
```

- **QuitConfirmScene.** `on_enter` spawns `UiNode::confirm("Quit to Desktop?", "", "YES", "NO")`. `ui` maps a `Clicked` "yes" to `SceneAction::Quit` and "no" to `SceneAction::Pop` (Back answers No). It stays transparent.
- **PauseScene.**
  - `on_enter` spawns the dim, `stack().fill().background(Color::new(0, 0, 0, 180))`, then the panel: `panel().focus_scope().anchor(Center).width(400.0).padding(16.0).gap(12.0)`. The panel holds, in order:
    - "PAUSED" (white) and the score (yellow), each `text(..).size(TEXT_SIZE).color(..).text_align(TextAlign::Center)`;
    - three `volume_row(label, name, value)`s;
    - RESUME (`default_focus`), QUIT TO MENU and QUIT TO DESKTOP, each a `button(..).fill_width().height(40.0)` with the actions "resume", "menu" and "desktop".
  - `volume_row` is a `row().gap(8.0).align(Center)` of:
    - `column().width(96.0)` around the label;
    - `slider(0.0..=1.0, value).step(0.05).fill_width().name(name)`;
    - `column().width(64.0)` around `text(percent).text_align(TextAlign::End).name("{name}-percent")`.
  - `ui` reads `ctx.ui_events()`:
    - `Changed` from "master", "music" or "sfx": take the slider's `UiValue` into the `SystemState` field, call `ctx.audio.set_{master,music,sfx}_volume`, write the percent label (`format!("{:.0}%", value * 100.0)`), and set `settings_changed`;
    - `Clicked` "resume", "menu" or "desktop": the closure version's `Choice`.
  - Back resumes, as before (`if action.is_none() && ui.back()`). `update` keeps Start and Q.
- **The HUD** (`GameScene`) stays closure UI.

- [ ] **Step 1: Run the existing tests to see them pass first**

Run: `cargo test -p breakout`
Expected: pass. They are the regression net: `the_pause_menu_is_navigable_by_pad` drives the pause menu by pad, and must pass unchanged after the move.

- [ ] **Step 2: Move the three scenes to nodes** by the rules above.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p breakout` and `cargo test --workspace`
Expected: pass, with `tests/menus.rs` unchanged.

- [ ] **Step 4: Look at it**

From `examples/games/breakout`, capture the menu (`RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=30 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=/tmp/breakout-nodes/menu cargo run -p breakout`) and the pause screen (the same with `RYO_INPUT="20:Enter 90:Escape" RYO_CAPTURE_FRAMES=120` and its own directory). Do the same on `main` (in a scratch worktree), and compare. On the Mac, play the menus by mouse, keyboard and pad: the start button, Escape's confirm (Back answers No), and the pause sliders changing the volume as they move.

- [ ] **Step 5: Gate and commit**

```bash
git add examples/games/breakout/src/scenes/menu.rs examples/games/breakout/src/scenes/pause.rs examples/games/breakout/src/scenes/quit_confirm.rs
git commit -m "refactor: breakout's menus are node trees" -m "- the title, pause and quit screens are spawned once and react to their nodes' events
- the pause sliders apply their volume on Changed; the HUD stays closure UI"
```

### Task 15: The guard bench, docs and skills

**Files:**
- Modify:
  - `benches/ui.rs`: the node settings screen.
  - `.claude/skills/ryo-ui/SKILL.md` and `src/cli/skills/ryo-ui.md`: the Nodes section.
  - `.claude/skills/ryo-api/SKILL.md` and `src/cli/skills/ryo-api.md`: the new API.
  - `src/core/ui/AGENTS.md`: a Nodes section.
  - `docs/architecture/overview.md`: the UI section.
  - `docs/backlog/ui-retained-nodes.md`: what 4c did; what is left for 4d and 4e.
- Test: the bench (run by hand); the CLI's skill tests (`cargo test -p ryo2d --lib cli::`), which check the embedded copies

**Interfaces:**
- Consumes: everything above.
- Produces: `settings_nodes_frame` in `benches/ui.rs`; the docs.

**The bench.** Add beside `settings_frame`:

```rust
/// The settings screen as a node tree: the same panel, tabs and 50 rows
fn settings_nodes(settings: &Settings) -> UiNode {
  let rows = (0..ROWS).map(|i| {
    let label = settings.labels[i].as_str();
    let (widget, value) = match i % 4 {
      0 => (UiNode::slider(0.0..=1.0, settings.values[i]).width(200.0), format!("{:.0}%", settings.values[i] * 100.0)),
      1 => (UiNode::checkbox("Enabled", settings.flags[i]), "On".to_string()),
      2 => (UiNode::dropdown(&OPTIONS, settings.choices[i]).width(200.0), OPTIONS[settings.choices[i]].to_string()),
      _ => (UiNode::toggle("Enabled", settings.flags[i]), "On".to_string()),
    };
    UiNode::row().gap(12.0).align(Align::Center).child(UiNode::text(label)).child(widget).child(UiNode::text(value))
  });
  UiNode::panel()
    .focus_scope()
    .wrap_focus()
    .anchor(Anchor::Center)
    .width(600.0)
    .height(460.0)
    .gap(8.0)
    .child(UiNode::tabs(&TABS, settings.tab))
    .child(UiNode::scroll().fill_width().grow(1.0).gap(6.0).children(rows))
}

fn settings_nodes_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(settings_nodes(&Settings::new()));
  for _ in 0..3 {
    harness.frame(|_| {});
  }
  assert!(harness.focused().is_some(), "the screen's scope took focus");
  for _ in 0..ROWS + 5 {
    let before = harness.focused();
    harness.action("ui.down");
    harness.frame(|_| {});
    assert_ne!(harness.focused(), before, "ui.down moved the focus");
  }
  let mut frame = 0u64;
  c.bench_function("ui frame, the 200-widget settings screen as nodes", |b| {
    b.iter(|| {
      frame += 1;
      if frame.is_multiple_of(10) {
        harness.action("ui.down");
      }
      harness.frame(|_| {});
    })
  });
}
```

Add it to `criterion_group!` after `settings_frame`. Update the file's doc line to name it.

**The docs:**
- **`ryo-ui` skill:** a `## Nodes` section after the closure sections, covering:
  - when to use nodes (a screen described once, found and changed from systems, loaded from files in 4d) and when to use closures (HUDs, screens computed every frame, custom painters);
  - spawning (`ctx.spawn_ui`, `spawn_ui_in`, ownership by the scene, despawning);
  - the state components and the markers, with game-to-UI and UI-to-game examples;
  - events (`ctx.ui_events()`, the kinds and which widgets send which, observers with `ctx.world.entity_mut(node).observe(..)`), drag and drop by node entity, `UiInteraction`;
  - embedding with `ui.node(root, ctx.world)`;
  - reusable components as functions;
  - modals, `UiNode::confirm`, popups and Back;
  - recipes taken from the node pages: a menu with a confirm, a settings screen with tabs, an inventory grid with drag and drop, and a dialogue with swapped choices.
  Copy it to `src/cli/skills/ryo-ui.md` and `cmp` the two.
- **`ryo-api` skill:** a UI Nodes block listing every new public type, method and signature from this plan's Interfaces blocks: `UiNode` and all its methods, `TextRole`, `UiNodeKind`, the components, `UiEvent`, `UiEventKind`, the `SceneContext` methods, `UiContext::node`, `Container::background`, and the `UiHarness` methods. Copy and `cmp` it.
- **`src/core/ui/AGENTS.md`:** a Nodes section covering:
  - the walk (between `begin_scene` and the scene's `ui`, through the builders, keyed by entity, `UiInteraction` recording ids);
  - ownership (`UiNodes::roots`, despawned on exit);
  - components flowing both ways (write-back only on change);
  - events (delivered after the walk; embedded ones after the scene's `ui`);
  - Back (the innermost dialog node);
  - the rule that nodes never write records themselves.
- **`docs/architecture/overview.md`:** the UI section names the two styles: closures each frame, and node trees as entities drawn through the same builders. It says how they meet: nodes before the scene's closure, and embedding.
- **`docs/backlog/ui-retained-nodes.md`:** mark 4c done (with this plan's spec) and leave the 4d (RON files, templates, hot reload, file keys) and 4e (subtree caching, virtualised lists) items, with the node bench's number as 4e's baseline.

- [ ] **Step 1: Add the bench and run it**

Run: `cargo bench --bench ui -- "settings screen"`
Expected: both settings benches run. Write both times down. The node version must be within 10% of the closure version. If it is not:
1. profile one frame (`cargo flamegraph --bench ui -- --bench "as nodes"`, or puffin scopes around `draw_nodes`);
2. cut the walk's per-node lookups: one `entity_mut` and one `get_components_mut` tuple per node, no `Vec<&str>` for kinds without options, no clones of names or actions unless an event is sent;
3. measure again.
If it is still over 10%, stop and report both numbers and the profile to the owner before going on. Do not loosen the guard.

- [ ] **Step 2: Write the docs and copy the skills**

```bash
cp .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md && cmp .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md
cp .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md && cmp .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md
```

Expected: no output from `cmp`.

- [ ] **Step 3: Run the tests**

Run: `cargo test --workspace` (the CLI's skill tests included) and `cargo test --doc -p ryo2d`
Expected: pass.

- [ ] **Step 4: The full gate**

Run all of: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace`, `cargo check --release -p ryo2d --lib`.
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add benches/ui.rs .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md src/core/ui/AGENTS.md docs/architecture/overview.md docs/backlog/ui-retained-nodes.md
git commit -m "docs: retained UI nodes in the skills and architecture" -m "- the ryo-ui skill's Nodes section with recipes; ryo-api lists the node API
- a guard bench: the settings screen as nodes beside the closure version"
```
