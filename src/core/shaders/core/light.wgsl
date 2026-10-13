// 2D Light rendering shader
// Renders individual lights to the light accumulation buffer

// The camera the lights are drawn through (`CameraGlobals`)
// Per-light instance data
struct LightInstance {
    // Position (x, y) and radius (z), type (w: 0=point, 1=spot)
    position_radius_type: vec4<f32>,
    // Color (RGB) and intensity (A)
    color_intensity: vec4<f32>,
    // Falloff (x), angle (y), direction (z), softness (w) - for spotlights
    params: vec4<f32>,
    // Volumetric: intensity (x), density (y), scattering (z), god_rays (w: 0 or 1)
    volumetric: vec4<f32>,
};

@group(0) @binding(1) var<storage, read> lights: array<LightInstance>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
    @location(1) light_center: vec2<f32>,
    @location(2) light_color: vec3<f32>,
    @location(3) light_params: vec4<f32>, // radius, intensity, falloff, type
    @location(4) spot_params: vec4<f32>,  // angle, direction, softness, unused
    @location(5) volumetric: vec4<f32>,   // intensity, density, scattering, god_rays
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
    out.volumetric = light.volumetric;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius = in.light_params.x;
    let intensity = in.light_params.y;
    let falloff = in.light_params.z;
    let light_type = in.light_params.w;

    // Volumetric settings
    let vol_intensity = in.volumetric.x;
    let vol_density = in.volumetric.y;
    let vol_scattering = in.volumetric.z;

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

    // Track cone factor for volumetric beam
    var cone_factor = 1.0;
    var is_spotlight = false;

    // Apply spotlight cone if this is a spotlight
    if light_type > 0.5 {
        is_spotlight = true;
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
        cone_factor = 1.0 - smoothstep(inner_angle, angle_rad, pixel_angle);

        attenuation *= cone_factor;
    }

    // Calculate base light color
    var final_color = in.light_color * attenuation * intensity;

    // Add volumetric beam effect for spotlights
    if is_spotlight && vol_intensity > 0.001 {
        // Volumetric beam: visible light scattering in dusty air
        // Creates a visible "cone of light" like a flashlight in fog

        // Beam visibility - more uniform along the beam length
        // Use a gentler falloff so beam extends throughout the cone
        let beam_falloff = 1.0 - pow(normalized_dist, 2.0 + vol_density);

        // Beam core: visible inside the cone, stronger in the center
        // Higher density = thicker/more visible beam
        let beam_core = vol_intensity * vol_density * cone_factor * beam_falloff;

        // Edge scatter: creates soft glow at beam edges
        // Extends slightly beyond the hard cone edge
        let angle_rad = radians(in.spot_params.x * 0.5);
        let direction_rad = radians(in.spot_params.y);
        let light_dir = vec2<f32>(cos(direction_rad), sin(direction_rad));
        let to_pixel = normalize(diff);
        let dot_val = dot(light_dir, to_pixel);
        let pixel_angle = acos(clamp(dot_val, -1.0, 1.0));

        // Soft cone factor for scatter (extends 20% beyond main cone)
        let scatter_cone = 1.0 - smoothstep(angle_rad * 0.7, angle_rad * 1.2, pixel_angle);
        let edge_scatter = vol_scattering * scatter_cone * beam_falloff * 0.5;

        // Combine beam effects with higher visibility
        let volumetric_contribution = in.light_color * (beam_core + edge_scatter);

        final_color += volumetric_contribution;
    }

    // Add subtle volumetric glow for point lights too
    if !is_spotlight && vol_intensity > 0.001 {
        // Point light volumetric: subtle atmospheric glow
        let glow_falloff = pow(1.0 - normalized_dist, 0.3 + vol_density * 0.5);
        let glow = in.light_color * vol_intensity * vol_density * glow_falloff * 0.2;
        final_color += glow;
    }

    return vec4<f32>(final_color, 1.0);
}
