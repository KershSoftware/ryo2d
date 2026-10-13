// Lights as batch quads, drawn into a camera's light buffer.
//
// A light's instance (`lighting::light_instance`) carries:
// - the transform rows: its quad, centred on it, two radii wide
// - uv_rect.x: 0 for a point light, 1 for a spot
// - tint: its linear colour (rgb) and intensity (a)
// - effect_params: falloff, cone angle and direction (degrees), cone softness
// - effect_extra: volumetric intensity, density, scattering, god rays
// - uv_bounds: its shadow colour (linear; alpha is the shadow's strength)

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
    @builtin(position) position: vec4<f32>,
    // From the light's centre, in world units
    @location(0) offset: vec2<f32>,
    // Colour (rgb) and intensity (a)
    @location(1) @interpolate(flat) color: vec4<f32>,
    // Radius, type, falloff
    @location(2) @interpolate(flat) shape: vec3<f32>,
    // Cone angle, direction, softness
    @location(3) @interpolate(flat) spot: vec3<f32>,
    // Intensity, density, scattering, god rays
    @location(4) @interpolate(flat) volumetric: vec4<f32>,
    @location(5) @interpolate(flat) shadow_color: vec4<f32>,
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    let transform = mat3x3<f32>(
        instance.transform0.xyz,
        instance.transform1.xyz,
        instance.transform2.xyz,
    );
    let world = (transform * vec3<f32>(vertex.position, 1.0)).xy;
    var out: VertexOutput;
    out.position = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
    out.offset = world - instance.transform2.xy;
    out.color = instance.tint;
    out.shape = vec3<f32>(instance.transform0.x * 0.5, instance.uv_rect.x, instance.effect_params.x);
    out.spot = instance.effect_params.yzw;
    out.volumetric = instance.effect_extra;
    out.shadow_color = instance.uv_bounds;
    return out;
}

// The light reaching this fragment: point or spot falloff, a volumetric
// beam for spots and a faint glow for points
fn light_at(in: VertexOutput) -> vec3<f32> {
    let radius = in.shape.x;
    let dist = length(in.offset);
    if dist > radius {
        return vec3<f32>(0.0);
    }
    let normalized_dist = dist / radius;
    let attenuation = pow(1.0 - normalized_dist, in.shape.z);
    let vol_intensity = in.volumetric.x;
    let vol_density = in.volumetric.y;
    let vol_scattering = in.volumetric.z;
    if in.shape.y > 0.5 {
        let half_angle = radians(in.spot.x * 0.5);
        let direction = radians(in.spot.y);
        let light_dir = vec2<f32>(cos(direction), sin(direction));
        let to_pixel = select(in.offset / dist, light_dir, dist < 1e-6);
        let pixel_angle = acos(clamp(dot(light_dir, to_pixel), -1.0, 1.0));
        let inner = half_angle * (1.0 - in.spot.z);
        let cone = 1.0 - smoothstep(inner, half_angle, pixel_angle);
        var color = in.color.rgb * attenuation * cone * in.color.a;
        if vol_intensity > 0.001 {
            // A visible beam in dusty air, with a soft scatter past the cone
            let beam_falloff = 1.0 - pow(normalized_dist, 2.0 + vol_density);
            let beam_core = vol_intensity * vol_density * cone * beam_falloff;
            let scatter_cone = 1.0 - smoothstep(half_angle * 0.7, half_angle * 1.2, pixel_angle);
            let edge_scatter = vol_scattering * scatter_cone * beam_falloff * 0.5;
            color += in.color.rgb * (beam_core + edge_scatter);
        }
        return color;
    }
    var color = in.color.rgb * attenuation * in.color.a;
    if vol_intensity > 0.001 {
        let glow_falloff = pow(1.0 - normalized_dist, 0.3 + vol_density * 0.5);
        color += in.color.rgb * vol_intensity * vol_density * glow_falloff * 0.2;
    }
    return color;
}

@fragment
fn fs_light(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(light_at(in), 0.0);
}

// A shadowed light's mask starts full inside its quad
@fragment
fn fs_mask_reset(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// The light through a coloured shadow: tinted, where the mask held it back
@fragment
fn fs_light_coloured(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(light_at(in) * in.shadow_color.rgb, 0.0);
}
