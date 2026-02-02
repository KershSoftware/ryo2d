//! Transform and pivot system integration tests
//!
//! Tests for transform pivots and their effect on rendering.

use ryo2d::prelude::*;

/// Test that default pivot is top-left (0, 0) for backwards compatibility
#[test]
fn default_pivot_is_top_left() {
    let transform = Transform::new(100.0, 200.0);
    assert_eq!(transform.pivot_x, 0.0);
    assert_eq!(transform.pivot_y, 0.0);
}

/// Test pivot builder methods
#[test]
fn pivot_builder_methods() {
    let t = Transform::new(0.0, 0.0).with_pivot_center();
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 0.5);

    let t = Transform::new(0.0, 0.0).with_pivot_top_center();
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 0.0);

    let t = Transform::new(0.0, 0.0).with_pivot_bottom_center();
    assert_eq!(t.pivot_x, 0.5);
    assert_eq!(t.pivot_y, 1.0);

    let t = Transform::new(0.0, 0.0).with_pivot(0.25, 0.75);
    assert_eq!(t.pivot_x, 0.25);
    assert_eq!(t.pivot_y, 0.75);
}

/// Test that top-left pivot (0,0) uses simple positioning (no offset)
#[test]
fn top_left_pivot_no_offset() {
    // TODO: Implement with frame buffer inspection
    // When pivot is (0,0), sprite should render with position as top-left corner
    // No pivot offset calculation should be applied
}

/// Test that center pivot (0.5, 0.5) centers sprite on position
#[test]
fn center_pivot_centers_sprite() {
    // TODO: Implement with frame buffer inspection
    // When pivot is (0.5, 0.5), sprite center should be at transform position
}

/// Test that rotation happens around the pivot point
#[test]
fn rotation_around_pivot() {
    // TODO: Implement with frame buffer inspection
    // Rotation should occur around the pivot point, not the top-left corner
}
