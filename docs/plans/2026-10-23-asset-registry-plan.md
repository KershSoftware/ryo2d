# Asset Registry (5a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One typed asset registry replaces the engine's per-type asset managers. It provides:
- typed handles with generations;
- loaders by extension;
- load states with real errors;
- automatic lifetimes, GPU memory included;
- dependency-aware readiness;
- debounced hot reload decoded off the main thread.

Images, shaders, fonts, UI files and themes all move onto it, along with every game and demo.

**Architecture:**
- **Core (Tasks 3 to 5).** An `AssetId<T>` is a slot and a generation. A `Handle<T>` is a drop-counted strong reference. `Assets<T>` is a resource per type. The `AssetServer` is cheaply cloneable and holds:
  - path → id maps;
  - loaders by extension;
  - load states and dependencies;
  - a worker pool.

  The pre-update pump applies results, reloads and frees.
- **The engine (Tasks 6 and 7).**
  - The engine registers the asset types and runs the pump.
  - Scene code reaches the registry through `ctx.assets()`.
  - `DiskSource` reads, and watches, the project's asset root.
- **The asset types (Tasks 8 to 12):** images, shaders, fonts, UI files and themes. Each one gets:
  - its loader;
  - its engine consumers keyed by id;
  - its callers migrated.

  The renderer takes images' pixels by `Arc` at extract time and holds no handles.
- **Cleanup, measurement and docs (Tasks 13 to 15).** The old `AssetManager` goes, the targets are measured, and the module notes and skills are written.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (as `ryo2d::ecs`; `crate::core::ecs::{Resource, kernel::{World, Message}}`), crossbeam-channel 0.5, notify 7, image 0.25, resvg 0.44, parley/fontique 0.12, thiserror 2, wgpu, `HeadlessEngine`/`UiHarness` tests, criterion.

**Spec:** `docs/specs/2026-10-23-asset-registry-design.md`

**Research (read-only, outside the repo; read the parts your task names):**
- `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/assets-survey.md`: the asset and audio layer as it is on main, with `path:line` for every claim.
- `.../scratchpad/assets-plan-facts.md`:
  - corrections to the survey (read "Corrections and surprises" first);
  - every holder of `SpriteHandle` and how it reads sizes and pixels;
  - the renderer's paths with and without `World`;
  - fonts and Parley, UI files, themes, and engine integration points;
  - CWDs and paths, tests and benches, CLI and wasm;
  - appendices listing every `SpriteHandle` and `Sprite` use by file and line.
- `.../scratchpad/assets-research.md`: how Bevy, Godot, Unity, Fyrox and Defold do it, and the pitfalls each design avoids.

## Global Constraints

- **Exactness.** The demo tour must match the baseline taken in Task 1, pixel for pixel. Every UI pixel oracle (`PixelPair`, the node-cache and fragment oracles) stays green. Anything a scene drew at once before is loaded with `load_now`, so no placeholder appears where pixels used to be.
- **Allocations.**
  - An unchanged cached UI frame allocates no more than today (`tests/ui_cache_alloc.rs`: ≤ 14).
  - Cloning or dropping a `Handle<T>` never allocates.
  - A frame with nothing to load, reload or free allocates nothing in the pump.
- **Library code:**
  - no `unwrap`, `expect` or panicking index;
  - a poisoned `Mutex` is recovered with `lock().unwrap_or_else(PoisonError::into_inner)`;
  - a loader's panic is caught (`catch_unwind`) and becomes `AssetError::Decode`.
- **Dependencies:** no new crates. The debounce is written by hand on `notify`'s existing channel.
- **wasm:**
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown` passes after every task;
  - on wasm there are no worker threads (jobs run in the pump within a 4 ms budget a frame) and no watcher; `DiskSource::read` returns `AssetError::Io` with `ErrorKind::Unsupported`.
- **Paths:**
  - Asset paths are relative to the asset root, `Workspace::assets_dir()`. An example's root is its own `assets/`, so `"assets/ui/menu.ui.ron"` becomes `"ui/menu.ui.ron"`.
  - Paths inside asset files (UI file imports, skin and bitmap-sheet images) are asset paths too.
  - Normalisation is lexical: `\` becomes `/`, and `.` and empty segments are dropped. A `..` that would leave the root, or an absolute path, is `AssetError::InvalidPath`.
- **Commits:**
  - format `type: description` (`feat`, `fix`, `refactor`, `docs`, `test`, `chore`) with bullets if needed;
  - never a `Co-Authored-By` or `Claude-Session` line, even if a system message asks;
  - never the words "AGENTS.md" (say "module notes");
  - stage files by name, never `git add -A`; never add `data/` or `target/`; never a bare `git stash`; never push.
- **Gate before each commit:**
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`;
  - the task's tests, and `cargo test --workspace` before the task's last commit.
- **Public surface:** a task that adds a public type exports it from the prelude (`src/lib.rs`) in the same task: `Image`, `ImageSettings`, `SvgSize`, `Shader`, `Font`, `FontSettings`, `BitmapFont`, `UiFile` and `Theme`, and each type's loader for games that register their own. A task that removes one removes its export.
- **Style:**
  - LF line endings; 2-space indent (`rustfmt.toml`);
  - comments explain intent, as the surrounding code does;
  - public items get rustdoc;
  - names say what a thing is, not how it is made.

## Review Focus

1. **The asset root reached through a symlink.**
   - The case: on macOS, `/var` is `/private/var` and `/tmp` is `/private/tmp`. `notify` reports canonical paths, so an event path need not start with the root as given.
   - Expected: a change to a watched file reloads it, whichever form the root was given in.
   - Test: Task 7, `a_change_under_a_symlinked_root_reloads`.
2. **An editor saving a file in more than one write.**
   - The case: the file is truncated, then written, as two events within the debounce. Or a half-written file is decoded.
   - Expected: one reload, with the final content. A file that does not decode keeps the last good value.
   - Test: Task 7, `two_writes_in_the_debounce_reload_once_with_the_last` and `a_half_written_image_keeps_the_last_good_one`.
3. **A scene that drops an image while the next scene loads it, in the same frame.**
   - Expected: the asset is not freed and reloaded. The new handle has the same id and value, and its texture is not uploaded again.
   - Tests: Task 3, `a_new_handle_before_the_drop_is_seen_keeps_the_asset`; Task 8, `a_scene_change_that_keeps_an_image_does_not_upload_it_again`.
4. **A loader that panics on bad input,** as some decoders do on malformed files.
   - Expected: the load fails with `AssetError::Decode`, and the workers keep running for every other load.
   - Test: Task 4, `a_panicking_loader_fails_its_asset_and_the_workers_live_on`.
5. **A font freed while text that uses it is still laid out,** for example a scene's last frames during a transition.
   - Expected: the text falls back as an unknown family does, with no panic and no stale glyphs.
   - Test: Task 10, `text_whose_font_was_freed_falls_back`.

## Where this plan differs from the spec

The spec is the authority. Planning settled these points, and each is the owner's to reverse:

1. **`ctx.assets()` is a method, not a field.** It returns an `AssetsView<'_>` that borrows the scene's world, because `get`, `add` and `load_now` need the typed stores. `HeadlessEngine::assets()` and `UiHarness::assets()` return the same view.
2. **Loaders get a `check` step.** It runs on the main thread once an asset's dependencies are ready, and again when one of them reloads. A UI file checks its templates against its imports there, as 4d checked at load. A failed check is a failed load, or a failed reload that keeps the last good value.
3. **`AssetError::InvalidPath` is added,** for absolute paths and paths that leave the root.
4. **`EngineBuilder::asset_source(..)` is added,** so tests and tools can load from a `MemorySource` or another folder.
5. **`load_now` needs exclusive world access.** It is called from `ctx.assets()` or `AssetServer::load_now(world, ..)`, not from systems holding `Res`. It loads dependencies at once, on the same thread.
6. **The debounce is hand-written** on `notify`'s channel (100 ms after a path's last event), with no new crate.
7. **Pixels are shared.** `Image` keeps its pixels in an `Arc<Vec<u8>>` (`Arc::make_mut` on edit), so the extract step hands the renderer a cheap reference and the renderer holds no handle.
8. **The ui demo's `theme.ron` becomes `ui/demo.theme.ron`,** so the `.theme.ron` loader takes it.

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/core/asset/mod.rs` | rewrite | module exports, the `Asset` trait |
| `src/core/asset/id.rs` | create | `AssetId<T>`, `UntypedAssetId` |
| `src/core/asset/handle.rs` | create | `Handle<T>` and its drop notice |
| `src/core/asset/assets.rs` | create | `Assets<T>`: the per-type store, its shared ledger of slots |
| `src/core/asset/path.rs` | create | `AssetPath`: lexical normalisation, extensions |
| `src/core/asset/error.rs` | create | `AssetError` |
| `src/core/asset/source.rs` | create | `AssetSource`, `DiskSource`, `MemorySource` |
| `src/core/asset/loader.rs` | create | `AssetLoader`, `AssetSettings`, `LoadContext`, the erased loader table |
| `src/core/asset/server.rs` | create | `AssetServer`: keys, states, dependencies, load/load_with/load_now, progress, stats |
| `src/core/asset/workers.rs` | create | the worker pool (native threads; wasm runs jobs in the pump) |
| `src/core/asset/pump.rs` | create | the pre-update pump: apply results, checks, reloads, frees, events |
| `src/core/asset/event.rs` | create | `LoadState`, `AssetEvent<T>`, `Progress`, `AssetStats` |
| `src/core/asset/view.rs` | create | `AssetsView<'w>` (`ctx.assets()`) |
| `src/core/asset/watch.rs` | create | `DiskSource` watching with the debounce (native) |
| `src/core/asset/loaders/{image,shader,font,bitmap_font,ui_file,theme}.rs` | create | the built-in loaders |
| `src/core/asset/manager.rs`, `hot_reload.rs` | delete (Task 13) | the old manager and watcher |
| `src/core/graphics/sprite.rs` → `image.rs` | rename (Task 2) | the pixel buffer `Image` |
| `src/core/graphics/texture_registry.rs`, `sprite_gpu.rs`, `atlas.rs` | modify (Task 8) | textures keyed by `AssetId<Image>`, freed on `Removed` |
| `src/core/engine/{mod.rs,frame.rs,extract.rs}` | modify | resources, the pump system, images handed to the renderer |
| `src/core/scene/context.rs` | modify | `ctx.assets()` |
| `src/core/text/{fonts.rs,engine.rs,style.rs}`, `src/core/graphics/glyph_atlas.rs` | modify (Task 10) | fonts as assets, freeing faces and glyphs |
| `src/core/ui/file/*`, `src/core/ui/nodes/{reconcile,list}.rs` | modify (Task 11) | UI files as assets |
| `src/core/ui/theme/{mod.rs,file.rs}` | modify (Task 12) | themes as assets |
| `benches/assets.rs` | create (Task 1) | scene-start loading; the reload hitch (Task 14) |
| examples, tests, benches, `src/cli` | modify | callers migrated |
| docs, module notes, skills | modify (Task 15) | |

