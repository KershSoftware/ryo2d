# Retained UI Nodes (Sub-project 4c)

Noted at the end of sub-project 4b (UI interaction, `docs/specs/2026-10-16-ui-interaction-design.md`). 4c gets its own spec; this is what it starts from.

## The Idea

UIs authored as data: a menu, a HUD or an inventory screen described in a file (or built once in code as a tree of nodes) instead of a `Scene::ui` closure that runs every frame. The nodes live across frames, the game changes them only when its data changes, and a designer (or an agent) can edit the file and see it hot-reload, as theme files do today.

The point is to put retained nodes **over the same memory and layout**, not beside them: a node is one more way to record what 4b's builders record, so focus, hit testing, scrolling, popups, drag and drop, the theme and the tests all work unchanged.

## What 4b Leaves in Place

- **Ids.** `UiId` is a hash of a scope, a site and an occurrence count, or of a key (`.id(key)`, `ui.scope(key, ..)`). A node can take its id from its path of keys in the tree, with no call site, through the same `IdScopes` (`src/core/ui/id.rs`).
- **Records.** Hit testing, the wheel, navigation and the debug overlay read only `Record`s in `UiMemory` (rect, clip, layer, order, scene, flags, focus scope, scroll area, accepted payload). A node that writes the same record behaves like a widget, in the same frame order: resolve, then record, then lay out.
- **Layout.** `LayoutEngine` (`src/core/ui/layout/`, the only Taffy code) keeps a persistent Taffy tree keyed by `UiId` and drops nodes not laid out in a frame. The style vocabulary is the engine's own (`Length`, `Edges`, `Align`, `Justify`, the crate's `UiStyle`), so a file can name it and Taffy stays swappable.
- **Widget state** (`UiMemory` typed state: an open list, a caret, a scroll offset, a tooltip timer) is keyed by id and lives as long as the widget is drawn, so a node keeps it as long as it exists.
- **Focus** lives in explicit scopes, with default focus, traps, wrap and explicit neighbours as record flags and fields. A node tree marks scopes the same way.
- **The theme** is data already: `Theme` is RON (merged over the dark theme), widgets name variants and skins by string, and `ThemeWatch` reloads it in debug builds. A UI file can follow the same merge and reload rules.
- **Testing.** `UiHarness` runs UI frames without a GPU; it can drive a node tree as it drives a closure.

## What 4c Has to Add

- **A node tree and its format:** node kinds for the containers and widgets, their options, and a file format (RON, like the theme) with hot reload.
- **Binding to game data:** widgets take `&mut` values today; nodes need a way to read and write game state (a resource, a component, or named values) and to show computed text (`"Gold {gold}"`).
- **Events out:** a `Response` is read in the same closure today. Nodes need their clicks, changes, submits and drops delivered to the game, as messages or callbacks named in the file (an action name per button, say).
- **Recording without rebuilding:** today every widget is rebuilt, re-styled and laid out each frame. A retained tree can skip unchanged subtrees, which the 1,000-row scroll list bench (about 0.6 ms a frame at 4b's end, against 0.11 ms for the 200-widget screen) shows is worth doing. List virtualization (recording only the rows in view) fits here too.
- **Mixing:** a closure UI and a node tree in the same scene, sharing focus scopes and z order.

## Left for Later (Not 4c)

From 4b's out-of-scope list: touch input; analog action axes and more than one gamepad; screen readers (AccessKit); right-to-left layout; rich text editing in a text field; windows the player drags or resizes.
