# Renderer HDR (2c): Float World, Tonemapping, Grading, Lighting on the Batch - Design

## Overview

Sub-project 2c of the professional-grade overhaul (`docs/research/2026-10-11-engine-audit.md`, Part 1 §3) follows the renderer core (2a, `docs/specs/2026-10-12-renderer-core-design.md`) and the 2D batch (2b, `docs/specs/2026-10-13-renderer-batch-design.md`). It renders the world in **linear HDR** (`Rgba16Float`), adds a **tonemap pass** with exposure (manual and automatic), tonemap curves and **colour grading** (parameters and a LUT), splits effects into an **HDR stage and a display stage**, makes every `Color` mean **sRGB**, lets drawables **glow above white**, and turns **lighting into a batch client** whose shadows darken only their own light.

**Why:** every image the world draws into is `Rgba8UnormSrgb` except the light buffer, so lit scenes, additive effects and bloom clip at white and bloom thresholds clamped values. Light, ambient, atmosphere, line and effect colours read their sRGB bytes as linear while sprites decode them, so a light or a line never matches a sprite of the same `Color`. Shadows are subtracted from the shared light buffer, so one light's shadow darkens every light and overlapping occluders subtract twice; the ray-marched shadow path is dead code that `Light2D::with_raymarched_shadows` silently feeds. Lights draw through a separate renderer with no per-camera culling or layers. God rays are placed within the main camera's rectangle but drawn over the whole frame, so they are wrong whenever that camera does not cover the window.

**Owner decisions (2026-10-13):**
- **Goal:** full lighting look control: light and glow above white that roll off instead of clipping, bloom from genuinely bright pixels, emission, tonemap curves, colour grading and auto-exposure.
- **Approach:** two stages: an HDR world (`Rgba16Float`) and a display stage (`Rgba8UnormSrgb`) after a tonemap pass; display-space effects (fade, flash, tint, transition) run after tonemapping.
- **Shadows:** keep the shadow volumes, but each light's shadow darkens only that light; delete the dead ray-marched path. The shadow rewrite (shadow maps or distance fields, an occluder index) stays a later feature-module sub-project.
- **Default look:** identity up to a knee (about 0.8 linear), then a smooth shoulder; filmic curves are opt-in.
- **Colour:** a `Color` is sRGB everywhere (lights, ambient, atmosphere, lines, effect colours), decoded to linear before the GPU sees it.
- **Breaking changes:** acceptable; networking stays out of scope beyond what compiles.

**Out of scope:** the shadow rewrite and an occluder spatial index (feature modules); MSAA; a separate look per camera (post-processing stays once per frame); HDR output to HDR displays (the surface stays SDR); normal maps and forward lighting; a GPU UI (sub-project 4); the remaining 2a/2b minors not named here.

## The frame

```
per camera:  clear → world → lighting → unlit_world                        (Rgba16Float, linear, premultiplied)
per frame:   cameras → hdr_effects → tonemap → transition → post_process → ui → composite → capture
             └─ Rgba16Float ─────┘   └────────── Rgba8UnormSrgb, display-referred ──────────┘
```

- **New passes:** `pass_ids::HDR_EFFECTS` runs the HDR-stage effects; `pass_ids::TONEMAP` applies the look (exposure, curve, grading) and writes the display image. Auto-exposure's measurement runs inside `TONEMAP` before it applies the look.
- **`post_process`** keeps its id and runs only the display-stage effects; disabling it no longer removes bloom (disabling `hdr_effects` does). `transition`, `ui`, `composite` and `capture` keep their behaviour; the composite only letterboxes, encodes sRGB for a non-sRGB surface and blends the UI.
- **Images.** `Rgba16Float`: the scene image, the lit image, camera images, render-target textures, the lighting scene copy, the HDR ping-pong textures and bloom scratch; the light buffer stays `Rgba16Float` (its alpha becomes the per-light shadow mask). `Rgba8UnormSrgb`: the display image, the display ping-pong textures and the transition copy. Unchanged: atlas pages (`Rgba8Unorm` raw sRGB bytes), the UI texture, the surface, the offscreen target and the capture readback.
- `FrameImage` gains `Display(..)`. `RenderContext::format()` reports `Rgba16Float` in camera passes and frame passes before `tonemap`, `Rgba8UnormSrgb` after it. A game pass inserted before `composite` draws on the display image, as today.
- Every format that today comes from `game_texture_format()` follows its stage; copies between images keep matching formats (scene → lighting scene copy and → the first HDR buffer are float; the transition copies the display image).

## Colour