---

### Task 1: Baselines

Record what "no slower" and "pixel-identical" are measured against, before anything changes.

**Files:**
- Create: `benches/assets.rs` (register it in `Cargo.toml` as `[[bench]] name = "assets" harness = false`)

**The bench** (current API) is `scene_start`:
1. In a `tempfile::TempDir`, generate 40 PNGs: 32×32, 64×64, 128×128 and 256×256, ten of each, with distinct deterministic pixels written by `image::RgbaImage::save`. Also generate 3 UI files (`a.ui.ron`, `b.ui.ron` importing `a`, `c.ui.ron`), each 30 to 60 lines of the vocabulary breakout's `menu.ui.ron` uses.
2. Each iteration does the following on a fresh `AssetManager::with_base_path(dir)` and a fresh `Fonts::with_builtins()`:
   - `load_sprite` for every PNG;
   - `fonts.add_bytes(..)` for two fonts (`examples/games/breakout/assets/fonts/PressStart2P-Regular.ttf` read once outside the timed loop, and a second distinct font from `src/assets/fonts/`);
   - `load_ui` for the 3 files.

Time it with criterion (`sample_size(20)`). Print the median in µs, and the machine's `uptime` load once at start.

**The tour baseline.** Run `./dev.sh demos:tour` and copy `target/demo-tour/` to `/private/tmp/claude-501/5a-tour/base/`. Run it a second time into `.../base2/`, and list the pages that differ between the two runs: those are nondeterministic, and are excluded from later comparisons.

- [ ] **Step 1: Write `benches/assets.rs`** as above. Run `cargo bench --bench assets` twice. Report both medians and the load.
- [ ] **Step 2: Take the tour baseline** twice, as above. Report the page count and the nondeterministic pages.
- [ ] **Step 3: Commit.**

```bash
git add benches/assets.rs Cargo.toml
git commit -m "test: a scene-start asset loading bench"
```

---

### Task 2: `Sprite` becomes `Image`

A mechanical rename with no behaviour change, done before anything else so a tour run separates it from the rest.

**Files:** every file in `assets-plan-facts.md` Appendix D (the bare `Sprite` identifier), plus `src/lib.rs`'s prelude.

**Rules:**
- **Renamed:**
  - `src/core/graphics/sprite.rs` becomes `image.rs`, with its module declared `image` in `graphics/mod.rs`. The struct `Sprite` becomes `Image`; its constructors and methods keep their names (`Image::solid_rect`, `Image::from_rgba`, `Image::load`, `Image::load_svg_fit` and the rest).
  - `SpriteError` becomes `ImageError` and `SpriteResult` becomes `ImageResult` (error.rs and the prelude). `AssetError::Sprite` becomes `AssetError::Image`.
  - The UI widget builder `ui::widgets::Image<'u, 'a>` becomes `ImageWidget<'u, 'a>`, so the two never meet. `ui.image(..)` is unchanged.
- **Not renamed:**
  - `SpriteHandle` (replaced in Task 8);
  - `AnimatedSprite` (a drawable);
  - every enum variant named `Sprite`. The list is in `assets-plan-facts.md` §1 ("a blind `\bSprite\b` sed would corrupt them"): `DrawKind::Sprite` (two enums), `UiCommand::Sprite`, `TextureSource::Sprite` and the rest;
  - `SpriteInstance`, `SpriteDraw`, `SpriteGpuRenderer`, `SpriteShader` and the other `Sprite*` names;
  - strings and labels.
- **Method:** rename the struct, then let `cargo check --workspace --all-targets` list the errors, and fix only type positions, constructor paths (`Sprite::solid_rect` → `Image::solid_rect`) and `use` lines. Check the remaining diff for any variant touched by mistake.
- Doc comments that name the type are updated in the same pass. Skills and module notes wait for Task 15.

- [ ] **Step 1: Rename** as above.
- [ ] **Step 2: Run** `cargo test --workspace`. Expected: PASS, with the same test count as before.
- [ ] **Step 3: Run the tour** and compare with Task 1's baseline (ImageMagick `compare -metric AE`, excluding the nondeterministic pages). Expected: 0 pages differ.
- [ ] **Step 4: Commit.**

```bash
git add -u src examples tests benches
git commit -m "refactor: the pixel buffer is called Image, so sprite names only what is drawn"
```

(`git add -u` stages only tracked files the rename modified. Check `git status` first: no untracked file may be left that belongs to the change, such as `graphics/image.rs` after a `git mv`.)

---

### Task 3: Ids, handles and stores

The core types, pure and unit-tested, with no server yet.

**Files:**
- Create: `src/core/asset/id.rs`, `src/core/asset/handle.rs`, `src/core/asset/assets.rs`
- Modify: `src/core/asset/mod.rs` (declare the modules, define `Asset`, export)

**Interfaces (produced):**

```rust
// src/core/asset/mod.rs
/// A type the registry stores: loaded from files by an `AssetLoader`, or
/// made in code and added with `Assets::add`
pub trait Asset: Send + Sync + 'static {}
```

```rust
// src/core/asset/id.rs
use std::any::TypeId;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use super::Asset;

/// An asset of type `T` by its slot: the slot's index and generation. A
/// slot's generation moves on when the slot is reused, so a stale id finds
/// nothing, never a newer asset. An id does not keep its asset alive; a
/// `Handle` does
pub struct AssetId<T: Asset> {
  index: u32,
  generation: u32,
  marker: PhantomData<fn() -> T>,
}

impl<T: Asset> AssetId<T> {
  pub(crate) const fn new(index: u32, generation: u32) -> Self {
    Self { index, generation, marker: PhantomData }
  }

  /// The slot's index
  pub fn index(self) -> u32 {
    self.index
  }

  /// The slot's generation when this id was made
  pub fn generation(self) -> u32 {
    self.generation
  }

  /// The id with its type erased, for tables over every asset type
  pub fn untyped(self) -> UntypedAssetId {
    UntypedAssetId { type_id: TypeId::of::<T>(), index: self.index, generation: self.generation }
  }
}

impl<T: Asset> Clone for AssetId<T> {
  fn clone(&self) -> Self {
    *self
  }
}
impl<T: Asset> Copy for AssetId<T> {}
impl<T: Asset> PartialEq for AssetId<T> {
  fn eq(&self, other: &Self) -> bool {
    (self.index, self.generation) == (other.index, other.generation)
  }
}
impl<T: Asset> Eq for AssetId<T> {}
impl<T: Asset> Hash for AssetId<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    (self.index, self.generation).hash(state);
  }
}
impl<T: Asset> fmt::Debug for AssetId<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("?");
    write!(f, "AssetId<{name}>({}v{})", self.index, self.generation)
  }
}

/// An asset id of any type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UntypedAssetId {
  pub(crate) type_id: TypeId,
  pub(crate) index: u32,
  pub(crate) generation: u32,
}

impl UntypedAssetId {
  /// The typed id, if it is of type `T`
  pub fn typed<T: Asset>(self) -> Option<AssetId<T>> {
    (self.type_id == TypeId::of::<T>()).then(|| AssetId::new(self.index, self.generation))
  }
}
```

```rust
// src/core/asset/handle.rs
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;

use crossbeam_channel::Sender;

use super::{Asset, AssetId};

/// What a slot's strong handles share; dropping the last one tells the store
pub(crate) struct Shared {
  pub(crate) index: u32,
  pub(crate) generation: u32,
  pub(crate) drops: Sender<(u32, u32)>,
}

impl Drop for Shared {
  fn drop(&mut self) {
    // The store may be gone already (the world is being torn down); nothing to tell then
    let _ = self.drops.send((self.index, self.generation));
  }
}

/// A strong reference to an asset of type `T`: while any clone of it lives,
/// the asset stays loaded. Cloning bumps a count; dropping the last clone
/// frees the asset at the next pump. Compares and hashes by id
pub struct Handle<T: Asset> {
  shared: Arc<Shared>,
  marker: PhantomData<fn() -> T>,
}

impl<T: Asset> Handle<T> {
  pub(crate) fn from_shared(shared: Arc<Shared>) -> Self {
    Self { shared, marker: PhantomData }
  }

  /// The asset's id, which does not keep it alive
  pub fn id(&self) -> AssetId<T> {
    AssetId::new(self.shared.index, self.shared.generation)
  }
}

impl<T: Asset> Clone for Handle<T> {
  fn clone(&self) -> Self {
    Self { shared: Arc::clone(&self.shared), marker: PhantomData }
  }
}
impl<T: Asset> PartialEq for Handle<T> {
  fn eq(&self, other: &Self) -> bool {
    self.id() == other.id()
  }
}
impl<T: Asset> Eq for Handle<T> {}
impl<T: Asset> Hash for Handle<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.id().hash(state);
  }
}
impl<T: Asset> fmt::Debug for Handle<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "Handle({:?})", self.id())
  }
}
impl<T: Asset> From<&Handle<T>> for AssetId<T> {
  fn from(handle: &Handle<T>) -> Self {
    handle.id()
  }
}
```

```rust
// src/core/asset/assets.rs
use std::sync::{Arc, Mutex, PoisonError, Weak};

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::handle::Shared;
use super::{Asset, AssetId, Handle};
use crate::core::ecs::Resource;

/// Which slots are live, shared by a type's store and the asset server so
/// ids can be reserved from any thread
pub(crate) struct Ledger {
  slots: Vec<SlotMeta>,
  free: Vec<u32>,
  drops: Sender<(u32, u32)>,
}

struct SlotMeta {
  generation: u32,
  live: bool,
  /// The slot's strong handles, if any still live
  shared: Weak<Shared>,
}

impl Ledger {
  /// A live slot with no value yet, and its first strong handle
  pub(crate) fn reserve<T: Asset>(&mut self) -> Handle<T>;
  /// A strong handle to a live slot: one of its live handles, or a new one
  /// when every handle was dropped and the slot is not freed yet
  pub(crate) fn strong<T: Asset>(&mut self, id: AssetId<T>) -> Option<Handle<T>>;
  /// Whether `id` is live
  pub(crate) fn is_live<T: Asset>(&self, id: AssetId<T>) -> bool;
  /// Frees the slot if `id` is live and no strong handle remains; the slot's
  /// generation moves on and its index is reused. True if freed
  fn release(&mut self, index: u32, generation: u32) -> bool;
}

/// Every asset of type `T`: a slot table. A world resource per type
#[derive(Resource)]
pub struct Assets<T: Asset> {
  ledger: Arc<Mutex<Ledger>>,
  drops: Receiver<(u32, u32)>,
  values: Vec<Option<Entry<T>>>,
  next_version: u64,
}

struct Entry<T> {
  generation: u32,
  version: u64,
  value: T,
}

impl<T: Asset> Default for Assets<T> { /* empty ledger, unbounded() drop channel, next_version 1 */ }

impl<T: Asset> Assets<T> {
  /// Adds an asset made in code; it lives while the returned handle (or a clone) does
  pub fn add(&mut self, value: T) -> Handle<T>;
  /// The asset, if `id` is live and loaded
  pub fn get(&self, id: impl Into<AssetId<T>>) -> Option<&T>;
  /// The asset for changing in place; its content version moves on
  pub fn get_mut(&mut self, id: impl Into<AssetId<T>>) -> Option<&mut T>;
  /// The asset's content version: it changes whenever the value does, so
  /// caches keyed by (id, version) never see stale content
  pub fn version(&self, id: impl Into<AssetId<T>>) -> Option<u64>;
  /// The number of loaded assets
  pub fn len(&self) -> usize;
  pub fn is_empty(&self) -> bool;
  /// Every loaded asset with its id
  pub fn iter(&self) -> impl Iterator<Item = (AssetId<T>, &T)>;

  pub(crate) fn ledger(&self) -> Arc<Mutex<Ledger>>;
  /// Puts a value in a reserved (or loaded) slot; false if `id` is stale
  pub(crate) fn insert(&mut self, id: AssetId<T>, value: T) -> bool;
  /// Frees every asset whose last handle dropped, adding their ids to `freed`
  pub(crate) fn free_unheld(&mut self, freed: &mut Vec<AssetId<T>>);
}
```

