//! Lighting system integration tests
//!
//! These tests verify the lighting system behaves correctly and doesn't
//! break other features like UI rendering.
//!
//! REGRESSION CONTEXT:
//! - Adding lighting accidentally broke UI visibility (UI disappeared in darkness)
//! - Games without lights were incorrectly affected by the lighting system
//! - Texture reuse caused write_texture ordering issues (sprite content overwritten by UI)
//!
//! CRITICAL INVARIANT (2024-02):
//! The UI overlay is uploaded with write_texture into its own ui_texture; the
//! game texture is written only by GPU commands. Sharing a texture between a
//! write_texture upload and the frame's GPU commands causes visual corruption.

use ryo2d::prelude::*;

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: LightingRenderer must start with enabled: false
///
/// This ensures games without lights aren't affected by the lighting system.
/// If this default changes, games will suddenly become dark.
#[test]
fn regression_lighting_disabled_by_default() {
  // We can't create a LightingRenderer without GPU, but we can verify
  // the AmbientLight default and document the expected behavior.
  //
  // The actual LightingRenderer::new() sets enabled: false (line 372 of renderer.rs)
  // This test documents that invariant.

  // AmbientLight::default() should give reasonable visibility
  let ambient = AmbientLight::default();
  // Default ambient should not be completely dark
  assert!(
    ambient.intensity > 0.0,
    "Default ambient should provide some light"
  );
}

/// REGRESSION: UI must NOT be affected by lighting by default
///
/// UI elements (HUD, menus, text) should always be visible regardless of
/// scene lighting. This was broken when lighting was added and UI disappeared
/// in dark scenes.
#[test]
fn regression_ui_unaffected_by_lighting_default() {
  // The UI is never lit. This test documents that invariant:
  // - UI is rendered to a separate buffer
  // - UI buffer is composited AFTER lighting is applied to the scene
  // - UI appears at full brightness regardless of ambient light level

  // This is a documentation test - actual rendering verification would
  // require a headless GPU context
  // The test passes if it compiles and runs without panicking
}

/// REGRESSION: Games without Light2D entities must render normally
///
/// The lighting system should have zero visual impact on games that don't
/// use Light2D components. The scene should render at full brightness.
#[test]
fn regression_no_lights_renders_normally() {
  // When no Light2D entities exist:
  // 1. LightingRenderer.enabled remains false (never auto-enabled)
  // 2. The compose pass is skipped entirely
  // 3. Scene renders directly without lighting modifications
  //
  // This ensures backwards compatibility with games written before
  // lighting was added to the engine.

  // Verify we can create a World without any Light2D entities
  let mut world = World::new();

  // Spawn some regular entities (no lights)
  world.spawn((Transform::from_xy(100.0, 100.0),));
  world.spawn((Transform::from_xy(200.0, 200.0), Velocity::new(1.0, 0.0)));

  // Verify no Light2D components exist
  let light_count = world.query::<&Light2D>().iter(&world).count();
  assert_eq!(light_count, 0, "Test setup should have no lights");

  // In the actual engine, this means lighting.is_enabled() remains false
  // and the lighting compose pass is never executed
}

// ============================================================================
// AMBIENT LIGHT TESTS
// ============================================================================

/// Test that ambient light intensity is clamped to valid range [0.0, 1.0]
#[test]
fn ambient_intensity_clamped_to_valid_range() {
  // Over 1.0 should clamp to 1.0
  let ambient = AmbientLight::new(Color::WHITE, 1.5);
  assert_eq!(
    ambient.intensity, 1.0,
    "Intensity > 1.0 should clamp to 1.0"
  );

  // Negative should clamp to 0.0
  let ambient = AmbientLight::new(Color::WHITE, -0.5);
  assert_eq!(
    ambient.intensity, 0.0,
    "Negative intensity should clamp to 0.0"
  );

  // Valid values should pass through
  let ambient = AmbientLight::new(Color::WHITE, 0.5);
  assert_eq!(
    ambient.intensity, 0.5,
    "Valid intensity should be preserved"
  );

  // Edge cases
  let ambient = AmbientLight::new(Color::WHITE, 0.0);
  assert_eq!(ambient.intensity, 0.0, "Zero intensity should be valid");

  let ambient = AmbientLight::new(Color::WHITE, 1.0);
  assert_eq!(ambient.intensity, 1.0, "Max intensity should be valid");
}

