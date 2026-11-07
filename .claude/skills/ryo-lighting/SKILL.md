---
name: ryo-lighting
description: 2D lighting - Light2D and LightOccluder2D entities (polygons and open chains), ctx.lighting() for ambient, atmosphere and on/off, shadows (tile maps' too), lights and shadows on storeys (Floor), volumetric beams, sprite shadows and ambient occlusion.
metadata:
  version: 2.4.0
---

# Ryo2D Lighting Specialist

You help developers light scenes and cast shadows. Complete signatures are in `ryo-api`.

## Resources

- **Lighting**: `src/core/lighting/` (`settings.rs` is what scenes control)
- **Shaders**: `src/core/shaders/core/light.wgsl` (lights, batch quads), `shadow.wgsl` (shadow volumes, batch triangles); `post-process/lighting.wgsl` (compose), `god_rays.wgsl`, `ambient_occlusion.wgsl`
- **Demos**: `examples/demos/lighting/`, `lighting-volumetric/`, `shadows/`

## The Model

- Lights and occluders are components: `Light2D` and `LightOccluder2D` on entities with a `Transform`. Despawn or disable the entity to remove them.
- What the scene controls lives in the `LightingSettings` resource, reached with `ctx.lighting()` (a `Mut<LightingSettings>`): ambient light, atmosphere, and an optional on/off override. The lighting renderer itself is engine-owned; there is no renderer access from game code.
- Lighting is **automatic**: it turns on the first frame any `Light2D` exists and stays on. `ctx.lighting().set_enabled(true)` forces it on (a dark scene before any light exists); `set_enabled(false)` forces it off. `enabled()` returns the override (`None` when automatic).
- Once on, lighting stays on across scene changes, even after `SceneCleanup::LIGHTING` resets the settings to their defaults (dim grey ambient, 0.15). A later scene that wants to be fully lit calls `ctx.lighting().set_enabled(false)` in `on_enter`.
- The UI is never lit: it is composited after lighting and post-processing.
- Every camera is lit, each in light buffers of its own size, and lights are drawn through the camera's matrix: they move, zoom, turn and shake with the sprites. A split screen or minimap shows each light where it is. God rays follow the main camera, on the whole frame.
- **Lights go through the 2D batch.** They are instanced quads, so a light has `RenderLayers` like any drawable (a component on its entity, default layer 0) and lights only the cameras on those layers whose view its reach meets. There is no cap on lights.
- **Colours are sRGB, decoded for the GPU.** A light of `Color::new(128, 128, 128, 255)` is mid-grey light (linear 0.22), and ambient, shadow and fog colours decode the same way. The engine's presets keep their colour bytes, which now mean the sRGB colour they name, so torches and ambient presets look warmer and darker than before HDR.
- **Intensity above 1 shows.** Light accumulates in a float buffer, so `with_intensity(3.0)` makes a surface brighter than white; the look (`ctx.look()`) rolls it off, and bloom picks it up.

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

<!-- given: color: Color -->
```rust
// Point: color, radius (px)
Light2D::point(Color::WHITE, 200.0).with_intensity(1.3).with_falloff(2.0);

// Spot: color, radius, cone angle (degrees), direction (degrees)
Light2D::spot(Color::WHITE, 200.0, 45.0, 270.0).with_intensity(1.5);

// Presets
Light2D::torch(150.0);            // warm orange
Light2D::moonlight(150.0);        // cool blue
Light2D::lamp(150.0);             // bright white
Light2D::warning(100.0);          // red
Light2D::magic(120.0, color);     // custom colour, soft falloff
Light2D::flashlight(200.0, 270.0);
```

Change a light at runtime through a query or `get_mut`: `set_color`, `set_radius`, `set_intensity`, `set_direction` (spot), `set_cast_shadows`, `set_shadow_softness`, `set_shadow_color`, `set_volumetric`.

<!-- self: torch: Entity -->
```rust
if let Some(mut light) = ctx.world.get_mut::<Light2D>(self.torch) {
  light.set_intensity(1.0 + (ctx.time.game.elapsed() * 8.0).sin() * 0.1);
}
```

Lights are placed at their entity's `GlobalTransform` translation (a child entity follows its parent), drawn lifted on a storey (Floors, below); on an interpolated entity they are blended with its sprite, so a light carried by a player does not jitter.

## Shadows

<!-- given: color: Color, radius: f32 -->
```rust
Light2D::point(color, radius).with_shadows();                 // hard (shadow volumes)
Light2D::point(color, radius).with_soft_shadows(0.5);         // fades with distance, 0.0-1.0
Light2D::point(color, radius).with_shadows().with_shadow_color(Color::new(50, 30, 80, 255));
```

Each shadow darkens only the light that casts it, never another light's. A soft shadow starts whole at the light and fades with distance from it, letting `softness` of the light through past a distance of `softness / 2` of the radius; its outline stays sharp. The old ray-marched shadows (`with_raymarched_shadows`) are gone: use `with_soft_shadows`. A shadow colour is the light that gets through: black blocks it, a tinted colour lets that colour through (stained glass), and its alpha is the shadow's strength.

## Occluders

<!-- given: width: f32, height: f32, x: f32, y: f32, radius: f32, segments: u32, start: Vec2, end: Vec2, thickness: f32 -->
```rust
LightOccluder2D::rect(width, height);                  // centred on the entity
LightOccluder2D::rect_offset(x, y, width, height);
LightOccluder2D::circle(radius, segments);
LightOccluder2D::line(start, end, thickness);          // Vec2 points
LightOccluder2D::polygon(vec![Vec2::ZERO, Vec2::new(64.0, 0.0), Vec2::new(32.0, 48.0)]);   // CCW winding
LightOccluder2D::chain(vec![Vec2::new(0.0, 32.0), Vec2::ZERO, Vec2::new(64.0, 0.0)]);       // open: no edge back to the start
```

Vertices are local to the entity and follow its `GlobalTransform` (rotation included). `with_enabled(false)` / `set_enabled(..)` turns one off without removing it.

- **Chains** are open runs of edges: no edge joins the last vertex back to the first (`is_closed()` is false; true for every other kind). Wind a chain as the polygon whose boundary it is a stretch of (counter-clockwise on screen): each edge casts exactly as it would as that polygon's edge, so chains that together close a ring cast as the polygon does. Use them for a boundary built in pieces. Two closed occluders that touch cast their touching edges' shadows through each other's lit art, a dark seam; the chains of one boundary do not.
- **Reach:** a light casts from the occluders that may come within twice its radius, measured from each occluder's entity less its farthest vertex, so an occluder's entity can sit anywhere along its shape (a tile map's chunk-long chains sit at their middles).

## Tile Maps

A tile map's layers cast shadows from their tiles (`ryo-tilemap`, Shadows):

- A layer occludes as its `occludes` property says (a bool layer property in Tiled), or as it collides when that is left out; a map spawned `without_collision` still casts.
- What blocks light is a tile's collision shapes, or only its shapes marked `occluder` if it has any (a bool property on the shape in Tiled's collision editor). An `occluder` shape blocks light and does not collide, so a tile that casts a shadow but lets the player through (foliage) has an `occluder` shape alone; tiles that collide but let the light through go on a layer whose `occludes` is false. An LDtk IntGrid's solid cells block whole and its slopes as their half cell.
- The map builds the boundary of what occludes as `LightOccluder2D::chain`s, one or more for each 32 x 32-cell chunk, as children of its first occluding layer marked `MapOccluder { chunk }`. Tiles that touch cast no shadow between them, except a hexagonal tile with a shape of its own (a rock, a trunk: any shape but its whole hexagon), which casts its whole outline, unmerged with the hexes beside it: where that outline lies along a neighbour's side (a solid hex's, or another shape's), both cast there, so under a light whose shadows are not at full strength that stretch of shadow is darker. A tile edit rebuilds only the chains of the chunks it touches, and casts its shadow that frame.
- Lights still need `with_shadows()` (or `with_soft_shadows`) to cast them.
- On a map with storeys (`ryo-tilemap`), each storey's layers cast on their own: a higher storey's chains carry its `Floor::On(n)` (Floors, below).

## Floors

A light or occluder is on a storey (raised ground, an upper floor, a bridge; `ryo-tilemap`, Floors) as any entity is: by its own `Floor`, else its nearest ancestor's, else the ground floor (`Floor::GROUND`).

- **A light on a storey is blocked only by that storey's occluders and by those on every storey** (`Floor::Every`). A light on the ground floor is blocked by the ground floor's and every storey's, as before; a light on `Every` is blocked by all of them.
- **A game's own occluders without a `Floor` are on the ground floor**, so they stop blocking a light once it is on a higher storey. Give a wall on a storey that storey's `Floor`, or `Floor::Every` for one that should block every storey's lights.
- **A light and its shadows draw lifted** by its storey's height on the map with floors under it, as its storey's tiles and characters do, so they line up with that storey's art. Shadows are cast where the light and its occluders stand, on the ground, then drawn lifted with the light. A light held by a character (a child entity) is on the character's storey and goes up with it.
- **A light on `Every` draws at ground height.**
- **A light still brightens whatever lies under it on screen, whatever its storey**: there is one light image per camera, so a torch on an upper floor lights the ground-floor art drawn beneath it (without the ground floor's shadows), and a ground light lights an upper floor drawn over it. Only shadows are kept per storey.

<!-- given: hero_look: Handle<Image> -->
```rust
// A hero on the first storey up, holding a torch: the torch is on that
// storey too, and draws lifted with the hero
let hero = ctx.world.spawn((
  Transform::from_xy(320.0, 240.0),
  Renderable::image(hero_look),
  Floor::On(1),
)).id();
ctx.spawn_child(hero, (Transform::from_xy(8.0, -12.0), Light2D::torch(160.0).with_shadows()));
// A pillar on that storey blocks the torch, not the ground floor's lights
ctx.world.spawn((Transform::from_xy(400.0, 240.0), LightOccluder2D::rect(16.0, 16.0), Floor::On(1)));
// A wall that blocks every storey's lights
ctx.world.spawn((Transform::from_xy(500.0, 240.0), LightOccluder2D::rect(16.0, 64.0), Floor::Every));
```

## Ambient Light

```rust
ctx.lighting().set_ambient(AmbientLight::new(Color::new(20, 20, 40, 255), 0.15));
let current = *ctx.lighting().ambient();

AmbientLight::dark();      // 0 intensity: pitch-black shadows
AmbientLight::night();
AmbientLight::moonlit();
AmbientLight::indoor();
AmbientLight::daylight();
AmbientLight::sunset();
AmbientLight::with_intensity(0.25);   // default colour
```

## Volumetric Light and Atmosphere

Volumetric beams are per light:

<!-- given: color: Color, radius: f32 -->
```rust
Light2D::flashlight(250.0, 0.0).with_volumetric();              // default beam
Light2D::point(color, radius).with_volumetric_and_god_rays();
Light2D::point(color, radius)
  .with_volumetric_settings(VolumetricSettings::new(0.3, 0.5, 0.2).with_god_rays());   // intensity, density, scattering
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

<!-- given: handle: Handle<Image> -->
```rust
Renderable::image(handle.clone()).with_drop_shadow(DropShadow::below(8.0));
Renderable::image(handle.clone()).with_drop_shadow(DropShadow::diagonal(6.0));
Renderable::image(handle.clone()).with_drop_shadow(DropShadow::squashed(12.0, 0.3));
Renderable::image(handle.clone()).with_drop_shadow(
  DropShadow::new().offset(4.0, 8.0).color(Color::new(0, 0, 0, 100)).scale(1.0, 0.6),
);
Renderable::image(handle.clone()).with_blob_shadow(
  BlobShadow::ellipse(20.0, 10.0).offset(48.0).with_height_scale(true).max_height(150.0),
);
Renderable::image(handle.clone()).with_contact_shadow(ContactShadow::new().radius(8.0).intensity(0.6).falloff(2.0));
```

## Ambient Occlusion

A screen-space HDR-stage effect on `EffectsManager`:

```rust
ctx.effects().set_ambient_occlusion(0.3, 16.0);              // intensity, radius
ctx.effects().set_ambient_occlusion_custom(0.3, 16.0, 0.05); // + bias
ctx.effects().disable_ambient_occlusion();
```

## What Gets Lit

The world (entities, `Scene::render` GPU sprites, trails and lines) is drawn first and lit. Anything with `.with_lighting(false)` (a `Renderable`, `Trail2D`, `Line2D`, particles) draws at full brightness over the lit scene (lasers, magic, glowing pickups); post-processing still applies. Give it `with_emission(n)` with n above 1 to glow past white and feed bloom. The UI is never lit.

## Pipeline

Per camera (the `lighting` camera pass):

```
1. Light accumulation: lights add into the camera's float light buffer, through its view
2. Shadows: each shadow-casting light masks itself in the buffer's alpha, so its shadows darken only its own light (overlapping shadows combine; a coloured shadow lets its colour through)
3. Composition: lit = scene x (ambient + light), plus atmosphere fog, in float with no clamp to 1
```

The look then maps the result to the screen. God rays are an HDR effect: they take the main camera's view of the god-ray lights, in frame coordinates, and the atmosphere's settings; their threshold is linear luminance (default 1.0, white), so only light brighter than white makes rays.

## Performance

- Lights have no cap: each lit camera draws the lights on its `RenderLayers` whose reach meets its view.
- Lighting runs once per camera: a minimap camera multiplies its cost, since each lit camera draws its lights and shadows into its own buffer. Lights and shadow geometry are built once per frame, then each camera draws the ones it sees.
- Disable shadows on unimportant lights; simple occluder shapes are cheaper.
- Every lit frame gathers every occluder, and each shadowed light loops over all of them (casting from those that may come within twice its radius), with no spatial index: a big lit tile map pays for all its chains (one shadowed light over a 1024 x 1024 cave of about 2,400 chains costs about 30 µs a frame more than the unlit map, on an Apple M5 Pro). Turn `occludes` off on layers whose shadows nothing shows.
- Lower AO radius is faster.
