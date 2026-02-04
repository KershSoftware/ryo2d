//! Y-Sort integration tests
//!
//! These tests verify that the YSort component integrates correctly with
//! the ECS World and other engine components.
//!
//! CONTEXT:
//! - YSort enables Zelda/RPG-style depth sorting by Y position
//! - Entities with YSort sort by their foot position within render layers
//! - Optional depth scaling creates perspective illusion

use ryo2d::prelude::*;

// ============================================================================
// ECS INTEGRATION TESTS
// ============================================================================

/// Test that YSort can be spawned as a component
#[test]
fn ysort_spawn_basic() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(100.0, 200.0), YSort::new()));

  assert!(
    world.has::<YSort>(entity),
    "Entity should have YSort component"
  );
  assert!(
    world.has::<Transform>(entity),
    "Entity should have Transform component"
  );
}

/// Test that YSort can be retrieved and has correct default values
#[test]
fn ysort_get_defaults() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(0.0, 0.0), YSort::new()));

  let ysort = world.get::<YSort>(entity);
  assert!(ysort.is_some());

  let ysort = ysort.unwrap();
  assert_eq!(ysort.foot_offset, 1.0);
  assert_eq!(ysort.sort_offset, 0.0);
  assert!(!ysort.depth_scale_enabled);
}

/// Test that YSort with custom settings can be spawned
#[test]
fn ysort_spawn_with_settings() {
  let mut world = World::new();

  let entity = world.spawn((
    Transform::new(50.0, 100.0),
    YSort::new()
      .with_foot_offset(0.5)
      .with_sort_offset(10.0)
      .with_depth_scale(0.8, 1.2, 0.0, 200.0),
  ));

  let ysort = world.get::<YSort>(entity).unwrap();
  assert_eq!(ysort.foot_offset, 0.5);
  assert_eq!(ysort.sort_offset, 10.0);
  assert!(ysort.depth_scale_enabled);
  assert_eq!(ysort.scale_min, 0.8);
  assert_eq!(ysort.scale_max, 1.2);
}

/// Test that YSort can be modified after spawn
#[test]
fn ysort_modify() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(0.0, 0.0), YSort::new()));

  // Modify the YSort component
  if let Some(mut ysort) = world.get_mut::<YSort>(entity) {
    ysort.foot_offset = 0.75;
    ysort.sort_offset = 25.0;
    ysort.depth_scale_enabled = true;
    ysort.scale_min = 0.5;
    ysort.scale_max = 1.5;
    ysort.y_min = 10.0;
    ysort.y_max = 190.0;
  }

  // Verify changes persisted
  let ysort = world.get::<YSort>(entity).unwrap();
  assert_eq!(ysort.foot_offset, 0.75);
  assert_eq!(ysort.sort_offset, 25.0);
  assert!(ysort.depth_scale_enabled);
  assert_eq!(ysort.scale_min, 0.5);
  assert_eq!(ysort.scale_max, 1.5);
  assert_eq!(ysort.y_min, 10.0);
  assert_eq!(ysort.y_max, 190.0);
}

/// Test that YSort can be replaced entirely
#[test]
fn ysort_replace() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(0.0, 0.0), YSort::new()));

  // Replace with a completely new YSort
  if let Some(mut ysort) = world.get_mut::<YSort>(entity) {
    *ysort = YSort::new()
      .with_foot_offset(0.0)
      .with_depth_scale(0.6, 1.4, 50.0, 150.0);
  }

  let ysort = world.get::<YSort>(entity).unwrap();
  assert_eq!(ysort.foot_offset, 0.0);
  assert!(ysort.depth_scale_enabled);
}

// ============================================================================
// QUERY TESTS
// ============================================================================

/// Test querying entities with YSort component
#[test]
fn ysort_query_entities_with() {
  let mut world = World::new();

  // Spawn some entities with YSort
  world.spawn((Transform::new(0.0, 0.0), YSort::new()));
  world.spawn((Transform::new(10.0, 10.0), YSort::new()));
  world.spawn((Transform::new(20.0, 20.0), YSort::new()));

  // Spawn some entities without YSort
  world.spawn((Transform::new(30.0, 30.0),));
  world.spawn((Transform::new(40.0, 40.0),));

  let ysort_entities = world.entities_with::<YSort>();
  assert_eq!(ysort_entities.len(), 3, "Should find 3 entities with YSort");
}

