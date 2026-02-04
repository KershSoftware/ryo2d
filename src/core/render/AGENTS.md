# AGENTS.md - Render Pipeline System

## Overview

This folder contains the configurable render pipeline system. The pipeline orchestrates multiple render passes that each perform one phase of rendering (clear, sprites, lighting, effects, etc.).

## Module Structure

| Module | Purpose |
|--------|---------|
| `mod.rs` | Public exports and pass ID constants |
| `pass.rs` | `RenderPass` trait definition |
| `pipeline.rs` | `RenderPipeline` orchestrator |
| `context.rs` | `RenderContext` with shared render state |
| `passes/` | Built-in pass implementations (see `passes/AGENTS.md`) |

## Architecture

```text
Game code describes intent
    │
    ▼
Engine extracts intent into render data
    │
    ▼
RenderPipeline plans passes
    │
    ├── ClearPass: Clear buffers
    ├── ParallaxPass: GPU parallax layers
    ├── CpuSpritePass: Software sprite rendering
    ├── UploadPass: CPU buffer → GPU texture
    ├── GpuSpritePass: Batched GPU sprites
    ├── TrailPass: Motion trails
    ├── LightingPass: 2D lighting
    ├── UiPass: User interface
    ├── EffectsPass: Post-processing
    └── PresentPass: Final output
    │
    ▼
Swapchain shows the result
```

## Key Types

### RenderPass Trait

Defines a single phase of rendering:

```rust
pub trait RenderPass: Send + Sync {
    fn id(&self) -> &'static str;
    fn dependencies(&self) -> &[&'static str] { &[] }
    fn execute(&mut self, ctx: &mut RenderContext);
    fn should_execute(&self, ctx: &RenderContext) -> bool { true }
    fn is_enabled(&self) -> bool { true }
    fn set_enabled(&mut self, enabled: bool) {}
}
```

### RenderPipeline

Orchestrates passes with dependency ordering:

- `add_pass()` - Add a pass (auto-sorted by dependencies)
- `remove_pass()` - Remove a pass by ID
- `set_enabled()` - Enable/disable a pass at runtime
- `execute()` - Run all enabled passes in order

### RenderContext

Shared state available to all passes:

- Frame buffer access
- GPU resources (textures, pipelines)
- Camera and viewport info
- Texture slots for multi-pass rendering

## Pass IDs

Standard pass IDs from `pass_ids` module:

| ID | Purpose |
|----|---------|
| `clear` | Clear frame buffer |
| `parallax` | GPU parallax backgrounds |
| `cpu_sprite` | Software sprite rendering |
| `upload` | CPU buffer → GPU texture |
| `gpu_sprite` | Batched GPU sprites |
| `trail` | Motion trail effects |
| `lighting` | 2D lighting system |
| `ui` | User interface rendering |
| `effects` | Post-processing effects |
| `present` | Final swapchain present |

## Creating Custom Passes

```rust
struct MyCustomPass {
    enabled: bool,
}

impl RenderPass for MyCustomPass {
    fn id(&self) -> &'static str { "my_custom" }

    fn dependencies(&self) -> &[&'static str] {
        &["gpu_sprite"]  // Run after GPU sprites
    }

    fn execute(&mut self, ctx: &mut RenderContext) {
        // Access frame buffer, GPU resources, etc.
    }

    fn is_enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
}
```

## Usage in Engine

The engine creates and manages the default pipeline:

```rust
let mut pipeline = RenderPipeline::default_2d();

// Disable lighting for performance
pipeline.set_enabled("lighting", false);

// Add custom pass
pipeline.add_pass(MyCustomPass::new());

// Each frame
pipeline.execute(&mut render_context);
```

## Design Notes

- **Dependency-based ordering**: Passes declare what must run before them, not explicit order
- **Runtime toggling**: Passes can be enabled/disabled without rebuilding the pipeline
- **Custom passes**: Games can add their own passes for special effects
- **Zero-overhead disabled**: Disabled passes skip all work including dependency checks
