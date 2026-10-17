# Retained UI Nodes (4c) Design

## Overview

Sub-project 4c of the professional-grade overhaul adds retained UI: a UI described once as a tree of nodes, held as entities, changed only when the game's data changes. It sits on top of 4b (`docs/specs/2026-10-16-ui-interaction-design.md`): a node is one more way to call 4b's builders, so focus, hit testing, scrolling, popups, drag and drop, the theme, sounds and tests behave exactly as they do for closure UI.

What it adds:

- **`UiNode`**, a plain description of a node tree in the closure API's vocabulary: every 4b container and widget, their options, children, and game components to attach.
- **Node trees as entities.** `ctx.spawn_ui(tree)` spawns an entity per node in the existing `ChildOf` hierarchy. A node's state is its components, which the game reads and writes.
- **Events out**, as `UiEvent` messages and as entity events on the node for observers.
- **Mixing** with closure UI: node trees beside a scene's `ui` closure, or embedded inside a closure's container.
- **Reusable components** as plain functions that return a `UiNode`.

The immediate-mode `Scene::ui` call style stays as it is.

**Why:** 4b's UI is immediate only. Every screen is a closure that runs each frame and takes the game's data by `&mut`. That suits HUDs and code-built screens, but a game cannot describe a screen once, find its parts, change them from systems, or (in 4d) load them from a file. The backlog note `docs/backlog/ui-retained-nodes.md` set the direction: put retained nodes over the same memory and layout, not beside them.

**The owner's decisions behind this design (2026-10-17):**

- **Purpose.** All four: UI as ECS entities, reusable components, UI as data files, and fast big UIs.
- **Split.** Three sub-projects, each with its own spec and plan:
  - 4c: entity nodes (this spec);
  - 4d: UI files and templates in RON, with hot reload;
  - 4e: fast big UIs (subtree caching, virtualised lists for node and closure UI).
- **Data.** Components are the data. A node's state is its components; the game writes them and hears changes as events. 4d's named bindings will write these same components.
- **Events.** Both: `UiEvent` messages with action names as the main path, and entity events for observers.
- **Authoring.** A declarative tree in the closure API's names (`UiNode::column().gap(8.0).child(UiNode::button("Resume").action("resume"))`), spawned with `ctx.spawn_ui`. Reusable components are functions returning a `UiNode`. The spawned components are public, and 4d's files load into the same `UiNode`.
- **Acceptance.** Both: node versions of 4b's four acceptance pages and a mixed page in the `ui` demo, and breakout's menus moved to nodes.
- **Approach A.** Nodes drive the immediate builders: each frame, a scene's node trees are walked and drawn through the same builders a closure calls. Approach B (a separate retained pipeline) needed a second implementation of every widget. Approach C (a shared widget core used by both paths) meant refactoring all of 4b's widgets for performance that 4e can deliver more simply.

## Architecture

```
ctx.spawn_ui(UiNode tree)  ->  entities: UiRoot + one entity per node (ChildOf)
                                  each node: UiNodeKind + options + state components
                                  (+ any game components the tree inserted)

each frame, in each scene's UI pass:
  begin_scene(scope, is_top)
  walk the scene's roots (spawn order): for each node, call 4b's builder
    container -> open it, walk the children inside, close it
    widget    -> call the widget with the node's state components
    read the Response -> write state components back, send UiEvents
  scene.ui(ui, ctx)            (closure UI, drawn above the scene's nodes)
  end_scene()
```

The walk is `ui::nodes::draw_nodes(world, ui, scene)`, a function of the world, the UI context and the scene scope. `SceneManager::ui_pass` calls it between `begin_scene` and the scene's `ui`. Because the walk calls the same builders, a node tree records the same `Record`s, lays out through the same `LayoutEngine` and resolves its input against last frame's records, exactly as the equivalent closure would.

Module layout: `src/core/ui/nodes/`:
- `node.rs` holds the `UiNode` value and its builders;
- `spawn.rs` turns a tree into entities;
- `components.rs` holds the public state components;
- `draw.rs` holds the walk;
- `events.rs` holds `UiEvent`.