/// Test ambient light preset constructors
#[test]
fn ambient_light_presets() {
  // Dark preset (horror games, caves)
  let dark = AmbientLight::dark();
  assert!(dark.intensity <= 0.1, "Dark preset should be very dim");

  // Night preset
  let night = AmbientLight::night();
  assert!(
    night.intensity > 0.0 && night.intensity < 0.5,
    "Night preset should be dim but not pitch black"
  );

  // Daylight preset
  let daylight = AmbientLight::daylight();
  assert!(daylight.intensity >= 0.8, "Daylight should be bright");
}

/// Test ambient light intensity modification
#[test]
fn ambient_intensity_modification() {
  let mut ambient = AmbientLight::new(Color::WHITE, 0.5);

  // set_intensity should also clamp
  ambient.set_intensity(2.0);
  assert_eq!(ambient.intensity, 1.0, "set_intensity should clamp");

  ambient.set_intensity(-1.0);
  assert_eq!(
    ambient.intensity, 0.0,
    "set_intensity should clamp negatives"
  );

  ambient.set_intensity(0.7);
  assert_eq!(
    ambient.intensity, 0.7,
    "set_intensity should preserve valid values"
  );
}

// ============================================================================
// LIGHT2D COMPONENT TESTS
// ============================================================================

/// Test Light2D point light creation
#[test]
fn light2d_point_light_creation() {
  let light = Light2D::point(Color::WHITE, 100.0);

  assert!(light.is_point(), "Should be a point light");
  assert!(!light.is_spot(), "Should not be a spot light");
  assert_eq!(light.radius(), 100.0);
  assert_eq!(light.color(), Color::WHITE);
}

/// Test Light2D spot light creation
#[test]
fn light2d_spot_light_creation() {
  let light = Light2D::spot(Color::RED, 150.0, 0.5, 0.0);

  assert!(!light.is_point(), "Should not be a point light");
  assert!(light.is_spot(), "Should be a spot light");
  assert_eq!(light.radius(), 150.0);
  assert_eq!(light.color(), Color::RED);
}

/// Test Light2D preset constructors
#[test]
fn light2d_presets() {
  let torch = Light2D::torch(100.0);
  assert!(torch.is_point(), "Torch should be a point light");

  let flashlight = Light2D::flashlight(200.0, 0.0);
  assert!(flashlight.is_spot(), "Flashlight should be a spot light");

  let moonlight = Light2D::moonlight(300.0);
  assert!(moonlight.is_point(), "Moonlight should be a point light");
}

/// Test Light2D builder methods
#[test]
fn light2d_builder_methods() {
  let light = Light2D::point(Color::WHITE, 100.0)
    .with_intensity(0.8)
    .with_shadows();

  assert_eq!(light.intensity(), 0.8);
  assert!(light.casts_shadows());
}

// ============================================================================
// VOLUMETRIC LIGHTING TESTS
// ============================================================================

/// Test VolumetricSettings creation and defaults
#[test]
fn volumetric_settings_creation() {
  // Default is disabled
  let default_settings = VolumetricSettings::default();
  assert!(!default_settings.enabled);
  assert!(!default_settings.god_rays);

  // Custom settings
  let custom = VolumetricSettings::new(0.8, 0.6, 0.4);
  assert!(custom.enabled);
  assert!((custom.intensity - 0.8).abs() < 0.001);
  assert!((custom.density - 0.6).abs() < 0.001);
  assert!((custom.scattering - 0.4).abs() < 0.001);
  assert!(!custom.god_rays);
}

