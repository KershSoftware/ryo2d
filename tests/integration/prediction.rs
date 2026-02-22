//! Integration tests for client-side prediction

use ryo2d::core::ecs::{Transform, World};
use ryo2d::core::network::prediction::{
  Predicted, PredictionHelper, ServerPositionUpdate, UpdateResult,
};

fn create_player(world: &mut World, owner: u64) -> hecs::Entity {
  world.spawn((Transform::new(0.0, 0.0), Predicted::new(owner)))
}

fn create_player_spawned(world: &mut World, owner: u64) -> hecs::Entity {
  world.spawn((Transform::new(0.0, 0.0), Predicted::new_spawned(owner)))
}

#[test]
fn full_prediction_workflow() {
  let mut world = World::new();
  let local_id = 1;
  let entity = create_player(&mut world, local_id);

  let helper = PredictionHelper::new(Some(local_id));

  // Step 1: First server update spawns the entity
  let update1 = ServerPositionUpdate::new(local_id, 100.0, 100.0, 0);
  let result = helper.handle_position_update(&mut world, entity, &update1);
  assert_eq!(result, UpdateResult::Spawned);

  // Verify position
  let pos = world.get::<Transform>(entity).unwrap();
  assert_eq!(pos.x, 100.0);
  assert_eq!(pos.y, 100.0);
  drop(pos);

  // Step 2: Client predicts movement
  if let Some(mut t) = world.get_mut::<Transform>(entity) {
    t.x = 120.0; // Client thinks we moved to 120
  }

  // Step 3: Server sends update (we were at 115 at sequence 1)
  let update2 = ServerPositionUpdate::new(local_id, 115.0, 100.0, 1);
  let result = helper.handle_position_update(&mut world, entity, &update2);
  assert_eq!(result, UpdateResult::StoredForReconciliation);

  // Position should still be predicted value (reconciliation happens elsewhere)
  let pos = world.get::<Transform>(entity).unwrap();
  assert_eq!(pos.x, 120.0);
}

#[test]
fn remote_players_always_apply() {
  let mut world = World::new();
  let local_id = 1;
  let remote_id = 2;
  let entity = create_player(&mut world, remote_id);

  let helper = PredictionHelper::new(Some(local_id));

  // First update spawns
  let update1 = ServerPositionUpdate::new(remote_id, 0.0, 0.0, 0);
  let result = helper.handle_position_update(&mut world, entity, &update1);
  assert_eq!(result, UpdateResult::Spawned);

  // All subsequent updates apply directly
  for i in 1..5 {
    let update = ServerPositionUpdate::new(remote_id, i as f32 * 10.0, 0.0, i);
    let result = helper.handle_position_update(&mut world, entity, &update);
    assert_eq!(result, UpdateResult::Applied);

    let pos = world.get::<Transform>(entity).unwrap();
    assert_eq!(pos.x, i as f32 * 10.0);
  }
}

#[test]
fn respawn_forces_correction() {
  let mut world = World::new();
  let local_id = 1;
  let entity = create_player_spawned(&mut world, local_id);

  let helper = PredictionHelper::new(Some(local_id));

  // Move somewhere
  if let Some(mut t) = world.get_mut::<Transform>(entity) {
    t.x = 500.0;
    t.y = 500.0;
  }

  // Respawn: force correction to spawn point
  let spawn_update = ServerPositionUpdate::new(local_id, 0.0, 0.0, 100);
  helper.force_correction(&mut world, entity, &spawn_update);

  // Should be at spawn point, not predicted position
  let pos = world.get::<Transform>(entity).unwrap();
  assert_eq!(pos.x, 0.0);
  assert_eq!(pos.y, 0.0);
}

#[test]
fn multiple_players() {
  let mut world = World::new();
  let local_id = 1;

  // Create local player and 3 remote players
  let local_player = create_player(&mut world, local_id);
  let remote1 = create_player(&mut world, 2);
  let remote2 = create_player(&mut world, 3);
  let remote3 = create_player(&mut world, 4);

  let helper = PredictionHelper::new(Some(local_id));

  // Spawn all players
  for (entity, id) in [
    (local_player, local_id),
    (remote1, 2),
    (remote2, 3),
    (remote3, 4),
  ] {
    let update = ServerPositionUpdate::new(id, id as f32 * 100.0, 0.0, 0);
    let result = helper.handle_position_update(&mut world, entity, &update);
    assert_eq!(result, UpdateResult::Spawned);
  }

  // Update all players
  for (entity, id) in [
    (local_player, local_id),
    (remote1, 2),
    (remote2, 3),
    (remote3, 4),
  ] {
    let update = ServerPositionUpdate::new(id, id as f32 * 100.0 + 50.0, 0.0, 1);
    let result = helper.handle_position_update(&mut world, entity, &update);

    if id == local_id {
      assert_eq!(result, UpdateResult::StoredForReconciliation);
    } else {
      assert_eq!(result, UpdateResult::Applied);
    }
  }
}

#[test]
fn server_helper_no_local_client() {
  let mut world = World::new();
  let entity = create_player(&mut world, 1);

  // Server doesn't have a local client
  let helper = PredictionHelper::new(None);

  // All updates should apply directly
  let update1 = ServerPositionUpdate::new(1, 100.0, 100.0, 0);
  let result = helper.handle_position_update(&mut world, entity, &update1);
  assert_eq!(result, UpdateResult::Spawned);

  let update2 = ServerPositionUpdate::new(1, 200.0, 200.0, 1);
  let result = helper.handle_position_update(&mut world, entity, &update2);
  assert_eq!(result, UpdateResult::Applied); // No local client, applies directly
}

#[test]
fn position_update_with_velocity() {
  let update = ServerPositionUpdate::new(1, 100.0, 200.0, 5).with_velocity(10.0, -5.0);

  assert_eq!(update.x, 100.0);
  assert_eq!(update.y, 200.0);
  assert_eq!(update.sequence, 5);
  assert_eq!(update.velocity_x, Some(10.0));
  assert_eq!(update.velocity_y, Some(-5.0));
}

#[test]
fn predicted_component_ownership() {
  let pred = Predicted::new(42);
  assert!(pred.is_owned_by(42));
  assert!(!pred.is_owned_by(1));
  assert!(!pred.is_owned_by(0));
}