**Rules:**
- **Locking.** Every `Mutex` lock uses `lock().unwrap_or_else(PoisonError::into_inner)`. `get` and `version` never lock: they compare the entry's stored `generation` with the id's.
- **`reserve`:**
  - pop a free index, or push a new `SlotMeta { generation: 0, .. }`;
  - set `live = true`;
  - make the `Shared` with the slot's index and generation and a clone of `drops`;
  - store `Arc::downgrade`.
- **`strong`:**
  - if the slot is live and the generation matches, upgrade `shared`;
  - if the upgrade fails, make a new `Shared` for the same id and store its weak. The drop notice already sent for the old one will then find a live handle and be ignored.
- **`release`:** if live, the generation matches and `shared.upgrade()` is `None`, then:
  - set `live = false`;
  - set `generation = generation.wrapping_add(1)`;
  - push the index onto `free`;
  - return true.
- **`free_unheld`:**
  - drain `drops` with `try_iter`;
  - for each notice that `release` frees, set `values[index] = None` and push the id;
  - with nothing to drain it does not lock and does not allocate.
- **Versions.** `insert` and `get_mut` set the entry's version to `next_version` and then increment it, so versions are unique across the store. `insert` checks liveness under the lock and grows `values` with `resize_with(.., || None)`.

- [ ] **Step 1: Write the failing tests** in `assets.rs`'s `tests` module (`struct Text(String); impl Asset for Text {}`):

```rust
  #[test]
  fn an_added_asset_is_got_by_its_handle() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    assert_eq!(assets.get(&handle).map(|t| t.0.as_str()), Some("a"));
    assert_eq!(assets.len(), 1);
  }

  #[test]
  fn the_last_dropped_handle_frees_its_asset_at_the_next_free() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    let id = handle.id();
    let copy = handle.clone();
    drop(handle);
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert!(freed.is_empty(), "a clone still holds it");
    drop(copy);
    assets.free_unheld(&mut freed);
    assert_eq!(freed, vec![id]);
    assert!(assets.get(id).is_none());
  }

  #[test]
  fn a_stale_id_finds_nothing_in_its_reused_slot() {
    let mut assets = Assets::<Text>::default();
    let old = assets.add(Text("old".into())).id();
    assets.free_unheld(&mut Vec::new());
    let new = assets.add(Text("new".into()));
    assert_eq!(new.id().index(), old.index(), "the slot is reused");
    assert!(assets.get(old).is_none());
    assert_eq!(assets.get(&new).map(|t| t.0.as_str()), Some("new"));
  }

  #[test]
  fn a_new_handle_before_the_drop_is_seen_keeps_the_asset() {
    let mut assets = Assets::<Text>::default();
    let ledger = assets.ledger();
    let id = assets.add(Text("a".into())).id();
    // The handle is gone, but the store has not seen it yet; a load of the
    // same asset hands out a new handle
    let again = ledger.lock().unwrap_or_else(PoisonError::into_inner).strong(id);
    assert!(again.is_some());
    let mut freed = Vec::new();
    assets.free_unheld(&mut freed);
    assert!(freed.is_empty());
    assert_eq!(assets.get(id).map(|t| t.0.as_str()), Some("a"));
  }

  #[test]
  fn changing_an_asset_moves_its_version_on() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    let before = assets.version(&handle);
    if let Some(text) = assets.get_mut(&handle) {
      text.0.push('b');
    }
    assert!(assets.version(&handle) > before);
  }

  #[test]
  fn a_handle_outliving_its_store_drops_quietly() {
    let mut assets = Assets::<Text>::default();
    let handle = assets.add(Text("a".into()));
    drop(assets);
    drop(handle);
  }
```

  Also in `id.rs`: `typed` round-trips, and an `UntypedAssetId` of another type gives `None`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib asset::`. Expected: FAIL (the types don't exist).
- [ ] **Step 3: Implement** to the interfaces and rules.
- [ ] **Step 4: Run** `cargo test -p ryo2d --lib asset::`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/asset/mod.rs src/core/asset/id.rs src/core/asset/handle.rs src/core/asset/assets.rs
git commit -m "feat: typed asset ids, drop-counted handles and per-type asset stores"
```

---

### Task 4: The asset server

Paths, sources, loaders, the worker pool, load states, errors, events, `load`/`load_with`/`load_now`, and the pump's insertion of results. Dependencies, readiness and reloads come in Task 5.

**Files:**
- Create: `src/core/asset/{path.rs, error.rs, source.rs, loader.rs, server.rs, workers.rs, pump.rs, event.rs}`
- Modify: `src/core/asset/mod.rs`; `src/core/error.rs` (the old `AssetError` becomes `ManagerError` until Task 13; update its users: `asset/manager.rs`, `text/fonts.rs`, `ui/file/error.rs`, `testing/ui.rs` and `examples/games/breakout/src/look.rs`, which matches `NotFound`)

**Interfaces (produced):**

```rust
// path.rs
/// An asset's path: relative to the asset root, `/`-separated, normalised
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetPath(Arc<str>);
impl AssetPath {
  /// Normalises `path`: `\` to `/`, drops `.` and empty segments, resolves
  /// `..` within the path; an absolute path or one leaving the root is an error
  pub fn parse(path: &str) -> Result<AssetPath, AssetError>;
  pub fn as_str(&self) -> &str;
  /// The extensions to try, longest first: "menu.ui.ron" gives ["ui.ron", "ron"]
  pub fn extensions(&self) -> impl Iterator<Item = &str>;
  /// The folder of this path, for resolving nothing (paths in files are asset paths)
  pub fn parent(&self) -> Option<&str>;
}
impl fmt::Display for AssetPath; impl fmt::Debug for AssetPath;
```

```rust
// error.rs
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AssetError {
  #[error("no asset at \"{path}\"")]
  NotFound { path: AssetPath },
  #[error("could not read \"{path}\"")]
  Io { path: AssetPath, #[source] source: std::io::Error },
  #[error("could not load \"{path}\"")]
  Decode { path: AssetPath, #[source] source: Box<dyn std::error::Error + Send + Sync> },
  #[error("no loader for \"{path}\" (extension \"{extension}\")")]
  NoLoader { path: AssetPath, extension: String },
  #[error("\"{path}\" needs \"{dependency}\", which did not load")]
  Dependency { path: AssetPath, dependency: AssetPath, #[source] error: Arc<AssetError> },
  #[error("\"{path}\" imports itself: {chain}")]
  Cycle { path: AssetPath, chain: String },
  #[error("\"{path}\" is not an asset path: {reason}")]
  InvalidPath { path: String, reason: &'static str },
}
impl AssetError {
  /// A `Decode` error from any error type
  pub fn decode(path: &AssetPath, error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> AssetError;
}
```

```rust
// source.rs
/// Where asset bytes come from
pub trait AssetSource: Send + Sync + 'static {
  /// The bytes at `path`
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;
  /// Starts telling `changed` about paths whose bytes changed; a source that
  /// cannot watch returns Ok and never sends
  fn watch(&self, changed: crossbeam_channel::Sender<AssetPath>) -> Result<(), AssetError> {
    let _ = changed;
    Ok(())
  }
}
/// Files under a folder (Task 7 adds watching)
pub struct DiskSource { root: PathBuf }
impl DiskSource { pub fn new(root: impl Into<PathBuf>) -> Self; pub fn root(&self) -> &Path; }
/// Bytes in memory: tests and tools. Clones share their files; `insert` over
/// an existing path tells a watcher it changed
#[derive(Clone, Default)]
pub struct MemorySource { /* Arc<Mutex<(HashMap<AssetPath, Arc<[u8]>>, Option<Sender<AssetPath>>)>> */ }
impl MemorySource {
  pub fn new() -> Self;
  pub fn insert(&self, path: &str, bytes: impl Into<Vec<u8>>);
  pub fn remove(&self, path: &str);
}
```

```rust
// loader.rs
/// A loader's settings: part of the asset's key, so one file loaded with two
/// settings is two assets
pub trait AssetSettings: Default + Clone + Hash + Eq + Send + Sync + 'static {}
impl<S: Default + Clone + Hash + Eq + Send + Sync + 'static> AssetSettings for S {}

/// Turns a file's bytes into an asset
pub trait AssetLoader: Send + Sync + 'static {
  type Asset: Asset;
  type Settings: AssetSettings;
  /// The extensions this loader takes, without the leading dot ("png", "ui.ron")
  fn extensions(&self) -> &[&'static str];
  /// Makes the asset; dependencies are loaded through `cx`
  fn load(&self, bytes: Vec<u8>, settings: &Self::Settings, cx: &mut LoadContext<'_>)
    -> Result<Self::Asset, AssetError>;
  /// Checks the asset against its dependencies, once they are loaded and
  /// whenever one reloads (on the main thread). The default accepts
  fn check(&self, asset: &Self::Asset, world: &World) -> Result<(), AssetError> {
    let _ = (asset, world);
    Ok(())
  }
}

/// What a loader can do while loading
pub struct LoadContext<'a> { /* path, server, deps, mode: Queued | Now(&'a mut World) + cycle stack */ }
impl LoadContext<'_> {
  pub fn path(&self) -> &AssetPath;
  /// Loads a dependency: the asset being loaded holds the handle and is ready
  /// only once this is. While `load_now` runs, dependencies load now too
  pub fn load<T: Asset>(&mut self, path: &str) -> Result<Handle<T>, AssetError>;
  pub fn load_with<T: Asset, S: AssetSettings>(&mut self, path: &str, settings: S)
    -> Result<Handle<T>, AssetError>;
}
```

