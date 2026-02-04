# AGENTS.md - Built-in Render Passes

## Overview

This folder contains the built-in render passes that make up the standard 2D rendering pipeline. Each pass handles one phase of rendering.

## Module Structure

| Module | Purpose |
|--------|---------|
| `mod.rs` | Public exports for built-in passes |
| `clear.rs` | `ClearPass` - Clear frame buffer to background color |
| `upload.rs` | `UploadPass` - Transfer CPU pixel buffer to GPU texture |
| `parallax.rs` | `ParallaxPass` - GPU-accelerated parallax backgrounds |
| `lighting.rs` | `LightingPass` - 2D lighting with point lights and shadows |

## Pass Descriptions

### ClearPass

Clears the frame buffer to a solid color at the start of each frame.

- **ID**: `clear`
- **Dependencies**: None (always runs first)
- **Configuration**: Background color (default: black)

### UploadPass

Transfers the CPU-rendered pixel buffer to a GPU texture for display.

- **ID**: `upload`
- **Dependencies**: `cpu_sprite` (after CPU rendering complete)
- **Purpose**: Bridge between software and hardware rendering

### ParallaxPass

Renders parallax background layers directly on the GPU.

- **ID**: `parallax`
- **Dependencies**: `clear`
- **Features**:
  - Multiple layers with different scroll speeds
  - Horizontal tiling modes (none, tile, infinite scroll)
  - GPU-accelerated for better performance than CPU tiling

### LightingPass

Applies 2D lighting to the scene.

- **ID**: `lighting`
- **Dependencies**: `gpu_sprite`
- **Features**:
  - Point lights with falloff
  - Ambient light level
  - Light colors and intensities
  - Optional shadow casting

## Adding New Passes

1. Create a new file in this folder (e.g., `my_effect.rs`)
2. Implement the `RenderPass` trait
3. Export from `mod.rs`
4. Register in the default pipeline (in `engine.rs`)

```rust
// my_effect.rs
use super::super::{RenderContext, RenderPass};

pub struct MyEffectPass {
    enabled: bool,
}

impl MyEffectPass {
    pub fn new() -> Self {
        Self { enabled: true }
    }
}

impl RenderPass for MyEffectPass {
    fn id(&self) -> &'static str { "my_effect" }

    fn dependencies(&self) -> &[&'static str] {
        &["lighting"]  // Run after lighting
    }

    fn execute(&mut self, ctx: &mut RenderContext) {
        // Effect implementation
    }

    fn is_enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
}
```

## Pipeline Order

Default 2D pipeline execution order:

```
1. ClearPass       - Clear to background color
2. ParallaxPass    - GPU parallax backgrounds
3. CpuSpritePass   - Software sprite rendering (not in this folder)
4. UploadPass      - CPU buffer → GPU texture
5. GpuSpritePass   - Batched GPU sprites (not in this folder)
6. TrailPass       - Motion trails (not in this folder)
7. LightingPass    - 2D lighting
8. UiPass          - User interface (not in this folder)
9. EffectsPass     - Post-processing (not in this folder)
10. PresentPass    - Final output (not in this folder)
```

Note: Some passes live outside this folder in the main render module or engine.

## Design Notes

- Each pass should do one thing well
- Passes should be independently toggleable
- Dependencies define ordering, not explicit indices
- Passes receive `RenderContext` with all shared state
