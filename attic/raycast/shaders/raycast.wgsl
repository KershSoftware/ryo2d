// DDA Raycasting fragment shader
//
// Performs per-pixel Wolfenstein-style raycasting on a fullscreen quad.
// Each column casts one ray through the 2D tile map using the DDA algorithm,
// then renders wall strips, textured floors, and ceilings.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const FLAG_DOOR: u32        = 1u;
const FLAG_THIN_WALL: u32   = 2u;
const FLAG_PORTAL: u32      = 4u;      // Not yet implemented
const FLAG_TRANSPARENT: u32 = 8u;      // Not yet implemented

const MAX_STEPS: u32 = 256u;

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
    pitch: f32,
    _pad: f32,
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

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
}

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @location(1) depth: vec4<f32>,
}
// ---------------------------------------------------------------------------
// Bindings (all group 0)
// ---------------------------------------------------------------------------

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> map_data: array<u32>;
@group(0) @binding(2) var<uniform> map_info: MapInfo;
@group(0) @binding(3) var wall_textures: texture_2d_array<f32>;
@group(0) @binding(4) var floor_textures: texture_2d_array<f32>;
@group(0) @binding(5) var tex_sampler: sampler;
@group(0) @binding(6) var<storage, read> lights: array<LightInstance>;
@group(0) @binding(7) var<uniform> light_info: LightCount;
@group(0) @binding(8) var lightmap: texture_2d<f32>;
@group(0) @binding(9) var lightmap_sampler: sampler;

// ---------------------------------------------------------------------------
// Map cell unpacking
// ---------------------------------------------------------------------------
// Rust counterpart: MapCell::pack() in raycast/world.rs
//
// Rust pack layout (MapCell::pack):
//   word0: wall_tex(31..24) | floor_tex(23..16) | ceil_tex(15..8) | flags(7..0)
//   word1: floor_height_f16(31..16) | ceiling_height_f16(15..0)
//   word2: door_state_f16(31..16)   | light_level_f16(15..0)
//
// unpack2x16float maps lower 16 bits to .x and upper 16 bits to .y.

fn read_cell(x: i32, y: i32) -> vec3<u32> {
    let idx = u32(y) * map_info.dimensions.x + u32(x);
    let base = idx * 3u;
    return vec3<u32>(map_data[base], map_data[base + 1u], map_data[base + 2u]);
}

fn get_wall_texture(packed: vec3<u32>) -> u32 {
    return (packed.x >> 24u) & 0xFFu;
}

fn get_floor_texture(packed: vec3<u32>) -> u32 {
    return (packed.x >> 16u) & 0xFFu;
}

fn get_ceiling_texture(packed: vec3<u32>) -> u32 {
    return (packed.x >> 8u) & 0xFFu;
}

fn get_flags(packed: vec3<u32>) -> u32 {
    return packed.x & 0xFFu;
}

fn get_floor_height(packed: vec3<u32>) -> f32 {
    return unpack2x16float(packed.y).y;
}

fn get_ceiling_height(packed: vec3<u32>) -> f32 {
    return unpack2x16float(packed.y).x;
}

fn get_door_state(packed: vec3<u32>) -> f32 {
    return unpack2x16float(packed.z).y;
}

fn get_light_level(packed: vec3<u32>) -> f32 {
    return unpack2x16float(packed.z).x;
}
// ---------------------------------------------------------------------------
// Lightmap sampling
// ---------------------------------------------------------------------------

fn sample_lightmap(world_pos: vec2<f32>) -> f32 {
    let uv = world_pos / vec2<f32>(f32(map_info.dimensions.x), f32(map_info.dimensions.y));
    return textureSample(lightmap, lightmap_sampler, uv).r;
}

// ---------------------------------------------------------------------------
// Lighting helpers
// ---------------------------------------------------------------------------

