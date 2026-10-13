// Sprite rendering shader with per-instance effects
//
// This shader renders textured sprite quads with support for:
// - Per-instance transforms (position, rotation, scale)
// - Per-instance UV rectangles (for sprite sheets/atlases)
// - Per-instance tint colors with alpha
// - Per-instance visual effects (outline, glow, flash, etc.)
//
// Atlas and standalone pages (Rgba8Unorm) are sampled as raw sRGB bytes.
// Effects and tint run in that sRGB space, in the CPU renderer's order
// (effect, then tint), and the result is converted to linear on output.
// Render-target pages hold linear colour premultiplied by alpha: the base
// entry tints them in linear light, the effect entries read them back as
// straight sRGB. Output goes to the linear float scene image. Samples outside
// the sprite's own rect (uv_bounds) are transparent, so atlas neighbours
// never show. Output is premultiplied by alpha, like every batch shader.

// The camera the sprites are drawn through (`CameraGlobals`)
@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct InstanceInput {
    @location(2) transform0: vec4<f32>,
    @location(3) transform1: vec4<f32>,
    @location(4) transform2: vec4<f32>,
    @location(5) uv_rect: vec4<f32>,
    @location(6) tint: vec4<f32>,
    @location(7) effect_params: vec4<f32>,
    @location(8) effect_extra: vec4<f32>,
    @location(9) uv_bounds: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) effect_params: vec4<f32>,
    @location(3) @interpolate(flat) effect_extra: vec4<f32>,
    @location(4) @interpolate(flat) uv_bounds: vec4<f32>,
}

// Alpha above which a texel counts as visible (the CPU tests alpha > 0)
const VISIBLE: f32 = 0.002;

// The fragment's UV derivatives, taken first thing in every fragment entry
// (where control flow is uniform), so any sample picks the right mip level
var<private> uv_ddx: vec2<f32>;
var<private> uv_ddy: vec2<f32>;

fn take_derivatives(uv: vec2<f32>) {
    uv_ddx = dpdx(uv);
    uv_ddy = dpdy(uv);
}

// Set by the entries for render-target pages, which hold linear colour
// premultiplied by alpha: samples come back as straight sRGB, like any
// other texture's
var<private> linear_page: bool;

// The sprite texture at `uv`, transparent outside `bounds`
fn sample_sprite(uv: vec2<f32>, bounds: vec4<f32>) -> vec4<f32> {
    let texel = sample_masked(sprite_texture, sprite_sampler, uv, bounds, uv_ddx, uv_ddy);
    if linear_page {
        return straight_srgb(texel);
    }
    return texel;
}

// Linear colour premultiplied by alpha as straight sRGB (0 to 1)
fn straight_srgb(c: vec4<f32>) -> vec4<f32> {
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let rgb = clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(linear_to_srgb(rgb), min(c.a, 1.0));
}

// A linear premultiplied texel tinted in linear light (the tint decoded from
// sRGB); values above 1 are kept
fn finish_linear(color: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let a = clamp(tint.a, 0.0, 1.0);
    return vec4<f32>(color.rgb * srgb_to_linear(tint.rgb) * a, color.a * a);
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    
    // Apply 2D affine transform from instance
    let transform = mat3x3<f32>(
        instance.transform0.xyz,
        instance.transform1.xyz,
        instance.transform2.xyz
    );
    
    let local_pos = vec3<f32>(vertex.position, 1.0);
    let world_pos = transform * local_pos;
    
    // Through the camera
    var clip = globals.view_proj * vec4<f32>(world_pos.xy, 0.0, 1.0);
    // Pixel art: a quad axis-aligned on screen (itself and the camera
    // unturned) rounds its top-left and size to whole pixels once, so it
    // keeps its width as it moves; under a turned camera it draws unsnapped
    let unturned = abs(instance.transform0.y) < 1e-5 && abs(instance.transform1.x) < 1e-5;
    let camera_unturned = abs(globals.view_proj[0][1]) < 1e-5 && abs(globals.view_proj[1][0]) < 1e-5;
    let axis_aligned = unturned && camera_unturned;
    if globals.snap > 0.5 && axis_aligned {
        let p0 = to_pixel((transform * vec3<f32>(-0.5, -0.5, 1.0)).xy);
        let p1 = to_pixel((transform * vec3<f32>(0.5, 0.5, 1.0)).xy);
        let origin = round(min(p0, p1));
        let size = round(abs(p1 - p0));
        // A corner that maps to the larger pixel coordinate counts from the far side
        let along = select(vertex.position + 0.5, 0.5 - vertex.position, p1 < p0);
        clip = from_pixel(origin + along * size);
    }
    out.clip_position = clip;
    
    // Interpolate UVs from uv_rect
    out.uv = mix(instance.uv_rect.xy, instance.uv_rect.zw, vertex.uv);
    
    out.tint = instance.tint;
    out.effect_params = instance.effect_params;
    out.effect_extra = instance.effect_extra;
    out.uv_bounds = instance.uv_bounds;
    
    return out;
}

