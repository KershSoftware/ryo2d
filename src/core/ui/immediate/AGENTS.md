# AGENTS.md - Immediate Mode UI

## Clipping Stack

- `push_clip()`/`pop_clip()` scissor regions nest up to 16 deep (`MAX_CLIP_STACK_DEPTH`). Exceeding that **panics** (assert in `push_clip`).
- Nested clips intersect with their parents. Always pair every `push_clip` with a `pop_clip`.

```rust
ui.push_clip(outer);  // depth 1
ui.push_clip(inner);  // depth 2, intersected with outer
ui.pop_clip();        // depth 1
ui.pop_clip();        // depth 0 (no clipping)
```