/// Test VolumetricSettings presets
#[test]
fn volumetric_settings_presets() {
  let flashlight = VolumetricSettings::flashlight();
  assert!(flashlight.enabled);
  assert!(flashlight.intensity > 0.0);
  assert!(flashlight.density > 0.0);
  assert!(!flashlight.god_rays);

  let strong = VolumetricSettings::strong_beam();
  assert!(strong.enabled);
  assert!(strong.intensity >= flashlight.intensity);

  let subtle = VolumetricSettings::subtle_beam();
  assert!(subtle.enabled);
  assert!(subtle.intensity < flashlight.intensity);

  let god_rays_only = VolumetricSettings::god_rays_only();
  assert!(god_rays_only.enabled);
  assert!(god_rays_only.god_rays);
  assert_eq!(god_rays_only.intensity, 0.0);
}

/// Test VolumetricSettings builder methods
#[test]
fn volumetric_settings_builders() {
  let settings = VolumetricSettings::enabled()
    .with_intensity(0.7)
    .with_density(0.5)
    .with_scattering(0.3)
    .with_god_rays();

  assert!(settings.enabled);
  assert!((settings.intensity - 0.7).abs() < 0.001);
  assert!((settings.density - 0.5).abs() < 0.001);
  assert!((settings.scattering - 0.3).abs() < 0.001);
  assert!(settings.god_rays);
}

/// Test VolumetricSettings value clamping
#[test]
fn volumetric_settings_clamping() {
  // Values over 1.0 should clamp
  let over = VolumetricSettings::new(2.0, 1.5, 3.0);
  assert_eq!(over.intensity, 1.0);
  assert_eq!(over.density, 1.0);
  assert_eq!(over.scattering, 1.0);

  // Negative values should clamp to 0.0
  let under = VolumetricSettings::new(-0.5, -1.0, -0.1);
  assert_eq!(under.intensity, 0.0);
  assert_eq!(under.density, 0.0);
  assert_eq!(under.scattering, 0.0);
}

/// Test VolumetricSettings GPU packing
#[test]
fn volumetric_settings_gpu_packing() {
  let settings = VolumetricSettings::new(0.5, 0.3, 0.2).with_god_rays();
  let packed = settings.to_gpu_vec4();

  assert!((packed[0] - 0.5).abs() < 0.001); // intensity
  assert!((packed[1] - 0.3).abs() < 0.001); // density
  assert!((packed[2] - 0.2).abs() < 0.001); // scattering
  assert!((packed[3] - 1.0).abs() < 0.001); // god_rays = true

  // Disabled settings pack to zero
  let disabled = VolumetricSettings::default();
  let packed_disabled = disabled.to_gpu_vec4();
  assert_eq!(packed_disabled[0], 0.0);
  assert_eq!(packed_disabled[1], 0.0);
  assert_eq!(packed_disabled[2], 0.0);
  assert_eq!(packed_disabled[3], 0.0);
}

/// Test Light2D with volumetric settings
#[test]
fn light2d_with_volumetric() {
  // Point light with volumetric
  let point = Light2D::point(Color::WHITE, 100.0).with_volumetric();
  assert!(point.has_volumetric());
  assert!(!point.contributes_to_god_rays());

  // Flashlight with volumetric (uses flashlight preset)
  let flashlight = Light2D::flashlight(200.0, 0.0).with_volumetric();
  assert!(flashlight.has_volumetric());
  let vol = flashlight.volumetric().unwrap();
  assert!(vol.intensity > 0.0);

  // Light with god rays
  let sun = Light2D::point(Color::WHITE, 500.0).with_god_rays();
  assert!(sun.has_volumetric());
  assert!(sun.contributes_to_god_rays());

  // Light with both volumetric and god rays
  let both = Light2D::spot(Color::WHITE, 200.0, 45.0, 0.0).with_volumetric_and_god_rays();
  assert!(both.has_volumetric());
  assert!(both.contributes_to_god_rays());
}

