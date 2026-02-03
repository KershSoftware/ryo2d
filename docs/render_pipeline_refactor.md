# Render Pipeline Refactor Design

## Goals

1. **Simplify** - Replace callback-based spaghetti with a clear, linear pipeline
2. **Configurable** - Allow passes to be added, removed, or reordered at runtime
3. **Testable** - Each pass can be tested in isolation
4. **GPU Parallax** - Move parallax rendering from CPU to GPU for performance

## Current Architecture Problems

### 1. Three Separate Render Methods
```rust
render_with_effects()      // CPU only + effects
render_with_gpu_sprites()  // CPU + GPU sprites + effects
render_with_lighting()     // CPU + GPU sprites + lighting + UI + effects
```

Each method duplicates the same core logic with slight variations. Adding a new feature requires modifying multiple methods.

### 2. Callback Spaghetti
The engine orchestrates rendering via nested closures:
```rust
let gpu_callback = |queue, encoder, view| { ... };
let lighting_callback = |device, queue, encoder, pixel_data, output_view| { ... };
rend.render_with_lighting(effects, gpu_callback, lighting_callback, ui_buffer)
```

This makes the render order implicit and hard to modify.

### 3. CPU-Bound Parallax
Parallax layers are drawn pixel-by-pixel to the CPU frame buffer, causing FPS drops with large images.

## New Architecture

### Core Concept: Render Passes

Each rendering phase becomes a `RenderPass` that can be composed into a pipeline:

```rust
pub trait RenderPass {
    /// Unique identifier for this pass
    fn id(&self) -> &'static str;

    /// Render pass dependencies (passes that must run before this one)
    fn dependencies(&self) -> &[&'static str] { &[] }

    /// Execute the pass
    fn execute(&mut self, ctx: &mut RenderContext);

    /// Whether this pass should run this frame
    fn should_execute(&self, ctx: &RenderContext) -> bool { true }
}
```

### Render Context

A `RenderContext` provides access to all rendering resources:

```rust
pub struct RenderContext<'a> {
    // GPU resources
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,

    // Textures
    pub game_texture: &'a wgpu::TextureView,     // Main render target
    pub game_texture_raw: &'a wgpu::Texture,     // For copies
    pub scratch_a: &'a wgpu::TextureView,        // Ping-pong buffer A
    pub scratch_b: &'a wgpu::TextureView,        // Ping-pong buffer B

    // CPU buffer (for hybrid rendering)
    pub pixel_buffer: &'a mut [u8],

    // Frame info
    pub width: u32,
    pub height: u32,
    pub time: f32,

    // Camera for world-to-screen transforms
    pub camera: &'a Camera,

    // Current texture routing
    pub current_source: TextureSlot,
}

pub enum TextureSlot {
    Game,      // Main game texture
    ScratchA,  // Post-process buffer A
    ScratchB,  // Post-process buffer B
}
```

### Default Render Passes

```
┌──────────────────────────────────────────────────────────────────┐
│                       RENDER PIPELINE                             │
├──────────────────────────────────────────────────────────────────┤
│                                                                   │
│  1. ClearPass                                                     │
│     - Clear pixel buffer to background color                      │
│     - Always runs first                                           │
│                                                                   │
│  2. ParallaxPass (NEW - GPU)                                      │
│     - Renders parallax layers via GPU batching                    │
│     - Uses instancing for tiled layers                            │
│     - Dependencies: [ClearPass]                                   │
│                                                                   │
│  3. CpuSpritePass                                                 │
│     - CPU software rendering for sprites without shaders          │
│     - Respects z-order with GPU sprites                           │
│     - Dependencies: [ParallaxPass]                                │
│                                                                   │
│  4. UploadPass                                                    │
│     - Uploads CPU pixel buffer to game_texture                    │
│     - Dependencies: [CpuSpritePass]                               │
│                                                                   │
│  5. GpuSpritePass                                                 │
│     - Renders batched GPU sprites with instancing                 │
│     - Dependencies: [UploadPass]                                  │
│                                                                   │
│  6. TrailPass                                                     │
│     - Renders motion trails                                       │
│     - Dependencies: [GpuSpritePass]                               │
│                                                                   │
│  7. LightingPass                                                  │
│     - Accumulates lights and composes with scene                  │
│     - Conditionally enabled                                       │
│     - Dependencies: [TrailPass]                                   │
│     - Output: scratch_a                                           │
│                                                                   │
│  8. UiPass                                                        │
│     - Renders UI elements                                         │
│     - Can render before or after lighting                         │
│     - Dependencies: [LightingPass or TrailPass]                   │
│                                                                   │
│  9. EffectsPass                                                   │
│     - Applies post-processing effect chain                        │
│     - Ping-pongs between scratch_a/scratch_b                      │
│     - Dependencies: [UiPass]                                      │
│                                                                   │
│ 10. PresentPass                                                   │
│     - Renders final texture to swapchain                          │
│     - Applies letterboxing/scaling                                │
│     - Dependencies: [EffectsPass]                                 │
│     - Always runs last                                            │
│                                                                   │
└──────────────────────────────────────────────────────────────────┘
```

