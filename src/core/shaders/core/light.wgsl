// Lights as batch quads, drawn into a camera's light buffer, every light of
// a camera in one draw, each reading its shadow from the shadow atlas.
// Composed after `include/camera.wgsl` and `include/shadow_view.wgsl`.
//
// A light's instance (`lighting::light_instance`) carries:
// - the transform rows: its quad, centred on it, two radii wide
// - uv_rect: 0 for a point light, 1 for a spot; its atlas slot (-1 for
//   none: it is unshadowed); its source's radius in pixels (0: hard
//   shadows); its radius
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

struct ShadowFilter {
    // x: the soft filter's look-ups (`ShadowQuality::taps`); the blocker
    // search takes half as many
    taps: vec4<u32>,
}
@group(1) @binding(2) var<uniform> shadow_filter: ShadowFilter;

// The widest arc, in radians, a soft shadow's blocker search or filter
// reads round a pixel's angle, so a huge source cannot read a whole view.
// MAX_ARC / 2 must stay below the planner's spot cone margin (0.2 rad,
// `SPOT_CONE_MARGIN` in `lighting/plan.rs`, which asserts it), or a soft
// spot's look-ups near its cone would miss edges the plan left out
const MAX_ARC: f32 = 0.35;

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

// Where the ray from a light along `dir` (any length) meets the atlas: the
// row of the view it lies in (slot x 4 + view), its place along the row in
// texels, and the bias (in stored units) for a surface `dist` away on it
struct AtlasPoint {
    texel: f32,
    row: u32,
    bias: f32,
}

fn atlas_point(dir: vec2<f32>, dist: f32, slot: u32, size: vec2<f32>, radius: f32) -> AtlasPoint {
    let view = view_of(dir);
    let local = in_view(dir, view);
    let tangent = local.y / local.x;
    // A texel's world width here: the tangent maps to the row linearly, so
    // a texel covers (2 / width) / (1 + t²) of angle, less towards the
    // view's ends. A bias of 1.5 of them keeps a surface from shadowing
    // itself, and the shadow starts at its occluder
    let texel = dist * (2.0 / size.x) / (1.0 + tangent * tangent);
    return AtlasPoint((tangent + 1.0) * 0.5 * size.x, slot * 4u + view, 1.5 * texel / radius);
}

// 1 where `point`'s row holds nothing nearer than `depth` less its bias,
// filtered along the row, read at the row's centre (never across rows)
fn lit_at(point: AtlasPoint, depth: f32, size: vec2<f32>) -> f32 {
    let uv = vec2<f32>(point.texel / size.x, (f32(point.row) + 0.5) / size.y);
    return textureSampleCompareLevel(shadow_atlas, shadow_compare, uv, depth - point.bias);
}

// `v` turned by the angle whose cosine and sine are `by`
fn turned(v: vec2<f32>, by: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(v.x * by.x - v.y * by.y, v.x * by.y + v.y * by.x);
}

// `v` turned by `angle` radians
fn turned_by(v: vec2<f32>, angle: f32) -> vec2<f32> {
    return turned(v, vec2<f32>(cos(angle), sin(angle)));
}