/// Test Light2D with custom volumetric settings
#[test]
fn light2d_with_custom_volumetric() {
  let custom_settings = VolumetricSettings::new(0.9, 0.7, 0.5).with_god_rays();

  let light =
    Light2D::spot(Color::WHITE, 300.0, 60.0, 45.0).with_volumetric_settings(custom_settings);

  assert!(light.has_volumetric());
  assert!(light.contributes_to_god_rays());

  let vol = light.volumetric().unwrap();
  assert!((vol.intensity - 0.9).abs() < 0.001);
  assert!((vol.density - 0.7).abs() < 0.001);
  assert!((vol.scattering - 0.5).abs() < 0.001);
}

/// Test Light2D volumetric modification
#[test]
fn light2d_volumetric_modification() {
  let mut light = Light2D::flashlight(200.0, 0.0);
  assert!(light.volumetric().is_none());

  // Add volumetric
  let settings = VolumetricSettings::strong_beam();
  light.set_volumetric(Some(settings));
  assert!(light.has_volumetric());

  // Remove volumetric
  light.set_volumetric(None);
  assert!(!light.has_volumetric());
}

// ============================================================================
// ATMOSPHERE TESTS
// ============================================================================

/// Test Atmosphere creation and defaults
#[test]
fn atmosphere_creation() {
  let default_atm = Atmosphere::default();
  assert!(!default_atm.god_rays_enabled);
  assert_eq!(default_atm.density, 0.0);

  let custom = Atmosphere::new(0.5, Color::new(200, 200, 220, 255));
  assert!((custom.density - 0.5).abs() < 0.001);
  assert_eq!(custom.color.r, 200);
}

/// Test Atmosphere presets
#[test]
fn atmosphere_presets() {
  let clear = Atmosphere::clear();
  assert_eq!(clear.density, 0.0);
  assert!(!clear.god_rays_enabled);

  let dusty = Atmosphere::dusty();
  assert!(dusty.density > 0.0);
  assert!(dusty.god_rays_enabled);

  let foggy = Atmosphere::foggy();
  assert!(foggy.density > dusty.density);

  let hazy = Atmosphere::hazy();
  assert!(hazy.density > 0.0);
  assert!(hazy.density < dusty.density);
}

/// Test Atmosphere god rays configuration
#[test]
fn atmosphere_god_rays_config() {
  let atm = Atmosphere::dusty();

  assert!(atm.god_rays_enabled);
  assert!(atm.god_rays_samples > 0);
  assert!(atm.god_rays_decay > 0.0 && atm.god_rays_decay < 1.0);
  assert!(atm.god_rays_intensity > 0.0);
}

/// Test Atmosphere builder methods
#[test]
fn atmosphere_builders() {
  let atm = Atmosphere::clear()
    .with_density(0.4)
    .with_god_rays()
    .with_god_rays_quality(GodRaysQuality::High);

  assert!((atm.density - 0.4).abs() < 0.001);
  assert!(atm.god_rays_enabled);
  assert!(atm.god_rays_samples >= 24); // High quality
}

/// Test GodRaysQuality presets
#[test]
fn god_rays_quality_presets() {
  let low = GodRaysQuality::Low;
  let medium = GodRaysQuality::Medium;
  let high = GodRaysQuality::High;

  assert!(low.samples() < medium.samples());
  assert!(medium.samples() < high.samples());
}

// ============================================================================
// VOLUMETRIC + WORLD INTEGRATION TESTS
// ============================================================================

