// 2D Shadow rendering shader
// Renders shadow volume geometry to subtract from the light accumulation buffer
// Supports soft shadows with configurable edge softness and colored shadows

struct ShadowGlobals {
    view_proj: mat4x4<f32>,     // Camera projection matrix
    resolution: vec2<f32>,      // Screen resolution
    time: f32,                  // Current time
    _padding: f32,
};

struct ShadowVertex {
    // Position in world space
    @location(0) position: vec2<f32>,
    // Light position (for falloff)
    @location(1) light_pos: vec2<f32>,
    // Light radius
    @location(2) light_radius: f32,
    // Shadow softness (0.0 = hard, 1.0 = very soft)
    @location(3) softness: f32,
    // Shadow color (packed RGBA as u32)
    @location(4) shadow_color: u32,
};

@group(0) @binding(0) var<uniform> globals: ShadowGlobals;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
    @location(1) light_pos: vec2<f32>,
    @location(2) light_radius: f32,
    @location(3) softness: f32,
    @location(4) shadow_color: vec4<f32>,
};

// Unpack RGBA from u32 (RGBA order, little-endian)
fn unpack_color(packed: u32) -> vec4<f32> {
    let r = f32(packed & 0xFFu) / 255.0;
    let g = f32((packed >> 8u) & 0xFFu) / 255.0;
    let b = f32((packed >> 16u) & 0xFFu) / 255.0;
    let a = f32((packed >> 24u) & 0xFFu) / 255.0;
    return vec4<f32>(r, g, b, a);
}

@vertex
fn vs_main(in: ShadowVertex) -> VertexOutput {
    var out: VertexOutput;

    // Transform to clip space
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 0.0, 1.0);
    out.world_pos = in.position;
    out.light_pos = in.light_pos;
    out.light_radius = in.light_radius;
    out.softness = in.softness;
    out.shadow_color = unpack_color(in.shadow_color);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate distance from this fragment to the light
    let dx = in.world_pos.x - in.light_pos.x;
    let dy = in.world_pos.y - in.light_pos.y;
    let dist = sqrt(dx * dx + dy * dy);

    // Shadow intensity based on distance from light
    // Shadows are darker closer to the light (where there would be more light)
    // and fade out toward the edge of the light radius
    let normalized_dist = clamp(dist / in.light_radius, 0.0, 1.0);

    // Quadratic falloff matching the light falloff
    // This ensures shadows properly subtract the light contribution
    var shadow_intensity = pow(1.0 - normalized_dist, 2.0);

    // Apply softness - soft shadows have gradual edges
    // Softness affects how quickly the shadow fades at the edges of the shadow volume
    // We use the fragment's position relative to the shadow edge
    if in.softness > 0.0 {
        // Calculate edge softness based on distance from light center
        // Soft shadows fade more gradually based on the softness parameter
        // Higher softness = more gradual fade
        let softness_factor = in.softness * 0.5; // Scale softness effect

        // Apply smooth gradient to shadow edges
        // The shadow intensity is reduced based on distance, creating softer edges
        let edge_fade = smoothstep(0.0, softness_factor, normalized_dist);
        shadow_intensity = shadow_intensity * (1.0 - edge_fade * in.softness);
    }

    // Apply shadow color
    // With reverse-subtract blending: dst = dst - src
    // To get a colored shadow (e.g., red tint), we subtract the complement of the color
    // shadow_color = (0,0,0) black -> subtract equally from all channels -> black shadow
    // shadow_color = (1,0,0) red -> subtract less red, more G/B -> reddish shadow
    let color_mask = vec3<f32>(1.0) - in.shadow_color.rgb;

    // Output shadow color
    // Alpha from shadow_color can be used to modulate overall shadow strength
    // Multiply by 10.0 to ensure full light subtraction regardless of light intensity
    let alpha_mod = in.shadow_color.a;
    return vec4<f32>(shadow_intensity * color_mask * alpha_mod * 10.0, 1.0);
}