- **One rule.** `Color` is sRGB. `Color::to_linear(self) -> [f32; 4]` (straight alpha, exact sRGB transfer function) is the one decoder, used for light colours, ambient and atmosphere, trail/line/debug vertex colours, the fade, flash and colour-tint effect colours, and the clear colours (already decoded).
- **Sprites keep their path:** tint applied to the sRGB bytes with the old CPU rounding, then `srgb_to_linear`, then premultiplied. Unlit art with every channel below sRGB 231 (0.8 linear) is byte-identical after the default look.
- **Emission.** `emission: f32` (default 1.0) multiplies a drawable's linear colour after tinting (colour, not alpha), so values can exceed 1 and feed bloom. On `Renderable` (`with_emission`), `SpriteDraw` (`emission`), `SpriteInstance` (`with_emission`), `ParticleEmitter` (`with_emission`), `Trail2D` and `Line2D` (`with_emission`). Sprites carry it as `emission - 1` in `transform0.w` (the transform rows' `w` lanes are unused, and a zeroed lane, as in an instance a game builds field by field, means no glow), so instances stay 128 bytes; `with_transform` keeps the lane; lines bake it into their float vertex colour. A lit emissive sprite is still darkened by the light buffer; to glow in the dark it also takes `with_lighting(false)`.
- **Linear target pages.** Render targets hold linear premultiplied colour, so a sprite showing one samples linear values. `DrawState` and `PipelineKey` gain the page's texture kind (`Srgb8` atlas or standalone pages, `Linear` targets); `Linear` picks `fs_base_linear`, `fs_effect_linear` and, for game sprite shaders, `fs_custom_linear`, whose `sample()` converts the texel to straight sRGB so `sprite()` sees what it sees for any texture. On linear pages the tint multiplies the linear colour (the tint decoded from sRGB). `Renderable::render_target` keeps `BlendMode::Premultiplied`.
- **Blend modes in HDR.** Alpha, Premultiplied, Additive (now unbounded) and Multiply work unchanged. Screen is a 0..1 formula: over pixels brighter than white it dims them; the docs say so.

## Lighting