### Render Pipeline Struct

```rust
pub struct RenderPipeline {
    passes: Vec<Box<dyn RenderPass>>,
    pass_order: Vec<usize>,  // Indices into passes, computed from dependencies
}

impl RenderPipeline {
    /// Create with default passes
    pub fn default_2d() -> Self { ... }

    /// Add a pass (re-sorts based on dependencies)
    pub fn add_pass(&mut self, pass: impl RenderPass + 'static) { ... }

    /// Remove a pass by ID
    pub fn remove_pass(&mut self, id: &str) -> Option<Box<dyn RenderPass>> { ... }

    /// Enable/disable a pass
    pub fn set_enabled(&mut self, id: &str, enabled: bool) { ... }

    /// Execute all enabled passes in order
    pub fn execute(&mut self, ctx: &mut RenderContext) { ... }
}
```

### GPU Parallax Pass Design

The new `ParallaxPass` renders layers using the existing GPU batching system:

```rust
pub struct ParallaxPass {
    layers: Vec<GpuParallaxLayer>,
    enabled: bool,
}

pub struct GpuParallaxLayer {
    texture_id: String,          // Cached in SpriteGpuRenderer
    scroll_speed: f32,           // Camera parallax factor
    auto_scroll_speed: f32,      // Time-based scrolling
    y_offset: f32,
    z_order: i32,
    tile_mode: TileMode,

    // Pre-computed for performance
    uv_rect: [f32; 4],
    tint: [f32; 4],
}

impl RenderPass for ParallaxPass {
    fn id(&self) -> &'static str { "parallax" }
    fn dependencies(&self) -> &[&'static str] { &["clear"] }

    fn execute(&mut self, ctx: &mut RenderContext) {
        for layer in &self.layers {
            // Calculate instances for this layer
            let instances = layer.compute_instances(
                ctx.width, ctx.height,
                ctx.camera, ctx.time
            );

            // Add to batch collector
            ctx.batch_collector.add_batch(
                &layer.texture_id,
                instances,
                layer.z_order,
                false, // no effects
            );
        }

        // Render all parallax batches
        ctx.sprite_gpu.render_batches(
            ctx.queue, ctx.encoder,
            ctx.game_texture,
            &ctx.batch_collector
        );
    }
}
```

### Migration Strategy

1. **Phase 1**: Create the `RenderPass` trait and `RenderPipeline` struct
2. **Phase 2**: Wrap existing render logic in pass implementations
3. **Phase 3**: Add comprehensive tests for current behavior
4. **Phase 4**: Implement GPU parallax pass
5. **Phase 5**: Refactor WgpuRenderer to use single `execute_pipeline()` method
6. **Phase 6**: Remove old render methods

### API Changes

**Before:**
```rust
// In Engine::run_frame()
if has_gpu_sprites || has_trails || lighting_enabled {
    if lighting_enabled {
        rend.render_with_lighting(effects, gpu_callback, lighting_callback, ui_buffer)
    } else {
        rend.render_with_gpu_sprites(effects, gpu_callback)
    }
} else {
    rend.render_with_effects(effects)
}
```

**After:**
```rust
// In Engine::run_frame()
self.render_pipeline.execute(&mut self.render_context);
```

### Testing Strategy

1. **Unit tests** for each pass in isolation
2. **Integration tests** for default pipeline configuration
3. **E2E tests** using headless GPU rendering (existing TestGpu infrastructure)
4. **Regression tests** comparing pixel output before/after refactor

### Performance Considerations

1. **GPU Parallax**: Should significantly improve FPS for games with large parallax layers
2. **Batch sorting**: Passes already sorted by dependencies; no runtime sorting needed
3. **Texture routing**: Use texture views to avoid copies between passes
4. **Memory**: Reuse scratch textures across frames

## File Structure

```
src/core/render/
├── mod.rs           # Module exports
├── context.rs       # RenderContext struct
├── pass.rs          # RenderPass trait
├── pipeline.rs      # RenderPipeline orchestration
└── passes/
    ├── mod.rs
    ├── clear.rs
    ├── parallax.rs  # NEW GPU parallax
    ├── cpu_sprite.rs
    ├── upload.rs
    ├── gpu_sprite.rs
    ├── trail.rs
    ├── lighting.rs
    ├── ui.rs
    ├── effects.rs
    └── present.rs
```

## Success Criteria

1. All existing tests pass
2. Bullet hell example runs at higher FPS with GPU parallax
3. New integration tests cover render pipeline configuration
4. Single render method in WgpuRenderer (backward-compatible API preserved)
5. Passes can be added/removed/reordered via API
