# UI Files and Templates (4d) Design

## Overview

Sub-project 4d of the professional-grade overhaul puts UI in files. A screen is written once in a RON file, loads into the same `UiNode` that 4c spawns (`docs/specs/2026-10-17-ui-nodes-design.md`), and in debug builds updates live while the game runs. Game code answers `UiEvent`s and sets values; the look and layout live in the file.

What it adds:

- **UI files** (`*.ui.ron`): a node tree in `UiNode`'s own names, read by a small reader of RON syntax that knows every value's line and column.
- **Templates with parameters**, the file form of 4c's reusable component functions, used from files and from code, shared between files by imports.
- **The reconcile**: a spawned tree brought in line with a new `UiNode` in place. Hot reload uses it, and so can code (`ctx.update_ui`). Entities, the game's components, its values and widget state survive.
- **Named bindings**: text that shows values (`"Score: {score}"`) and value widgets tied to names, set by the game by name, two-way. They write 4c's state components; there is no second store.
- **Hot reload** through the asset manager's watcher, and **`ryo2d check-ui`**, which checks UI files without running the game.

**Why:** 4c made UI retained but only from Rust. The engine is agent-first: an agent (or a person) should be able to write a screen as data, see its mistakes as precise errors, and change it while the game runs without a rebuild. The backlog note `docs/backlog/ui-retained-nodes.md` set 4d's scope: files, templates, bindings that write the same components, and ids that survive reloads.

**Decisions taken with the owner:**

- **A reload updates in place.** Nodes are matched by key and only what changed is applied, so nothing in game code breaks on a reload.
- **Bindings are two-way and appear in text.** A label follows a slider bound to the same name with no game code.
- **Templates work from files and from code.** Code builds dynamic lists (inventory slots, dialogue choices) from file templates. Named placeholders take nodes code adds. Rust functions callable from files were considered and left out: placeholders cover the need, and they can be added later without breaking anything.
- **Acceptance:** the `ui` demo's node pages and breakout's menus and HUD move to files.
- **Approach A:** our own reader for RON syntax and a table into `UiNode`, rather than serde types through the `ron` crate. `ron::Value` drops names (`Button(..)` becomes a nameless map), typed serde structs fight template parameters and need `flatten` for shared options, and a serde schema would repeat `UiNode`'s option list.

The closure UI and code-built `UiNode` trees stay as they are; files are a third way to write the same trees.

## Architecture

```
assets/ui/pause.ui.ron
        |  AssetManager::load_ui (cached; imports through the same cache)
        v
   reader (RON syntax -> values with positions)
        |  check against the vocabulary table; templates and imports resolved
        v
   UiFile (checked document, generation)
        |  root(&mut assets) / template(name, args, &mut assets)
        |  expand templates, substitute parameters, load images
        v
   UiNode (carrying its source: file, template, arguments)
        |  ctx.spawn_ui / spawn_ui_in (4c)          ctx.update_ui / reload
        v                                               |
   entities + Ui* components  <---- reconcile ----------+
        |
        v  walk (4c), bindings synced from bound components
   4b builders
```

- **Reader** (`src/core/ui/file/reader.rs`): RON syntax into a tree of values, each with its line and column. No serde.
- **Vocabulary** (`src/core/ui/file/vocab.rs`): one table of kinds and options, each naming the `UiNode` constructor or method it calls and the value form it takes. Checking and building both read it, so the file vocabulary cannot drift from `UiNode`.
- **Document** (`src/core/ui/file/document.rs`): `UiFile`, its templates, imports and checks; errors in `src/core/ui/file/error.rs`.
- **Build** (`src/core/ui/file/build.rs`): template expansion and the `UiNode` it produces.
- **Reconcile** (`src/core/ui/nodes/reconcile.rs`) and **bindings** (`src/core/ui/nodes/bindings.rs`) belong to the nodes module: they work on any spawned tree, from a file or from code.
- **Assets** (`src/core/asset/`): `load_ui`, `add_ui`, `reload_ui`, and `.ui.ron` files in the hot-reload watch.
- **CLI** (`src/cli/commands/check_ui.rs`).

No new dependencies. The crate boundaries stand: taffy stays in layout, bevy_ecs comes through the kernel.

## The file

A UI file is `assets/<path>.ui.ron` and is valid RON. Its top level is an unnamed struct with these fields, all optional, though a file needs a `root` or `templates`:

- `root`: the tree to spawn.
- `templates`: a map from template name to template.
- `imports`: a list of other UI files whose templates this file uses.
- `bindings`: starting values for names (Bindings).