- **Lights are batch quads.** Each `Light2D` becomes a light item: a quad covering its radius, its colour decoded and multiplied by its intensity (unbounded), its spot, falloff and volumetric parameters in the 128-byte instance's other fields. Lights honour `RenderLayers` on their entity (default layer 0). Per lit camera, the batch plans the lights whose box meets its view and whose layers intersect its own, and draws them into the camera's `Rgba16Float` light buffer with the `Light` shader (`light.wgsl`'s point, spot, volumetric and glow formulas) and additive blending; the light buffer's pipelines differ from the scene's by the format already in `PipelineKey`.
- **Shadows darken only their own light.** Lights without shadows draw in one instanced draw. Each shadow-casting light then draws, inside its quad:
  1. the light buffer's alpha set to 1 (colour masked off);
  2. its shadow volumes (today's CPU geometry, now one triangle-stream item per light, with no vertex cap) multiplying the alpha by `1 - strength`, where strength is the shadow colour's alpha times the softness edge term, so overlapping occluders combine instead of double subtracting;
  3. the light with colour blending `light × dst alpha + dst` (alpha masked off);
  4. for a coloured shadow (`with_shadow_color` not black), the light again with `light × shadow colour × (1 - dst alpha) + dst`, so stained-glass shadows let their colour through.
  No stencil buffer and no optional WebGPU feature is needed. Shadow geometry generation itself is unchanged until the shadow rewrite.
- **Compose** stays one full-screen pass per lit camera, `lit = scene × (ambient + light)` plus atmosphere fog, reading and writing float images with no clamp; ambient and atmosphere colours are decoded. `LightingRenderer` keeps the per-camera light targets, the compose pipeline and its bind-group cache, and becomes engine-internal; light and shadow pipelines move to the batch.
- **God rays** take light positions in frame coordinates (`world_to_screen` divided by the render size), so they are right whatever the main camera's viewport, and their intensity and threshold from `Atmosphere`'s god-ray settings. They run in `hdr_effects` and follow the main camera's lights, as today.
- **Removed:** the ray-marched shadow pipeline and shader, `render_lights_with_shadows`, `render_raymarched_shadow`, `compose_from_texture`, `Light2D::with_raymarched_shadows` (the shadows demo's page switches to shadow volumes), and the standalone `LightingRenderer` API (`render_lights`, `compose`).

## The look

`ctx.look() -> Mut<'_, Look>` (`ResMut<Look>` in systems), one per frame:

```rust
pub struct Look {
  pub exposure: f32,                       // EV, default 0
  pub auto_exposure: Option<AutoExposure>, // default None
  pub tonemap: Tonemap,                    // default Tonemap::Soft { knee: 0.8, white: 4.0 }
  pub grading: ColorGrading,               // default neutral
}
pub enum Tonemap { Soft { knee: f32, white: f32 }, Clamp, Aces, AgX }
pub struct AutoExposure {                  // Default: min_ev -4, max_ev 4, compensation 0,
  pub min_ev: f32, pub max_ev: f32,        //          speed_up 3, speed_down 1 (EV per second)
  pub compensation: f32, pub speed_up: f32, pub speed_down: f32,
}
pub struct ColorGrading {                  // Default: neutral (temperature 0, tint 0, contrast 1,
  pub temperature: f32, pub tint: f32,     //  saturation 1, lift 0, gamma 1, gain 1, no LUT, strength 1)
  pub contrast: f32, pub saturation: f32,
  pub lift: Vec3, pub gamma: Vec3, pub gain: Vec3,
  pub lut: Option<ColorLut>, pub lut_strength: f32,
}
```

- **The tonemap pass, in order:** multiply by `2^(exposure + auto EV)`; white balance (temperature, tint), then contrast (about mid-grey 0.18, in log2 space) and saturation, in linear HDR; the curve; lift, gamma and gain on the display-referred colour; encode sRGB, look up the LUT there (where LUTs are authored) and blend by `lut_strength`; store to the display image.
- **Curves.** `Soft`: identity up to `knee`, then extended Reinhard on the excess (`t = (m - knee) / (1 - knee)`, `g(t) = t (1 + t / tw²) / (1 + t)` with `tw = (white - knee) / (1 - knee)`, result `knee + (1 - knee) g(t)`), so slope 1 at the knee, monotonic, exactly 1 at `white` and clamped beyond; applied to the largest channel `m` and scaling all three, so hues do not shift. With the defaults a 1.0 channel shows at about 0.9 (sRGB 243). `Clamp` is today's look. `Aces` (the fitted curve) and `AgX` (with its base contrast) are the filmic options.
- **LUT.** `ColorLut::from_sprite(&SpriteHandle) -> Result<ColorLut, LutError>` reads a strip of N tiles of N×N side by side (1024×32 for N = 32, 256×16 for N = 16; `LutError` names a wrong size), uploads it as a 3D texture with trilinear sampling and follows the handle's hot reload.
- **Auto-exposure** runs on the GPU only. A compute pass over the HDR image builds a 64-bin histogram of log2 luminance in [-12, 4]; a second dispatch averages it without the darkest and brightest 5%, sets the target `clamp(log2(0.18 / average) + compensation, min_ev, max_ev)`, and moves a stored exposure toward it at `speed_up` (exposure rising) or `speed_down` EV per second of real time. The first frame starts at the target. The tonemap pass reads the stored value; nothing is read back.

## Effect stages

| Stage (pass) | Built-in effects | Image |
|---|---|---|
| HDR (`hdr_effects`) | Bloom, GodRays, AmbientOcclusion | `Rgba16Float`, linear, can exceed 1 |
| Display (`post_process`) | Fade, ColorTint, Vignette, ScreenFlash, CRT, ChromaticAberration | `Rgba8UnormSrgb`, sampled as linear 0..1 |

- `EffectsManager` keeps one list and its API; built-in effects have a fixed stage; within a stage, insertion order, then custom effects.
- **Custom effects:** `add_custom(name, handle)` stays a display effect (`scene(uv)` linear 0..1, as now); `add_custom_in(name, handle, EffectStage::Hdr)` adds an HDR one (`scene(uv)` linear HDR). `EffectStage::{Hdr, Display}`.
- **Bloom** extracts by linear luminance above `threshold` with a soft knee of half the threshold, unclamped throughout; `threshold` is any value ≥ 0 and defaults to 1.0 (was 0.7 on a 0..1 image), so only light brighter than white glows.
- **God rays** drop the `min(.., 1)` clamp and the `(b - t)/(1 - t)` remap for a soft-knee threshold.
- **Display effects** keep their maths; their colours are decoded, so `fade(Color(128, 128, 128))` fades to the mid-grey it names. The transition runs first on the display image, so its sRGB-paced fade is unchanged.
- Each stage has its own ping-pong pair; the tonemap pass writes the first display buffer. Both chains take their bind groups from the `BindGroupCache`.

## Public API changes

**Added:** `Look`, `Tonemap`, `ColorGrading`, `ColorLut`, `LutError`, `AutoExposure`, `SceneContext::look`; `EffectStage`, `EffectsManager::add_custom_in`; `Color::to_linear`; `emission` / `with_emission` on `Renderable`, `SpriteDraw`, `SpriteInstance`, `ParticleEmitter`, `Trail2D`, `Line2D`; `RenderLayers` honoured on lights; `pass_ids::HDR_EFFECTS`, `pass_ids::TONEMAP`.

**Changed:** the frame's pass order; `post_process` runs only display effects; `RenderContext::format()` depends on the stage; render targets are `Rgba16Float`; light, ambient, atmosphere, line, debug and effect colours are decoded from sRGB; bloom's `threshold` is unbounded and defaults to 1.0; shadows darken only their own light.

**Removed:** `Light2D::with_raymarched_shadows` and its sample count; `LightingRenderer` from the public API with `render_lights`, `compose`, `compose_from_texture` and `render_lights_with_shadows`; `raymarch_shadow.wgsl`.

## Migration (in place, every step green)

1. Float world images and the two stages, with the tonemap pass applying `Clamp` only and the display image; unlit art below 1 stays byte-identical.
2. The sRGB colour rule everywhere (`Color::to_linear`).
3. `Look`: the `Soft` default, `Aces`, `AgX`, grading and the LUT.
4. The effect stages, the bloom and god-ray rework and the god-ray position fix.
5. Emission and linear render-target pages.
6. Lighting on the batch: light quads, layers and culling, per-light shadow masks, coloured shadows; the dead path removed; `tests/e2e/lighting_renderer.rs` moved to engine-level tests.
7. Auto-exposure.
8. The `look` demo, demo retuning, documentation and verification.

## Testing

- **Unit:** `Color::to_linear`; the `Soft` curve (identity below the knee, slope 1 at it, monotonic, 1 at `white`, hue kept); `Clamp`, `Aces` and `AgX` map 0 to 0 and are monotonic; neutral grading and an identity LUT are identity; `LutError` for a wrong strip size; the histogram average against a CPU reference.
- **Engine pixel tests (`tests/e2e/gpu_world.rs` and friends):** a sprite below the knee is byte-identical under the default look and pure white is about 243; `Clamp` reproduces today; a fade to white reaches 255; an emissive sprite and an additive pile-up bloom while plain white does not; two lights and one occluder shadowing the first: the second light's area is untouched; a coloured shadow lets its colour through; a layer-1 light does not light a layer-0 camera; god rays sit on their light with a half-window main camera; auto-exposure brightens a dark scene over frames within `max_ev`; an inverting LUT inverts; an emissive sprite seen through a render target still blooms; a custom HDR effect sees values above 1; ambient `Color(128, 128, 128)` lights to mid-grey; steady frames create no bind groups with both effect chains.
- **Migrated:** `tests/e2e/lighting_renderer.rs` becomes engine-level tests; tests pinning `Rgba8UnormSrgb` (`render_pipeline.rs`, `gpu_basics.rs`) follow the stages.
- **Gates:** the demo tour, with every changed page checked by eye and listed; the five games captured at frame 60 and checked by eye; the `lighting` and `look` demos on the web in Chromium, WebKit and Firefox; the 10k-sprite headless frame within 20% of 2b's (about 1.02 ms).

## Documentation

- A new `look` demo (demo-kit standard, loading no files so it runs on the web): emission and bloom, tonemap curves, grading sliders, a LUT generated in code, auto-exposure across a dark-to-bright scene. The lighting, lighting-volumetric, bloom, post-processing and shadows demos are retuned.
- Skills: `ryo-api` (the look types, emission, light layers, `Color::to_linear`, effect stages, removals), `ryo-renderer` (stages, formats, the tonemap pass, linear pages), `ryo-lighting` (lights on the batch, per-light shadows, decoded colours, the compose formula in HDR), `ryo-game-dev` (recipes: a glowing pickup, a moody graded level, a cave with auto-exposure); copies in `src/cli/skills/`.
- AGENTS.md: `render/` (pass table, images, stages), `lighting/`, `shaders/`, `graphics/`; `docs/architecture/overview.md`.

## Design decisions

- **Two stages, not a tonemap in the composite:** fade, flash, tint and the transition mix toward exact colours; under a shoulder curve they would never reach them in HDR.
- **8-bit sRGB display image:** display effects keep today's behaviour and precision where it is perceptual; the transition's byte-exact test still holds.
- **Lights through the batch, compose kept:** lights gain culling and layers and share pipelines and bind groups with the batch, while lighting's model (an accumulated light buffer composed with the scene) stays; forward lighting is a later decision.
- **Shadow masks in the light buffer's alpha:** per-light correctness with blend factors every WebGPU device has, without a stencil attachment.
- **Hue-preserving shoulder:** a per-channel curve would turn saturated bright colours toward white and shift hues; 2D art relies on exact hues.
- **GPU-only auto-exposure:** no readback stall, and it works the same on the web.

## Risks

- Each shadow-casting light costs three or four small draws; dozens of them add up until the shadow rewrite.
- Float images double the world's memory bandwidth (the benchmark gate covers it).
- Decoded colours shift tuned looks in the lighting demos and in leap-king and nigh_t; each is retuned or checked by eye.
- f16 holds values up to 65504 with about three decimal digits; additive pile-ups stay well inside it.