```rust
// event.rs
#[derive(Debug, Clone)]
pub enum LoadState { NotLoaded, Loading, Loaded, Failed(Arc<AssetError>) }
impl LoadState { pub fn is_loaded(&self) -> bool; pub fn is_failed(&self) -> bool; pub fn error(&self) -> Option<&Arc<AssetError>>; }

/// Something happened to an asset of type `T` (read with `MessageReader` or `ctx.messages`)
#[derive(Message, Debug, Clone)]
pub enum AssetEvent<T: Asset> {
  Added { id: AssetId<T> },
  Modified { id: AssetId<T> },
  Removed { id: AssetId<T> },
  Failed { id: AssetId<T>, error: Arc<AssetError> },
  /// A reload failed; the last good value stays
  ReloadFailed { id: AssetId<T>, error: Arc<AssetError> },
}

/// A group's progress
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress { pub loaded: usize, pub failed: usize, pub pending: usize }
impl Progress { pub fn is_done(&self) -> bool; pub fn fraction(&self) -> f32; }

/// Per type: loaded, loading and failed counts
#[derive(Debug, Clone, Default)]
pub struct AssetStats { pub types: Vec<TypeStats> }
#[derive(Debug, Clone, Default)]
pub struct TypeStats { pub name: &'static str, pub loaded: usize, pub loading: usize, pub failed: usize }
```

```rust
// server.rs
/// Loads assets by path, through loaders by extension, on worker threads.
/// Cheap to clone (shared inside); a world resource
#[derive(Resource, Clone)]
pub struct AssetServer { inner: Arc<ServerInner> }
impl AssetServer {
  pub fn new(source: impl AssetSource) -> Self;
  /// Makes `T` storable: inserts `Assets<T>` and registers `AssetEvent<T>`
  pub fn register_asset<T: Asset>(&self, world: &mut World);
  pub fn register_loader<L: AssetLoader>(&self, loader: L);
  /// The asset at `path`, loading it if needed; returns at once. A bad path
  /// or a missing loader gives a handle whose state is `Failed`
  pub fn load<T: Asset>(&self, path: &str) -> Handle<T>;
  pub fn load_with<T: Asset, S: AssetSettings>(&self, path: &str, settings: S) -> Handle<T>;
  /// Loads at once on this thread (dependencies too) and inserts the result
  pub fn load_now<T: Asset>(&self, world: &mut World, path: &str) -> Result<Handle<T>, Arc<AssetError>>;
  pub fn load_now_with<T: Asset, S: AssetSettings>(&self, world: &mut World, path: &str, settings: S)
    -> Result<Handle<T>, Arc<AssetError>>;
  pub fn state<T: Asset>(&self, id: impl Into<AssetId<T>>) -> LoadState;
  pub fn path<T: Asset>(&self, id: impl Into<AssetId<T>>) -> Option<AssetPath>;
  pub fn progress(&self, ids: &[UntypedAssetId]) -> Progress;
  pub fn stats(&self) -> AssetStats;
}
```

```rust
// pump.rs
/// Applies finished loads, reloads and frees, and sends their events. Runs in
/// `Phase::PreUpdate`; with nothing to do it polls a few channels
pub(crate) fn pump(world: &mut World);
```

**Rules:**
- **Keys.** An asset's key is `(TypeId of the asset, AssetPath, settings hash)`, with the settings kept as `Box<dyn Any + Send + Sync>` for reloads.
  - `load` of a key already present returns `ledger.strong(id)`.
  - A key whose last handle dropped, but which is not freed yet, is found again. That is Review Focus 3.
  - A key that is freed is loaded anew.
- **Loader choice.**
  - Try `AssetPath::extensions()` longest first. The loader must also produce `T`.
  - If no extension has a loader, or the loader for it produces another type, the result is a reserved handle with state `Failed(NoLoader)`, and `AssetEvent::Failed` is sent at the next pump.
  - The same applies when `AssetPath::parse` fails, with `InvalidPath`.
- **`load`:**
  - reserve through `Assets<T>`'s ledger (the server keeps each type's `Arc<Mutex<Ledger>>`, taken at `register_asset`);
  - set the state to `Loading`;
  - queue a job.
- **A job** runs on a worker. It:
  - reads the bytes through the source;
  - calls the loader inside `std::panic::catch_unwind(AssertUnwindSafe(..))`. A panic becomes `AssetError::decode(path, "the loader panicked")`;
  - sends a result to the pump: the boxed typed value and the dependencies its `LoadContext` recorded.

  A job whose handle was dropped before it ran still runs, and its result is discarded.
- **Workers.** On native, `min(available_parallelism - 1, 4)` threads (at least 1), started at `AssetServer::new`, fed through an unbounded crossbeam channel. On wasm (`cfg(target_arch = "wasm32")`), jobs are queued, and the pump runs them until 4 ms (`web_time::Instant`) have passed in the frame.
- **The pump** drains results. For each:
  - insert into `Assets<T>` (`insert` fails silently for a stale id);
  - set the state to `Loaded`;
  - send `Added`.

  (Task 5 adds waiting for dependencies and running `check` before `Added`.) Failures set `Failed(Arc<error>)` and send `Failed`, and are logged once per asset with `engine_warn!`. The pump then calls each registered type's `free_unheld`, sends `Removed` for each freed id, and drops the freed keys and states.
- **`load_now`:**
  - runs the job inline on the calling thread (dependencies loaded now as well, through `LoadContext` in `Now` mode);
  - inserts and sends `Added` at once;
  - if a queued or running job exists for the same key, marks it stale, so its result is dropped when it arrives;
  - returns the error on failure, and leaves the state `Failed`.
- **The cycle stack.** `LoadContext` in `Now` mode keeps a stack of paths. Loading a path already on the stack is `AssetError::Cycle` naming the chain (`a.ui.ron → b.ui.ron → a.ui.ron`).
- **Stats** come from the states table. `TypeStats::name` is the type's short name.

- [ ] **Step 1: Write the failing tests** in `server.rs`'s `tests` module. A test asset `Text(String)` gets a `TextLoader` for `"txt"` (UTF-8 or `Decode`), a `BoomLoader` for `"boom"` that panics, and a `Settings`-using loader `UpperLoader` for `"up"` with `struct Upper(bool)`. A helper makes a world with an `AssetServer::new(source)`, `register_asset::<Text>` and the loaders, and `pump_until(world, cond)` pumps up to 200 times with 1 ms sleeps.