// Target pixel coordinates (top-left origin) of a world position
fn to_pixel(world: vec2<f32>) -> vec2<f32> {
    let clip = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
    return (clip.xy * vec2<f32>(0.5, -0.5) + 0.5) * globals.resolution;
}

// The clip position of target pixel coordinates
fn from_pixel(pixel: vec2<f32>) -> vec4<f32> {
    let ndc = (pixel / globals.resolution - 0.5) * vec2<f32>(2.0, -2.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

// Base fragment shader (no effects)
@fragment
fn fs_base(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    return finish(sample_sprite(in.uv, in.uv_bounds), in.tint);
}

// Effect fragment shader
@fragment
fn fs_effect(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    return finish(effect_color(in), in.tint);
}

// The same for premultiplied textures (render targets): their colour is
// already multiplied by alpha
@fragment
fn fs_base_premultiplied(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    let color = sample_sprite(in.uv, in.uv_bounds);
    return finish_premultiplied(color, in.tint);
}

@fragment
fn fs_effect_premultiplied(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    return finish_premultiplied(effect_color(in), in.tint);
}

// The same for render-target pages
@fragment
fn fs_base_linear(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    let color = sample_masked(sprite_texture, sprite_sampler, in.uv, in.uv_bounds, uv_ddx, uv_ddy);
    return finish_linear(color, in.tint);
}

// Effects work on straight sRGB, so the page reads back as it
@fragment
fn fs_effect_linear(in: VertexOutput) -> @location(0) vec4<f32> {
    take_derivatives(in.uv);
    linear_page = true;
    return finish(effect_color(in), in.tint);
}

// The sprite's colour with its built-in effect applied
fn effect_color(in: VertexOutput) -> vec4<f32> {
    let base = sample_sprite(in.uv, in.uv_bounds);
    let params = in.effect_params;
    let extra = in.effect_extra;
    var color = base;
    
    switch (i32(params.x)) {
        case 0: { // None
            // No effect, just return base color
        }
        case 1: { // Outline: y=thickness, extra=color
            color = apply_outline(base, in.uv, in.uv_bounds, params.y, extra);
        }
        case 2: { // Glow: y=radius, z=intensity, extra=color
            color = apply_glow(base, in.uv, in.uv_bounds, params.y, params.z, extra);
        }
        case 3: { // Flash: y=intensity, extra=color
            color = vec4<f32>(mix(base.rgb, extra.rgb, params.y), base.a);
        }
        case 4: { // ColorShift: y=amount, extra=color
            color = vec4<f32>(mix(base.rgb, extra.rgb, params.y), base.a);
        }
        case 5: { // Silhouette: extra=color
            color = vec4<f32>(extra.rgb, base.a * extra.a);
        }
        case 6: { // Pixelate: y=block size
            color = apply_pixelate(in.uv, in.uv_bounds, params.y);
        }
        case 7: { // Wave: y=amplitude, z=frequency, w=time
            color = apply_wave(in.uv, in.uv_bounds, params.y, params.z, params.w);
        }
        case 8: { // Dissolve: y=progress, extra=edge color
            color = apply_dissolve(base, in.uv, in.uv_bounds, params.y, extra);
        }
        case 9: { // Invert
            color = vec4<f32>(1.0 - base.rgb, base.a);
        }
        case 10: { // Grayscale: y=intensity
            let gray = dot(base.rgb, vec3<f32>(0.299, 0.587, 0.114));
            color = vec4<f32>(mix(base.rgb, vec3<f32>(gray), params.y), base.a);
        }
        case 11: { // DirectionalOutline: y=thickness, z/w=passable color, extra=solid color
            color = apply_directional_outline(base, in.uv, in.uv_bounds, params, extra);
        }
        default: {
            // Unknown effect, just return base color
        }
    }
    
    return color;
}

// =============================================================================
// Sampling and colour space
// =============================================================================

fn texel_size() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(sprite_texture));
}

// Tint like the CPU renderer, then convert RGB to linear and premultiply.
//
// RGB is quantised to bytes: the source byte times the tint fraction,
// truncated. Alpha is source alpha times tint alpha, truncated to a byte
// unless it reaches 1 (so opaque stays opaque).
fn finish(color: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let source = round(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0);
    let rgb = floor(min(source * tint.rgb, vec3<f32>(255.0))) / 255.0;
    let a = clamp(color.a * tint.a, 0.0, 1.0);
    let alpha = select(floor(a * 255.0) / 255.0, 1.0, a >= 1.0);
    return vec4<f32>(srgb_to_linear(rgb) * alpha, alpha);
}