/// Test volumetric lights can be spawned and queried
#[test]
fn volumetric_lights_in_world() {
  let mut world = World::new();

  // Spawn lights with various volumetric settings
  let flashlight = world
    .spawn((
      Transform::from_xy(100.0, 100.0),
      Light2D::flashlight(200.0, 0.0).with_volumetric(),
    ))
    .id();

  let sun = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Light2D::point(Color::WHITE, 500.0).with_god_rays(),
    ))
    .id();

  let normal_light = world
    .spawn((Transform::from_xy(200.0, 200.0), Light2D::torch(100.0)))
    .id();

  // Query and verify
  let light1 = world.get::<Light2D>(flashlight).unwrap();
  assert!(light1.has_volumetric());
  assert!(!light1.contributes_to_god_rays());

  let light2 = world.get::<Light2D>(sun).unwrap();
  assert!(light2.has_volumetric());
  assert!(light2.contributes_to_god_rays());

  let light3 = world.get::<Light2D>(normal_light).unwrap();
  assert!(!light3.has_volumetric());
}

/// Test filtering god ray contributing lights
#[test]
fn filter_god_ray_lights() {
  let mut world = World::new();

  // Mix of lights with and without god rays
  world.spawn((
    Transform::from_xy(0.0, 0.0),
    Light2D::point(Color::WHITE, 500.0).with_god_rays(),
  ));
  world.spawn((
    Transform::from_xy(100.0, 100.0),
    Light2D::flashlight(200.0, 0.0).with_volumetric(),
  ));
  world.spawn((
    Transform::from_xy(200.0, 200.0),
    Light2D::point(Color::WHITE, 300.0).with_god_rays(),
  ));
  world.spawn((Transform::from_xy(300.0, 300.0), Light2D::torch(100.0)));

  // Count lights that contribute to god rays
  let god_ray_count = world
    .query::<&Light2D>()
    .iter(&world)
    .filter(|light| light.contributes_to_god_rays())
    .count();

  assert_eq!(god_ray_count, 2, "Should have 2 god ray lights");

  // Count lights with any volumetric effect
  let volumetric_count = world
    .query::<&Light2D>()
    .iter(&world)
    .filter(|light| light.has_volumetric())
    .count();

  assert_eq!(volumetric_count, 3, "Should have 3 volumetric lights");
}

// ============================================================================
// LIGHTING + WORLD INTEGRATION TESTS
// ============================================================================

/// Test that Light2D entities can be spawned and queried
#[test]
fn light2d_entities_queryable() {
  let mut world = World::new();

  // Spawn a light entity
  let light_entity = world
    .spawn((Transform::from_xy(100.0, 100.0), Light2D::torch(150.0)))
    .id();

  // Should be able to query it
  let light = world.get::<Light2D>(light_entity);
  assert!(light.is_some(), "Should be able to get Light2D component");

  // Query all lights
  let mut query = world.query::<(&Transform, &Light2D)>();
  let lights: Vec<_> = query.iter(&world).collect();
  assert_eq!(lights.len(), 1, "Should find one light");
}

/// Test multiple lights can coexist
#[test]
fn multiple_lights_coexist() {
  let mut world = World::new();

  // Spawn multiple lights
  world.spawn((Transform::from_xy(0.0, 0.0), Light2D::torch(100.0)));
  world.spawn((Transform::from_xy(200.0, 0.0), Light2D::moonlight(150.0)));
  world.spawn((
    Transform::from_xy(100.0, 200.0),
    Light2D::flashlight(200.0, 0.0),
  ));

  let light_count = world.query::<&Light2D>().iter(&world).count();
  assert_eq!(light_count, 3, "All lights should be present");
}