```rust
  #[test]
  fn a_load_returns_at_once_and_the_pump_inserts_the_asset() {
    let source = MemorySource::new();
    source.insert("a.txt", "hello");
    let (mut world, server) = world_with(source);
    let handle: Handle<Text> = server.load("a.txt");
    assert!(matches!(server.state(&handle), LoadState::Loading));
    pump_until(&mut world, |w| w.resource::<Assets<Text>>().get(&handle).is_some());
    assert!(server.state(&handle).is_loaded());
    assert_eq!(world.resource::<Assets<Text>>().get(&handle).map(|t| t.0.as_str()), Some("hello"));
  }

  #[test]
  fn loading_one_path_twice_gives_one_asset() {
    let source = MemorySource::new();
    source.insert("a.txt", "hello");
    let (_world, server) = world_with(source);
    let a: Handle<Text> = server.load("a.txt");
    let b: Handle<Text> = server.load("./a.txt");
    assert_eq!(a, b);
  }

  #[test]
  fn settings_are_part_of_the_key() {
    let source = MemorySource::new();
    source.insert("a.up", "hi");
    let (mut world, server) = world_with(source);
    let plain: Handle<Text> = server.load_with("a.up", Upper(false));
    let upper: Handle<Text> = server.load_with("a.up", Upper(true));
    assert_ne!(plain, upper);
    pump_until(&mut world, |w| w.resource::<Assets<Text>>().len() == 2);
    assert_eq!(world.resource::<Assets<Text>>().get(&upper).map(|t| t.0.as_str()), Some("HI"));
  }

  #[test]
  fn a_missing_file_fails_with_not_found() { /* load "nope.txt"; pump; state Failed(NotFound{path}) */ }

  #[test]
  fn a_path_with_no_loader_fails_with_no_loader() { /* "a.xyz" -> NoLoader { extension: "xyz" } */ }

  #[test]
  fn a_path_leaving_the_root_is_invalid() { /* "../a.txt" and "/a.txt" -> InvalidPath */ }

  #[test]
  fn a_panicking_loader_fails_its_asset_and_the_workers_live_on() {
    let source = MemorySource::new();
    source.insert("a.boom", "x");
    source.insert("b.txt", "fine");
    let (mut world, server) = world_with(source);
    let boom: Handle<Text> = server.load("a.boom");
    let fine: Handle<Text> = server.load("b.txt");
    pump_until(&mut world, |_| server.state(&boom).is_failed() && server.state(&fine).is_loaded());
    assert!(matches!(server.state(&boom).error().map(|e| &**e), Some(AssetError::Decode { .. })));
  }

  #[test]
  fn load_now_inserts_at_once_and_wins_over_a_queued_load() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let queued: Handle<Text> = server.load("a.txt");
    source.insert("a.txt", "two");
    let now = server.load_now::<Text>(&mut world, "a.txt").map_err(|e| e.to_string());
    assert_eq!(now.as_ref().ok(), Some(&queued), "one asset");
    assert_eq!(world.resource::<Assets<Text>>().get(&queued).map(|t| t.0.as_str()), Some("two"));
    for _ in 0..50 { pump(&mut world); std::thread::sleep(Duration::from_millis(1)); }
    assert_eq!(world.resource::<Assets<Text>>().get(&queued).map(|t| t.0.as_str()), Some("two"),
      "the queued load's older result is dropped");
  }

  #[test]
  fn events_tell_added_failed_and_removed() { /* MessageReader over AssetEvent<Text>: Added for a loaded, Failed for a missing, Removed after the handle drops and a pump runs */ }

  #[test]
  fn errors_keep_their_source() {
    // An Io error's source() is the io::Error; a Decode error's is the loader's error
  }

  #[test]
  fn progress_and_stats_count_a_group() { /* 2 loaded, 1 failed, 1 pending (a file the source holds back: a test source that blocks until told) */ }
```

  In `path.rs`: normalisation cases (`"a//b/./c.png"` → `"a/b/c.png"`, `"a\\b.png"` → `"a/b.png"`, `"a/../b.png"` → `"b.png"`, `"../b.png"` and `"/b.png"` invalid), and `extensions()` for `"x/menu.ui.ron"` → `["ui.ron", "ron"]`.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib asset::`. Expected: FAIL.
- [ ] **Step 3: Implement** to the interfaces and rules.
- [ ] **Step 4: Run** `cargo test -p ryo2d --lib asset::`, then `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/asset src/core/error.rs src/core/text/fonts.rs src/core/ui/file/error.rs src/core/testing/ui.rs examples/games/breakout/src/look.rs
git commit -m "feat: an asset server that loads by path through loaders on worker threads"
```

---

### Task 5: Dependencies, readiness and reloads

**Files:**
- Modify: `src/core/asset/{server.rs, loader.rs, pump.rs}`

**Interfaces (produced):**

```rust
impl AssetServer {
  /// Loaded, with every dependency loaded too (recursively)
  pub fn is_ready<T: Asset>(&self, id: impl Into<AssetId<T>>) -> bool;
  /// The state counting dependencies: `Loading` until all are loaded,
  /// `Failed` with the first dependency's error wrapped in `Dependency`
  pub fn dependency_state<T: Asset>(&self, id: impl Into<AssetId<T>>) -> LoadState;
  /// Reloads every asset loaded from `path` (each settings variant) on the workers
  pub fn reload(&self, path: &str);
}
```

**Rules:**
- **Dependencies.**
  - A dependency loaded through `LoadContext` is a strong `Handle` that the parent's value holds (the loader stores it in the asset). The server also records `deps: Vec<UntypedAssetId>` on the parent and `dependents` on each dependency, by id only.
  - A parent whose result arrived waits, uninserted, until its dependencies are all `Loaded`.
  - Then the pump calls `check` with `&World`. On `Ok` it inserts and sends `Added`. On error it sets `Failed` and sends `Failed`.
  - A dependency that fails fails its parent with `Dependency { dependency, error }`.
- **Readiness.** `is_ready` and `dependency_state` walk `deps` recursively, with a visited set so a diamond is counted once.
- **Reload.**
  - `reload(path)`, and every path a source's `watch` sends (drained by the pump), queue a reload job per key with that path.
  - The result keeps the same id. The pump waits for its dependencies (new ones may load), runs `check`, then swaps the value in (`Assets::insert` on the loaded slot moves the version on) and sends `Modified`.
  - It then runs `check` for each dependent, transitively, outer last. A dependent that passes gets `Modified` (its version moves on with `get_mut`), so caches built from it rebuild. One that fails gets `ReloadFailed`, and warns once per (id, error text).
  - A reload whose read, load or `check` fails keeps the old value and state `Loaded`, sends `ReloadFailed` and warns once per (id, error text).
  - A path that changes while its reload is in flight reloads again when the first finishes, so the last write wins.
- **Freeing a parent** drops its value, and with it the dependency handles it holds. Each dependency is then freed by the usual path, once nothing else holds it.

- [ ] **Step 1: Write the failing tests** (in `server.rs`). A test `Bundle` asset's loader (`"bundle"`: lines of paths) loads each line as a `Text` dependency through `cx.load` and stores the handles. A `check` that fails when any dependency's text is `"bad"` checks it against `Assets<Text>`.

```rust
  #[test]
  fn a_parent_is_added_only_once_its_dependencies_are_loaded() { /* Added<Bundle> comes after both Added<Text>; is_ready true then */ }

  #[test]
  fn a_failed_dependency_fails_its_parent_with_dependency() { /* one line names a missing file: Bundle Failed(Dependency{dependency: "missing.txt", ..}) */ }

  #[test]
  fn a_reload_keeps_the_id_and_moves_the_version_on() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let handle = server.load_now::<Text>(&mut world, "a.txt").expect("loads");
    let before = world.resource::<Assets<Text>>().version(&handle);
    source.insert("a.txt", "two"); // MemorySource tells the server
    pump_until(&mut world, |w| w.resource::<Assets<Text>>().get(&handle).map(|t| t.0.as_str()) == Some("two"));
    assert!(world.resource::<Assets<Text>>().version(&handle) > before);
  }

  #[test]
  fn a_broken_reload_keeps_the_last_good_value() { /* insert invalid UTF-8; ReloadFailed sent; value still "one"; state Loaded */ }

  #[test]
  fn a_reloaded_dependency_tells_its_dependents() { /* Bundle's version moves on and Modified<Bundle> is sent when a.txt reloads */ }

  #[test]
  fn a_dependency_reload_that_fails_its_dependents_check_is_reported() { /* a.txt becomes "bad": Modified<Text>, ReloadFailed<Bundle> */ }

  #[test]
  fn freeing_a_parent_frees_dependencies_nothing_else_holds() { /* drop Bundle handle: after two pumps, both Texts are Removed unless one is held separately */ }

  #[test]
  fn load_now_loads_dependencies_now_and_rejects_cycles() { /* a.bundle names b.bundle names a.bundle: Err(Cycle{chain:"a.bundle → b.bundle → a.bundle"}) */ }
```

- [ ] **Step 2: Run** `cargo test -p ryo2d --lib asset::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p ryo2d --lib asset::`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/asset
git commit -m "feat: assets wait for their dependencies, and reload in place under the same id"
```

---

### Task 6: The engine runs the registry

**Files:**
- Create: `src/core/asset/view.rs`
- Modify:
  - `src/core/engine/mod.rs`: `Engine::new` and `init_world_resources` insert the server; the pump goes in `Phase::PreUpdate` before `asset_pump_system`; builder fns;
  - `src/core/engine/frame.rs`;
  - `src/core/scene/{context.rs, mod.rs}` (`init_world_resources_for_tests`);
  - `src/core/testing/{headless.rs, ui.rs}`;
  - `src/lib.rs`: the prelude exports `Handle, AssetId, UntypedAssetId, Assets, AssetServer, Asset, AssetLoader, AssetSettings, LoadContext, LoadState, AssetEvent, AssetPath, AssetSource, DiskSource, MemorySource, Progress, AssetsView`, and the new `AssetError` as `AssetError`.

**Interfaces (produced):**

```rust
// view.rs
/// The asset registry seen from a scene or a test: loads, gets and adds
/// assets of every type
pub struct AssetsView<'w> { world: &'w mut World, server: AssetServer }
impl<'w> AssetsView<'w> {
  pub fn load<T: Asset>(&self, path: &str) -> Handle<T>;
  pub fn load_with<T: Asset, S: AssetSettings>(&self, path: &str, settings: S) -> Handle<T>;
  pub fn load_now<T: Asset>(&mut self, path: &str) -> Result<Handle<T>, Arc<AssetError>>;
  pub fn load_now_with<T: Asset, S: AssetSettings>(&mut self, path: &str, settings: S) -> Result<Handle<T>, Arc<AssetError>>;
  pub fn add<T: Asset>(&mut self, value: T) -> Handle<T>;
  pub fn get<T: Asset>(&self, id: impl Into<AssetId<T>>) -> Option<&T>;
  pub fn get_mut<T: Asset>(&mut self, id: impl Into<AssetId<T>>) -> Option<&mut T>;
  pub fn state<T: Asset>(&self, id: impl Into<AssetId<T>>) -> LoadState;
  pub fn is_ready<T: Asset>(&self, id: impl Into<AssetId<T>>) -> bool;
  pub fn progress(&self, ids: &[UntypedAssetId]) -> Progress;
  pub fn stats(&self) -> AssetStats;
  pub fn server(&self) -> &AssetServer;
}
// SceneContext
impl SceneContext<'_> { pub fn assets(&mut self) -> AssetsView<'_>; }
// HeadlessEngine / UiHarness
impl HeadlessEngine { pub fn assets(&mut self) -> AssetsView<'_>; }
impl UiHarness { pub fn assets(&mut self) -> AssetsView<'_>; } // replaces today's `assets()` in Task 13; until then name it `registry()`
// EngineBuilder
impl EngineBuilder {
  /// Adds a loader for the game's own asset type (and registers the type)
  pub fn asset_loader<L: AssetLoader>(self, loader: L) -> Self;
  /// Loads assets from `source` instead of the project's asset folder
  pub fn asset_source(self, source: impl AssetSource) -> Self;
  /// Watches the asset folder and reloads changed files (default: on in debug builds)
  pub fn hot_reload(self, on: bool) -> Self;
}
```

**Rules:**
- **The default source** is `DiskSource::new(Workspace::discover().assets_dir())`, discovered once in `Engine::new` and passed down. Reuse the discovery `Engine::new` already does for hot reload (`assets-plan-facts.md` C9). Headless engines use the same source.
- **The pump** is an exclusive system in `Phase::PreUpdate`, before `asset_pump_system`, which stays until Task 13.
- **`ctx.assets()`** builds the view from `self.world` and a clone of the world's `AssetServer` (cheap). `SceneContext`'s existing `assets: &mut AssetManager` field stays until Task 13. A field and a method may share a name.
- **The test world:** `init_world_resources_for_tests` and `UiHarness` insert an `AssetServer` over an empty `MemorySource`, with the types registered so far.
- **Registering types:** asset types are registered by the tasks that add them (Images in Task 8, and so on). This task registers none, apart from the tests' own.

- [ ] **Step 1: Write the failing tests.**
  - `tests/e2e/assets.rs` (new; add `mod assets;` to `tests/e2e.rs`):
    - a headless engine built with `.asset_source(memory)` and `.asset_loader(TextLoader)` (a test loader in the file) loads `"a.txt"` in a scene's `enter` through `ctx.assets().load_now`, and the scene reads it back in `update`;
    - a `load` from `update` is `Loaded` within 10 frames;
    - dropping the scene's handle on exit sends `AssetEvent::Removed` within 2 frames.
  - A unit test: an engine with nothing to load runs 100 frames, and the pump allocates nothing. Use the counting allocator pattern of `tests/ui_cache_alloc.rs`, in a new `tests/asset_pump_alloc.rs` with its own `#[global_allocator]`.
- [ ] **Step 2: Run** `cargo test --test e2e assets` and `cargo test --test asset_pump_alloc`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests above, then `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/asset/view.rs src/core/asset/mod.rs src/core/engine/mod.rs src/core/engine/frame.rs src/core/scene/context.rs src/core/scene/mod.rs src/core/testing/headless.rs src/core/testing/ui.rs src/lib.rs tests/e2e/assets.rs tests/e2e.rs tests/asset_pump_alloc.rs Cargo.toml
git commit -m "feat: the engine runs the asset registry, and scenes reach it through ctx.assets()"
```

---

### Task 7: Watching the asset folder

**Files:**
- Create: `src/core/asset/watch.rs` (native only)
- Modify: `src/core/asset/source.rs` (`DiskSource::watch`), `src/core/engine/mod.rs` (start watching when hot reload is on)

**Rules:**
- **Watching.**
  - `DiskSource::watch` canonicalises the root once (`std::fs::canonicalize`, falling back to the root as given), then starts a `notify::recommended_watcher` over it, recursive.
  - The notify callback thread maps each event path to an asset path:
    - strip the canonical root, or failing that the root as given;
    - normalise with `AssetPath::parse`;
    - ignore paths outside the root, directories, and files whose name starts with `.` or ends with `~`.
  - It sends `(AssetPath, Instant)` to a debounce thread.
  - Event kinds: create, modify and rename-to are changes; remove is ignored, so the last value stays.