```ron
// assets/ui/pause.ui.ron
(
  imports: ["ui/common.ui.ron"],
  templates: {
    "MenuButton": (
      params: ["label", "action"],
      node: Button(label: label, action: action, fill_width: true),
    ),
  },
  bindings: { "score": 0 },
  root: Panel(
    modal: true, width: 260, gap: 8,
    children: [
      Text(text: "Paused", heading: true),
      Text("Score: {score}"),
      MenuButton(label: "Resume", action: "resume", default_focus: true),
      MenuButton(label: "Quit", action: "quit"),
      Slider(range: (0, 1), value: 0.8, bind: "volume"),
      Text("Volume: {volume:.1}"),
      Column(name: "extras"),
    ],
  ),
)
```

### Syntax

The reader takes this part of RON:

- `//` and `/* */` comments; trailing commas.
- Strings (`"..."` with RON's escapes, and raw `r"..."`, `r#"..."#`), numbers (integers, decimals, a leading `-`, exponents), `true` and `false`.
- Lists `[a, b]`, tuples `(a, b)`, maps `{ "key": value }` with string keys.
- Named values: `Name`, `Name(value)`, `Name(field: value, ...)`.

Not taken: `#![enable(..)]` extensions, `Some(..)`/`None`, chars, byte strings. Each is an error saying so. Every demo and test file is also parsed by the `ron` crate in a test, so the files stay valid RON and editors highlight them.

### Nodes

A node is a named value: its name is a kind or a template, its fields are options.

- **Kinds** are `UiNode`'s constructors in CamelCase (`column` is `Column`, `radio_group` is `RadioGroup`).
- **Options** are `UiNode`'s method names (`fill_width`, `gap`, `nav_up`).
- **A constructor's arguments are options** named after its parameters (`Checkbox(label: "Sound", checked: true)`), except where a name clashes with a method: `Popup`'s anchor node is `anchor_node`, and `Prompt`'s and `Rebind`'s input action is `input`.
- **Short forms.** RON cannot mix a positional value with named fields, so a node takes either named options or its main value alone: `Text("Paused")`, `Button("OK")`, `Separator`, `Spacer`.
- **`children: [..]`** lists a container's children.

| Kind | Short form | Constructor options (required in bold) |
|---|---|---|
| `Column`, `Row`, `Stack`, `Panel`, `Scroll` | none | none |
| `Grid` | `columns` | **`columns`** |
| `Popup` | none | **`anchor_node`** (a node name) |
| `Text` | `text` | `text` (default `""`) |
| `Button` | `label` | `label` (default `""`) |
| `Image` | `sprite` | **`sprite`** (an asset path) |
| `Checkbox` | `label` | `label`, `checked` (default `false`) |
| `Toggle` | `label` | `label`, `on` (default `false`) |
| `RadioGroup`, `Dropdown`, `Tabs` | none | **`options`** (a list of strings), `selected` (default 0) |
| `ChoiceList` | none | **`options`** |
| `Slider` | none | **`range`** (`(min, max)`), `value` (default min) |
| `Progress` | `fraction` | `fraction` (default 0) |
| `TextInput` | `text` | `text` (default `""`) |
| `Slot` | none | none |
| `Prompt`, `Rebind` | `input` | **`input`** (an input action name) |
| `Separator`, `Spacer` | (the bare name) | none |
| `Confirm` | none | **`title`**, `message` (default `""`), `yes` (default `"Yes"`), `no` (default `"No"`) |

### Option values

| Form | Options |
|---|---|
| `true` (a method with no argument; `false` leaves it off) | `fill_width`, `fill_height`, `fill`, `center`, `wrap_items`, `focus_scope`, `trap`, `wrap_focus`, `block_pointer`, `default_focus`, `nav_skip`, `drag_source`, `drop_target`, `modal`, `hidden`, `embedded`, `markup`, `heading`, `muted`, `ellipsis`, `show_value`, `password`, `horizontal`, `both`, `no_bar` |
| a bool | `enabled`, `focusable`, `selected` (on `Slot`; on `RadioGroup`, `Dropdown` and `Tabs` `selected` is the constructor's index) |
| a number | `min_width`, `max_width`, `min_height`, `max_height`, `gap`, `grow`, `aspect`, `size`, `wrap`, `max_lines`, `step`, `max_len`, `default_choice`, `count`, `binding_slot` |
| a length: `240`, `Pct(0.5)`, `Fill`, `Fit` | `width`, `height` |
| edges: `8`, `(x, y)`, or `(left: .., top: .., right: .., bottom: ..)` | `padding`, `margin` |
| an enum by name | `align` (`Start`, `Center`, `End`, `Stretch`), `justify` (`Start`, `Center`, `End`, `SpaceBetween`, `SpaceAround`, `SpaceEvenly`), `anchor` (`TopLeft`, `TopCenter`, `TopRight`, `CenterLeft`, `Center`, `CenterRight`, `BottomLeft`, `BottomCenter`, `BottomRight`), `text_align` (`Start`, `Center`, `End`, `Justify`), `filter` (`Nearest`, `Linear`), `role` (`Body`, `Heading`, `Button`, `Caption`, `Mono`) |
| a tuple or list of numbers | `offset` `(x, y)`, `at` `(x, y, width, height)`, `image_size` `(width, height)`, `uv` `(u0, v0, u1, v1)`, `tracks` `[1, 2, 1]` |
| a colour: `"#rrggbb"`, `"#rrggbbaa"`, or `(r: .., g: .., b: .., a: ..)` as theme files write it | `background`, `color`, `tint` |
| a string | `tooltip`, `variant`, `action`, `name`, `key`, `bind`, `nav_up`, `nav_down`, `nav_left`, `nav_right`, `label`, `placeholder` |
| an asset path | `icon` |
| a number (characters per second), or `(per_second: .., punctuation_pause: .., fade: ..)` | `typewriter` |

Not in files: `insert` (Rust components) and `style` (a full `TextStyle`; files use `role`, `size`, `color` and `text_align`). An option that does not apply to a kind is an error, by the same rule 4c's spawn warns with (`Kind::takes`).

### Keys

Every node has a key, used to match nodes across reloads and updates (The reconcile):

- its `key` option, else
- its `name`, else
- its kind (or template name) and its index among the siblings of that kind: the third `Button` among its siblings is `Button#2`.

Two siblings with the same key are an error in a file. In a code tree they are a warning, and they match in order.

### Templates

```ron
templates: {
  "ItemSlot": (
    params: ["icon", "count"],
    defaults: { "count": 1 },
    node: Slot(icon: icon, count: count, drag_source: true, drop_target: true),
  ),
}
```

- **A template is used like a kind:** `ItemSlot(icon: "items/sword.png", count: 3)`. Its fields are its parameters first, then any option of its root node, applied over the template's own (`MenuButton(label: "Resume", action: "resume", default_focus: true)`).
- **Parameters.** `params` lists the required ones; `defaults` gives the optional ones and their values. A missing required parameter, or an unknown field, is an error at the use.
- **Parameter references.** Inside a template, a bare lowercase name that is one of its parameters stands for the argument's value wherever a value can go (`icon: icon`). Lowercase names are always parameter references; enum values and kinds are CamelCase. In a string, `{param}` is replaced by the argument's text when the template expands. A `{name}` that is not a parameter stays a binding (Bindings), so inside a template a parameter shadows a binding of the same name.
- **Nesting.** A template may use other templates and passes them values explicitly (`Inner(label: label)`). A template that uses itself, directly or through others, is an error.
- **Names.** A template name is CamelCase and must not be a kind's name.

### Imports

- `imports` lists files by asset path; their templates become usable in this file. Their `root`s are ignored.
- A template name defined twice across a file and its imports is an error, as is an import cycle.
- Paths, in imports and for images, are relative to the assets directory, as `load_sprite` takes them.

### Placeholders

An empty named container, such as `Column(name: "extras")`, is where code adds nodes with 4c's `ctx.spawn_ui_in`. A reload leaves the nodes code added there alone (The reconcile).

## Loading and building

- **`ctx.assets.load_ui(path) -> AssetResult<UiFile>`.** Cached by path like sprites and shaders, so loading a file twice returns the same handle. The file's imports load through the same cache. The whole file is checked at load:
  - syntax;
  - kinds, options and value types against the vocabulary;
  - templates: parameters, defaults, references, nesting and cycles;
  - imports: missing files, duplicate names and cycles;
  - sibling keys;
  - `bind` only on value widgets (Bindings).
- **`UiFileError`** lists every error found, each with the file, line, column and a message, and suggests the nearest name for an unknown kind, option, enum value, template or parameter: `ui/pause.ui.ron:12:5: Button has no option "fil_width"; did you mean "fill_width"?`. Its `Display` prints one error per line. `AssetError` gains a variant carrying it.
- **`ctx.assets.add_ui(path, text) -> Result<UiFile, UiFileError>`** checks text the game embedded (with `include_str!`, for the web or a single binary) and caches it under `path`, so later loads and imports of that path use it. Such files are not watched.
- **`UiFile::parse(name, text) -> Result<UiFile, UiFileError>`** checks text without an asset manager, for tests and tools. A file read this way cannot import.
- **`UiFile`** is a cheap, cloneable handle (shared document and a `generation()`), like `SpriteHandle`; a reload replaces its document in place.

Building turns a file into a `UiNode`:

- **`file.root(&mut assets) -> Result<UiNode, UiFileError>`** builds the file's `root`. **`file.template(name, args, &mut assets) -> Result<UiNode, UiFileError>`** builds one template instance. Both expand templates, substitute parameters and load images through the assets; a missing image is an error at the `sprite` or `icon` value's position.
- **`UiArgs::new().with("label", "Resume").with("count", 3)`** gives a template's arguments: strings, numbers, bools and lists.
- **The result is an ordinary `UiNode`.** Code may still `.insert(SlotIndex(i))`, add `.child(..)`ren, or place it in a code tree. It carries its source (the file, the template or root, and the arguments), so wherever it is spawned a reload can find and update it.
- **Shortcuts on `SceneContext`:** `ctx.spawn_ui_file(path) -> Result<Entity, UiFileError>` loads, builds and spawns a file's root; `ctx.ui_template(path, name, args) -> Result<UiNode, UiFileError>` loads and builds a template instance.
- Spawning is 4c's: scene ownership, despawn on exit, modals, embedding and events are unchanged.

## The reconcile

`ctx.update_ui(node, tree) -> Entity` brings a spawned tree in line with `tree`, in place. A hot reload is the same operation with the file's new tree. It returns the node's entity, or the new entity if `tree`'s root is a different kind (then the old node is despawned and the new one spawned in its place: same parent, same position among its siblings).

- **Matching.** Children are matched by key among their siblings:
  - same key and kind: updated in place;
  - same key, different kind: despawned, and a new node spawned (a different widget);
  - a key only in `tree`: spawned;
  - a key only in the spawned tree: despawned with its subtree;
  - the tree's own children end in `tree`'s order.
- **What the tree owns.** Its own settings are rewritten from `tree`: kind options, layout, flags, placement (`anchor`, `at`, `offset`, `modal`), variant, action, name, key, neighbours, tooltip patterns and label patterns, and `bind`.
- **What three-way merging protects.** The state the game and the player write is compared against the value the tree last applied, stored per node (two exceptions below):
  - `UiLabel`, `UiChecked`, `UiValue`, `UiIndex`, `UiOptions`, `UiTextValue`, `UiSlot`, `UiImage`, `UiTooltip`, `UiHidden`, `UiDisabled`.
  - One is written only when `tree`'s value differs from the last applied one. Otherwise the live value stays, whoever wrote it.
  - So a reload that moves a button keeps the score the game wrote into a label and the slider the player dragged, while a reload that changes a label's text in the file replaces it.
  - **A bound widget's value belongs to its binding.** The tree's value only seeds the name (Bindings); the reconcile never writes it.
  - **A label or tooltip with a pattern** is formatted from the bindings. Its pattern is the tree's: a changed pattern formats the label again; an unchanged one leaves it.
- **What belongs to the game.**
  - Components the game inserted stay on matched nodes. A tree's `.insert` bundles apply only when a node is spawned.
  - Subtrees added with `spawn_ui_in` are the game's: the reconcile leaves them alone, each after the tree sibling it followed (first, if it followed none). They go only when their parent despawns.
  - **A reload replaces only what came from the file.** Nodes the game put into a file-built tree before spawning it (with `.child(..)`) are left alone by a reload, like `spawn_ui_in` subtrees. `ctx.update_ui` replaces everything in the tree it is given except `spawn_ui_in` subtrees.
- **State survives.** Matched nodes keep their entities, so their ids keep focus, scroll offsets, an open list, a caret and every other `UiMemory` state. A despawned focused node follows 4b's rule: focus becomes none, and the next navigation lands on the scope's default.
- **Quiet.** Components are written only on a real change, so change detection stays truthful. A reconcile sends no `UiEvent`s.
- **Rebuilding from code.** `ctx.update_ui` lets a game rebuild a `UiNode` from its data whenever the data changes and have only the differences applied. A dialogue swaps its choices this way.
- **Cost.** One walk over both trees; allocation only for spawned nodes. An unchanged update of the 200-widget settings tree must cost less than spawning that tree fresh (a bench guard).

## Bindings

- **One set of values per root.** Each spawned root keeps its own values by name, so two HUDs from one file never share. Nodes code adds under a root use that root's values.
- **Setting.** `ctx.ui_set(node, name, value)` sets the value on `node`'s root. Values are numbers, strings and bools (`BindValue`, with `From` for the usual types). It then:
  - writes every node bound to the name (`bind: "name"`): `UiValue` for `Slider` and `Progress`, `UiChecked` for `Checkbox` and `Toggle`, `UiIndex` for `RadioGroup`, `Dropdown`, `Tabs` and `ChoiceList` (a whole, non-negative number), `UiTextValue` for `TextInput` (a number or bool is formatted);
  - formats again every label and tooltip pattern that uses the name;
  - writes only on a change.
  A value of the wrong type for a bound widget (a bool on a slider) warns once, naming the binding and the node, and leaves the widget as it is.
- **Reading.** `ctx.ui_get::<T>(node, name) -> Option<T>` for `f32`, `f64`, `i32`, `u32`, `usize`, `bool` and `String`.
- **The components stay the truth.** Each frame the walk syncs a bound widget's name from its component, whoever wrote it: the player, `ui_set`, or game code writing `UiValue` directly as in 4c. When a bound widget's value changes, its name changes, and so does every other widget and pattern bound to that name. A pattern drawn after the widget shows the new value in the same frame; one drawn before it shows it the next. The widget's `Changed` `UiEvent` fires as in 4c.
- **Patterns.** In a file, every string that becomes a label or tooltip is a pattern: `Text`'s `text`, any kind's `label`, `tooltip`, and `Confirm`'s `title`, `message`, `yes` and `no`. `{name}` is replaced by the value: whole numbers print without a decimal point, other numbers in their shortest form, bools as `true` or `false`. `{name:.N}` gives a number N decimals, and `{{` and `}}` print braces. A `TextInput`'s `text` and `placeholder` are not patterns.
- **Starting values.** A bound value widget's file value (its `value`, `checked`, `on`, `selected` or `text`) seeds its name (`value: 0.8` seeds `volume`); a file's top-level `bindings` seeds names that only text uses. Seeds apply only to names with no value yet, so a reload never resets a score. A name never set shows as nothing, and debug builds warn once per name.
- **Code trees.** `UiNode::bind(name)` binds a value widget; `UiNode::bound()` makes a node's label and tooltip patterns (code text is literal unless marked). A `bind` on a kind that holds no value is an error in a file and a warning in code.
- **Reloads** keep the values, pick up new and removed bindings, and format new patterns with the current values.

## Hot reload

- **The watch.** The asset manager's existing watcher (debug builds with an assets directory, never headless) also watches `.ui.ron` files. When a UI file changes, it is read and checked again, and so is every loaded file that imports it.
- **Applying.** If the new document is clean, every tree built from it is reconciled before that frame's UI pass: roots and code-built template instances, in every scene, with the file's new tree or the template expanded again with the instance's arguments. Then a `UiReloaded { root }` message goes out for each root touched, so a game whose filled placeholder was respawned can fill it again.
- **Errors.** On an error, the old document and trees stay, and the errors are warned once per change with their positions, as theme reloads behave. A file that lost its `root`, or a template that code-built instances use, keeps the old document too and warns.
- **Without the watcher.** `ctx.assets.reload_ui(path)` asks for the same reload directly; tests and headless runs use it.
- **Release builds** read files once and never watch them.

## `ryo2d check-ui`

`ryo2d check-ui [paths...]` checks UI files without running the game: the given files, or every `*.ui.ron` under the project's assets directory. It runs the load checks, resolves imports from disk, and reports image paths that do not exist. It prints errors in the same `path:line:col: message` form and exits non-zero when any file has an error.

## Public API changes

All additive.

- `ryo2d::prelude`: `UiFile`, `UiFileError`, `UiArgs`, `BindValue`, `UiReloaded`.
- `AssetManager`: `load_ui`, `add_ui`, `reload_ui`. `AssetError` gains a UI file variant.
- `SceneContext`: `spawn_ui_file`, `ui_template`, `update_ui`, `ui_set`, `ui_get`.
- `UiNode`: `key`, `bind`, `bound`.
- `UiHarness` (`ryo2d::testing`): `update_ui`, `ui_set`, `ui_get`, and `spawn_ui_file` and `reload_ui` through an asset manager the harness owns, rooted at a directory the test gives.
- The CLI: `check-ui`.

## Games and demos

- **The `ui` demo's node pages load from files** in `examples/demos/ui/assets/ui/`: menu, settings, inventory, dialogue and Mixed.
  - Shared templates live in `common.ui.ron`, imported by the others.
  - Settings binds its sliders to labels ("Volume: {volume}").
  - Inventory's slots are file templates that code builds into a placeholder.
  - Dialogue rebuilds its choices with `ctx.update_ui`.
  - The pages stay 16. The acceptance tests that walk both versions of each page now walk the closure and file versions.
- **breakout's menus move to files** (menu, pause, quit confirm). **Its HUD moves to a file**: one full-width row with lives and level on the left, score and high score on the right, set each frame with `ctx.ui_set` (which writes nothing when a value has not changed). The "+1" fly-by and the game-over and level banners stay closure UI (they animate).

## Testing

- **Reader:** every syntax form, positions, comments, escapes and raw strings, and each refused form with its message.
- **Vocabulary:** every kind and option builds the same `UiNode` as the method it names; value forms; options a kind does not take; suggestions.
- **Templates and imports:** parameters, defaults, references in values and strings, root options over a template, nesting, cycles, missing and unknown arguments, duplicate names, import cycles.
- **Reconcile:** every matching case; tree-owned settings rewritten; the three-way rule (a game-written label kept, a file-changed label replaced, a player-moved slider kept); game components and `spawn_ui_in` subtrees kept; `.child(..)` additions kept by a reload; a kind change respawning in place; focus and scroll surviving; no events; change detection untouched by an unchanged update.
- **Bindings:** set and get; both directions; one name on several widgets; patterns, decimals and escapes; type mismatches; seeds and reloads; per-root values.
- **End to end** (`tests/e2e/`): a file on disk spawned, edited and reloaded with `reload_ui` while a game holds its entities, including an error reload that keeps the old tree.
- **`check-ui`:** a clean file, a broken file, a missing image, the exit code.
- **Validity:** every demo and test UI file parses with the `ron` crate.
- **Bench** (`benches/ui.rs`): an unchanged `update_ui` of the 200-widget settings tree against spawning it. The 4c guard (the node settings frame within 10% of the closure one) still holds.
- **Acceptance:** the `ui` demo's walks over the file pages, and breakout's existing tests.

## Documentation

- `ryo-ui` skill: a Files section (syntax, the vocabulary table, templates, imports, placeholders, bindings, `update_ui`, hot reload) with recipes. `ryo-api`, `ryo-cli` (for `check-ui`) and `ryo-messages` (for `UiReloaded`). Each skill copied byte-identical to `src/cli/skills/`.
- `src/core/ui/AGENTS.md` (files, reconcile and bindings conventions), `src/core/asset/AGENTS.md` (UI files in the asset manager), `src/cli/AGENTS.md`.
- `docs/architecture/overview.md`, and `docs/backlog/ui-retained-nodes.md` (4d done).

## Design decisions

- **Our own reader.** It costs some 600 lines with tests, and buys errors with positions and suggestions everywhere, template parameters in any value position, and one vocabulary table shared with `UiNode`.
- **Files are a spelling of `UiNode`.** Every file feature has a `UiNode` method, so a file never says anything code cannot, and mixing the two is free.
- **One reconcile for reloads and code.** Entities persist through both, so ids, focus and widget state persist without separate file-keyed ids; keys exist only to match nodes.
- **Three-way state.** The tree owns its settings; the game and the player own the state they wrote, unless the tree changes that same value.
- **Bindings write components.** The bound components stay the truth for drawing and reading; the per-root values hold only what patterns format and `ui_get` reads.

## Risks

- **The reader drifting from RON.** Every UI file in the repo also parses with the `ron` crate in a test.
- **Reconcile complexity.** The rules are many; each has a test, and the reconcile writes only on a real change.
- **Parameters shadowing bindings.** A template parameter named like a binding hides it inside the template. Documented in the skill.
- **Per-frame cost.** Syncing bindings costs only bound nodes, and formatting runs only on a change. The 4c guard bench must still hold.

## Out of scope

- Rust functions callable from files as templates (can be added later: an unknown template name would fall back to a registry).
- Loops and conditionals in files.
- A visual editor; a JSON schema for files.
- Localisation tables.
- Animations and transitions in files.
- Custom painters and absolute `draw_*` calls as file nodes (closure UI, as in 4c).
- Subtree caching and virtualised lists (4e).