Nothing outside `nodes/` changes except the call in `ui_pass`, the scene-exit despawn, `ui.node` (Mixing) and the prelude.

## The tree value

`UiNode` is a plain value: a node kind, the options shared by every node, the kind's own options, children, and a list of game components to insert. It is built with the closure API's names, so the same words mean the same things in both styles.

| Constructor | Closure equivalent | State components it spawns |
|---|---|---|
| `UiNode::column()`, `row()`, `grid(columns)`, `stack()`, `panel()`, `scroll()` | `ui.column()` and the others | none (`scroll` keeps its offset in `UiMemory` as today) |
| `UiNode::text(impl Into<String>)` | `ui.text(..)` | `UiLabel` (markup allowed, as in closure text) |
| `UiNode::button(impl Into<String>)` | `ui.button(..)` | `UiLabel` |
| `UiNode::image(SpriteHandle)` | `ui.image(..)` | `UiImage` |
| `UiNode::checkbox(label, checked)`, `toggle(label, on)` | `ui.checkbox`, `ui.toggle` | `UiLabel`, `UiChecked(bool)` |
| `UiNode::radio_group(&[&str], selected)` | a column of `ui.radio` over one index | `UiOptions`, `UiIndex(usize)` |
| `UiNode::slider(range, value)` | `ui.slider` | `UiValue(f32)` |
| `UiNode::progress(fraction)` | `ui.progress` | `UiValue(f32)` |
| `UiNode::text_input(text)` | `ui.text_input` | `UiTextValue(String)` |
| `UiNode::dropdown(&[&str], selected)`, `tabs(&[&str], selected)` | `ui.dropdown`, `ui.tabs` | `UiOptions`, `UiIndex(usize)` |
| `UiNode::choice_list(&[&str])` | `ui.choice_list` | `UiOptions`, `UiIndex(usize)` (the last choice) |
| `UiNode::slot()` | `ui.slot()` | `UiSlot { icon, count, selected }` |
| `UiNode::prompt(action)`, `rebind(action)` | `ui.prompt`, `ui.rebind` | none |
| `UiNode::separator()`, `spacer()` | `ui.separator()`, `ui.spacer()` | none |
| `UiNode::confirm(title, message, yes, no)` | `ui.confirm_with` | a modal root (Modals and dialogs) |
| `UiNode::popup(anchor_name)` | `ui.popup` | a popup root (Modals and dialogs) |

- **Kind options** are the closure builders' own: a column's `gap`, `padding` and `align`; a grid's `tracks`; a scroll's axes; a slider's `step`, `show_value` and `label`; a text field's `placeholder`, `max_len` and `password`; a slot's `icon`, `count`, `selected` and `filter`; a text's `typewriter(TextReveal)`, which inserts the `TextReveal` component so the game can `finish()` a line; a rebind row's `label` and `slot`; a choice list's `default`; a button's `icon` and `variant`.
- **Shared options** are 4b's `widget_builders!` and container options under the same names:
  - layout: `width`, `height`, `fill_width`, `margin`, `anchor`, `offset`, `grow`, `min_width`, `max_width`, `at`;
  - focus: `focus_scope`, `trap`, `wrap_focus`, `default_focus`, `nav_skip`, `focusable`;
  - drag and drop: `drag_source`, `drop_target`;
  - other: `tooltip` and `enabled`.
- **Node-only options:**
  - `action("play")`: the action name its events carry;
  - `name("gold-label")`: a `UiName` to find it by;
  - `hidden()`: spawns it hidden;
  - on a root, `modal()` and `embedded()` (Modals and dialogs, Mixing);
  - `insert(component)`: attaches any game component to that node's entity;
  - `child(node)` and `children(iter)`.
- **Explicit neighbours.** 4b's `nav_up(UiId)` and the other neighbour options take ids, which a node does not have before it is spawned. They take a node name instead (`nav_down("quit")`), resolved within the same root when the tree is spawned.

## Spawning and ownership

`ctx.spawn_ui(tree) -> Entity` spawns the tree at once and returns the root entity. The root carries a `UiRoot` component and the placement a top-level closure container takes: `anchor` and `offset`, or `at(rect)`. Every node is an entity with `ChildOf` its parent, its `UiNodeKind`, its options and its state components. Children keep the order they were given in, and the walk follows `Children`.

