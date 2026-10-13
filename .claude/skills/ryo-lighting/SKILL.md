---
name: ryo-lighting
description: 2D lighting - Light2D and LightOccluder2D entities, ctx.lighting() for ambient, atmosphere and on/off, shadows, volumetric beams, sprite shadows and ambient occlusion.
metadata:
  version: 2.0.0
---

# Ryo2D Lighting Specialist

You help developers light scenes and cast shadows. Complete signatures are in `ryo-api`.

## Resources

- **Lighting**: `src/core/lighting/` (`settings.rs` is what scenes control)
- **Shaders**: `src/core/shaders/core/light.wgsl`, `shadow.wgsl`; `post-process/lighting.wgsl`, `god_rays.wgsl`, `ambient_occlusion.wgsl`
- **Demos**: `examples/demos/lighting/`, `lighting-volumetric/`, `shadows/`

## The Model

- Lights and occluders are components: `Light2D` and `LightOccluder2D` on entities with a `Transform`. Despawn or disable the entity to remove them.
- What the scene controls lives in the `LightingSettings` resource, reached with `ctx.lighting()` (a `Mut<LightingSettings>`): ambient light, atmosphere, and an optional on/off override. The lighting renderer itself is engine-owned; there is no renderer access from game code.
- Lighting is **automatic**: it turns on the first frame any `Light2D` exists and stays on. `ctx.lighting().set_enabled(true)` forces it on (a dark scene before any light exists); `set_enabled(false)` forces it off. `enabled()` returns the override (`None` when automatic).
- Once on, lighting stays on across scene changes, even after `SceneCleanup::LIGHTING` resets the settings to their defaults (dim grey ambient, 0.15). A later scene that wants to be fully lit calls `ctx.lighting().set_enabled(false)` in `on_enter`.
- The UI is never lit: it is composited after lighting and post-processing.
- Every camera is lit, each in light buffers of its own size, and lights are drawn through the camera's matrix: they move, zoom, turn and shake with the sprites. A split screen or minimap shows each light where it is. God rays follow the main camera.

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  let mut lighting = ctx.lighting();
  lighting.set_ambient(AmbientLight::night());
  lighting.set_atmosphere(Atmosphere::hazy());

  ctx.world.spawn((Transform::from_xy(400.0, 300.0), Light2D::torch(180.0).with_shadows()));
  ctx.world.spawn((Transform::from_xy(500.0, 320.0), LightOccluder2D::rect(64.0, 64.0)));
}
```

Bind `let mut lighting = ctx.lighting();` for several calls; it borrows `ctx` until its last use.

## Lights

```rust
// Point: color, radius (px)
Light2D::point(Color::WHITE, 200.0).with_intensity(1.3).with_falloff(2.0)

// Spot: color, radius, cone angle (degrees), direction (degrees)
Light2D::spot(Color::WHITE, 200.0, 45.0, 270.0).with_intensity(1.5)

// Presets
Light2D::torch(150.0)            // warm orange
Light2D::moonlight(150.0)        // cool blue
Light2D::lamp(150.0)             // bright white
Light2D::warning(100.0)          // red
Light2D::magic(120.0, color)     // custom colour, soft falloff
Light2D::flashlight(200.0, 270.0)
```

Change a light at runtime through a query or `get_mut`: `set_color`, `set_radius`, `set_intensity`, `set_direction` (spot), `set_cast_shadows`, `set_shadow_softness`, `set_shadow_color`, `set_volumetric`.

```rust
if let Some(mut light) = ctx.world.get_mut::<Light2D>(self.torch) {
  light.set_intensity(1.0 + (ctx.time.game.elapsed() * 8.0).sin() * 0.1);
}
```

Lights are placed at their entity's `GlobalTransform` translation (a child entity follows its parent); on an interpolated entity they are blended with its sprite, so a light carried by a player does not jitter.

## Shadows

```rust
Light2D::point(color, radius).with_shadows()                 // hard (shadow volumes)
Light2D::point(color, radius).with_soft_shadows(0.5)         // softness 0.0-1.0
Light2D::point(color, radius).with_shadows().with_shadow_color(Color::new(50, 30, 80, 255))
```

## Occluders

```rust
LightOccluder2D::rect(width, height)                  // centred on the entity
LightOccluder2D::rect_offset(x, y, width, height)
LightOccluder2D::circle(radius, segments)
LightOccluder2D::line(start, end, thickness)          // Vec2 points
LightOccluder2D::polygon(vec![Vec2::new(0.0, 0.0), ..])   // CCW winding
```

Vertices are local to the entity and follow its `GlobalTransform` (rotation included). `with_enabled(false)` / `set_enabled(..)` turns one off without removing it.

## Ambient Light

```rust
ctx.lighting().set_ambient(AmbientLight::new(Color::new(20, 20, 40, 255), 0.15));
let current = *ctx.lighting().ambient();

