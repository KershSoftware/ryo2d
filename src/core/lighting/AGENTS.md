# AGENTS.md - 2D Lighting System

## Architecture

The lighting system uses a **deferred lighting approach**:

1. **Light Accumulation Pass**: all lights render to a light buffer with additive blending.
2. **Shadow Pass**: occluder geometry is projected away from lights and subtracted from the light buffer (reverse-subtract blending).
3. **Composition Pass**: `final_color = base_color × (ambient + light_buffer)`, plus atmosphere scattering/fog.

Scenes control lighting through the `LightingSettings` resource (`ctx.lighting()`): `set_ambient(AmbientLight::...)`, `set_atmosphere(..)` and `set_enabled(bool)`. The `LightingRenderer` is engine-owned; `present_frame` copies the settings into it before each frame. Volumetric light is per light (`Light2D::set_volumetric`); there is no global switch.

- **When lighting runs:** with no override (`enabled()` is `None`), lighting turns on once any `Light2D` exists and then stays on, also after the lights are gone and across scene changes. `set_enabled(true)` gives a dark scene before any light exists; `set_enabled(false)` forces it off.
- `SceneCleanup::LIGHTING` resets the settings (ambient, atmosphere, the override) to their defaults; it does not switch off a renderer that a light already turned on.
- The UI is never lit: it is composited after lighting and post-processing.

## Limits

- **`MAX_LIGHTS` = 256**: all lights draw in one instanced call. Lights beyond the cap are silently dropped. There is no viewport culling.
- **`MAX_RAYMARCH_EDGES` = 64**: ray-marched shadows consider at most 64 occluder edges. Extra edges are ignored.
- Shadow casting and complex occluder polygons add cost. Disable shadows on unimportant lights.

## Conventions

- **Artist-friendly units**: colors use 0-255 RGB, not 0.0-1.0. Radius is in pixels.
- **Placed by `GlobalTransform`**: lights draw at their entity's world translation; occluders use its world translation and rotation (not its scale). On an interpolated entity both are blended with its sprite (`render_global`).
- **Coordinate space**: occluder vertices are local space (relative to the entity), CCW winding.

## Notes for AI Agents

- Lighting is visual: validate changes by running `examples/demos/lighting`, `lighting-volumetric`, or `shadows`.
- Usage and shadow-type APIs are covered by the `ryo-lighting` skill.

## Future Work

- [ ] Light cookies (textured lights)
- [ ] Normal maps for depth-aware lighting
- [ ] Light layers (selective illumination)
