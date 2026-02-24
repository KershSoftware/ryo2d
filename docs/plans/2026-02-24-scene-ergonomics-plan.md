# Scene Ergonomics Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add ergonomic helpers for scene data passing and document the data-passing decision tree.

**Architecture:** Three small additions to the existing scene system — a `downcast_return` helper function, convenience methods on `SceneContext` for `Resources` access, and documentation updates. No architectural changes.

**Tech Stack:** Rust, existing `scene.rs` and `resources.rs` modules.

---

### Task 1: Add `downcast_return<T>()` Helper Function

**Files:**
- Modify: `src/core/scene.rs` (add function after `SceneAction` enum, around line 855)
- Modify: `src/core/scene.rs` (add test in `mod tests` block, around line 1929)

**Step 1: Write the failing test**

Add at the end of the `mod tests` block in `src/core/scene.rs` (before the closing `}`):

```rust
#[test]
fn downcast_return_some_correct_type() {
    let data: Option<Box<dyn Any>> = Some(Box::new(42u32));
    let result = downcast_return::<u32>(data);
    assert_eq!(result, Some(42u32));
}

#[test]
fn downcast_return_some_wrong_type() {
    let data: Option<Box<dyn Any>> = Some(Box::new(42u32));
    let result = downcast_return::<String>(data);
    assert_eq!(result, None);
}

#[test]
fn downcast_return_none() {
    let result = downcast_return::<u32>(None);
    assert_eq!(result, None);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib downcast_return`
Expected: FAIL — `downcast_return` not found.

**Step 3: Write the implementation**

Add this public function in `src/core/scene.rs`, after the `SceneAction` enum (after line 855), before the `Scene` trait:

```rust
/// Convenience helper for extracting typed data from `on_resume`'s returned parameter.
///
/// Combines the `Option` unwrap and `Any` downcast into a single call.
///
/// # Example
///
/// ```rust,ignore
/// fn on_resume(&mut self, _ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {
///     if let Some(level) = downcast_return::<u32>(returned) {
///         self.selected_level = level;
///     }
/// }
/// ```
pub fn downcast_return<T: Any>(returned: Option<Box<dyn Any>>) -> Option<T> {
    returned.and_then(|data| data.downcast::<T>().ok().map(|b| *b))
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib downcast_return`
Expected: 3 tests PASS.

**Step 5: Export from prelude**

In `src/lib.rs`, add `downcast_return` to the scene re-export (line 58-60):

```rust
pub use crate::core::scene::{
    Scene, SceneAction, SceneCleanup, SceneContext, SceneManager, SlideDirection, Transition,
    downcast_return,
};
```

**Step 6: Verify full build**

Run: `cargo build --lib`
Expected: Compiles clean.

**Step 7: Commit**

```bash
git add src/core/scene.rs src/lib.rs
git commit --author="Kersh <mattbreckon@gmail.com>" -m "feat(scene): add downcast_return<T>() helper for on_resume data extraction"
```

---

### Task 2: Add Resource Convenience Methods on SceneContext

**Files:**
- Modify: `src/core/scene.rs` (add methods to `impl SceneContext`, around line 221)
- Modify: `src/core/resources.rs` (add test for `get_or_default`)

**Step 1: Write the failing test for Resources::get_or_default**

Add at the end of the `mod tests` block in `src/core/resources.rs`:

```rust
#[test]
fn get_or_default_creates_default_when_missing() {
    let mut res = Resources::new();
    let val: &mut u32 = res.get_or_default();
    assert_eq!(*val, 0);
}

#[test]
fn get_or_default_returns_existing() {
    let mut res = Resources::new();
    res.insert(42u32);
    let val: &mut u32 = res.get_or_default();
    assert_eq!(*val, 42);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib get_or_default`
Expected: FAIL — `get_or_default` not found.

**Step 3: Add `get_or_default` to Resources**

In `src/core/resources.rs`, add after the `get_or_insert_with` method (after line 72):

```rust
/// Returns a mutable reference to the resource of type `T`, inserting
/// `T::default()` if none exists.
pub fn get_or_default<T: Any + Default>(&mut self) -> &mut T {
    self.get_or_insert_with(T::default)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib get_or_default`
Expected: 2 tests PASS.

**Step 5: Add convenience methods to SceneContext**

In `src/core/scene.rs`, add these methods inside `impl<'a> SceneContext<'a>` (after the `display_mode()` method, around line 274):

```rust
// ===========================================================================
// Resource Access (Convenience)
// ===========================================================================

/// Get a shared reference to a resource by type.
///
/// Shorthand for `ctx.resources.get::<T>()`.
///
/// # Example
///
/// ```rust,ignore
/// if let Some(progress) = ctx.resource::<GameProgress>() {
///     println!("Level: {}", progress.level);
/// }
/// ```
pub fn resource<T: Any>(&self) -> Option<&T> {
    self.resources.get::<T>()
}

/// Get an exclusive reference to a resource by type.
///
/// Shorthand for `ctx.resources.get_mut::<T>()`.
///
/// # Example
///
/// ```rust,ignore
/// if let Some(progress) = ctx.resource_mut::<GameProgress>() {
///     progress.score += 100;
/// }
/// ```
pub fn resource_mut<T: Any>(&mut self) -> Option<&mut T> {
    self.resources.get_mut::<T>()
}

/// Get an exclusive reference to a resource, inserting `T::default()` if missing.
///
/// Shorthand for `ctx.resources.get_or_default::<T>()`.
///
/// # Example
///
/// ```rust,ignore
/// let settings = ctx.resource_or_default::<GameSettings>();
/// settings.volume = 0.8;
/// ```
pub fn resource_or_default<T: Any + Default>(&mut self) -> &mut T {
    self.resources.get_or_default::<T>()
}
```

**Step 6: Verify full build**

Run: `cargo build --lib`
Expected: Compiles clean.

**Step 7: Commit**

```bash
git add src/core/scene.rs src/core/resources.rs
git commit --author="Kersh <mattbreckon@gmail.com>" -m "feat(scene): add resource convenience methods on SceneContext"
```

---

### Task 3: Document Data-Passing Decision Tree

**Files:**
- Modify: `docs/src/concepts/scenes.md` (add section after "Best Practices", around line 229)

**Step 1: Add the data-passing guide**

Replace the existing "Best Practices" section (lines 229-245) with an expanded version:

```markdown
## Passing Data Between Scenes

Three mechanisms exist for different use cases:

### Which Pattern Should I Use?

| Situation | Pattern | Example |
|-----------|---------|---------|
| New scene needs initialization config | **Constructor params** | `GameScene::new(difficulty, level)` |
| Modal/dialog returning a result | **PopReturn** | Level picker returns selected level |
| Data shared across many scenes | **Resources** | Player score, settings, save state |

### Constructor Parameters (Scene-Owned Data)

Pass data through the scene's constructor. The scene owns this data.

```rust
struct GameOverScene {
    final_score: u32,
    high_score: u32,
}

// In gameplay scene:
SceneAction::Switch(Box::new(GameOverScene {
    final_score: self.score,
    high_score: self.high_score,
}))
```

Best for one-time, scene-specific data. Keep parameter counts small (1-3).

### PopReturn (Child → Parent Data Flow)

Return typed data when popping a scene. The parent receives it in `on_resume`.

```rust
// In level select scene — return the chosen level
SceneAction::PopReturn(Box::new(selected_level as u32))

// In parent scene — receive the result
fn on_resume(&mut self, _ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {
    if let Some(level) = downcast_return::<u32>(returned) {
        self.current_level = level;
    }
}
```

Best for modal interactions: pickers, dialogs, confirmation screens.

### Resources (Cross-Scene Shared State)

Store typed data that persists across all scene transitions.

```rust
// Store before switching scenes
ctx.resources.insert(GameProgress { level: 3, score: 1000 });

// Read from any scene
if let Some(progress) = ctx.resource::<GameProgress>() {
    println!("Level: {}", progress.level);
}

// Modify from any scene
if let Some(progress) = ctx.resource_mut::<GameProgress>() {
    progress.score += bonus;
}
```

Resources are **not** affected by `SceneCleanup` — they survive Switch, Reset, and all transitions. One value per concrete type.

Best for: player state, settings, save data, anything multiple scenes need to read/write.

## Best Practices

1. **Automatic cleanup handles most resets**: World, camera, effects, lighting, and time are automatically reset on Switch/Reset. Override `cleanup_on_exit()` to customize.
2. **Keep scenes focused**: Each scene should have a single responsibility.
3. **Prefer Resources over threading data through constructors**: If more than 2 scenes need the same data, put it in Resources.
4. **Use `downcast_return` for PopReturn**: Avoids nested `if let` chains.
```

**Step 2: Verify docs build (if mdbook is available)**

Run: `mdbook build docs/ 2>&1 | head -5` (skip if mdbook not installed)

**Step 3: Commit**

```bash
git add docs/src/concepts/scenes.md
git commit --author="Kersh <mattbreckon@gmail.com>" -m "docs(scenes): add data-passing decision tree and Resources guide"
```

---

### Task 4 (Optional/Low Priority): Add Data-Carrying Scene Action Variants

> Only implement if Tasks 1-3 are done and user confirms they want this.

**Files:**
- Modify: `src/core/scene.rs` (SceneAction enum, Scene trait, SceneManager methods)

**Step 1: Add new SceneAction variants**

In the `SceneAction` enum, add:

```rust
/// Push a new scene with initialization data delivered to on_enter
PushData(Box<dyn Scene>, Box<dyn Any>),
/// Push with data and a transition effect
PushDataWith(Box<dyn Scene>, Box<dyn Any>, Transition),
/// Switch with initialization data
SwitchData(Box<dyn Scene>, Box<dyn Any>),
/// Switch with data and a transition effect
SwitchDataWith(Box<dyn Scene>, Box<dyn Any>, Transition),
/// Reset with initialization data
ResetData(Box<dyn Scene>, Box<dyn Any>),
/// Reset with data and a transition effect
ResetDataWith(Box<dyn Scene>, Box<dyn Any>, Transition),
```

**Step 2: Modify `on_enter` to accept data**

Change the Scene trait's `on_enter` signature:

```rust
fn on_enter(&mut self, _ctx: &mut SceneContext, _data: Option<Box<dyn Any>>) {}
```

> **Warning:** This is a breaking change — all existing Scene implementations must update their `on_enter` signatures. Every demo and game scene will need `_data: Option<Box<dyn Any>>` added.

**Step 3: Update SceneManager to pass data through**

Update `push()`, `switch()`, `reset()` to pass `None` for data, and add data-carrying variants that pass `Some(data)`.

**Step 4: Update all existing on_enter implementations**

Search for all `fn on_enter` implementations and add the new parameter.

**Step 5: Tests and commit**

This task has high blast radius. Defer unless constructor ergonomics become a real pain point.
