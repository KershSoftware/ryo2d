# Post-Processing Effects

Ryo2D includes a flexible post-processing system for applying visual effects to your game. Effects are processed on the GPU after all sprites are rendered, allowing for screen-wide visual transformations.

## Quick Start

```rust
use ryo2d::prelude::*;

fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Fade the screen to black (for scene transitions)
    ctx.effects.set_fade(0.5);  // 50% faded
    
    // Trigger a red flash (damage feedback)
    ctx.effects.flash(Color::RED, Duration::from_millis(150));
    
    // Add a subtle vignette
    ctx.effects.set_vignette(0.3);
}
```

## Available Effects

### Fade

Fade the screen to a solid color. Perfect for scene transitions.

```rust
// Fade to black
ctx.effects.set_fade(progress);  // 0.0 = visible, 1.0 = black

// Fade to custom color (white flash, red death screen, etc.)
ctx.effects.set_fade_color(0.8, Color::WHITE);
```

### Color Tint

Apply a color grade or mood to the entire screen.

```rust
// Apply a color tint
ctx.effects.set_color_tint(Color::new(255, 200, 150, 255), 0.3);

// Sepia tone (built-in convenience)
ctx.effects.set_sepia(0.5);  // 0.0-1.0 intensity
```

**Use cases:**
- Sepia for flashback scenes
- Blue tint for underwater levels
- Green tint for night vision
- Warm tones for sunset/cozy atmospheres

### Vignette

Darken the edges of the screen for a cinematic look.

```rust
// Simple vignette
ctx.effects.set_vignette(0.3);  // intensity 0.0-1.0

// Fine-tuned vignette
ctx.effects.set_vignette_custom(
    0.4,   // intensity - how dark the edges get
    0.5,   // radius - how far from edges (0=edge, 1=center)
    0.5    // softness - gradient smoothness
);
```

### Screen Flash

One-shot effect that fades out automatically. Great for feedback.

```rust
// Damage flash
ctx.effects.flash(Color::RED, Duration::from_millis(100));

// Power-up collected
ctx.effects.flash(Color::YELLOW, Duration::from_millis(200));

// Using seconds directly
ctx.effects.flash_secs(Color::WHITE, 0.15);
```

The flash automatically decays over the specified duration—no need to manually animate it.

### CRT Monitor

Retro CRT television/monitor effect with scanlines and curvature.

```rust
ctx.effects.set_crt(
    0.3,   // scanline_intensity - darkness of scanlines
    0.02,  // curvature - screen bend amount (0 = flat)
    0.002  // rgb_offset - color channel separation
);
```

**Tip:** Start with subtle values. Heavy CRT effects can obscure gameplay.

### Chromatic Aberration

RGB color channel separation for a glitchy or lens-like effect.

```rust
// Subtle lens effect
ctx.effects.set_chromatic_aberration(0.003, true);  // radial from center

// Glitch effect (stronger, directional)
ctx.effects.set_chromatic_aberration(0.01, false);  // uniform direction
```

### God Rays

Radial light beams emanating from bright light sources.

```rust
// Simple: intensity + light positions in screen UV space (0-1)
ctx.effects.set_god_rays(0.8, vec![(0.5, 0.3)]);

// Custom: intensity, samples, decay, density, threshold, positions
ctx.effects.set_god_rays_custom(1.0, 32, 0.96, 1.0, 0.5, vec![(0.5, 0.3)]);

// Update light positions each frame (efficient, keeps other settings)
ctx.effects.update_god_rays_lights(vec![(light_x, light_y)]);

// Disable
ctx.effects.disable_god_rays();
```

### Ambient Occlusion

Darkens corners and crevices based on edge detection.

```rust
// Simple: intensity (0-1) and sampling radius in pixels (1-64)
ctx.effects.set_ambient_occlusion(0.3, 16.0);

// Custom: intensity, radius, edge detection bias (0.001-0.5)
ctx.effects.set_ambient_occlusion_custom(0.3, 16.0, 0.05);

// Disable
ctx.effects.disable_ambient_occlusion();
```

### Bloom

Extracts bright areas and blurs them to create a soft glow. This is a multi-pass effect.

