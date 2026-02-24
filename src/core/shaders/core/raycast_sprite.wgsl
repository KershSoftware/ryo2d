// Billboard sprite shader for raycasted world
//
// Renders camera-facing quads via instancing. Each instance is a sprite
// positioned in 2D world space that is projected onto a screen-space quad.
// Depth testing against the wall depth texture ensures correct occlusion.

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

struct Camera {
    pos: vec2<f32>,
    dir: vec2<f32>,
    plane: vec2<f32>,
    screen_size: vec2<f32>,
    height: f32,
    time: f32,
    _pad: vec2<f32>,
}

struct SpriteData {
    pos: vec2<f32>,
    scale: vec2<f32>,
    texture_id: u32,
    v_offset: f32,
    _pad: vec2<f32>,
}

struct LightInstance {
    pos: vec2<f32>,
    radius: f32,
    intensity: f32,
    color: vec3<f32>,
    height: f32,
    falloff: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

struct LightCount {
    count: u32,
}

struct MapInfo {
    dimensions: vec2<u32>,
    max_distance: f32,
    fog_density: f32,
    fog_color: vec3<f32>,
    diminish_factor: f32,
    ambient_color: vec3<f32>,
    ambient_intensity: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) sprite_depth: f32,
    @location(2) @interpolate(flat) texture_id: u32,
    @location(3) sprite_world_pos: vec2<f32>,
}

// ---------------------------------------------------------------------------
// Bindings (group 0)
// ---------------------------------------------------------------------------

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> sprites: array<SpriteData>;
@group(0) @binding(2) var sprite_textures: texture_2d_array<f32>;
@group(0) @binding(3) var tex_sampler: sampler;
@group(0) @binding(4) var depth_texture: texture_2d<f32>;
@group(0) @binding(5) var<storage, read> lights: array<LightInstance>;
@group(0) @binding(6) var<uniform> light_info: LightCount;
@group(0) @binding(7) var lightmap: texture_2d<f32>;
@group(0) @binding(8) var lightmap_sampler: sampler;
@group(0) @binding(9) var<uniform> map_info: MapInfo;

// ---------------------------------------------------------------------------
// Vertex shader
// ---------------------------------------------------------------------------

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let sprite = sprites[instance_index];

    let screen_w = camera.screen_size.x;
    let screen_h = camera.screen_size.y;

    // Transform sprite position into camera space
    let dx = sprite.pos.x - camera.pos.x;
    let dy = sprite.pos.y - camera.pos.y;

    let inv_det = 1.0 / (camera.plane.x * camera.dir.y - camera.dir.x * camera.plane.y);
    let transform_x = inv_det * (camera.dir.y * dx - camera.dir.x * dy);
    let transform_y = inv_det * (-camera.plane.y * dx + camera.plane.x * dy); // depth

    // Clip sprites behind the camera
    if transform_y <= 0.0 {
        var out: VertexOutput;
        out.position = vec4<f32>(0.0, 0.0, 2.0, 1.0); // behind clip plane
        out.tex_coord = vec2<f32>(0.0, 0.0);
        out.sprite_depth = 0.0;
        out.texture_id = 0u;
        out.sprite_world_pos = vec2<f32>(0.0, 0.0);
        return out;
    }

    // Screen-space quad dimensions
    let sprite_screen_x = screen_w * 0.5 * (1.0 + transform_x / transform_y);
    let sprite_height = abs(screen_h / transform_y) * sprite.scale.y;
    let sprite_width = abs(screen_h / transform_y) * sprite.scale.x;

    // Vertical center accounting for camera height and sprite vertical offset
    let v_center = screen_h * 0.5 + (camera.height - 0.5 - sprite.v_offset) * screen_h / transform_y;

    // Quad bounds in pixel space
    let left = sprite_screen_x - sprite_width * 0.5;
    let right = sprite_screen_x + sprite_width * 0.5;
    let top = v_center - sprite_height * 0.5;
    let bottom = v_center + sprite_height * 0.5;

    // 6 vertices forming two triangles (quad)
    //   0--1
    //   | /|
    //   |/ |
    //   2--3
    // Triangle 1: 0, 2, 1  Triangle 2: 1, 2, 3
    var pixel_pos: vec2<f32>;
    var uv: vec2<f32>;

    switch vertex_index % 6u {
        case 0u: { // top-left
            pixel_pos = vec2<f32>(left, top);
            uv = vec2<f32>(0.0, 0.0);
        }
        case 1u: { // bottom-left
            pixel_pos = vec2<f32>(left, bottom);
            uv = vec2<f32>(0.0, 1.0);
        }
        case 2u: { // top-right
            pixel_pos = vec2<f32>(right, top);
            uv = vec2<f32>(1.0, 0.0);
        }
        case 3u: { // top-right
            pixel_pos = vec2<f32>(right, top);
            uv = vec2<f32>(1.0, 0.0);
        }
        case 4u: { // bottom-left
            pixel_pos = vec2<f32>(left, bottom);
            uv = vec2<f32>(0.0, 1.0);
        }
        case 5u: { // bottom-right
            pixel_pos = vec2<f32>(right, bottom);
            uv = vec2<f32>(1.0, 1.0);
        }
        default: {
            pixel_pos = vec2<f32>(0.0, 0.0);
            uv = vec2<f32>(0.0, 0.0);
        }
    }

    // Convert pixel coordinates to NDC (-1..1)
    let ndc_x = (pixel_pos.x / screen_w) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_pos.y / screen_h) * 2.0; // flip Y

    var out: VertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.tex_coord = uv;
    out.sprite_depth = transform_y;
    out.texture_id = sprite.texture_id;
    out.sprite_world_pos = sprite.pos;
    return out;
}

