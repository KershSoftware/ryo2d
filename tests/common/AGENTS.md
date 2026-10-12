# AGENTS.md - E2E Testing Infrastructure

## Contract

GPU tests must **skip, not fail**, when no GPU is available (CI without GPU).

## Running and Registering Tests

- Run: `cargo test --test e2e` or `./dev test:e2e` (`./dev.ps1 test:e2e` on Windows)
- `tests/e2e.rs` lists modules with explicit `#[path = "e2e/<file>.rs"]` entries. A new file in `tests/e2e/` won't run until you add one (`tests/integration.rs` works the same way)

## Two Levels

- **Whole engine** (scenes, rendering, input): `ryo2d::testing::HeadlessEngine::new_or_skip(builder)` returns `None` without a GPU; step it and read `frame()`. See `src/core/testing/AGENTS.md`.
- **A renderer on its own** (lighting, trails, capture readback): `TestGpu` from `tests/common/gpu.rs`.

```rust
#[test]
fn test_my_gpu_feature() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test: no GPU available");
    return;
  };
  let target = gpu.create_target(100, 100);
  // ... render ...
  assert_eq!(target.get_pixel(gpu, 50, 50), (255, 0, 0, 255));
}
```

## Notes for AI Agents

1. **Use `TestGpu::shared()`** - `new()` is private. Both levels share the engine's one device (`ryo2d::testing::shared_gpu`): separate GPU initializations running concurrently deadlock on Windows
2. **Pixel tolerance** - GPU rendering can vary slightly; use fuzzy comparisons when needed