- **The debounce thread** keeps the latest event time per path. It forwards a path to the server's changed channel once 100 ms have passed since that path's last event, checking every 25 ms (`recv_timeout`).
- **Lifetime:** the watcher and its threads stop when the `DiskSource` drops.
- **Turning it on:** the engine starts watching when `hot_reload` is on (default `cfg!(debug_assertions)`) and the build is not headless. `EngineBuilder::hot_reload(true)` turns it on for headless builds too, which the tests use.

- [ ] **Step 1: Write the failing tests** (in `watch.rs`, `#[cfg(test)]`, native; each test uses a `tempfile::TempDir` as root and a `Text` asset server with a `DiskSource`, and pumps with sleeps up to 3 s):
  - `a_saved_file_reloads_under_the_same_handle`;
  - `two_writes_in_the_debounce_reload_once_with_the_last`: write `"a"`, then 30 ms later write `"b"`. Exactly one `Modified` arrives, and the value is `"b"`;
  - `a_half_written_image_keeps_the_last_good_one`: in this task, with the `Text` loader and invalid UTF-8; Task 8 repeats it with a truncated PNG. `ReloadFailed`, and the last value stays;
  - `a_change_under_a_symlinked_root_reloads`: the root is given through a symlink (`std::os::unix::fs::symlink`, `#[cfg(unix)]`) to the temp dir;
  - `a_deleted_file_keeps_its_last_value`;
  - `editor_temp_files_are_ignored`: writing `.a.txt.swp` and `a.txt~` reloads nothing.