// DDA shadow ray: marches FROM the light TO the surface.
// Returns true if a solid wall blocks the path.
fn is_occluded(surface_pos: vec2<f32>, light_pos: vec2<f32>) -> bool {
    // March from the light (always in open space) toward the surface
    let dir = surface_pos - light_pos;
    let total_dist = length(dir);
    if total_dist < 0.01 {
        return false;
    }
    let ray = dir / total_dist;
    let abs_ray = abs(ray);
    let safe_ray = vec2<f32>(
        select(ray.x, 1e-8, abs_ray.x < 1e-8),
        select(ray.y, 1e-8, abs_ray.y < 1e-8)
    );

    var map_pos = vec2<i32>(floor(light_pos));
    // Nudge surface toward light to resolve cell boundary ambiguity:
    // inner wall faces (e.g. x=28.0) map to interior cell, not the wall cell
    let dst_cell = vec2<i32>(floor(surface_pos + ray * -0.01));

    // Same cell — no wall between them
    if map_pos.x == dst_cell.x && map_pos.y == dst_cell.y {
        return false;
    }

    let delta_dist = abs(vec2<f32>(1.0) / safe_ray);

    var step_dir: vec2<i32>;
    var side_dist: vec2<f32>;

    if safe_ray.x < 0.0 {
        step_dir.x = -1;
        side_dist.x = (light_pos.x - f32(map_pos.x)) * delta_dist.x;
    } else {
        step_dir.x = 1;
        side_dist.x = (f32(map_pos.x) + 1.0 - light_pos.x) * delta_dist.x;
    }
    if safe_ray.y < 0.0 {
        step_dir.y = -1;
        side_dist.y = (light_pos.y - f32(map_pos.y)) * delta_dist.y;
    } else {
        step_dir.y = 1;
        side_dist.y = (f32(map_pos.y) + 1.0 - light_pos.y) * delta_dist.y;
    }

    // March from light toward surface
    for (var i: u32 = 0u; i < 64u; i = i + 1u) {
        if side_dist.x < side_dist.y {
            side_dist.x += delta_dist.x;
            map_pos.x += step_dir.x;
        } else {
            side_dist.y += delta_dist.y;
            map_pos.y += step_dir.y;
        }

        // Out of bounds
        if map_pos.x < 0 || map_pos.y < 0 ||
           u32(map_pos.x) >= map_info.dimensions.x ||
           u32(map_pos.y) >= map_info.dimensions.y {
            return true;
        }

        // Wall check BEFORE destination check — blocks outer wall faces correctly
        let cell = read_cell(map_pos.x, map_pos.y);
        let wall_tex = get_wall_texture(cell);
        let flags = get_flags(cell);
        if wall_tex > 0u && (flags & (FLAG_DOOR | FLAG_THIN_WALL)) == 0u {
            return true;
        }

        // Reached the surface's cell — no obstruction
        if map_pos.x == dst_cell.x && map_pos.y == dst_cell.y {
            return false;
        }
    }
    return false;
}

fn apply_lights(base_color: vec3<f32>, ambient: f32, world_pos: vec2<f32>, vert_offset: f32) -> vec3<f32> {
    var light_contrib = vec3<f32>(0.0);
    for (var i: u32 = 0u; i < light_info.count; i = i + 1u) {
        let light = lights[i];
        let dist_2d = distance(world_pos, light.pos);
        // Approximate 3D distance using vertical offset from light height
        let vert_diff = vert_offset - (light.height - 0.5);
        let dist = sqrt(dist_2d * dist_2d + vert_diff * vert_diff);
        if dist < light.radius {
            // Shadow ray: check if a wall blocks the path from light to surface
            if is_occluded(world_pos, light.pos) {
                continue;
            }
            let norm = dist / light.radius;
            // Configurable falloff: pow() exponent controls attenuation curve
            let atten = light.intensity * pow(max(1.0 - norm, 0.0), light.falloff);
            light_contrib += light.color * atten;
        }
    }
    // Reconstruct surface color from ambient-darkened base, then add dynamic light
    // This lets lights properly illuminate dark areas (adds to ambient, not scales it)
    let surface = base_color / max(ambient, 0.05);
    return min(base_color + surface * light_contrib, vec3<f32>(1.0));
}

// ---------------------------------------------------------------------------
// Fog helper
// ---------------------------------------------------------------------------

fn apply_fog(color: vec3<f32>, dist: f32) -> vec3<f32> {
    let fog_factor = 1.0 - exp(-dist * map_info.fog_density);
    return mix(color, map_info.fog_color, fog_factor);
}

