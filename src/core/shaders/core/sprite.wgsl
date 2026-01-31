// Sprite rendering shader with per-instance effects
//
// This shader renders textured sprite quads with support for:
// - Per-instance transforms (position, rotation, scale)
// - Per-instance UV rectangles (for sprite sheets/atlases)
// - Per-instance tint colors with alpha
// - Per-instance visual effects (outline, glow, flash, etc.)

struct Globals {
    view_proj: mat4x4<f32>,
    time_resolution: vec4<f32>, // x=time, y=width, z=height
}

@group(0) @binding(0) var<uniform> globals: Globals;
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
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) effect_params: vec4<f32>,
    @location(3) effect_extra: vec4<f32>,
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
    
    // Apply view-projection
    out.clip_position = globals.view_proj * vec4<f32>(world_pos.xy, 0.0, 1.0);
    
    // Interpolate UVs from uv_rect
    out.uv = mix(instance.uv_rect.xy, instance.uv_rect.zw, vertex.uv);
    
    out.tint = instance.tint;
    out.effect_params = instance.effect_params;
    out.effect_extra = instance.effect_extra;
    
    return out;
}

// Base fragment shader (no effects)
@fragment
fn fs_base(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(sprite_texture, sprite_sampler, in.uv);
    return tex_color * in.tint;
}

// Effect fragment shader
@fragment
fn fs_effect(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(sprite_texture, sprite_sampler, in.uv);
    var color = tex_color * in.tint;
    
    let effect_type = i32(in.effect_params.x);
    let time = globals.time_resolution.x;
    let tex_size = vec2<f32>(
        f32(textureDimensions(sprite_texture).x),
        f32(textureDimensions(sprite_texture).y)
    );
    
    switch (effect_type) {
        case 0: { // None
            // No effect, just return base color
        }
        case 1: { // Outline
            let thickness = in.effect_params.y;
            let outline_color = in.effect_extra;
            color = apply_outline(tex_color, in.uv, tex_size, thickness, outline_color, in.tint);
        }
        case 2: { // Glow
            let radius = in.effect_params.y;
            let intensity = in.effect_params.z;
            let glow_color = in.effect_extra;
            color = apply_glow(tex_color, in.uv, tex_size, radius, intensity, glow_color, in.tint);
        }
        case 3: { // Flash
            let intensity = in.effect_params.y;
            let flash_color = in.effect_extra;
            color = apply_flash(tex_color, intensity, flash_color, in.tint);
        }
        case 4: { // ColorShift
            let amount = in.effect_params.y;
            let shift_color = in.effect_extra;
            color = apply_color_shift(tex_color, amount, shift_color, in.tint);
        }
        case 5: { // Silhouette
            let sil_color = in.effect_extra;
            color = apply_silhouette(tex_color, sil_color);
        }
        case 6: { // Pixelate
            let block_size = in.effect_params.y;
            color = apply_pixelate(in.uv, tex_size, block_size, in.tint);
        }
        case 7: { // Wave
            let amplitude = in.effect_params.y;
            let frequency = in.effect_params.z;
            let phase = in.effect_params.w;
            let speed = in.effect_extra.x;
            color = apply_wave(in.uv, amplitude, frequency, speed, time, in.tint);
        }
        case 8: { // Dissolve
            let progress = in.effect_params.y;
            let edge_color = in.effect_extra;
            color = apply_dissolve(tex_color, in.uv, progress, edge_color, in.tint);
        }
        case 9: { // Invert
            color = apply_invert(tex_color, in.tint);
        }
        case 10: { // Grayscale
            let intensity = in.effect_params.y;
            color = apply_grayscale(tex_color, intensity, in.tint);
        }
        default: {
            // Unknown effect, just return base color
        }
    }
    
    return color;
}

// =============================================================================
// Effect implementations
// =============================================================================

fn apply_outline(
    tex_color: vec4<f32>,
    uv: vec2<f32>,
    tex_size: vec2<f32>,
    thickness: f32,
    outline_color: vec4<f32>,
    tint: vec4<f32>
) -> vec4<f32> {
    // If pixel is opaque, return it
    if tex_color.a > 0.5 {
        return tex_color * tint;
    }
    
    // Check neighboring pixels for outline
    let pixel_size = 1.0 / tex_size;
    var outline_alpha = 0.0;
    
    for (var dy = -thickness; dy <= thickness; dy += 1.0) {
        for (var dx = -thickness; dx <= thickness; dx += 1.0) {
            if dx == 0.0 && dy == 0.0 { continue; }
            let neighbor_uv = uv + vec2<f32>(dx, dy) * pixel_size;
            let neighbor = textureSample(sprite_texture, sprite_sampler, neighbor_uv);
            if neighbor.a > 0.5 {
                outline_alpha = 1.0;
                break;
            }
        }
        if outline_alpha > 0.0 { break; }
    }
    
    if outline_alpha > 0.0 {
        return outline_color;
    }
    return vec4<f32>(0.0);
}

