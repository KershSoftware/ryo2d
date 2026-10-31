# AGENTS.md - 2D Lighting System

## Architecture

The lighting system uses a **deferred lighting approach**:

1. **Light Accumulation Pass**: lights are batch quads (`batch.rs`), drawn additively into each lit camera's light buffer (`Rgba16Float`, so light above 1 is kept). A light draws on the cameras whose `RenderLayers` it shares and whose view its box meets.
2. **Shadows**: each shadow-casting light masks itself in the light buffer's alpha before it adds its light: the mask is reset to 1 in its quad, its shadow volumes (occluder edges facing away from it, projected past its radius, as batch triangles) each keep 1 - strength of it, then the light adds where the mask lets it through (and, for a coloured shadow, tinted where it held it back). One light's shadow never darkens another light. The strength of a shadow volume is the shadow colour's alpha times the softness edge term (`shadow.wgsl`; see `Light2D::with_soft_shadows`: a soft shadow fades with distance from the light but keeps its outline); overlapping volumes multiply, each keeping 1 - strength.
3. **Composition Pass**: `lit = scene × (ambient + light)`, plus atmosphere scattering/fog, in float with no clamp to 1 (the look rolls bright results off later). The light and the result are limited to 65504, the largest f16, so a huge intensity cannot make NaN.

Scenes control lighting through the `LightingSettings` resource (`ctx.lighting()`): `set_ambient(AmbientLight::...)`, `set_atmosphere(..)` and `set_enabled(bool)`. The `LightingRenderer` is engine-owned; `present_frame` copies the settings into it before each frame. Volumetric light is per light (`Light2D::set_volumetric`); there is no global switch.

- **When lighting runs:** with no override (`enabled()` is `None`), lighting turns on once any `Light2D` exists and then stays on, also after the lights are gone and across scene changes. `set_enabled(true)` gives a dark scene before any light exists; `set_enabled(false)` forces it off.
- `SceneCleanup::LIGHTING` resets the settings (ambient, atmosphere, the override) to their defaults; it does not switch off a renderer that a light already turned on.
- The UI is never lit: it is composited after lighting and post-processing.
- **Per camera:** the `lighting` pass runs for every camera. Each lit camera has light targets of its own (`LightTargets`: accumulation buffer, scene copy and compose uniforms, sized to its image), created on first use and dropped with the camera. Lights and shadows draw through the camera's uniform buffer (its `view_proj`), so they move, zoom, turn and shake with the sprites. A camera drawing straight into the scene image composes into the lit image; a camera with an image of its own is lit in place (the compose pass reads the scene copy and writes the camera's image).
- Each camera's compose uniforms are its own buffer: queue writes all land before the command buffer runs, so a shared buffer would give every camera the last one's values.
- Lights are batch quads (`lighting::batch`), drawn by the `lighting` pass into each lit camera's light buffer; `LightingRenderer` is engine-internal and keeps the per-camera light buffers, the scene copies and the compose pass.
- **Colours are sRGB in the API.** Every light, shadow, ambient and atmosphere colour decodes through `Color::to_linear` before it reaches the GPU, so `Color(128, 128, 128)` is mid-grey light. Preset colour bytes keep their bytes and now name the sRGB colour they say.
- God rays are an HDR effect: `god_rays.rs` puts the god-ray lights in frame coordinates (0 to 1 across the whole frame) from the main camera's view and gives the effect the atmosphere's settings. Their threshold is linear luminance.

## Limits

- **No light cap**: each lit camera draws the lights on its `RenderLayers` whose reach meets its view: those without shadow volumes in one instanced draw, each shadowed one in three or four draws of its own. A light with a radius or intensity that is not positive and finite, or a position that is not finite, draws nothing.
- **No shadow cap**: shadow volumes are built per light, once a frame, for every enabled occluder that may reach within twice its radius, and have no vertex limit. Shadow casting and complex occluder polygons add cost. Disable shadows on unimportant lights.
- **A lit frame allocates nothing for its occluders**: the occluders' query is kept on the engine (`occluder_query`), and shadow volumes place each occluder's vertices as they walk them (`LightOccluder2D::placed`, where `transform_vertices` collects a `Vec`), so a shadowed light over a tile map's thousands of chains collects nothing (`tests/tilemap_alloc.rs`).
- **No spatial index**: every lit frame gathers every occluder (`engine/extract.rs`), and each shadowed light loops over all of them for the reach test. A big tile map's chains are thousands (about 2,400 on the 1024 x 1024 cave of `benches/tilemap.rs`): one shadowed light over it costs about 30 µs a frame more than the unlit map (`docs/backlog/tilemap.md`). Map occluders are per chunk already, so an index by chunk, a light reading only the chunks its reach meets, is the proposed fix.

## Conventions

- **Artist-friendly units**: colors use 0-255 sRGB, not 0.0-1.0. Radius is in pixels. Intensity above 1 shows (the look rolls it off).
- **Placed by `GlobalTransform`**: lights draw at their entity's world translation; occluders use its world translation and rotation (not its scale). On an interpolated entity both are blended with its sprite (`render_global`).
- **Coordinate space**: occluder vertices are local space (relative to the entity), CCW winding: CCW on screen, which with y down is a negative signed area, so `casts`' normal `(-edge.y, edge.x)` faces out.
- **Chains**: `LightOccluder2D::chain` is an open run of edges (no edge from the last vertex back to the first; `is_closed` is false), wound as a polygon's: each edge casts exactly as it would as an edge of the polygon its boundary closes, so chains that together close a ring cast as that polygon does (`a_chain_occluder_casts_like_the_polygon_it_bounds`). Occluders that touch, each a closed polygon, cast their touching edges' shadows through each other; tile maps build their occluders as chains of their boundary for that reason (`tilemap/occluders.rs`).
- **Reach test by extent**: a light skips an occluder whose entity sits farther than twice its radius plus the occluder's extent (`LightOccluder2D::extent`, its farthest vertex from the entity, kept from construction, as the vertices never change), so no vertex can come within twice the radius. A test by entity alone missed a tile map's chunk-long chains, whose entities sit at their boxes' centres up to 256 px from their ends: a 64 px torch at a wall's end cast no shadow (`a_small_light_at_the_end_of_a_chunk_long_wall_casts_its_shadow`).
- **Tile occluders** (`tilemap/occluders.rs`, tilemap module notes): a spawned map's occluding layers (`LayerInfo::occludes`, else as they collide) keep `LightOccluder2D::chain`s of the boundary of what occludes, per 32-cell chunk, children of the first occluding layer marked `MapOccluder { chunk }` (prelude), rebuilt per changed chunk in `PostUpdate` before transforms propagate, so an edit casts that frame. A tile's shapes marked `occluder` block light and do not collide; without any, its collision shapes occlude.

## Notes for AI Agents

- Lighting is visual: validate changes by running `examples/demos/lighting`, `lighting-volumetric`, or `shadows`.
- Usage and shadow-type APIs are covered by the `ryo-lighting` skill.
- The `shadows` demo's mouse-driven lights start at a fixed point until the mouse first moves.

## Future Work

- [ ] Light cookies (textured lights)
- [ ] Normal maps for depth-aware lighting