// Tint a premultiplied colour like `finish`: RGB in sRGB, with the tint's
// alpha scaling colour and alpha together; then convert RGB to linear.
fn finish_premultiplied(color: vec4<f32>, tint: vec4<f32>) -> vec4<f32> {
    let rgb = srgb_to_linear(clamp(color.rgb * tint.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let alpha = clamp(color.a * tint.a, 0.0, 1.0);
    return vec4<f32>(rgb * tint.a, alpha);
}

// =============================================================================
// Effect implementations
// =============================================================================

// Whether a visible texel lies within `thickness` texels of `uv` (square
// neighbourhood, like the CPU outline)
fn near_visible(uv: vec2<f32>, bounds: vec4<f32>, thickness: f32) -> bool {
    let texel = texel_size();
    let t = i32(thickness);
    for (var dy = -t; dy <= t; dy += 1) {
        for (var dx = -t; dx <= t; dx += 1) {
            if dx == 0 && dy == 0 { continue; }
            let offset = vec2<f32>(f32(dx), f32(dy)) * texel;
            if sample_sprite(uv + offset, bounds).a > VISIBLE {
                return true;
            }
        }
    }
    return false;
}

fn apply_outline(
    base: vec4<f32>,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    thickness: f32,
    outline_color: vec4<f32>
) -> vec4<f32> {
    if base.a > VISIBLE || !near_visible(uv, bounds, thickness) {
        return base;
    }
    return outline_color;
}

// Two 8-bit channels packed as hi * 256 + lo
fn unpack_pair(v: f32) -> vec2<f32> {
    let hi = floor(v / 256.0);
    return vec2<f32>(hi, v - hi * 256.0) / 255.0;
}

// Outline whose part below the sprite's rect (the passable bottom edge) uses
// the passable colour; the top, left and right use the solid colour
fn apply_directional_outline(
    base: vec4<f32>,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    params: vec4<f32>,
    solid_color: vec4<f32>
) -> vec4<f32> {
    if base.a > VISIBLE || !near_visible(uv, bounds, params.y) {
        return base;
    }
    if uv.y > bounds.w {
        return vec4<f32>(unpack_pair(params.z), unpack_pair(params.w));
    }
    return solid_color;
}

fn apply_glow(
    base: vec4<f32>,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    radius: f32,
    intensity: f32,
    glow_color: vec4<f32>
) -> vec4<f32> {
    // Sample glow from surrounding pixels
    let texel = texel_size();
    var glow_amount = 0.0;
    let samples = 8.0;
    
    for (var i = 0.0; i < samples; i += 1.0) {
        let angle = i * 6.28318 / samples;
        let offset = vec2<f32>(cos(angle), sin(angle)) * radius * texel;
        glow_amount += sample_sprite(uv + offset, bounds).a;
    }
    glow_amount = glow_amount / samples * intensity;
    
    // Blend glow behind sprite
    let glow = vec4<f32>(glow_color.rgb, glow_amount * glow_color.a);
    return mix(glow, base, base.a);
}

fn apply_pixelate(uv: vec2<f32>, bounds: vec4<f32>, block_size: f32) -> vec4<f32> {
    // Blocks start at the sprite's top-left texel; each shows its first texel
    let texel = texel_size();
    let size = max(block_size, 1.0);
    let local = floor((uv - bounds.xy) / texel);
    let block = floor(local / size) * size;
    return sample_sprite(bounds.xy + (block + 0.5) * texel, bounds);
}

// Like the CPU renderer did: each row shifts right by `amplitude` texels times
// sin(phase), with `frequency` cycles down the sprite's height, wrapping
// round within the sprite. `time` is the effect's own phase.
fn apply_wave(
    uv: vec2<f32>,
    bounds: vec4<f32>,
    amplitude: f32,
    frequency: f32,
    time: f32
) -> vec4<f32> {
    let texel = texel_size();
    let slack = texel.x * 0.01;
    if uv.x < bounds.x - slack || uv.x > bounds.z + slack {
        return vec4<f32>(0.0);
    }
    let size = max(bounds.zw - bounds.xy, vec2<f32>(1e-6));
    let local_y = (uv.y - bounds.y) / size.y;
    let shift = amplitude * sin(local_y * frequency * 6.2831853 + time) * texel.x;
    let x = bounds.x + fract((uv.x - shift - bounds.x) / size.x) * size.x;
    return sample_sprite(vec2<f32>(x, uv.y), bounds);
}

fn apply_dissolve(
    base: vec4<f32>,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    progress: f32,
    edge_color: vec4<f32>
) -> vec4<f32> {
    // Simple noise-based dissolve over the sprite's own rect
    let size = max(bounds.zw - bounds.xy, vec2<f32>(1e-6));
    let local = (uv - bounds.xy) / size;
    let noise = fract(sin(dot(local, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    
    if noise < progress {
        return vec4<f32>(0.0); // Dissolved
    }
    
    // Edge glow on visible pixels
    let edge_width = 0.1;
    if noise < progress + edge_width && edge_color.a > 0.0 && base.a > VISIBLE {
        let edge_factor = (noise - progress) / edge_width;
        return mix(edge_color, base, edge_factor);
    }
    
    return base;
}