AmbientLight::dark()      // 0 intensity: pitch-black shadows
AmbientLight::night()
AmbientLight::moonlit()
AmbientLight::indoor()
AmbientLight::daylight()
AmbientLight::sunset()
AmbientLight::with_intensity(0.25)   // default colour
```

## Volumetric Light and Atmosphere

Volumetric beams are per light:

```rust
Light2D::flashlight(250.0, 0.0).with_volumetric()              // default beam
Light2D::point(color, radius).with_volumetric_and_god_rays()
Light2D::point(color, radius)
  .with_volumetric_settings(VolumetricSettings::new(0.3, 0.5, 0.2).with_god_rays())   // intensity, density, scattering
// Presets: VolumetricSettings::enabled(), subtle_beam(), strong_beam(), flashlight(), god_rays_only()
```

Atmosphere (fog, scattering, scene-wide god rays) is a setting:

```rust
ctx.lighting().set_atmosphere(
  Atmosphere::new(0.3, Color::new(100, 120, 150, 255))   // density, fog colour
    .with_scattering(0.2)
    .with_god_rays_quality(GodRaysQuality::Medium),
);
// Presets: Atmosphere::clear(), hazy(), dusty(), foggy(), smoky(), underwater(); .with_god_rays()
```

## Sprite Shadows

Drawn with the sprite, independent of lights:

```rust
Renderable::sprite(handle).with_drop_shadow(DropShadow::below(8.0))
Renderable::sprite(handle).with_drop_shadow(DropShadow::diagonal(6.0))
Renderable::sprite(handle).with_drop_shadow(DropShadow::squashed(12.0, 0.3))
Renderable::sprite(handle).with_drop_shadow(
  DropShadow::new().offset(4.0, 8.0).color(Color::new(0, 0, 0, 100)).scale(1.0, 0.6),
)
Renderable::sprite(handle).with_blob_shadow(
  BlobShadow::ellipse(20.0, 10.0).offset(48.0).with_height_scale(true).max_height(150.0),
)
Renderable::sprite(handle).with_contact_shadow(ContactShadow::new().radius(8.0).intensity(0.6).falloff(2.0))
```

## Ambient Occlusion

A screen-space post-process on `EffectsManager`:

```rust
ctx.effects().set_ambient_occlusion(0.3, 16.0);              // intensity, radius
ctx.effects().set_ambient_occlusion_custom(0.3, 16.0, 0.05); // + bias
ctx.effects().disable_ambient_occlusion();
```

## What Gets Lit

The world (entities, `Scene::render` GPU sprites, trails and lines) is drawn first and lit. A `Trail2D` or `Line2D` with `.with_lighting(false)` draws at full brightness over the lit scene (lasers, magic); post-processing still applies. The UI is never lit.

## Pipeline

Per camera (the `lighting` camera pass):

```
1. Light accumulation: lights add into the camera's light buffer, through its view
2. Shadows: occluder geometry projected away from each light, subtracted from the buffer
3. Composition: final = the camera's image x (ambient + light buffer), plus atmosphere
```

## Performance

- Lights have no cap: each lit camera draws the lights on its `RenderLayers` whose reach meets its view.
- Lighting runs once per camera: a minimap camera multiplies its cost. Lights and shadow geometry are built again for every camera.
- Disable shadows on unimportant lights; simple occluder shapes are cheaper.
- Lower AO radius is faster.
