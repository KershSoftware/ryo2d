// Lights as batch quads, drawn into a camera's light buffer, every light of
// a camera in one draw, each reading its shadow from the shadow atlas.
// Composed after `include/camera.wgsl` and `include/shadow_view.wgsl`.
//
// A light's instance (`lighting::light_instance`) carries:
// - the transform rows: its quad, centred on it, two radii wide
// - uv_rect: 0 for a point light, 1 for a spot; its atlas slot (-1 for
//   none: it is unshadowed); its source's size in pixels; its radius
// - tint: its linear colour (rgb) and intensity (a)
// - effect_params: falloff, cone angle and direction (degrees), cone softness
// - effect_extra: volumetric intensity, density, scattering, god rays
// - uv_bounds: its shadow colour (linear; alpha is the shadow's strength)

// Four rows a shadowed light (slot x 4 + view), each texel the nearest
// occluder's distance over the light's radius at that angle
@group(1) @binding(0) var shadow_atlas: texture_depth_2d;
// 1 where the distance compared is at most the stored one, filtered along
// a row
@group(1) @binding(1) var shadow_compare: sampler_comparison;

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
    // Slot (-1: none), source size (px), radius
    @location(6) @interpolate(flat) shadow: vec3<f32>,
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
    out.shadow = instance.uv_rect.yzw;
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

// How much of the light at `offset` from it (ground space: the light and the
// pixel are lifted alike on a storey) its occluders hold back, 0 to 1: one
// comparison against its view's row of the atlas, at the row's centre
fn shadowed(offset: vec2<f32>, shadow: vec3<f32>) -> f32 {
    let dist = length(offset);
    // No slot, or nearer than the map's near plane (0.5 px): unshadowed
    if shadow.x < 0.0 || dist < 0.5 {
        return 0.0;
    }
    let radius = shadow.z;
    let view = view_of(offset);
    let local = in_view(offset, view);
    let tangent = local.y / local.x;
    let size = vec2<f32>(textureDimensions(shadow_atlas));
    let row = u32(shadow.x) * 4u + view;
    let uv = vec2<f32>((tangent + 1.0) * 0.5, (f32(row) + 0.5) / size.y);
    // A texel's world width here: the tangent maps to the row linearly, so
    // a texel covers (2 / width) / (1 + t²) of angle, less towards the
    // view's ends. A bias of 1.5 of them keeps a surface from shadowing
    // itself, and the shadow starts at its occluder
    let texel = dist * (2.0 / size.x) / (1.0 + tangent * tangent);
    let bias = 1.5 * texel / radius;
    return 1.0 - textureSampleCompareLevel(shadow_atlas, shadow_compare, uv, dist / radius - bias);
}

// The light, held back where it is shadowed by its shadow's strength (the
// shadow colour's alpha), letting the shadow colour's share of it through
@fragment
fn fs_light(in: VertexOutput) -> @location(0) vec4<f32> {
    let light = light_at(in);
    let held = in.shadow_color.a * shadowed(in.offset, in.shadow);
    return vec4<f32>(light * (1.0 - held) + in.shadow_color.rgb * light * held, 0.0);
}