// How much of the light at `offset` from it (ground space: the light and the
// pixel are lifted alike on a storey) its occluders hold back, 0 to 1.
//
// A light of size 0 takes one comparison against its view's row of the
// atlas. A light whose source is `size` pixels in radius first searches
// `min(6 x size / dist, MAX_ARC)` radians round the pixel's angle with
// `taps / 2` raw distances, made odd (`| 1`), the middle one at the
// pixel's own angle, for the farthest thing nearer than the pixel
// (`blocker`); with none the pixel is lit. The search reaches the whole
// penumbra of every blocker at least a quarter of the way from the light
// to the pixel. Then it filters
// the penumbra of a source that size, `min(2 x size x (dist - blocker) /
// (blocker x dist), MAX_ARC)` radians, with `taps` comparisons spread
// evenly over it: narrow where an occluder meets its shadow, wider away
// from it. Each look-up finds its own view and row from its own angle, so
// an arc crossing a view's end carries on in the next view's row (view 3
// wraps to view 0).
fn shadowed(offset: vec2<f32>, shadow: vec3<f32>) -> f32 {
    let dist = length(offset);
    // No slot, or nearer than the map's near plane (0.5 px): unshadowed
    if shadow.x < 0.0 || dist < 0.5 {
        return 0.0;
    }
    let slot = u32(shadow.x);
    let source = shadow.y;
    let radius = shadow.z;
    let size = vec2<f32>(textureDimensions(shadow_atlas));
    let depth = dist / radius;
    if !(source > 0.0) {
        return 1.0 - lit_at(atlas_point(offset, dist, slot, size, radius), depth, size);
    }
    let dir = offset / dist;
    let taps = max(shadow_filter.taps.x, 1u);

    // The blocker search: the farthest of what is nearer than the pixel
    // round its angle, the middle look-up on the pixel's own ray. Behind a
    // thick occluder the middle look-up finds the face just in front of the
    // pixel, so the filter's arc shrinks to nothing there however the
    // others fall; a wider arc (from an average, or a nearer blocker alone)
    // would turn look-ups into the occluder's body, which only its far face
    // shadows, and they would read lit. (A side two closed occluders share
    // casts nothing, `lighting/plan.rs`, so it never hides a wall's far
    // face from the middle look-up.)
    // An odd count for any `taps` (3, 5, 7, 9 at the qualities' 4 to 16),
    // so one look-up lies on the pixel's own ray; a single one looks only
    // there
    let searches = (taps / 2u) | 1u;
    let search_arc = min(6.0 * source / dist, MAX_ARC);
    let search_step = search_arc / f32(max(searches - 1u, 1u));
    let search_turn = vec2<f32>(cos(search_step), sin(search_step));
    var look = turned_by(dir, -0.5 * search_step * f32(searches - 1u));
    let last_texel = i32(size.x) - 1;
    var farthest = 0.0;
    var found = false;
    for (var i = 0u; i < searches; i++) {
        let point = atlas_point(look, dist, slot, size, radius);
        let column = clamp(i32(floor(point.texel)), 0, last_texel);
        let stored = textureLoad(shadow_atlas, vec2<i32>(column, i32(point.row)), 0);
        if stored < depth - point.bias {
            farthest = max(farthest, stored);
            found = true;
        }
        look = turned(look, search_turn);
    }
    if !found {
        return 0.0;
    }
    let blocker = max(farthest * radius, 0.5);

    // The filter: comparisons spread evenly over the penumbra's arc
    let arc = min(2.0 * source * max(dist - blocker, 0.0) / (blocker * dist), MAX_ARC);
    let filter_step = arc / f32(taps);
    let filter_turn = vec2<f32>(cos(filter_step), sin(filter_step));
    look = turned_by(dir, 0.5 * filter_step - 0.5 * arc);
    var lit = 0.0;
    for (var i = 0u; i < taps; i++) {
        lit += lit_at(atlas_point(look, dist, slot, size, radius), depth, size);
        look = turned(look, filter_turn);
    }
    return 1.0 - lit / f32(taps);
}

// The light, held back where it is shadowed by its shadow's strength (the
// shadow colour's alpha), letting the shadow colour's share of it through
@fragment
fn fs_light(in: VertexOutput) -> @location(0) vec4<f32> {
    let light = light_at(in);
    // Past the radius (the quad's corners) or outside a spot's reach no
    // light arrives, so there is no shadow to read
    if all(light == vec3<f32>(0.0)) {
        return vec4<f32>(0.0);
    }
    let held = in.shadow_color.a * shadowed(in.offset, in.shadow);
    return vec4<f32>(light * (1.0 - held) + in.shadow_color.rgb * light * held, 0.0);
}
