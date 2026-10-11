# AGENTS.md - E2E Testing Infrastructure

## Contract

GPU tests must **skip, not fail**, when no GPU is available (CI without GPU).

## Running and Registering Tests

- Run: `cargo test --test e2e` or `./run test:e2e` (`./run.ps1 test:e2e` on Windows)
- `tests/e2e.rs` lists modules with explicit `#[path = "e2e/<file>.rs"]` entries. A new file in `tests/e2e/` won't run until you add one (`tests/integration.rs` works the same way)

## Basic Pattern

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

1. **Use `TestGpu::shared()`** - `new()` is private. Separate GPU initializations running concurrently deadlock on Windows, so tests share one context
2. **Pixel tolerance** - GPU rendering can vary slightly; use fuzzy comparisons when needed
