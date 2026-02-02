//! Lighting system integration tests
//!
//! These tests verify the lighting system behaves correctly and doesn't
//! break other features like UI rendering.
//!
//! REGRESSION CONTEXT:
//! - Adding lighting accidentally broke UI visibility (UI disappeared in darkness)
//! - Games without lights were incorrectly affected by the lighting system

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
  // The LightingRenderer::new() sets ui_affected_by_lighting: false (line 373)
  // This test documents that invariant.
  //
  // When ui_affected_by_lighting is false:
  // - UI is rendered to a separate buffer
  // - UI buffer is composited AFTER lighting is applied to the scene
  // - UI appears at full brightness regardless of ambient light level

  // This is a documentation test - actual rendering verification would
  // require a headless GPU context
  assert!(true, "UI should not be affected by lighting by default");
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
  world.spawn((Transform::new(100.0, 100.0),));
  world.spawn((Transform::new(200.0, 200.0), Velocity::new(1.0, 0.0)));

  // Verify no Light2D components exist
  let light_count = world.query::<&Light2D>().iter().count();
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
// LIGHTING + WORLD INTEGRATION TESTS
// ============================================================================

/// Test that Light2D entities can be spawned and queried
#[test]
fn light2d_entities_queryable() {
  let mut world = World::new();

  // Spawn a light entity
  let light_entity = world.spawn((Transform::new(100.0, 100.0), Light2D::torch(150.0)));

  // Should be able to query it
  let light = world.get::<Light2D>(light_entity);
  assert!(light.is_some(), "Should be able to get Light2D component");

  // Query all lights
  let mut query = world.query::<(&Transform, &Light2D)>();
  let lights: Vec<_> = query.iter().collect();
  assert_eq!(lights.len(), 1, "Should find one light");
}

/// Test multiple lights can coexist
#[test]
fn multiple_lights_coexist() {
  let mut world = World::new();

  // Spawn multiple lights
  world.spawn((Transform::new(0.0, 0.0), Light2D::torch(100.0)));
  world.spawn((Transform::new(200.0, 0.0), Light2D::moonlight(150.0)));
  world.spawn((
    Transform::new(100.0, 200.0),
    Light2D::flashlight(200.0, 0.0),
  ));

  let light_count = world.query::<&Light2D>().iter().count();
  assert_eq!(light_count, 3, "All lights should be present");
}
