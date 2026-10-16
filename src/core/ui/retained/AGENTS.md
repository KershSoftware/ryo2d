# AGENTS.md - Retained Mode UI

## Focus Management

- Create dropdowns that take part in focus with `DropdownState::with_id(n)` and a **unique, non-zero** `n`. `new()` uses id 0, which `UiContext` treats as "no dropdown", so click-to-focus won't track it.
- Call order inside `Scene::ui` matters:

```rust
// 1. Draw every dropdown
ui.dropdown(&mut self.dropdown1, rect1, &options1);
ui.dropdown(&mut self.dropdown2, rect2, &options2);

// 2. Then Tab navigation, with states in tab order
ui.handle_dropdown_tab_navigation(&mut [&mut self.dropdown1, &mut self.dropdown2]);

// Open lists draw on the popup layer; the engine flushes them after the scene's `ui`
```

`Focusable`/`FocusTrap` (`focus.rs`) handle focus for other widgets and modals.