// ---------------------------------------------------------------------------
// Fragment shader
// ---------------------------------------------------------------------------

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Texture dimensions for texel lookup
    let tex_dims = textureDimensions(sprite_textures);
    let tex_x = clamp(i32(in.tex_coord.x * f32(tex_dims.x)), 0, i32(tex_dims.x) - 1);
    let tex_y = clamp(i32(in.tex_coord.y * f32(tex_dims.y)), 0, i32(tex_dims.y) - 1);

    // Sample sprite texture
    let color = textureLoad(
        sprite_textures,
        vec2<i32>(tex_x, tex_y),
        i32(in.texture_id),
        0
    );

    // Alpha test
    if color.a < 0.5 {
        discard;
    }

    // Depth test against wall depth buffer
    let screen_pos = vec2<i32>(floor(in.position.xy));
    let wall_depth = textureLoad(depth_texture, screen_pos, 0).r;

    if in.sprite_depth > wall_depth && wall_depth > 0.0 {
        discard;
    }

    // Sample lightmap for ambient
    let lm_uv = in.sprite_world_pos / vec2<f32>(f32(map_info.dimensions.x), f32(map_info.dimensions.y));
    let ambient = textureSample(lightmap, lightmap_sampler, lm_uv).r;

    // Apply ambient to sprite
    var lit_color = color.rgb * ambient;

    // Global ambient floor
    let global_ambient = map_info.ambient_color * map_info.ambient_intensity;
    lit_color = max(lit_color, color.rgb * global_ambient);

    // Dynamic point lights (no shadow rays for sprites — too expensive, minimal visual benefit)
    var light_contrib = vec3<f32>(0.0);
    for (var i: u32 = 0u; i < light_info.count; i = i + 1u) {
        let light = lights[i];
        let dist = distance(in.sprite_world_pos, light.pos);
        if dist < light.radius {
            let norm = dist / light.radius;
            let atten = light.intensity * pow(max(1.0 - norm, 0.0), light.falloff);
            light_contrib += light.color * atten;
        }
    }
    let surface = lit_color / max(ambient, 0.05);
    lit_color = min(lit_color + surface * light_contrib, vec3<f32>(1.0));

    // Light diminishing: surfaces darken with distance from camera
    if map_info.diminish_factor > 0.0 {
        let diminish = 1.0 / (1.0 + in.sprite_depth * map_info.diminish_factor);
        lit_color = lit_color * diminish;
    }

    return vec4<f32>(lit_color, 1.0);
}