// ---------------------------------------------------------------------------
// Vertex shader - fullscreen quad from vertex index
// ---------------------------------------------------------------------------

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, -1.0)
    );

    var tex_coords = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.tex_coord = tex_coords[vertex_index];
    return out;
}
// ---------------------------------------------------------------------------
// Fragment shader - per-pixel DDA raycasting
// ---------------------------------------------------------------------------

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    // Query actual texture dimensions instead of assuming a fixed size
    let wall_tex_dims = textureDimensions(wall_textures);
    let floor_tex_dims = textureDimensions(floor_textures);

    let screen_w = camera.screen_size.x;
    let screen_h = camera.screen_size.y;

    // Pixel coordinates (0..screen_w, 0..screen_h)
    let screen_x = in.tex_coord.x * screen_w;
    let screen_y = in.tex_coord.y * screen_h;
    let horizon = screen_h * 0.5 + camera.pitch * screen_h;

    // Camera-space X coordinate: -1 (left) to +1 (right)
    let camera_x = 2.0 * screen_x / screen_w - 1.0;

    // Ray direction for this column
    let ray_dir = camera.dir + camera.plane * camera_x;

    // Avoid division by zero with safe minimum
    let abs_ray = abs(ray_dir);
    let safe_ray = vec2<f32>(
        select(ray_dir.x, 1e-8, abs_ray.x < 1e-8),
        select(ray_dir.y, 1e-8, abs_ray.y < 1e-8)
    );

    // DDA setup
    var map_pos = vec2<i32>(floor(camera.pos));
    let delta_dist = abs(vec2<f32>(1.0) / safe_ray);

    var step_dir: vec2<i32>;
    var side_dist: vec2<f32>;

    if safe_ray.x < 0.0 {
        step_dir.x = -1;
        side_dist.x = (camera.pos.x - f32(map_pos.x)) * delta_dist.x;
    } else {
        step_dir.x = 1;
        side_dist.x = (f32(map_pos.x) + 1.0 - camera.pos.x) * delta_dist.x;
    }

    if safe_ray.y < 0.0 {
        step_dir.y = -1;
        side_dist.y = (camera.pos.y - f32(map_pos.y)) * delta_dist.y;
    } else {
        step_dir.y = 1;
        side_dist.y = (f32(map_pos.y) + 1.0 - camera.pos.y) * delta_dist.y;
    }

    // DDA traversal loop
    var hit = false;
    var side: u32 = 0u; // 0 = X-side hit, 1 = Y-side hit
    var cell = vec3<u32>(0u, 0u, 0u);

    for (var i: u32 = 0u; i < MAX_STEPS; i = i + 1u) {
        // Advance to next cell boundary along the shorter axis
        if side_dist.x < side_dist.y {
            side_dist.x = side_dist.x + delta_dist.x;
            map_pos.x = map_pos.x + step_dir.x;
            side = 0u;
        } else {
            side_dist.y = side_dist.y + delta_dist.y;
            map_pos.y = map_pos.y + step_dir.y;
            side = 1u;
        }

        // Out of map bounds
        if map_pos.x < 0 || map_pos.y < 0 ||
           u32(map_pos.x) >= map_info.dimensions.x ||
           u32(map_pos.y) >= map_info.dimensions.y {
            break;
        }

        cell = read_cell(map_pos.x, map_pos.y);
        let wall_tex = get_wall_texture(cell);
        let flags = get_flags(cell);

        // Door handling: the ray may pass through the open portion
        if (flags & FLAG_DOOR) != 0u && wall_tex > 0u {
            let door_open = get_door_state(cell);

            // Compute where along the wall face the ray hits (0..1)
            var hit_offset: f32;
            if side == 0u {
                hit_offset = camera.pos.y + (side_dist.x - delta_dist.x) * safe_ray.y;
            } else {
                hit_offset = camera.pos.x + (side_dist.y - delta_dist.y) * safe_ray.x;
            }
            hit_offset = hit_offset - floor(hit_offset);

            // If the hit falls in the open portion, the ray passes through
            if hit_offset < door_open {
                continue;
            }
            hit = true;
            break;
        }

        // Solid wall
        if wall_tex > 0u {
            hit = true;
            break;
        }
    }
    // -------------------------------------------------------------------
    // No wall hit - sky/ground fallback
    // -------------------------------------------------------------------
    if !hit {
        if screen_y < horizon {
            return FragmentOutput(vec4<f32>(0.53, 0.81, 0.92, 1.0), vec4<f32>(map_info.max_distance, 0.0, 0.0, 1.0)); // sky blue
        } else {
            return FragmentOutput(vec4<f32>(0.2, 0.2, 0.2, 1.0), vec4<f32>(map_info.max_distance, 0.0, 0.0, 1.0));    // dark gray ground
        }
    }

    // -------------------------------------------------------------------
    // Perpendicular wall distance (removes fisheye)
    // -------------------------------------------------------------------
    var perp_wall_dist: f32;
    if side == 0u {
        perp_wall_dist = side_dist.x - delta_dist.x;
    } else {
        perp_wall_dist = side_dist.y - delta_dist.y;
    }
    perp_wall_dist = max(perp_wall_dist, 0.001);

    // World position of the hit point (for lighting calculations)
    let world_hit = camera.pos + safe_ray * perp_wall_dist;

    // Cell properties
    let floor_h = get_floor_height(cell);
    let ceil_h = get_ceiling_height(cell);
    let cell_light = sample_lightmap(world_hit);
    let wall_tex_id = get_wall_texture(cell);

    // Wall strip vertical bounds on screen
    let line_height = screen_h / perp_wall_dist;
    let wall_top = -line_height * (ceil_h - camera.height) + horizon;
    let wall_bottom = line_height * (camera.height - floor_h) + horizon;

    // -------------------------------------------------------------------
    // Classify pixel: wall, floor, or ceiling
    // -------------------------------------------------------------------
    var final_color: vec3<f32>;
    var depth_value: f32 = perp_wall_dist;

    if screen_y >= wall_top && screen_y <= wall_bottom {
        // ---- WALL ----

        // wallX: fractional position along the wall surface
        var wall_x: f32;
        if side == 0u {
            wall_x = camera.pos.y + perp_wall_dist * safe_ray.y;
        } else {
            wall_x = camera.pos.x + perp_wall_dist * safe_ray.x;
        }
        wall_x = wall_x - floor(wall_x);

        // Horizontal texture coordinate
        var tex_x = i32(wall_x * f32(wall_tex_dims.x));
        tex_x = clamp(tex_x, 0, i32(wall_tex_dims.x) - 1);

        // Vertical texture coordinate (interpolate within wall strip)
        let wall_frac = (screen_y - wall_top) / (wall_bottom - wall_top);
        var tex_y = i32(wall_frac * f32(wall_tex_dims.y));
        tex_y = clamp(tex_y, 0, i32(wall_tex_dims.y) - 1);

        // Sample wall texture (IDs are 1-based, array indices are 0-based)
        let tex_color = textureLoad(
            wall_textures,
            vec2<i32>(tex_x, tex_y),
            i32(wall_tex_id - 1u),
            0
        );

        var wall_color = tex_color.rgb;

        // Side shading: Y-side walls are darker
        if side == 1u {
            wall_color = wall_color * 0.75;
        }

        // Per-tile light level
        wall_color = wall_color * cell_light;

        // Global ambient: ensure surfaces are never darker than ambient minimum
        let wall_global_ambient = map_info.ambient_color * map_info.ambient_intensity;
        wall_color = max(wall_color, tex_color.rgb * wall_global_ambient);

        // Dynamic point lights (with vertical attenuation)
        let vert_world = mix(ceil_h, floor_h, wall_frac);
        wall_color = apply_lights(wall_color, cell_light, world_hit, vert_world - 0.5);

        // Light diminishing: surfaces darken with distance from camera
        if map_info.diminish_factor > 0.0 {
            let diminish = 1.0 / (1.0 + perp_wall_dist * map_info.diminish_factor);
            wall_color = wall_color * diminish;
        }

        // Distance fog
        final_color = apply_fog(wall_color, perp_wall_dist);
    } else if screen_y > wall_bottom {
        // ---- FLOOR ----

        // Distance from camera plane to this floor pixel
        let floor_dist = (camera.height - floor_h) * screen_h
                         / (screen_y - horizon);
        depth_value = floor_dist;

        // World position of this floor pixel
        let floor_world = camera.pos + safe_ray * floor_dist;

        // Look up the map cell at the floor position
        let floor_map_x = i32(floor(floor_world.x));
        let floor_map_y = i32(floor(floor_world.y));

        if floor_map_x >= 0 && floor_map_y >= 0 &&
           u32(floor_map_x) < map_info.dimensions.x &&
           u32(floor_map_y) < map_info.dimensions.y {

            let floor_cell = read_cell(floor_map_x, floor_map_y);
            let floor_tex_id = get_floor_texture(floor_cell);
            let floor_light = sample_lightmap(floor_world);

            // Texture coords from world-space fractional part
            let frac_f = floor_world - floor(floor_world);
            let ftx = clamp(i32(frac_f.x * f32(floor_tex_dims.x)), 0, i32(floor_tex_dims.x) - 1);
            let fty = clamp(i32(frac_f.y * f32(floor_tex_dims.y)), 0, i32(floor_tex_dims.y) - 1);

            var floor_color: vec3<f32>;
            var floor_tex_color: vec3<f32>;
            if floor_tex_id > 0u {
                let tc = textureLoad(
                    floor_textures,
                    vec2<i32>(ftx, fty),
                    i32(floor_tex_id - 1u),
                    0
                );
                floor_color = tc.rgb;
                floor_tex_color = tc.rgb;
            } else {
                floor_color = vec3<f32>(0.3, 0.3, 0.3);
                floor_tex_color = floor_color;
            }

            floor_color = floor_color * floor_light;

            // Global ambient: ensure surfaces are never darker than ambient minimum
            let floor_global_ambient = map_info.ambient_color * map_info.ambient_intensity;
            floor_color = max(floor_color, floor_tex_color * floor_global_ambient);

            floor_color = apply_lights(floor_color, floor_light, floor_world, -0.5);

            // Light diminishing: surfaces darken with distance from camera
            if map_info.diminish_factor > 0.0 {
                let diminish = 1.0 / (1.0 + floor_dist * map_info.diminish_factor);
                floor_color = floor_color * diminish;
            }

            final_color = apply_fog(floor_color, floor_dist);
        } else {
            final_color = vec3<f32>(0.2, 0.2, 0.2);
        }
    } else {
        // ---- CEILING ----

        let denom = horizon - screen_y;
        if abs(denom) < 0.001 {
            final_color = map_info.fog_color;
            depth_value = map_info.max_distance;
        } else {
            // Distance from camera plane to this ceiling pixel
            let ceil_dist = (ceil_h - camera.height) * screen_h / denom;
            depth_value = ceil_dist;

            // World position of this ceiling pixel
            let ceil_world = camera.pos + safe_ray * ceil_dist;

            let ceil_map_x = i32(floor(ceil_world.x));
            let ceil_map_y = i32(floor(ceil_world.y));

            if ceil_map_x >= 0 && ceil_map_y >= 0 &&
               u32(ceil_map_x) < map_info.dimensions.x &&
               u32(ceil_map_y) < map_info.dimensions.y {

                let ceil_cell = read_cell(ceil_map_x, ceil_map_y);
                let ceil_tex_id = get_ceiling_texture(ceil_cell);
                let ceil_light = sample_lightmap(ceil_world);

                let frac_c = ceil_world - floor(ceil_world);
                let ctx_val = clamp(i32(frac_c.x * f32(floor_tex_dims.x)), 0, i32(floor_tex_dims.x) - 1);
                let cty_val = clamp(i32(frac_c.y * f32(floor_tex_dims.y)), 0, i32(floor_tex_dims.y) - 1);

                var ceil_color: vec3<f32>;
                var ceil_tex_color: vec3<f32>;
                if ceil_tex_id > 0u {
                    let tc = textureLoad(
                        floor_textures,
                        vec2<i32>(ctx_val, cty_val),
                        i32(ceil_tex_id - 1u),
                        0
                    );
                    ceil_color = tc.rgb;
                    ceil_tex_color = tc.rgb;
                } else {
                    ceil_color = vec3<f32>(0.4, 0.4, 0.4);
                    ceil_tex_color = ceil_color;
                }

                ceil_color = ceil_color * ceil_light;

                // Global ambient: ensure surfaces are never darker than ambient minimum
                let ceil_global_ambient = map_info.ambient_color * map_info.ambient_intensity;
                ceil_color = max(ceil_color, ceil_tex_color * ceil_global_ambient);

                ceil_color = apply_lights(ceil_color, ceil_light, ceil_world, 0.5);

                // Light diminishing: surfaces darken with distance from camera
                if map_info.diminish_factor > 0.0 {
                    let diminish = 1.0 / (1.0 + ceil_dist * map_info.diminish_factor);
                    ceil_color = ceil_color * diminish;
                }

                final_color = apply_fog(ceil_color, ceil_dist);
            } else {
                // Sky blue fallback for out-of-bounds ceiling
                final_color = vec3<f32>(0.53, 0.81, 0.92);
            }
        }
    }

    return FragmentOutput(vec4<f32>(final_color, 1.0), vec4<f32>(depth_value, 0.0, 0.0, 1.0));
}
