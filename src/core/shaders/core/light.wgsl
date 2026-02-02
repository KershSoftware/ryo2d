// 2D Light rendering shader
// Renders individual lights to the light accumulation buffer

struct LightGlobals {
    view_proj: mat4x4<f32>,     // Camera projection matrix
    resolution: vec2<f32>,      // Screen resolution
    time: f32,                  // Current time
    _padding: f32,
};

// Per-light instance data
struct LightInstance {
    // Position (x, y) and radius (z), type (w: 0=point, 1=spot)
    position_radius_type: vec4<f32>,
    // Color (RGB) and intensity (A)
    color_intensity: vec4<f32>,
    // Falloff (x), angle (y), direction (z), softness (w) - for spotlights
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: LightGlobals;
@group(0) @binding(1) var<storage, read> lights: array<LightInstance>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
    @location(1) light_center: vec2<f32>,
    @location(2) light_color: vec3<f32>,
    @location(3) light_params: vec4<f32>, // radius, intensity, falloff, type
    @location(4) spot_params: vec4<f32>,  // angle, direction, softness, unused
};

// Generate a quad for each light instance
@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32
) -> VertexOutput {
    let light = lights[instance_index];

    let center = light.position_radius_type.xy;
    let radius = light.position_radius_type.z;
    let light_type = light.position_radius_type.w;

    // Generate quad vertices (2 triangles = 6 vertices)
    var quad_positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );

    let local_pos = quad_positions[vertex_index] * radius;
    let world_pos = center + local_pos;

    // Transform to clip space
    let clip_pos = globals.view_proj * vec4<f32>(world_pos, 0.0, 1.0);

    var out: VertexOutput;
    out.position = clip_pos;
    out.world_pos = world_pos;
    out.light_center = center;
    out.light_color = light.color_intensity.rgb;
    out.light_params = vec4<f32>(radius, light.color_intensity.a, light.params.x, light_type);
    out.spot_params = vec4<f32>(light.params.y, light.params.z, light.params.w, 0.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius = in.light_params.x;
    let intensity = in.light_params.y;
    let falloff = in.light_params.z;
    let light_type = in.light_params.w;

    // Calculate distance from light center
    let diff = in.world_pos - in.light_center;
    let dist = length(diff);

    // Early out if outside light radius
    if dist > radius {
        discard;
    }

    // Calculate base attenuation
    let normalized_dist = dist / radius;
    var attenuation = pow(1.0 - normalized_dist, falloff);

    // Apply spotlight cone if this is a spotlight
    if light_type > 0.5 {
        let angle_rad = radians(in.spot_params.x * 0.5); // Half-angle
        let direction_rad = radians(in.spot_params.y);
        let softness = in.spot_params.z;

        // Direction vector from light center
        let light_dir = vec2<f32>(cos(direction_rad), sin(direction_rad));

        // Angle to current pixel
        let to_pixel = normalize(diff);
        let dot_val = dot(light_dir, to_pixel);
        let pixel_angle = acos(clamp(dot_val, -1.0, 1.0));

        // Cone attenuation with soft edge
        let inner_angle = angle_rad * (1.0 - softness);
        let cone_factor = 1.0 - smoothstep(inner_angle, angle_rad, pixel_angle);

        attenuation *= cone_factor;
    }

    // Apply intensity
    let final_color = in.light_color * attenuation * intensity;

    return vec4<f32>(final_color, 1.0);
}