fn apply_glow(
    tex_color: vec4<f32>,
    uv: vec2<f32>,
    tex_size: vec2<f32>,
    radius: f32,
    intensity: f32,
    glow_color: vec4<f32>,
    tint: vec4<f32>
) -> vec4<f32> {
    // Sample glow from surrounding pixels
    let pixel_size = 1.0 / tex_size;
    var glow_amount = 0.0;
    let samples = 8.0;
    
    for (var i = 0.0; i < samples; i += 1.0) {
        let angle = i * 6.28318 / samples;
        let offset = vec2<f32>(cos(angle), sin(angle)) * radius * pixel_size;
        let sample = textureSample(sprite_texture, sprite_sampler, uv + offset);
        glow_amount += sample.a;
    }
    glow_amount = glow_amount / samples * intensity;
    
    // Blend glow behind sprite
    let base = tex_color * tint;
    let glow = vec4<f32>(glow_color.rgb, glow_amount * glow_color.a);
    return mix(glow, base, base.a);
}

fn apply_flash(
    tex_color: vec4<f32>,
    intensity: f32,
    flash_color: vec4<f32>,
    tint: vec4<f32>
) -> vec4<f32> {
    let base = tex_color * tint;
    // Mix toward flash color
    var result = mix(base.rgb, flash_color.rgb, intensity);
    return vec4<f32>(result, base.a);
}

fn apply_color_shift(
    tex_color: vec4<f32>,
    amount: f32,
    shift_color: vec4<f32>,
    tint: vec4<f32>
) -> vec4<f32> {
    let base = tex_color * tint;
    var result = mix(base.rgb, shift_color.rgb, amount);
    return vec4<f32>(result, base.a);
}

fn apply_silhouette(
    tex_color: vec4<f32>,
    sil_color: vec4<f32>
) -> vec4<f32> {
    // Replace color with silhouette, keep alpha
    return vec4<f32>(sil_color.rgb, tex_color.a * sil_color.a);
}

fn apply_pixelate(
    uv: vec2<f32>,
    tex_size: vec2<f32>,
    block_size: f32,
    tint: vec4<f32>
) -> vec4<f32> {
    let pixel_uv = floor(uv * tex_size / block_size) * block_size / tex_size;
    let tex_color = textureSample(sprite_texture, sprite_sampler, pixel_uv);
    return tex_color * tint;
}

fn apply_wave(
    uv: vec2<f32>,
    amplitude: f32,
    frequency: f32,
    speed: f32,
    time: f32,
    tint: vec4<f32>
) -> vec4<f32> {
    var wave_uv = uv;
    wave_uv.x += sin(uv.y * frequency + time * speed) * amplitude * 0.01;
    let tex_color = textureSample(sprite_texture, sprite_sampler, wave_uv);
    return tex_color * tint;
}

fn apply_dissolve(
    tex_color: vec4<f32>,
    uv: vec2<f32>,
    progress: f32,
    edge_color: vec4<f32>,
    tint: vec4<f32>
) -> vec4<f32> {
    // Simple noise-based dissolve
    let noise = fract(sin(dot(uv, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    
    if noise < progress {
        return vec4<f32>(0.0); // Dissolved
    }
    
    // Edge glow
    let edge_width = 0.1;
    if noise < progress + edge_width && edge_color.a > 0.0 {
        let edge_factor = (noise - progress) / edge_width;
        let base = tex_color * tint;
        return mix(edge_color, base, edge_factor);
    }
    
    return tex_color * tint;
}

fn apply_invert(
    tex_color: vec4<f32>,
    tint: vec4<f32>
) -> vec4<f32> {
    let base = tex_color * tint;
    return vec4<f32>(1.0 - base.rgb, base.a);
}

fn apply_grayscale(
    tex_color: vec4<f32>,
    intensity: f32,
    tint: vec4<f32>
) -> vec4<f32> {
    let base = tex_color * tint;
    let gray = dot(base.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let result = mix(base.rgb, vec3<f32>(gray), intensity);
    return vec4<f32>(result, base.a);
}
