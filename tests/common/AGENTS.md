# AGENTS.md - E2E Testing Infrastructure

## Purpose

This folder provides shared utilities for end-to-end testing of Ryo2D engine features. The primary focus is enabling **headless GPU testing** for validating rendering, lighting, and GPU-based systems without requiring a window.

## Design Principles

1. **Optional GPU** - Tests gracefully skip when no GPU is available (CI environments)
2. **Async-friendly** - Uses `pollster::block_on()` for async GPU operations
3. **Reusable** - `TestGpu` and `TestRenderTarget` are shared across test modules
4. **Extensible** - Easy to add new capabilities without breaking existing tests

## Current Implementation (Phase 1)

### `gpu.rs` - Headless GPU Context

**Key insight:** wgpu can create Device/Queue without a surface:

```rust
instance.request_adapter(&RequestAdapterOptions {
    compatible_surface: None,  // No window needed!
})
```

**Types:**

| Type | Purpose |
|------|---------|
| `TestGpu` | Headless GPU context with device, queue, and default format |
| `TestRenderTarget` | Texture that can be rendered to and read back to CPU |

**Usage:**

```rust
// Create headless GPU (returns None if unavailable)
let gpu = TestGpu::new()?;

// Create a render target
let target = gpu.create_target(100, 100);

// Clear to a color
target.clear(&gpu, 1.0, 0.0, 0.0, 1.0);  // Red

// Read pixels back
let pixels = target.read_pixels_sync(&gpu);

// Get specific pixel
let (r, g, b, a) = target.get_pixel(&gpu, 50, 50);
```

## Roadmap

### Phase 2 - Component Testing

Add utilities to test engine GPU components in isolation:

- `LightingRenderer` creation and configuration
- `SpriteGpuRenderer` batching and instance uploads
- Shader compilation validation
- Post-processing pipeline effects

### Phase 3 - Visual Regression

Add image comparison for golden-master testing:

```rust
// Proposed API
let target = gpu.create_target(100, 100);
// ... render scene ...
target.compare_to_reference("tests/references/my_scene.png", tolerance: 0.01)?;
```

- `tests/references/` folder for golden images
- Configurable pixel diff threshold for minor variations
- Generate diff images on failure for debugging

### Phase 4 - Full Scene Testing

Test complete rendering pipeline integration:

- CPU sprite rendering to frame buffer
- GPU sprite overlay
- Lighting composition with multiple light types
- UI overlay compositing
- Post-processing effect chains

## Writing Tests

### Basic Pattern

```rust
#[test]
fn test_my_gpu_feature() {
    // Skip gracefully if no GPU
    let Some(gpu) = TestGpu::new() else {
        eprintln!("Skipping test: no GPU available");
        return;
    };

    let target = gpu.create_target(100, 100);

    // ... perform rendering operations ...

    // Verify results
    let (r, g, b, a) = target.get_pixel(&gpu, 50, 50);
    assert_eq!((r, g, b, a), (255, 0, 0, 255));
}
```

### CI Considerations

Tests using `TestGpu` will:
- **Pass on machines with GPU** - Full test execution
- **Skip on machines without GPU** - Graceful early return

This ensures CI pipelines without GPU support don't fail, while local development and GPU-enabled CI get full coverage.

## Module Structure

```
tests/
├── common/
│   ├── mod.rs          # Exports gpu module
│   ├── gpu.rs          # TestGpu, TestRenderTarget
│   └── AGENTS.md       # This file
├── e2e.rs              # E2E test entry point
└── e2e/
    ├── engine_lifecycle.rs  # Engine startup/shutdown tests
    └── gpu_basics.rs        # Basic GPU context tests
```

## Notes for AI Agents

1. **Always handle missing GPU** - Use `let Some(gpu) = TestGpu::new() else { return; }`
2. **Use sync helpers for simplicity** - `read_pixels_sync()` and `get_pixel()` are easier than async
3. **Keep tests focused** - One feature per test, minimal setup
4. **Pixel tolerance** - GPU rendering can have minor variations; use fuzzy comparisons when needed
5. **Clean up** - Textures and buffers are dropped automatically via RAII
