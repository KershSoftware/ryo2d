# AGENTS.md - Immediate Mode UI

## Clipping Stack

- `push_clip()`/`pop_clip()` nest without a depth limit; nested clips intersect with their parents. Each recorded command carries the clip it was drawn under, drawn as a scissor.
- `pop_clip()` on an empty stack does nothing and is warned once. Always pair every `push_clip` with a `pop_clip`.
- Rects and text wholly outside the current clip are not recorded.

```rust
ui.push_clip(outer);  // depth 1
ui.push_clip(inner);  // depth 2, intersected with outer
ui.pop_clip();        // depth 1
ui.pop_clip();        // depth 0 (no clipping)
```