- **A root belongs to the scene that spawned it.** `UiRoot` holds the scene's scope id (from `SceneContext`), and only that scene's UI pass draws it, by the same top-scene rules as closure UI: a lower scene's nodes draw but get no input.
- **Exit despawns.** When a scene exits, `Engine::draw_ui` already drains `SceneManager::take_exited` into `UiMemory::forget_scene`. It also despawns every root owned by an exited scene, recursively, so a popped pause menu's nodes leave with it. A `Switch` with `SceneCleanup::WORLD` despawns them anyway, as game entities.
- **Despawning a root,** or any node, removes its subtree; the next frame simply draws without it.
- **Roots draw in spawn order;** `UiRoot` keeps a spawn sequence number, and a later root draws above an earlier one. Roots draw on the page layer, and a `.modal()` root draws through `ui.modal()` (Modals and dialogs).

## Ids and widget state

A node's `UiId` is made from its entity under its scene's root scope: a hash of the scene scope and the entity's index and generation, the same way `.id(key)` keys a closure widget. So:

- **State persists.** Focus memory, a scroll offset, an open list, a text field's caret, a tooltip timer and the other `UiMemory` state persist for the node's lifetime. They survive siblings being added, removed or reordered, because the entity, not the position, is the key.
- **Forgotten on despawn.** A despawned node's state is forgotten like any widget that stops being drawn. A new entity in the same slot has a new generation, so it never inherits it.
- **Ids from code.** `ctx.ui_id(entity) -> Option<UiId>` gives a node's id, so code can `request_focus`, `scroll_to` or name it as a neighbour in closure UI. `ctx.focus_ui(entity)` is the shorthand for focusing a node.
- **Respawns lose state.** Ids that survive a respawn are 4d's concern: a file's node keys will key their nodes, as `.id(key)` does.

## Drawing (the walk)

For each of the scene's roots, `draw_nodes` places the root by its placement and walks its subtree depth first, calling 4b's builders:

