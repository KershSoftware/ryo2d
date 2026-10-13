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
- **Per camera:** the `lighting` pass runs for every camera. Each lit camera has light targets of its own (`LightTargets`: accumulation buffer, scene copy and compose uniforms, sized to its image), created on first use and dropped with the camera. Lights and shadows draw through the camera's uniform buffer (its `view_proj`), so they move, zoom, turn and shake with the sprites. A camera drawing straight into the scene image composes into the lit image; a camera with an image of its own is lit in place (the compose pass reads the scene copy and writes the camera's image).
- Each camera's compose uniforms are its own buffer: queue writes all land before the command buffer runs, so a shared buffer would give every camera the last one's values.
- The renderer's own targets (`LightingRenderer::new(device, width, height, ..)`) serve only its standalone API (`render_lights(.., camera_pos, camera_zoom)`, `compose`, `compose_from_texture`); the engine creates it at 1x1.
- God rays (a post-process effect) follow the main camera.

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