/// Test querying entities with both Transform and YSort
#[test]
fn ysort_query_with_transform() {
  let mut world = World::new();

  world.spawn((Transform::new(100.0, 50.0), YSort::new()));
  world.spawn((Transform::new(200.0, 100.0), YSort::new()));

  let entities = world.entities_with_two::<Transform, YSort>();
  assert_eq!(entities.len(), 2);

  // Verify we can access both components
  for entity in entities {
    assert!(world.has::<Transform>(entity));
    assert!(world.has::<YSort>(entity));
  }
}

// ============================================================================
// COMBINED WITH RENDERABLE TESTS
// ============================================================================

/// Test typical game entity setup with Transform, Renderable, and YSort
#[test]
fn ysort_with_renderable() {
  let mut world = World::new();

  // Create a simple sprite for testing
  let sprite = Sprite::solid_rect(16, 24, 255, 0, 0, 255);
  let sprite_handle = SpriteHandle::from_sprite(sprite, "test_sprite");

  let entity = world.spawn((
    Transform::new(100.0, 150.0).with_anchor(Anchor::BottomCenter),
    Renderable::sprite(sprite_handle).with_z_order(0),
    YSort::new(),
  ));

  assert!(world.has::<Transform>(entity));
  assert!(world.has::<Renderable>(entity));
  assert!(world.has::<YSort>(entity));

  // Verify the sprite height can be used with YSort
  let renderable = world.get::<Renderable>(entity).unwrap();
  let ysort = world.get::<YSort>(entity).unwrap();
  let transform = world.get::<Transform>(entity).unwrap();

  let sprite_height = renderable.sprite.height() as f32;
  let sort_y = ysort.calculate_sort_y(transform.y, sprite_height);

  // sort_y = 150 + (24 * 1.0) + 0 = 174
  assert_eq!(sort_y, 174.0);
}

// ============================================================================
// SORT ORDER VERIFICATION TESTS
// ============================================================================

/// Test that entities at different Y positions produce different sort keys
#[test]
fn ysort_ordering_by_y() {
  let mut world = World::new();

  let sprite = Sprite::solid_rect(16, 16, 255, 0, 0, 255);
  let sprite_handle = SpriteHandle::from_sprite(sprite, "test");

  // Create entities at different Y positions
  let entity_top = world.spawn((
    Transform::new(100.0, 50.0),
    Renderable::sprite(sprite_handle.clone()),
    YSort::new(),
  ));

  let entity_middle = world.spawn((
    Transform::new(100.0, 100.0),
    Renderable::sprite(sprite_handle.clone()),
    YSort::new(),
  ));

  let entity_bottom = world.spawn((
    Transform::new(100.0, 150.0),
    Renderable::sprite(sprite_handle),
    YSort::new(),
  ));

  // Calculate sort Y for each
  let sprite_height = 16.0;

  let sort_top = world
    .get::<YSort>(entity_top)
    .unwrap()
    .calculate_sort_y(50.0, sprite_height);
  let sort_middle = world
    .get::<YSort>(entity_middle)
    .unwrap()
    .calculate_sort_y(100.0, sprite_height);
  let sort_bottom = world
    .get::<YSort>(entity_bottom)
    .unwrap()
    .calculate_sort_y(150.0, sprite_height);

  // Entities lower on screen (higher Y) should have higher sort keys
  assert!(
    sort_bottom > sort_middle,
    "Bottom entity should sort after middle"
  );
  assert!(
    sort_middle > sort_top,
    "Middle entity should sort after top"
  );
}

/// Test that z_order still takes precedence over Y-sorting
#[test]
fn ysort_z_order_precedence() {
  // This test documents the expected behavior:
  // Sort key = (z_order * 10000) + sort_y
  // So entities with different z_orders will be completely separated

  let ysort = YSort::new();
  let sprite_height = 16.0;

  // Entity at y=200 (high on screen), z_order=0
  let z_order_0: i64 = 0;
  let sort_y_layer0 = ysort.calculate_sort_y(200.0, sprite_height);
  let key_layer0 = (z_order_0 * 10000) + (sort_y_layer0 as i64);

  // Entity at y=50 (low on screen), z_order=1
  let z_order_1: i64 = 1;
  let sort_y_layer1 = ysort.calculate_sort_y(50.0, sprite_height);
  let key_layer1 = (z_order_1 * 10000) + (sort_y_layer1 as i64);

  // Even though layer0 entity has higher Y, layer1 entity should sort after
  // because z_order takes precedence
  assert!(
    key_layer1 > key_layer0,
    "Higher z_order should always sort after lower z_order"
  );
}

// ============================================================================
// DEPTH SCALE CALCULATION TESTS
// ============================================================================

