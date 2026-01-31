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

## Effect Stacking

Multiple effects can be active simultaneously. They're applied in the order they were set.

```rust
// Combine effects for atmosphere
ctx.effects.set_vignette(0.4);
ctx.effects.set_color_tint(Color::new(200, 180, 160, 255), 0.2);
ctx.effects.set_crt(0.2, 0.01, 0.001);
```

**Performance note:** Each active effect adds a GPU render pass. For most games, 2-4 simultaneous effects have negligible performance impact.

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

Setting an effect to zero intensity effectively disables it:

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
