// Tile chunks: a chunk's instances are built once, in its layer's own
// pixels, with UVs over its tileset's padded image (`render/tiles.rs` has
// the lanes). The draw's slot places them: the layer's world transform, the
// camera's parallax offset and where the padded image sits on its page, so
// moving the map, moving the camera or the atlas moving the image changes
// no instance. Animated tiles step through their frames on game time. The
// fragment stage is the sprite's (`fs_base` and its variants).
//
// Repeated image layers: one quad over the copies a camera sees, its UVs
// counted in copies, the image wrapping along both axes (`vs_repeat`,
// `fs_repeat`), placed as a tile layer is: the layer's origin snapped to a
// whole pixel once, the copies at whole multiples of the image from it.

// One draw's slot (`TileSlot`, `render/batch/chunks.rs`)
struct TileSlot {
    // The layer's world transform: its x and y axes, then its origin
    x_axis: vec2<f32>,
    y_axis: vec2<f32>,
    origin: vec2<f32>,
    // The camera's parallax offset for the layer, in world units
    parallax: vec2<f32>,
    // Where the padded image sits on its page: UV offset, then scale
    uv: vec4<f32>,
};

@group(2) @binding(0) var<uniform> tile_slot: TileSlot;

// How far, in padded-image UV, an animated tile's frame for the game time
// is from its first. `params` is `[frames, seconds per frame, step u,
// step v]` and `extra` `[first column, first row, columns, phase]`: frame
// k is cell `first + k` counted row-major, rows wrapping at `columns`. A
// still tile has no frames
fn frame_step(params: vec4<f32>, extra: vec4<f32>) -> vec2<f32> {
    let frames = round(params.x);
    if frames < 2.0 || params.y <= 0.0 || params.z <= 0.0 {
        return vec2<f32>(0.0);
    }
    let beats = floor((globals.game_time + extra.w) / params.y);
    let k = clamp(beats - frames * floor(beats / frames), 0.0, frames - 1.0);
    let columns = max(round(extra.z), 1.0);
    let first = round(extra.x);
    let cell = first + k;
    let rows = floor(cell / columns);
    return vec2<f32>((cell - rows * columns - first) * params.z, rows * params.w);
}

@vertex
fn vs_tile(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    // The corner in the layer's pixels, then in the world
    let local = instance.transform2.xy
        + instance.transform0.xy * vertex.position.x
        + instance.transform1.xy * vertex.position.y;
    let origin = tile_slot.origin + tile_slot.parallax;
    let world = tile_slot.x_axis * local.x + tile_slot.y_axis * local.y + origin;
    var clip = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
    // Pixel art: the slot's origin moves onto a whole pixel, so every tile
    // corner, whole cells from it, lands on one at whole zooms, turned and
    // flipped tiles too (a quad's own snap skips those)
    let camera_unturned = abs(globals.view_proj[0][1]) < 1e-5 && abs(globals.view_proj[1][0]) < 1e-5;
    if globals.snap > 0.5 && camera_unturned {
        let at = to_pixel(origin);
        clip = from_pixel(pixel_of(clip) + round(at) - at);
    }
    out.clip_position = clip;

    // Padded-image UVs, stepped to the frame shown, into the page
    let step = frame_step(instance.effect_params, instance.effect_extra);
    let rect = instance.uv_rect + step.xyxy;
    let bounds = instance.uv_bounds + step.xyxy;
    let offset = tile_slot.uv.xy;
    let scale = tile_slot.uv.zw;
    out.uv = offset + mix(rect.xy, rect.zw, vertex.uv) * scale;
    out.uv_bounds = vec4<f32>(offset + bounds.xy * scale, offset + bounds.zw * scale);

    out.tint = instance.tint;
    out.effect_params = vec4<f32>(0.0);
    out.effect_extra = vec4<f32>(0.0);
    out.emission = instance.transform0.w + 1.0;
    return out;
}

// A repeated image layer's quad: a world-space instance as a sprite's (its
// transform lanes' xy), its UVs in copies of the image (`uv_rect`), the
// image's place on its page in `uv_bounds`, and the layer's origin in the
// world in the transform lanes' unused z (`transform0.z`, `transform1.z`).
// Pixel art moves the quad as that origin's snap to a whole pixel does, and
// never rounds the quad itself, so the copies stay whole multiples of the
// image from the snapped origin, at any zoom, with no gap between two
@vertex
fn vs_repeat(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let world = instance.transform2.xy
        + instance.transform0.xy * vertex.position.x
        + instance.transform1.xy * vertex.position.y;
    var clip = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
    let camera_unturned = abs(globals.view_proj[0][1]) < 1e-5 && abs(globals.view_proj[1][0]) < 1e-5;
    if globals.snap > 0.5 && camera_unturned {
        let at = to_pixel(vec2<f32>(instance.transform0.z, instance.transform1.z));
        clip = from_pixel(pixel_of(clip) + round(at) - at);
    }
    out.clip_position = clip;
    out.uv = mix(instance.uv_rect.xy, instance.uv_rect.zw, vertex.uv);
    out.uv_bounds = instance.uv_bounds;
    out.tint = instance.tint;
    out.effect_params = vec4<f32>(0.0);
    out.effect_extra = vec4<f32>(0.0);
    out.emission = instance.transform0.w + 1.0;
    return out;
}

// The sprite texture at `uv`, counted in copies of the image whose place on
// its page is `bounds`: the fraction of each coordinate runs across the
// image, kept half a texel inside it so filtering never reaches past it.
// The level comes from the unwrapped UV's derivatives, so it does not jump
// where one copy meets the next
fn sample_repeated(uv: vec2<f32>, bounds: vec4<f32>) -> vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(sprite_texture));
    let size = bounds.zw - bounds.xy;
    let low = bounds.xy + texel * 0.5;
    let high = max(low, bounds.zw - texel * 0.5);
    let inner = clamp(bounds.xy + fract(uv) * size, low, high);
    return textureSampleGrad(sprite_texture, sprite_sampler, inner, uv_ddx * size, uv_ddy * size);
}

@fragment
fn fs_repeat(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    return ui_out(finish(sample_repeated(in.uv, in.uv_bounds), in.tint, in.emission));
}

// The same for an image whose colour is already multiplied by alpha
@fragment
fn fs_repeat_premultiplied(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    let color = sample_repeated(in.uv, in.uv_bounds);
    return ui_out(finish_premultiplied(color, in.tint, in.emission));
}