/// Test depth scale calculation at various Y positions
#[test]
fn ysort_depth_scale_interpolation() {
  let mut world = World::new();

  let entity = world.spawn((
    Transform::new(100.0, 100.0),
    YSort::new().with_depth_scale(0.5, 1.5, 0.0, 200.0),
  ));

  let ysort = world.get::<YSort>(entity).unwrap();

  // At y_min (0), should be scale_min
  let (sx, sy) = ysort.calculate_depth_scale(0.0);
  assert_eq!(sx, 0.5);
  assert_eq!(sy, 0.5);

  // At midpoint (100), should be 1.0
  let (sx, sy) = ysort.calculate_depth_scale(100.0);
  assert_eq!(sx, 1.0);
  assert_eq!(sy, 1.0);

  // At y_max (200), should be scale_max
  let (sx, sy) = ysort.calculate_depth_scale(200.0);
  assert_eq!(sx, 1.5);
  assert_eq!(sy, 1.5);
}

/// Test that depth scale is applied correctly to transform
#[test]
fn ysort_depth_scale_application() {
  let ysort = YSort::new().with_depth_scale(0.8, 1.2, 0.0, 100.0);

  // Simulate what the engine does: multiply transform scale by depth scale
  let transform_scale = 2.0; // Entity has 2x scale

  let (depth_scale_x, depth_scale_y) = ysort.calculate_depth_scale(50.0);
  // At y=50 (midpoint), depth_scale should be 1.0

  let final_scale_x = transform_scale * depth_scale_x;
  let final_scale_y = transform_scale * depth_scale_y;

  assert_eq!(final_scale_x, 2.0);
  assert_eq!(final_scale_y, 2.0);

  // At y=100 (max), depth_scale should be 1.2
  let (depth_scale_x, depth_scale_y) = ysort.calculate_depth_scale(100.0);
  let final_scale_x = transform_scale * depth_scale_x;
  let final_scale_y = transform_scale * depth_scale_y;

  assert_eq!(final_scale_x, 2.4);
  assert_eq!(final_scale_y, 2.4);
}

// ============================================================================
// DESPAWN TESTS
// ============================================================================

/// Test that despawning entity removes YSort component
#[test]
fn ysort_despawn() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(0.0, 0.0), YSort::new()));

  assert!(world.has::<YSort>(entity));

  let _ = world.despawn(entity);

  // Entity should no longer exist
  assert!(!world.has::<YSort>(entity));
  assert!(!world.has::<Transform>(entity));
}

// ============================================================================
// MULTIPLE ENTITY TESTS
// ============================================================================

/// Test managing many YSort entities (stress test)
#[test]
fn ysort_many_entities() {
  let mut world = World::new();

  let sprite = Sprite::solid_rect(8, 8, 255, 255, 255, 255);
  let sprite_handle = SpriteHandle::from_sprite(sprite, "test");

  // Spawn 100 entities with YSort
  let mut entities = Vec::new();
  for i in 0..100 {
    let entity = world.spawn((
      Transform::new(i as f32 * 10.0, i as f32 * 5.0),
      Renderable::sprite(sprite_handle.clone()),
      YSort::new().with_depth_scale(0.8, 1.2, 0.0, 500.0),
    ));
    entities.push(entity);
  }

  // Verify all entities have YSort
  let ysort_count = world.entities_with::<YSort>().len();
  assert_eq!(ysort_count, 100);

  // Verify we can calculate sort keys for all
  for (i, &entity) in entities.iter().enumerate() {
    let transform = world.get::<Transform>(entity).unwrap();
    let ysort = world.get::<YSort>(entity).unwrap();

    let sort_y = ysort.calculate_sort_y(transform.y, 8.0);
    let expected = (i as f32 * 5.0) + 8.0; // y + sprite_height
    assert_eq!(sort_y, expected);
  }
}

/// Test updating multiple YSort components
#[test]
fn ysort_batch_update() {
  let mut world = World::new();

  // Spawn entities
  for i in 0..10 {
    world.spawn((
      Transform::new(i as f32 * 20.0, i as f32 * 10.0),
      YSort::new(),
    ));
  }

  // Collect entities to update
  let entities_to_update = world.entities_with::<YSort>();

  // Update all YSort components with new settings
  let new_ysort = YSort::new().with_depth_scale(0.7, 1.3, 0.0, 100.0);

  for entity in entities_to_update {
    if let Some(mut ysort) = world.get_mut::<YSort>(entity) {
      *ysort = new_ysort.clone();
    }
  }

  // Verify all were updated
  for entity in world.entities_with::<YSort>() {
    let ysort = world.get::<YSort>(entity).unwrap();
    assert!(ysort.depth_scale_enabled);
    assert_eq!(ysort.scale_min, 0.7);
    assert_eq!(ysort.scale_max, 1.3);
  }
}