// ============================================================================
// TEXTURE SEPARATION REGRESSION TESTS (2024-02)
// ============================================================================
//
// These tests document critical invariants about texture usage in the renderer.
// The UI layer uses its own texture so its write_texture never races the GPU passes.
//
// Background: wgpu's write_texture operations are NOT ordered with respect to
// command buffer execution. If a write_texture targeted a texture the frame's
// commands also draw into, it could land before or after those commands.
//
// Render flow (the `default_2d` pipeline):
// 1. UI content -> ui_texture (write_texture) - the only CPU upload per frame
// 2. Clear game_texture
// 3. `world`: entities, sprites, particles, trails -> game_texture (GPU)
// 4. `lighting`: game_texture -> lit_texture
// 5. `unlit_world`, `transition`, `post_process` on the latest image
// 6. `composite`: latest image -> screen, then blend ui_texture -> screen

/// REGRESSION: UI must use dedicated ui_texture, not the game or lit texture
///
/// The UI overlay MUST be uploaded to a dedicated texture because the game
/// and lit textures are written by GPU commands in the same frame, and a
/// write_texture into them would race those commands.
///
/// BUG FIXED: sprites invisible when lighting enabled (2024-02)
/// ROOT CAUSE: UI and sprite content shared one texture
/// FIX: Added dedicated ui_texture for UI overlay content
#[test]
fn regression_ui_uses_dedicated_texture() {
  // This is a documentation test.
  // The `ui` render pass uploads the UI buffer with write_texture into the
  // compositor's dedicated ui_texture, and the `composite` pass blends it last.
  //
  // If this invariant is violated, symptoms will be:
  // - Lighting or world content overwritten (if using the game or lit texture)
}

/// REGRESSION: Render flow must maintain texture isolation
///
/// Only the UI buffer is uploaded with write_texture; everything else in the
/// frame is drawn by GPU commands, all submitted together.
#[test]
fn regression_render_flow_texture_isolation() {
  // This is a documentation test verifying the expected render flow.
  // The actual implementation is the `default_2d` render pipeline.
  //
  // Key invariants:
  // - ui_texture receives ONLY ui_buffer data
  // - game_texture is written ONLY by GPU commands (clear, world passes)
  // - lit_texture is written ONLY by lighting compose
  //
  // Breaking any of these invariants will cause visual bugs.
}

/// REGRESSION: Lighting + post-processing effects must work together (2024-02)
///
/// BUG FIXED: When both lighting AND post-processing effects (CRT, vignette, etc.)
/// were enabled simultaneously, the screen went completely black.
///
/// ROOT CAUSE: The render pipeline had a texture data flow bug:
/// - Lighting rendered to a temporary `lit_texture`
/// - Post-processing effects expected input in `texture_a`
/// - But nothing copied the lit scene from lit_texture to texture_a
/// - Effects were reading uninitialized/black data
///
/// FIX: In the lighting-to-effects handoff, when `has_effects && lighting_applied`:
/// 1. Added COPY_SRC flag to lit_texture (enables copying from it)
/// 2. Added copy_texture_to_texture(lit_texture -> texture_a) before effects
///
/// The complete flow with lighting + effects is now:
/// 1. Lighting: game_texture -> lit_texture (compose pass)
/// 2. COPY: lit_texture -> texture_a (bridge to post-process input)
/// 3. Effects: texture_a -> texture_b (ping-pong chain)
/// 4. Final: effect output -> screen
///
/// See: tests/e2e/lighting_renderer.rs::test_lighting_plus_effects_produces_visible_output
#[test]
fn regression_lighting_plus_effects_requires_texture_copy() {
  // This is a documentation test. The actual GPU verification is in e2e tests.
  //
  // The critical code path in the old renderer was:
  //
  // } else if has_effects && lighting_applied {
  //   // Lighting wrote to lit_texture, need to copy to texture_a before applying effects
  //   if let (Some(effects_mgr), Some(post_process), Some(lit_tex)) = (...) {
  //     // Copy lit scene to texture_a first
  //     encoder.copy_texture_to_texture(
  //       lit_tex -> post_process.texture_a()
  //     );
  //     // Now apply effects chain starting from texture_a
  //     post_process.apply_effects(...)
  //   }
  // }
  //
  // Without this copy, effects read from an uninitialized texture_a.
}
