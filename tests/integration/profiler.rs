//! Profiler integration tests
//!
//! Tests for profiler <-> engine integration.
//! These tests verify that profiling data flows correctly between
//! the engine and profiler systems.

use ryo2d::core::profiler::{ScopeCategory, strip_scope_prefix};

/// Test that engine scope naming conventions are correctly categorized.
#[test]
fn test_engine_scope_conventions() {
  // Engine CPU scopes should use cpu: prefix
  let cpu_scopes = [
    "cpu:engine_update",
    "cpu:engine_render",
    "cpu:physics_step",
    "cpu:animation_update",
    "cpu:tween_update",
    "cpu:collision_detection",
  ];

  for scope in cpu_scopes {
    assert_eq!(
      ScopeCategory::from_scope_name(scope),
      ScopeCategory::EngineCpu,
      "Scope '{}' should be EngineCpu",
      scope
    );
  }

  // Engine GPU scopes should use gpu: prefix
  let gpu_scopes = [
    "gpu:sprite_batching",
    "gpu:submit_draw_calls",
    "gpu:lighting_pass",
    "gpu:post_process",
  ];

  for scope in gpu_scopes {
    assert_eq!(
      ScopeCategory::from_scope_name(scope),
      ScopeCategory::EngineGpu,
      "Scope '{}' should be EngineGpu",
      scope
    );
  }
}

/// Test that game developer scopes (without prefix) are categorized as Game.
#[test]
fn test_game_scope_conventions() {
  // Game scopes should NOT have prefixes
  let game_scopes = [
    "player_update",
    "enemy_ai",
    "physics_sim",
    "spawn_bullets",
    "check_collisions",
    "render_ui",
    "ai_pathfinding",
  ];

  for scope in game_scopes {
    assert_eq!(
      ScopeCategory::from_scope_name(scope),
      ScopeCategory::Game,
      "Scope '{}' should be Game",
      scope
    );
  }
}

/// Test prefix stripping for display names.
#[test]
fn test_display_name_stripping() {
  // CPU prefix should be stripped
  assert_eq!(strip_scope_prefix("cpu:engine_update"), "engine_update");
  assert_eq!(strip_scope_prefix("cpu:physics"), "physics");

  // GPU prefix should be stripped
  assert_eq!(strip_scope_prefix("gpu:render"), "render");
  assert_eq!(strip_scope_prefix("gpu:batching"), "batching");

  // Game scopes should pass through unchanged
  assert_eq!(strip_scope_prefix("player_update"), "player_update");
  assert_eq!(strip_scope_prefix("my_custom_scope"), "my_custom_scope");
}

/// Test edge cases in scope naming.
#[test]
fn test_scope_edge_cases() {
  // Partial prefix matches should NOT trigger categorization
  assert_eq!(
    ScopeCategory::from_scope_name("cpubound_work"),
    ScopeCategory::Game
  );
  assert_eq!(
    ScopeCategory::from_scope_name("gpuish_rendering"),
    ScopeCategory::Game
  );

  // Case sensitivity - prefixes are lowercase only
  assert_eq!(
    ScopeCategory::from_scope_name("CPU:update"),
    ScopeCategory::Game
  );
  assert_eq!(
    ScopeCategory::from_scope_name("GPU:render"),
    ScopeCategory::Game
  );

  // Empty and whitespace
  assert_eq!(ScopeCategory::from_scope_name(""), ScopeCategory::Game);
  assert_eq!(
    ScopeCategory::from_scope_name("   "),
    ScopeCategory::Game
  );

  // Just the prefix (edge case)
  assert_eq!(
    ScopeCategory::from_scope_name("cpu:"),
    ScopeCategory::EngineCpu
  );
  assert_eq!(
    ScopeCategory::from_scope_name("gpu:"),
    ScopeCategory::EngineGpu
  );
}

/// Test that prefix stripping handles edge cases.
#[test]
fn test_strip_prefix_edge_cases() {
  // Just the prefix
  assert_eq!(strip_scope_prefix("cpu:"), "");
  assert_eq!(strip_scope_prefix("gpu:"), "");

  // No prefix - passthrough
  assert_eq!(strip_scope_prefix(""), "");
  assert_eq!(strip_scope_prefix("normal_scope"), "normal_scope");

  // Nested colons
  assert_eq!(strip_scope_prefix("cpu:render:sprites"), "render:sprites");
  assert_eq!(strip_scope_prefix("gpu:pass:lighting"), "pass:lighting");
}

#[cfg(feature = "profiler")]
mod with_profiler {
  //! Tests that require the profiler feature to be enabled.
  //!
  //! Run with: cargo test --test integration --features profiler

  use ryo2d::{profile_function, profile_scope};

  /// Test that profile macros can be used in test code.
  #[test]
  fn test_profile_macros_in_tests() {
    profile_function!();

    {
      profile_scope!("test_inner_scope");
      // Do some work
      let _sum: u32 = (0..100).sum();
    }

    {
      profile_scope!("cpu:test_engine_like_scope");
      let _sum: u32 = (0..100).sum();
    }

    {
      profile_scope!("gpu:test_gpu_like_scope");
      let _sum: u32 = (0..100).sum();
    }
  }

  /// Test that puffin is collecting scopes when enabled.
  #[test]
  fn test_puffin_scope_collection() {
    use ryo2d::core::profiler;

    // Initialize profiler
    profiler::init(true);

    // Start a new frame
    profiler::new_frame();

    // Create some scopes
    {
      profile_scope!("test_collection_scope");
      std::hint::black_box((0..1000).sum::<u32>());
    }

    // End frame
    profiler::new_frame();

    // Verify puffin is enabled
    assert!(puffin::are_scopes_on());
  }
}