- [ ] **Step 2: Run** `cargo test -p ryo2d --lib asset::watch`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests above, three times (they touch the filesystem's timing). Expected: PASS each time.
- [ ] **Step 5: Commit.**

```bash
git add src/core/asset/watch.rs src/core/asset/source.rs src/core/asset/mod.rs src/core/engine/mod.rs
git commit -m "feat: the asset folder is watched, and a saved file reloads once after its last write"
```

---

### Task 8: Images

The largest task: the image loader, the renderer keyed by `AssetId<Image>`, every holder of `SpriteHandle` moved to `Handle<Image>`, and every caller migrated. It may take several commits; the workspace must build and pass at each one.

Read first: `assets-plan-facts.md` §2 (holders), §3 (renderer paths and where `World` is available), C2, C3 and C6, and Appendices A to C.

**Files:**
- Create: `src/core/asset/loaders/image.rs`
- Modify:
  - `src/core/graphics/{image.rs, texture_registry.rs, sprite_gpu.rs, atlas.rs, parallax.rs, tile.rs, animation.rs, gpu_context.rs}`;
  - `src/core/render/**` (entities, text, look, tonemap, ui_renderer/build.rs, ui_retain);
  - `src/core/ecs/renderable.rs`, `src/core/particles/**`, `src/core/trail/**`, `src/core/systems/animation.rs`;
  - `src/core/ui/**`: commands, tape, tree, fragment, widgets, nodes, file/build.rs (`SpriteSource` returns `Handle<Image>`), theme `Skin`;
  - `src/core/text/{fonts.rs, icons.rs, rich.rs}` (the bitmap sheet and icons hold `Handle<Image>`);
  - `src/core/scene/context.rs` (`set_custom_cursor`), `src/core/engine/extract.rs`;
  - `src/core/asset/manager.rs` (it now loads images through the registry: `load_sprite` becomes a thin call to `server.load_now`, kept until Task 13);
  - `src/core/testing/**`;
  - examples, tests and benches (Appendices B to D).

**Interfaces (produced):**

```rust
// graphics/image.rs: `data: Vec<u8>` becomes `data: Arc<Vec<u8>>`; `data()` is unchanged;
// pixel edits go through `data_mut(&mut self) -> &mut [u8]` (Arc::make_mut)
impl Asset for Image {}
impl Image {
  /// Decodes an image file's bytes (any raster format the `image` crate is built with)
  pub fn decode(bytes: &[u8]) -> Result<Image, ImageError>;
  /// The pixels, shared: the renderer takes this at extract time
  pub(crate) fn pixels(&self) -> &Arc<Vec<u8>>;
}
// loaders/image.rs
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum SvgSize { #[default] Native, Scale(f32), Fit { width: u32, height: u32 } }
impl Eq for SvgSize {} impl Hash for SvgSize { /* f32 by to_bits */ }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ImageSettings { pub svg: SvgSize }
pub struct ImageLoader; // extensions: "png","jpg","jpeg","bmp","gif","webp","tga","tif","tiff","qoi","ico","svg"
// ecs/renderable.rs
pub struct Renderable { pub image: Handle<Image>, /* .. unchanged */ }
impl Renderable { pub fn image(image: Handle<Image>) -> Self; /* replaces Renderable::sprite */ }
```

**Rules:**
- **Loading images.**
  - `ImageLoader` decodes rasters with `image::load_from_memory(..).to_rgba8()`, and SVG with resvg per `SvgSize`: `Native` is scale 1, `Scale(s)`, or `Fit` into the box keeping the aspect ratio, as `load_svg_fit` does.
  - For an image over 256 px on either side (it gets its own texture, `MAX_ATLAS_IMAGE`), the loader also builds the mip chain the atlas would build. It stores it in a private `Image::mips: Option<Arc<Vec<Vec<u8>>>>` that the atlas uses instead of calling `mip_chain` on the main thread. An image changed with `data_mut` drops its prepared mips.
  - `Image::load`, `load_svg`, `load_svg_fit`, `from_svg_data` and `from_svg_data_fit` are removed. Files go through the loader; `from_svg_data*` become private helpers of the loader.
- **The renderer holds ids, not handles.**
  - `TextureSource::Sprite(SpriteHandle)` becomes `TextureSource::Image { id: AssetId<Image>, pixels: Arc<Vec<u8>>, mips: Option<..>, version: u64 }`.
  - `by_sprite` becomes `by_image: HashMap<AssetId<Image>, TextureId>`.
  - The registry entry's `uploaded_content` compares with the image's version.
  - The extract step (`engine/extract.rs`, which has `&World`) passes `&Assets<Image>` to everything that turns a `Handle<Image>` into a texture:
    - `texture_of(id, &Assets<Image>)`;
    - the entity drawer;
    - parallax, trails and particles;
    - `render/text.rs` (icons, bitmap sheets);
    - `ui_renderer/build.rs`;
    - the look's LUT;
    - the custom cursor.

    Where a function had no world before (facts §3: `ui_renderer/build.rs:1172`, `render/text.rs:1083`), it takes `images: &Assets<Image>` from its caller in extract.
  - **Missing images:** an id that is not loaded (not yet, failed, or stale) draws the engine's checker image in debug builds and is skipped in release. The checker is a 16×16 magenta and black checker, added at startup and held by the engine.
- **Freeing GPU memory.** The renderer reads `AssetEvent<Image>::Removed` (drained by `extract` from the world's messages and passed in), and calls `registry.remove(id)` and the atlas's slot release at once. The 600-frame TTL no longer applies to images; it stays for named pixel textures and render targets.
- **Everything else that held a `SpriteHandle` holds `Handle<Image>`** (facts §2 table): `ParallaxLayer.sprite`, `Skin.sprite`, `Animation.frames`, `ParticleEmitter.sprite`, `Trail.texture`, `TilePart.sprite`, `ColorLut.sprite`, `BitmapFace.sprite`, `IconSource.sprite`, `Icon::Sprite`, `UiCommand::Sprite`, `Op/Read::Sprite`, `DrawKind::Sprite/NineSlice`, `UiNode` image and icon, `ImageBox`, button and slot icons, the drag ghost, the prompt, `BatchCollector`.
- **Comparisons and change keys:**
  - equality and hashing that used `serial()` use the id;
  - change keys that used `(serial, generation)` use `(id, version)`. These are the UI tape's `generation`, `fragment.rs`'s generation compare, `ui_retain`'s hash, `bitmap_stamp` and the tonemap LUT key;
  - the UI pass reads image sizes through `&Assets<Image>`. Give `UiContext` an `images: &Assets<Image>` taken with `world.resource_scope` around the UI pass, and the node walk the same.
- **Callers:**
  - `ctx.assets.create_sprite(img, name)` and `SpriteHandle::from_sprite(img, name)` become `ctx.assets().add(img)`;
  - unit tests with no world use `let mut images = Assets::<Image>::default(); images.add(..)` and pass `&images` where now needed;
  - `ctx.assets.load_sprite(p)` and `Sprite::load(p)` become `ctx.assets().load_now::<Image>(path without "assets/")`;
  - SVG loads become `load_now_with(path, ImageSettings { svg: SvgSize::Fit { .. } })`;
  - "simulated hot reload" tests that called `SpriteHandle::update` (C3) use `images.get_mut(&h)` and replace the pixels, or `server.reload`.
- **Removed:** `SpriteHandle`, `AsyncSpriteHandle`, `create_sprite`, `cache_sprite_handle`, `gc_sprites`, `clear_sprite_cache`, `is_sprite_cached`, `start_loading`, `loading_progress` and the other sprite-only `AssetManager` methods. The asset-loading demo moves to `load`, `progress` and `is_ready` (it is rewritten fully in Task 13).

- [ ] **Step 1: Write the failing tests:**
  - **`loaders/image.rs`** (with `MemorySource`):
    - a PNG made by `image::RgbaImage` decodes to the same pixels;
    - an SVG at `Native`, `Scale(2.0)` and `Fit { 32, 32 }` gives the sizes `load_svg*` gave;
    - a truncated PNG is `Decode`;
    - a 512×512 PNG has prepared mips equal to `atlas::mip_chain`'s.
  - **GPU tests** (`src/core/graphics/sprite_gpu.rs` tests or `tests/e2e/assets.rs`, `HeadlessEngine::new_or_skip`):
    - `an_unheld_image_frees_its_texture`: draw an image for 3 frames, drop every handle, step 2 frames. The texture registry no longer has it (a `#[doc(hidden)] HeadlessEngine::texture_count()` test hook), and its atlas slot is free;
    - `a_scene_change_that_keeps_an_image_does_not_upload_it_again`: Review Focus 3. Scene A holds the image and pushes scene B, whose `enter` loads the same path, then A is dropped. The upload count is unchanged (a `#[doc(hidden)]` counter);
    - `a_reloaded_image_draws_its_new_pixels`, and `a_reloaded_image_of_another_size_is_placed_again`: 64×64 becomes 128×128 while drawn; pixels and size are right;
    - `an_unloaded_image_draws_the_checker_in_debug`: assert the checker colour at the sprite's centre under `cfg(debug_assertions)`, and nothing drawn otherwise.
  - **Watch** (Task 7's file): `a_half_written_image_keeps_the_last_good_one`, with a truncated PNG.
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement**, holder by holder. Build often with `cargo check --workspace --all-targets`.
- [ ] **Step 4: Run** `cargo test --workspace`, then the tour against Task 1's baseline. Expected: PASS, and 0 deterministic pages differ.
- [ ] **Step 5: Run** `cargo bench --bench assets` (port its loads to `load_now::<Image>`) alternately with Task 1's build, two rounds each, with `uptime`. Report the result. A slowdown over 3% goes to the controller with a profile.
- [ ] **Step 6: Commit** (one or more commits, each green). The last one is:

```bash
git commit -m "feat: images are registry assets, and the renderer frees an image's texture with its last handle"
```

---

### Task 9: Shaders

**Files:**
- Create: `src/core/asset/loaders/shader.rs`
- Modify:
  - `src/core/ecs/material.rs` (`ShaderHandle` removed; a `Shader` asset);
  - `src/core/shaders/{manager.rs, pipeline.rs, custom.rs}`, `src/core/graphics/sprite_gpu.rs` (`CustomShaders`), `src/core/render/batch/*`, `src/core/render/entities.rs`, `src/core/graphics/gpu_context.rs`;
  - `src/core/asset/manager.rs` (shader parts removed);
  - `examples/demos/{post-processing, sprite-shaders}`, `tests/e2e/custom_shaders.rs`, `tests/e2e/post_process.rs`, `src/core/testing/gpu_world.rs`.

**Interfaces (produced):**

```rust
/// WGSL source for a sprite shader or a post-process effect
#[derive(Debug, Clone)]
pub struct Shader { name: Arc<str>, source: Arc<str> }
impl Asset for Shader {}
impl Shader {
  pub fn from_wgsl(name: impl Into<Arc<str>>, source: impl Into<Arc<str>>) -> Self;
  pub fn name(&self) -> &str;
  pub fn source(&self) -> &str;
}
pub struct ShaderLoader; // "wgsl"; the name is the asset path
// SpriteShader::Custom { shader: Handle<Shader>, params }
// EffectsManager::add_custom(name, Handle<Shader>), add_custom_in(name, Handle<Shader>, stage)
```

**Rules:**
- **Keys.** `CustomShaders` and `CustomEffectPipelines` key their entries by `AssetId<Shader>` and recompile when `Assets<Shader>::version` changes. The extract passes `&Assets<Shader>`, as Task 8 did for images.
- **Freeing.** On `AssetEvent<Shader>::Removed`, both drop the entry at once, and the batch's `pipelines.retain` (facts C7) then drops its pipeline in the same frame. The 600-frame TTL stays only for compiled sources, which are shared by text.
- **Errors** are reported once per (id, version), with the asset path as the file name, as today.
- **Embedded WGSL:** `ShaderHandle::from_source(id, src)` becomes `ctx.assets().add(Shader::from_wgsl(id, src))`.

- [ ] **Step 1: Write the failing tests:**
  - in `loaders/shader.rs`: a `.wgsl` loads with its path as name;
  - in `tests/e2e/custom_shaders.rs`, migrated: `a_reloaded_shader_recompiles` (reload through a `MemorySource` insert) and `an_unheld_shader_drops_its_pipeline` (a `#[doc(hidden)]` pipeline count).
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git commit -m "feat: shaders are registry assets, and a freed shader's pipelines go with it"
```

---

### Task 10: Fonts

**Files:**
- Create: `src/core/asset/loaders/{font.rs, bitmap_font.rs}`
- Modify:
  - `src/core/text/{fonts.rs, engine.rs, style.rs, raster.rs}`, `src/core/graphics/glyph_atlas.rs`;
  - `src/core/render/text.rs`;
  - the five games' `look.rs`;
  - text tests (facts §5).

**Interfaces (produced):**

```rust
/// One font face: a TTF/OTF file, or one face of a TTC
#[derive(Debug, Clone)]
pub struct Font { data: Arc<[u8]>, index: u32, family: Arc<str> }
impl Asset for Font {}
impl Font {
  pub fn from_bytes(data: impl Into<Arc<[u8]>>) -> Result<Font, FontError>;
  pub fn from_bytes_face(data: impl Into<Arc<[u8]>>, index: u32) -> Result<Font, FontError>;
  pub fn family(&self) -> &str;
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FontSettings { pub face: u32 }
pub struct FontLoader; // "ttf", "otf", "ttc"
/// A font drawn from a sheet image of fixed cells
#[derive(Debug, Clone)]
pub struct BitmapFont { pub family: String, pub sheet: Handle<Image>, pub cell: (u32, u32), pub chars: String }
impl Asset for BitmapFont {}
pub struct BitmapFontLoader; // "bitmap.ron": `(sheet: "fonts/pixel.png", cell: (8, 8), chars: " !\"#..", family: "Pixel")`
// text/style.rs
pub enum TextFont { Role(FontRole), Family(String), Face(Handle<Font>) } // `Id(FontId)` removed
// text/fonts.rs
impl Fonts {
  pub fn set_role(&mut self, role: FontRole, font: Handle<Font>); // holds it
  pub fn role_font(&self, role: FontRole) -> Option<&Handle<Font>>;
  // add_bytes, add_face, load and add_bitmap are removed; render, family, find, find_face, prewarm stay
}
```

**Rules:**
- **Registering.**
  - The pump's handling of `AssetEvent<Font>` (a system right after the pump, `fonts_follow_assets`) registers an `Added` font's face in `Fonts`, and removes a `Removed` font's face. `Fonts.faces` becomes `Vec<Option<Face>>`, and ids are never reused.
  - A `Modified` font replaces its face's data.
  - `Fonts::generation` moves on with each change, so `TextEngine::sync` rebuilds Parley's collection as it does today (facts C5).
  - `BitmapFont` assets are registered the same way. Their `bitmap_stamp` uses the sheet's (id, version).
- **Text styles.** `TextFont::Face(h)` resolves `h.id()` through `Fonts`' map from `AssetId<Font>` to face. A face that is not registered resolves like an unknown family: fall back, and warn once. That is Review Focus 5.
- **Built-in fonts** are added at startup (`Font::from_bytes(include_bytes!(..))` and `assets.add`). The engine holds their handles in `Fonts`' role table and in a `BuiltinFonts` resource, so they are never freed.
- **Glyphs.**
  - On `Removed`, the glyph atlas drops every glyph whose `GlyphKey.font` is the freed face. Add `GlyphShelves::forget_font(FontId)`: remove its entries, and mark the shelf space unused.
  - A page whose last glyph goes is freed: it is removed from `pages`, and its texture is dropped at the next prepare. This is new code (facts C4).
  - Pinned glyphs of this frame are never dropped mid-frame. Removal happens in the pump, before the frame's text is laid out.
- **Games.** `let font = ctx.fonts().add_bytes(PRESS_START_2P)?` and `set_role(FontRole::Ui, font)` become:

```rust
let font = ctx.assets().add(Font::from_bytes(PRESS_START_2P)?);
ctx.fonts().set_role(FontRole::Ui, font);
```

  The role holds the handle.

- [ ] **Step 1: Write the failing tests:**
  - in `loaders/font.rs`: a `.ttf` loads with its family; a `.ttc` with `FontSettings { face: 1 }`; a corrupt file is `Decode`;
  - in `loaders/bitmap_font.rs`: a descriptor loads, with its sheet as a dependency (ready only when the sheet is);
  - in `text`, at the `UiHarness`/`TextEngine` level:
    - `a_loaded_font_is_used_by_its_family`;
    - `text_whose_font_was_freed_falls_back`: lay out text with `TextFont::Face(h)`, drop `h`, pump, lay out again. No panic, and it lays out with the fallback;
    - `a_freed_fonts_glyphs_leave_the_atlas` and `an_emptied_glyph_page_is_freed` (GPU, `HeadlessEngine::new_or_skip`);
    - `a_reloaded_bitmap_sheet_redraws_its_text`: migrated from `ui_retain/gpu_tests.rs:81-125`.
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` and the tour. Expected: PASS, and 0 deterministic pages differ.
- [ ] **Step 5: Commit.**

```bash
git commit -m "feat: fonts and bitmap fonts are registry assets, and a freed font leaves the glyph atlas"
```

---

### Task 11: UI files

Read first: facts §6, and `src/core/ui/AGENTS.md`'s Files section.

**Files:**
- Create: `src/core/asset/loaders/ui_file.rs`
- Modify:
  - `src/core/ui/file/{document.rs, build.rs, check.rs, error.rs}`, `src/core/ui/nodes/{reconcile.rs, list.rs, components.rs}`;
  - `src/core/scene/context.rs` (`spawn_ui_file`), `src/core/testing/ui.rs`;
  - `src/core/asset/manager.rs` (UI parts removed);
  - `src/cli/commands/check_ui.rs`;
  - the ui demo pages and its `.ui.ron` imports (`"assets/ui/x.ui.ron"` → `"ui/x.ui.ron"`), and breakout's `look.rs` and scenes;
  - `tests/integration/ui_file_validity.rs`, `tests/e2e/ui_files.rs`.

**Interfaces (produced):**

```rust
impl Asset for UiFile {}
pub struct UiFileLoader; // "ui.ron"
// UiFile keeps: parse(name, text) (embedded, its imports must be added or loadable), path(),
// root(&mut AssetsView) / template(name, args, &mut AssetsView) build through the registry.
// Document.imports: Vec<Handle<UiFile>>; Document.images: Vec<Handle<Image>>
// TreeSource.file: Handle<UiFile> (held by every tree built from it); TreeSource.generation -> version: u64
```

**Rules:**
- **The loader:**
  - parses the document;
  - loads each import through `cx.load::<UiFile>` and each image path the file names through `cx.load::<Image>`. In `Now` mode, cycles are rejected with `AssetError::Cycle`. A queued load whose imports form a cycle fails with `Cycle` too: detect it in `check`, by walking `imports` through `Assets<UiFile>`;
  - keeps the handles in the document.
- **`check`** is today's `UiFile::check`: it builds the root and every template once, against the imports' documents in `Assets<UiFile>` and the images in `Assets<Image>`. Its `UiFileError` becomes `AssetError::Decode { source: UiFileError }`, which keeps every issue's line and column. `UiFileError` gets a `std::error::Error` impl if it lacks one.
- **Reload:**
  - a file's reload is the registry's reload (Task 5). Its importers are dependents, so they are checked again, which replaces `recheck_importers`;
  - `reload_trees` compares `TreeSource.version` with `Assets<UiFile>::version` of the file and of each import, transitively, in place of `FileCell.generation`;
  - its rebuild and keep-as-was behaviour, and the `UiReloaded` messages, are unchanged.
- **Embedded files.** `add_ui(path, text)` becomes `ctx.assets().add(UiFile::parse(name, text)?)`. An embedded file's imports are resolved by `load_now` when it is added. An embedded file never reloads.
- **Breakout's fallback.** `load_now::<UiFile>(path)` returning `NotFound` falls back to the embedded text, as today. Its temp-CWD tests keep working because there is no file to find.
- **`ryo2d check-ui`** builds an `AssetServer` over `DiskSource::new(workspace.assets_dir())` with the UI file and image loaders. It then calls `load_now` for every `*.ui.ron` under the root, and prints every issue as today. The `ImportResolver`, `SpriteSource`, `Disk`, `DiskSprites` and `Placeholders` traits and impls are removed. A template parameter's stand-in image is a 1×1 image the build adds once.
- **List rows** built from a file (`RowShape::File`, `list.rs:63-68`) get their file through `Assets<UiFile>`, not `AssetManager`.

- [ ] **Step 1: Write the failing tests:**
  - in `loaders/ui_file.rs`, with `MemorySource`:
    - a file with an import and an image is ready only when both are;
    - an import cycle fails with `Cycle` (both `load_now` and `load`);
    - a bad template reference fails with its line and column;
  - `tests/e2e/ui_files.rs`, migrated: `a_saved_file_reloads_its_trees_in_place` (through a `MemorySource` insert, ids kept, `UiReloaded` sent); `a_broken_save_keeps_the_trees_and_warns_once`; `an_import_reload_rebuilds_its_importers_trees`;
  - `tests/integration/ui_file_validity.rs`: every repo `.ui.ron` loads through a `DiskSource` at its project's `assets/`.
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` (the ui demo's and breakout's oracles included) and the tour. Expected: PASS, and 0 deterministic pages differ.
- [ ] **Step 5: Commit.**

```bash
git commit -m "feat: UI files are registry assets, with their imports and images as dependencies"
```

---

### Task 12: Themes

**Files:**
- Create: `src/core/asset/loaders/theme.rs`
- Modify:
  - `src/core/ui/theme/{mod.rs, file.rs}`, `src/core/engine/{frame.rs, mod.rs}` (`refresh_ui_theme`, `ThemeWatch` removed, `theme_watch` field removed);
  - the ui demo's `theme_demo.rs` and its theme file (`assets/ui/theme.ron` → `assets/ui/demo.theme.ron`).

**Interfaces (produced):**

```rust
impl Asset for Theme {}
pub struct ThemeLoader; // "theme.ron"; parses as Theme::load did (merged over Theme::dark()), skins' images loaded through cx
impl UiTheme {
  /// Follows a theme asset: set now, and again whenever it reloads
  pub fn follow(&mut self, theme: Handle<Theme>);
}
// Skin keeps `path: Option<String>`; its image becomes `image: Option<Handle<Image>>`, loaded by the
// theme's loader for file themes and by `refresh_skins` (through the server) for themes made in code
```

**Rules:**
- **Following a file.** A `UiTheme` following a handle applies its value when it is `Added` or `Modified`, through `set`, which moves the generation on as today. A `ReloadFailed` keeps the theme and warns once per error.
- **Removed:** `Theme::load` and the `ThemeWatch` poll. `Theme::save` stays.
- **UI sounds:** paths are unchanged (they go to audio as today, until 5b).
- **Skins:** a code-made theme with `Skin::from_path(path, ..)` still loads its image when set, through `server.load_now::<Image>` in `refresh_ui_theme` (the existing `refresh_skins` hook).

- [ ] **Step 1: Write the failing tests:**
  - in `loaders/theme.rs`: a `.theme.ron` with a skin loads with the skin's image as a dependency; a partial file merges over dark, as today's 7 `file.rs` tests do (move them);
  - in `tests/e2e` or `UiHarness`: `a_followed_theme_reloads_when_its_file_changes` (`MemorySource` insert, and the theme generation moves on); `a_broken_theme_save_keeps_the_theme`.
- [ ] **Step 2: Run** them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` and the tour. Expected: PASS, and 0 deterministic pages differ.
- [ ] **Step 5: Commit.**

```bash
git commit -m "feat: themes are registry assets, followed through their reloads"
```

---

### Task 13: The old manager goes

**Files:**
- Delete: `src/core/asset/manager.rs`, `src/core/asset/hot_reload.rs`
- Modify:
  - `src/core/asset/mod.rs`, `src/core/error.rs` (`ManagerError` and `HotReloadError` removed);
  - `src/core/messages/{builtin.rs, mod.rs}` (`AssetReloaded`, `AssetLoadFailed` removed);
  - `src/core/engine/{mod.rs, frame.rs}` (`asset_pump_system` removed; `AssetManager` no longer inserted);
  - `src/core/scene/context.rs` (the `assets` field removed: `SceneContext` no longer scopes `AssetManager`);
  - `src/core/testing/ui.rs` (`registry()` becomes `assets()`; `with_assets(dir)` becomes `with_asset_source(DiskSource)`);
  - `src/lib.rs`;
  - `examples/demos/asset-loading` (rewritten);
  - `src/cli/commands/init.rs` (the template uses `ctx.assets().add(Image::solid_rect(..))`).

**Rules:**
- **No trace of the old API.** Nothing in `src/`, `examples/`, `tests/`, `benches/` or `src/cli` names `AssetManager`, `SpriteHandle`, `ShaderHandle`, `AsyncSpriteHandle`, `LoadingState`, `LoadingProgress`, `AssetReloaded`, `AssetLoadFailed`, `HotReloadWatcher` or `ManagerError` (grep in Step 2).
- **The asset-loading demo** shows:
  - a loading screen that `load`s every image, the fonts and a UI file, and draws `progress` as a bar;
  - per-asset states;
  - a deliberately missing file's `Failed` error text;
  - "edit `assets/` while running" with `AssetEvent::Modified` counts shown.

  It keeps the demo-kit standard (`examples/demos/AGENTS.md`).
- **The CLI test** in `install_skills.rs` stays green. `init`'s generated project builds; there is an existing test, so run it.

- [ ] **Step 1: Remove and rewrite** as above.
- [ ] **Step 2: Run** `grep -rn "AssetManager\|SpriteHandle\|ShaderHandle\|AsyncSpriteHandle\|LoadingProgress\|AssetReloaded\|AssetLoadFailed\|HotReloadWatcher\|ManagerError" src examples tests benches`. Expected: no hits outside comments that explain history (remove those too).
- [ ] **Step 3: Run** `cargo test --workspace`, and the tour against Task 1's baseline. The asset-loading demo's pages are expected to differ: report them. Expected: PASS otherwise.
- [ ] **Step 4: Commit.**

```bash
git commit -m "refactor: the old asset manager and its watcher are gone; everything loads through the registry"
```

---

### Task 14: Measuring the targets

**Files:**
- Modify: `benches/assets.rs` (`scene_start` on the registry, `scene_start_async`, `reload_hitch`); `docs/backlog/` (a new `assets.md`, a "5a: measured" section)

**The benches:**
- **`scene_start`:** Task 1's load, with `load_now`. Measure it alternately with Task 1's build: build that commit in a detached scratch worktree in the scratchpad with its own `CARGO_TARGET_DIR`, at least three rounds each way, with `uptime` on every run.
- **`scene_start_async`:** the same assets through `load`, pumped until `progress(..).is_done()`.
- **`reload_hitch`:**
  1. A `HeadlessEngine` (1280×720, `new_or_skip`) draws a 2048×2048 PNG and an SVG sprite. Its source is a `MemorySource`.
  2. Over 2,000 frames, every 50th frame inserts a changed PNG (and SVG) into the source.
  3. Read each frame's main-thread time: its CPU time before the render step, plus the render step's (`HeadlessEngine::render_time`, with the GPU finished before each step, as `benches/ui_render.rs`'s guard does).
  4. Report the median of ordinary frames, the median and the maximum of the frames where a reload landed, and their difference.

  The guard: the difference ≤ 0.5 ms (spec), printed with the xctrace command on a miss. It skips on a filter, `--list` or `--profile-time` (`benches/guards/mod.rs`).
- **Idle and empty frames:** run `benches/ui_render.rs`'s guard and `benches/foundation.rs` alternately against Task 1's build. Both must be within 3%.

**On a miss,** stop. Report the miss with a profile (`xcrun xctrace record --template 'Time Profiler'`). The controller brings it to the owner. No target is loosened.

- [ ] **Step 1: Write the benches** as above.
- [ ] **Step 2: Measure** as above, and write the backlog section with every median, the load, and the met or missed verdicts.
- [ ] **Step 3: Commit.**

```bash
git add benches/assets.rs docs/backlog/assets.md
git commit -m "test: the asset registry's scene-start and reload-hitch benches, and their measurements"
```

---

### Task 15: Docs and skills

**Files:**
- Modify:
  - module notes: `src/core/asset/AGENTS.md` (rewrite), plus `src/core/AGENTS.md`, `src/core/graphics/AGENTS.md`, `src/core/render/AGENTS.md`, `src/core/ui/AGENTS.md`, `src/core/text/AGENTS.md` (if present), `src/core/ecs/AGENTS.md`, `src/core/shaders/AGENTS.md`, `examples/demos/AGENTS.md`;
  - `docs/architecture/overview.md` (lines 35, 49, 123, 251, 382 and 408 name the old types);
  - `README.md`.
- Create: `.claude/skills/ryo-assets/SKILL.md` and `src/cli/skills/ryo-assets.md` (byte-identical), with an entry in `EMBEDDED_SKILLS` (`src/cli/commands/install_skills.rs:37`).
- Modify skills, each with its CLI copy byte-identical and its version bumped: ryo-api, ryo-text, ryo-ui, ryo-renderer, ryo-game-dev, ryo-engine-dev, ryo-animation, ryo-character, ryo-ecs-internals, ryo-lighting and ryo-particles (facts §1: every skill that names `Sprite`, `SpriteHandle` or `AssetManager`). Also fix ryo-api's claim that dropping a `MusicHandle` stops the music; it does not, with kira 0.11 (survey §7.4).

**What each says** (written as the files are written: each fact with its reason, no history):
- **The asset module notes:**
  - ids, handles and lifetimes, and why only handles keep assets;
  - stores and versions;
  - the server, keys and settings;
  - loaders, `check`, sources;
  - workers and the wasm budget;
  - the pump's order;
  - dependencies, reloads and the debounce;
  - the renderer holding ids and taking pixels by `Arc`;
  - how to add an asset type.
- **ryo-assets:**
  - `load` and `load_now`, and when to use which;
  - paths relative to `assets/`;
  - states, `progress` and loading screens;
  - keeping assets alive (hold the handle; resources for whole-game assets);
  - `add` for made-in-code and embedded assets (`Image::decode(include_bytes!(..))`, `Font::from_bytes`, `UiFile::parse`, `Shader::from_wgsl`);
  - SVG settings;
  - hot reload and `AssetEvent`;
  - a custom loader;
  - the debug checker.

- [ ] **Step 1: Write the docs.** Check every named type, function and path against the code. Check every skill pair with `cmp`. Run `cargo test -p ryo2d --lib cli::` for the skills test.
- [ ] **Step 2: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 3: Commit.**

```bash
git commit -m "docs: the asset registry in the module notes, the architecture overview and the skills"
```