```rust
// Simple: just set intensity (0.0-5.0)
ctx.effects.set_bloom(1.0);

// Full control over all parameters
ctx.effects.set_bloom_custom(
    0.7,  // threshold (0.0-1.0) — brightness cutoff for extraction
    1.5,  // intensity (0.0-5.0) — glow strength
    5.0,  // radius (1.0-20.0) — blur spread in pixels
    2,    // blur_passes (1-5) — quality (each pass = H+V Gaussian blur)
);

// Disable
ctx.effects.disable_bloom();
```

**Parameter guide:**
- **threshold** controls which pixels glow. Lower values = more of the scene blooms
- **intensity** controls how bright the glow is. Values above 2.0 give a strong neon look
- **radius** controls how far the glow spreads. Higher = softer, dreamier
- **blur_passes** controls quality. 1 is fast but grainy, 3+ is smooth

**Common presets:**
- Subtle highlight glow: `set_bloom_custom(0.8, 0.6, 3.0, 1)`
- Dreamy soft focus: `set_bloom_custom(0.5, 1.2, 12.0, 3)`
- Neon glow: `set_bloom_custom(0.3, 2.5, 6.0, 2)`

## Effect Stacking

Multiple effects can be active simultaneously. They're applied in the order they were set.

```rust
// Combine effects for atmosphere
ctx.effects.set_vignette(0.4);
ctx.effects.set_color_tint(Color::new(200, 180, 160, 255), 0.2);
ctx.effects.set_crt(0.2, 0.01, 0.001);
```

**Performance note:** Each active effect adds a GPU render pass. Bloom is a multi-pass effect (extract + blur passes + composite), so it costs more than single-pass effects. For most games, 2-4 simultaneous effects have negligible performance impact.

## Managing Effects

### Checking Active Effects

```rust
if ctx.effects.has_active_effects() {
    // Effects are being applied
}
```

### Clearing Effects

```rust
// Remove all effects
ctx.effects.clear();
```

### Disabling Individual Effects

Some effects have dedicated disable methods:

```rust
ctx.effects.disable_crt();
ctx.effects.disable_chromatic_aberration();
ctx.effects.disable_god_rays();
ctx.effects.disable_ambient_occlusion();
ctx.effects.disable_bloom();
```

For others, setting intensity to zero effectively disables them:

```rust
ctx.effects.set_fade(0.0);      // No fade
ctx.effects.set_vignette(0.0);  // No vignette
```

## Common Patterns

### Scene Transitions

```rust
struct MyGame {
    fade_progress: f32,
    transitioning: bool,
}

fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    if self.transitioning {
        self.fade_progress += dt * 2.0;  // Fade over 0.5 seconds
        if self.fade_progress >= 1.0 {
            // Fully faded - switch scene
            self.load_next_scene();
            self.transitioning = false;
            self.fade_progress = 1.0;
        }
    } else if self.fade_progress > 0.0 {
        // Fade back in
        self.fade_progress -= dt * 2.0;
        self.fade_progress = self.fade_progress.max(0.0);
    }
    
    ctx.effects.set_fade(self.fade_progress);
}
```

### Damage Feedback

```rust
fn on_player_hit(&mut self, ctx: &mut GameContext) {
    ctx.effects.flash(Color::RED, Duration::from_millis(100));
    // Also play sound, shake camera, etc.
}
```

### Environmental Mood

```rust
fn enter_underwater_zone(&mut self, ctx: &mut GameContext) {
    ctx.effects.set_color_tint(Color::new(100, 150, 200, 255), 0.3);
    ctx.effects.set_vignette(0.2);
}

fn exit_underwater_zone(&mut self, ctx: &mut GameContext) {
    ctx.effects.clear();
}
```

### Retro Game Style

```rust
fn init(&mut self, ctx: &mut GameContext) {
    // Always-on retro effects
    ctx.effects.set_crt(0.25, 0.015, 0.001);
}
```

## Low-Level API

For advanced use cases, you can work with `PostProcessEffect` directly:

```rust
use ryo2d::shaders::PostProcessEffect;

// Create effect with full control
let custom_vignette = PostProcessEffect::Vignette {
    intensity: 0.5,
    radius: 0.6,
    softness: 0.4,
};

// Add to effect stack
ctx.effects.set_effect(custom_vignette);
```

## Performance Tips

1. **Combine visual feedback** - A single screen flash is cheaper than multiple overlapping effects

2. **Disable unused effects** - Clear effects when leaving areas that use them

3. **Use timed effects wisely** - Screen flash auto-removes itself; don't manually clear it

4. **Test on target hardware** - Effect cost varies by GPU; mobile devices may need fewer effects
