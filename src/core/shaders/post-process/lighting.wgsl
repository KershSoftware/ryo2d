// 2D Lighting post-process shader
// Applies the light accumulation buffer to the scene with atmosphere effects

struct EffectParams {
    color: vec4<f32>,             // Ambient light color (RGB) + unused (16 bytes)
    params: vec4<f32>,            // x: ambient intensity, y: atmosphere density, z: atmosphere scattering, w: unused (16 bytes)
    time_resolution: vec4<f32>,   // x: time, y: width, z: height, w: unused (16 bytes)
    atmosphere_color: vec4<f32>,  // Atmosphere fog color (RGB) + unused (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_scene: texture_2d<f32>;
@group(0) @binding(1) var t_light: texture_2d<f32>;
@group(0) @binding(2) var s_input: sampler;
@group(0) @binding(3) var<uniform> effect: EffectParams;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let scene_color = textureSample(t_scene, s_input, in.uv);
    // Additive pile-ups can overflow the scene's half floats the same way
    let scene_rgb = min(scene_color.rgb, vec3<f32>(65504.0));
    // Half floats past 65504 are infinite, and 0 x infinity is NaN: many
    // lights or a huge intensity can overflow the light buffer
    let light_color = min(textureSample(t_light, s_input, in.uv).rgb, vec3<f32>(65504.0));

    // Ambient lighting
    let ambient_color = effect.color.rgb;
    let ambient_intensity = effect.params.x;
    let ambient = ambient_color * ambient_intensity;

    // Atmosphere parameters
    let atmos_density = effect.params.y;
    let atmos_scattering = effect.params.z;
    let atmos_color = effect.atmosphere_color.rgb;

    // Apply scattering to lights - makes lights appear more diffuse/spread in fog
    // Higher scattering = lights affect a broader area with softer falloff
    let scattered_light = light_color * (1.0 + atmos_scattering * 0.5);

    // Calculate total light
    let total_light = ambient + scattered_light;

    // Apply lighting to scene
    var lit_color = scene_rgb * total_light;

    // Apply atmosphere fog - blend toward fog color based on density
    // The effect is subtle: density of 1.0 applies ~40% fog tint
    let fog_amount = atmos_density * 0.4;
    lit_color = mix(lit_color, atmos_color * total_light, fog_amount);

    // Within what the scene's half floats hold, so later stages see no infinity
    return vec4<f32>(min(lit_color, vec3<f32>(65504.0)), scene_color.a);
}
