# AGENTS.md - 2D Lighting System

## Architecture

The lighting system uses a **deferred lighting approach**:

1. **Light Accumulation Pass**: all lights render to a light buffer with additive blending.
2. **Shadow Pass**: occluder geometry is projected away from lights and subtracted from the light buffer (reverse-subtract blending).
3. **Composition Pass**: `final_color = base_color × (ambient + light_buffer)`, plus atmosphere scattering/fog.

Ambient and atmosphere state lives on the `LightingRenderer` (`ctx.lighting`, `None` when lighting is disabled). Set it with `set_ambient(AmbientLight::...)`, not as an ECS resource. UI is unaffected by lighting unless `set_ui_affected_by_lighting(true)`.

## Limits

- **`MAX_LIGHTS` = 256**: all lights draw in one instanced call. Lights beyond the cap are silently dropped. There is no viewport culling.
- **`MAX_RAYMARCH_EDGES` = 64**: ray-marched shadows consider at most 64 occluder edges. Extra edges are ignored.
- Shadow casting and complex occluder polygons add cost. Disable shadows on unimportant lights.

## Conventions

- **Artist-friendly units**: colors use 0-255 RGB, not 0.0-1.0. Radius is in pixels.
- **Transform required**: lights and occluders are positioned by their entity's `Transform`.
- **Coordinate space**: light positions are world space. Occluder vertices are local space (relative to the transform), CCW winding.

## Notes for AI Agents

- Lighting is visual: validate changes by running `examples/demos/lighting`, `lighting-volumetric`, or `shadows`.
- Usage and shadow-type APIs are covered by the `ryo-lighting` skill.

## Future Work

- [ ] Light cookies (textured lights)
- [ ] Normal maps for depth-aware lighting
- [ ] Light layers (selective illumination)