- **Containers.** A container node opens its builder with its options (an `.at` root, an anchored root, or a child inside its parent's flow), walks its children inside the builder's closure, and closes it. Focus scopes, traps, clips and scroll areas are therefore the builders' own.
- **Widgets.** A widget node calls its widget with its state components. A checkbox borrows `UiChecked`'s `bool`, a slider `UiValue`'s `f32`, a dropdown `UiIndex`'s `usize` with its `UiOptions`, and a text field `UiTextValue`'s `String`, just as a closure passes `&mut` game data.
- **Hidden and disabled.** A node with `UiHidden` is skipped with its subtree, and records nothing, so it takes no input and leaves the focus chain. A node with `UiDisabled` is drawn with `enabled(false)`.

The walk reads and writes components while the builders hold the UI context. Both are available in `ui_pass` (`world`, `ui`). For a widget, the walk takes the node's state components out, calls the builder, and puts them back. Containers hold no state components, so nothing is borrowed while a container's children are walked.

## Mixing with closure UI

- **Side by side.** A scene's roots draw before its `ui` closure runs, so closure UI draws above them in the same layer, and the two share the scene's focus, layout memory and hit testing. A HUD closure around a node-built menu, or the reverse, just works.
- **Embedded.** A root spawned with `.embedded()` is not drawn on its own. Instead, `ui.node(root, ctx)` draws it at that point, inside the closure's open container, in that container's flow, focus scope and clip. A closure screen can hold a reusable node subtree, such as an inventory grid, with one focus path through both. Drawing an embedded root twice in a frame warns once and draws it once.

## State components (the data)

A node's state is a set of public components, all prefixed `Ui` so they never clash with a game's own names. That covers the state components above: `UiLabel`, `UiImage`, `UiChecked`, `UiValue`, `UiIndex`, `UiOptions`, `UiTextValue` and `UiSlot`, plus the markers `UiName`, `UiHidden` and `UiDisabled`. They hold the data both ways:

- **Game to UI:** a system or `update` changes a node's component (`ctx.world.get_mut::<UiLabel>(gold)` and set the text), and the next UI pass draws the new value. Inserting or removing `UiHidden` or `UiDisabled` hides or disables a node.
- **UI to game:** when a widget changes its value (the player ticks a box, drags a slider, picks an option, types), the walk writes the new value into the component and sends a `Changed` event. The game can react to the event or just read the component when it needs it.
- **Finding nodes:**
  - `ctx.find_ui(name) -> Option<Entity>` finds a node by `UiName` under the scene's roots;
  - `ctx.find_ui_in(root, name)` searches within one tree;
  - a game component inserted with `.insert(..)` finds nodes by query, as with any entity.

## Events

Each thing a node's widget reports becomes a `UiEvent`:

```rust
pub struct UiEvent {
  pub node: Entity,
  pub name: Option<String>,   // the node's UiName
  pub action: Option<String>, // the node's action
  pub kind: UiEventKind,
}

pub enum UiEventKind {
  Clicked,
  SecondaryClicked,
  Changed,
  Submitted,
  Dropped { source: Entity },
  Back,
  FocusGained,
  FocusLost,
}
```

- **As a message.** `UiEvent` is a message, read with `ctx.messages::<UiEvent>()`. The UI pass runs before the frame's fixed steps, so `update` reads a frame's events in the same frame. A game matches on `action` for menus (`"resume"`, `"quit"`), on `node` or a game component for instances, or on `name`.
- **As an entity event.** The same event triggers on the node (`EntityEvent`, which the ECS kernel exports), for games that attach an observer to one node: `ctx.world.entity_mut(button).observe(|event: On<UiEvent>, ..| ..)`.
- **Drag and drop.** A node's drag payload is its own entity. A drop target node's `Dropped` event names the source node, and the game maps it to its item. `ui.held::<Entity>()` reads the node being dragged. Nodes use 4b's drag and drop as it is: ghost, refusal, gamepad pick-up and drop.
- **Choice lists** send `Clicked` with the chosen index in `UiIndex` (spawned with the list), so a dialogue reads the choice from the component.
- **Order.** Events are sent in walk order within a frame, after the widget's component write, so a `Changed` event and the component agree.

## Reusable components

A reusable component is a function that returns a `UiNode`:

```rust
fn item_slot(item: &Item, index: usize) -> UiNode {
  UiNode::slot()
    .icon(item.icon.clone())
    .count(item.count)
    .drag_source()
    .drop_target()
    .insert(SlotIndex(index))
}
```

It takes parameters, tags its nodes with game components or names, and is placed in any tree any number of times. Every instance is its own entities, ids and state. A component's events carry its own nodes, so the game tells instances apart by the components it inserted. In 4d, templates with parameters will give these a file form.

## Modals and dialogs

- **A `.modal()` root** draws through `ui.modal()`: the dim over the whole UI, the trap, opening on its `default_focus`, and Back. Back sends a `UiEvent` of kind `Back` with the root as its node and the root's action.
- **A ready-made confirm.** `UiNode::confirm(title, message, yes, no)` builds the same dialog as `ui.confirm_with`: a modal root, title, message, and buttons with the actions `"yes"` and `"no"`. It opens on No, and Back sends `Clicked` with the action `"no"`, as if No were pressed.
- **Popups:** a `UiNode::popup(anchor_name)` root opens below the named node, as `ui.popup` does.

## Public API changes

All additive; nothing in 4b's API changes.

- **New types:**
  - `UiNode` and its builders, including `UiNode::confirm` and `UiNode::popup`;
  - `UiRoot`, `UiNodeKind`;
  - the state components (`UiLabel`, `UiImage`, `UiChecked`, `UiValue`, `UiIndex`, `UiOptions`, `UiTextValue`, `UiSlot`) and the markers (`UiName`, `UiHidden`, `UiDisabled`);
  - `UiEvent`, `UiEventKind`.
- **`SceneContext`:** `spawn_ui(UiNode) -> Entity`, `find_ui(&str) -> Option<Entity>`, `find_ui_in(Entity, &str) -> Option<Entity>`, `ui_id(Entity) -> Option<UiId>`, `focus_ui(Entity)`.
- **`UiContext`:** `node(root: Entity, ctx: &mut SceneContext) -> Response` (embedding).
- **Testing:** `UiHarness` gains a world for node tests (Testing).
- **Prelude:** all of the above.

## Games and demos

- **The `ui` demo** gains five pages:
  - **Node Menus, Node Settings, Node Inventory and Node Dialogue:** the four 4b acceptance pages built from node trees, with the same layout, behaviour and look. Each goes through the same acceptance walks as its closure version: mouse, keyboard and pad, with no `focus_ui` before a step.
  - **Mixed:** a closure HUD around an embedded node grid, with one focus path from the closure's buttons into the grid and back.
- **Breakout:** the menu, pause and quit-confirm screens move to node trees. The pause screen's volume sliders are slider nodes whose `UiValue` the game applies on `Changed`. Its HUD stays closure UI. Its pad menu test and the headless tests keep passing unchanged.

## Testing

- **Unit tests** (`nodes/`):
  - `UiHarness` gains a world, so `draw_nodes` runs over spawned trees without a GPU.
  - Every node kind: what it records (rect, flags, focus), its component write on a change, and its events (kind, action, name, order).
  - Hidden and disabled nodes.
  - Explicit neighbours by name.
  - Ids stable across sibling reorders, and forgotten on despawn.
  - Embedding: once per frame, the warning on a second draw.
- **Engine tests** (`HeadlessEngine`):
  - a popped scene's roots are despawned and a lower scene's nodes get no input;
  - `update` reads the frame's `UiEvent`s;
  - an observer on a node runs;
  - a modal root traps focus and Back sends `"back"`.
- **Acceptance walks:** the four node pages with the 4b walks, and the Mixed page's focus path.
- **Breakout:** the existing menu and pause tests, now driving nodes.
- **A guard bench** (`benches/ui.rs`): the 200-widget settings screen built as nodes against the same screen as a closure. The node version stays within 10% of the closure version, and the number is 4e's baseline.

## Documentation

- **`ryo-ui` skill:** a Nodes section: when to use nodes or closures, spawning, the state components, events and observers, embedding, reusable components, modals and confirms, with recipes for a menu, a settings screen, an inventory grid and a dialogue.
- **`ryo-api` skill:** every new type and method, with signatures; CLI copies byte-identical.
- **`src/core/ui/AGENTS.md`:** a Nodes section covering the walk, ids, ownership and how components flow both ways.
- **`docs/architecture/overview.md`:** the UI section names the two styles and how they meet.

## Design decisions

- **Nodes drive the builders (approach A).** One implementation of every widget. A node tree costs what its closure costs today, and 4e's subtree caching speeds up both at once.
- **Entity ids.** Ids keyed by entity make widget state follow the node, not its position. Respawns losing state is accepted for 4c; 4d keys file nodes.
- **Components hold the data.** The game writes and reads the same components the walk does, with no binding layer. 4d's named bindings will write these components rather than add a second store.
- **Events are messages first, observers second.** Messages work for files (actions by name) and for many instances. Observers are there for one-off code.
- **Roots belong to scenes.** Node trees follow the scene stack's rules for drawing, input and exit, as closure UI does.

## Risks

- **Coverage gaps.** A builder option a node cannot express yet: each node kind maps to one builder, and an option missing from `UiNode` is a follow-up, not a redesign. The node pages and breakout exercise every kind.
- **Borrowing.** The walk needs the world and the UI context at once. A widget's components are taken out and put back around its own builder call, and containers hold none, so nothing is borrowed across a child's walk.
- **Cost of the walk.** Per-frame cost is the closure's plus a query walk. The guard bench catches a regression; 4e removes the repeat work.

## Out of scope

- UI files, templates in files, and hot reload (4d).
- Subtree caching and virtualised lists (4e).
- Node versions of `ui.custom` painters and absolute `draw_*` calls, which stay closure UI.
- Ids that survive a respawn (4d's file keys).
- From 4b's list, still later: touch input, analog action axes and more than one gamepad, screen readers, right-to-left layout, rich text editing, and windows the player drags or resizes.
