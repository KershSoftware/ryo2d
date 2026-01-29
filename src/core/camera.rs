/// 2D Camera for world-to-screen transformations
#[derive(Debug, Clone)]
pub struct Camera {
  /// Camera position in world space (center of view)
  pub x: f32,
  pub y: f32,
  /// Zoom level (1.0 = normal, 2.0 = zoomed in 2x, 0.5 = zoomed out)
  pub zoom: f32,
  /// Viewport dimensions (screen size)
  viewport_width: f32,
  viewport_height: f32,
}

impl Camera {
  /// Create a new camera centered at origin
  pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
    Camera {
      x: 0.0,
      y: 0.0,
      zoom: 1.0,
      viewport_width: viewport_width as f32,
      viewport_height: viewport_height as f32,
    }
  }

  /// Create a camera centered on a specific position
  pub fn centered_at(x: f32, y: f32, viewport_width: u32, viewport_height: u32) -> Self {
    Camera {
      x,
      y,
      zoom: 1.0,
      viewport_width: viewport_width as f32,
      viewport_height: viewport_height as f32,
    }
  }

  /// Set the camera position
  pub fn set_position(&mut self, x: f32, y: f32) {
    self.x = x;
    self.y = y;
  }

  /// Set zoom level (clamped to reasonable range)
  pub fn set_zoom(&mut self, zoom: f32) {
    self.zoom = zoom.clamp(0.1, 10.0);
  }

  /// Get viewport dimensions
  pub fn viewport_size(&self) -> (f32, f32) {
    (self.viewport_width, self.viewport_height)
  }

  /// Update viewport when window resizes
  pub fn set_viewport(&mut self, width: u32, height: u32) {
    self.viewport_width = width as f32;
    self.viewport_height = height as f32;
  }

  /// Convert world coordinates to screen coordinates
  ///
  /// The camera position represents the center of the view, so:
  /// - Objects at camera position appear at screen center
  /// - Objects to the left/above camera appear to the left/above center
  pub fn world_to_screen(&self, world_x: f32, world_y: f32) -> (f32, f32) {
    let half_w = self.viewport_width / 2.0;
    let half_h = self.viewport_height / 2.0;

    let screen_x = (world_x - self.x) * self.zoom + half_w;
    let screen_y = (world_y - self.y) * self.zoom + half_h;

    (screen_x, screen_y)
  }

  /// Convert screen coordinates to world coordinates
  ///
  /// Useful for mouse picking, clicking on objects, etc.
  pub fn screen_to_world(&self, screen_x: f32, screen_y: f32) -> (f32, f32) {
    let half_w = self.viewport_width / 2.0;
    let half_h = self.viewport_height / 2.0;

    let world_x = (screen_x - half_w) / self.zoom + self.x;
    let world_y = (screen_y - half_h) / self.zoom + self.y;

    (world_x, world_y)
  }

  /// Get the visible world bounds (AABB) at current zoom
  pub fn visible_bounds(&self) -> CameraBounds {
    let half_w = self.viewport_width / (2.0 * self.zoom);
    let half_h = self.viewport_height / (2.0 * self.zoom);

    CameraBounds {
      left: self.x - half_w,
      right: self.x + half_w,
      top: self.y - half_h,
      bottom: self.y + half_h,
    }
  }

  /// Check if a world-space rectangle is visible on screen
  pub fn is_visible(&self, world_x: f32, world_y: f32, width: f32, height: f32) -> bool {
    let bounds = self.visible_bounds();
    world_x + width > bounds.left
      && world_x < bounds.right
      && world_y + height > bounds.top
      && world_y < bounds.bottom
  }

  /// Smoothly follow a target position (for camera tracking)
  ///
  /// `lerp_factor` controls smoothness: 0.0 = no movement, 1.0 = instant snap
  /// Typical values: 0.05 to 0.2 for smooth follow
  pub fn follow(&mut self, target_x: f32, target_y: f32, lerp_factor: f32) {
    let factor = lerp_factor.clamp(0.0, 1.0);
    self.x += (target_x - self.x) * factor;
    self.y += (target_y - self.y) * factor;
  }

  /// Instantly snap camera to target position
  pub fn snap_to(&mut self, target_x: f32, target_y: f32) {
    self.x = target_x;
    self.y = target_y;
  }

  /// Clamp camera position to stay within world bounds
  ///
  /// Useful to prevent the camera from showing areas outside the game world.
  /// The bounds represent the minimum visible area (camera won't go beyond edges).
  pub fn clamp_to_bounds(
    &mut self,
    world_min_x: f32,
    world_min_y: f32,
    world_max_x: f32,
    world_max_y: f32,
  ) {
    let half_w = self.viewport_width / (2.0 * self.zoom);
    let half_h = self.viewport_height / (2.0 * self.zoom);

    // Camera center must be at least half-viewport away from world edges
    self.x = self.x.clamp(world_min_x + half_w, world_max_x - half_w);
    self.y = self.y.clamp(world_min_y + half_h, world_max_y - half_h);
  }
}

/// Represents the visible world-space bounds of the camera
#[derive(Debug, Clone, Copy)]
pub struct CameraBounds {
  pub left: f32,
  pub right: f32,
  pub top: f32,
  pub bottom: f32,
}

impl CameraBounds {
  pub fn width(&self) -> f32 {
    self.right - self.left
  }

  pub fn height(&self) -> f32 {
    self.bottom - self.top
  }

  pub fn contains(&self, x: f32, y: f32) -> bool {
    x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn camera_new_defaults() {
    let cam = Camera::new(800, 600);
    assert_eq!(cam.x, 0.0);
    assert_eq!(cam.y, 0.0);
    assert_eq!(cam.zoom, 1.0);
    assert_eq!(cam.viewport_size(), (800.0, 600.0));
  }

  #[test]
  fn camera_centered_at() {
    let cam = Camera::centered_at(100.0, 50.0, 800, 600);
    assert_eq!(cam.x, 100.0);
    assert_eq!(cam.y, 50.0);
  }

  #[test]
  fn world_to_screen_at_origin() {
    let cam = Camera::new(800, 600);
    // Camera at origin, looking at object at origin -> center of screen
    let (sx, sy) = cam.world_to_screen(0.0, 0.0);
    assert_eq!(sx, 400.0); // half of 800
    assert_eq!(sy, 300.0); // half of 600
  }

  #[test]
  fn world_to_screen_offset() {
    let cam = Camera::new(800, 600);
    // Object at (100, 50) with camera at origin
    let (sx, sy) = cam.world_to_screen(100.0, 50.0);
    assert_eq!(sx, 500.0); // 400 + 100
    assert_eq!(sy, 350.0); // 300 + 50
  }

  #[test]
  fn world_to_screen_with_camera_offset() {
    let cam = Camera::centered_at(100.0, 100.0, 800, 600);
    // Object at (100, 100) with camera centered there -> screen center
    let (sx, sy) = cam.world_to_screen(100.0, 100.0);
    assert_eq!(sx, 400.0);
    assert_eq!(sy, 300.0);
  }

  #[test]
  fn screen_to_world_roundtrip() {
    let cam = Camera::centered_at(150.0, 75.0, 800, 600);
    let world_pos = (200.0, 100.0);
    let screen_pos = cam.world_to_screen(world_pos.0, world_pos.1);
    let back = cam.screen_to_world(screen_pos.0, screen_pos.1);

    assert!((back.0 - world_pos.0).abs() < 0.001);
    assert!((back.1 - world_pos.1).abs() < 0.001);
  }

  #[test]
  fn zoom_affects_transform() {
    let mut cam = Camera::new(800, 600);
    cam.set_zoom(2.0);

    // Object at (100, 0) should appear further from center when zoomed
    let (sx, _) = cam.world_to_screen(100.0, 0.0);
    assert_eq!(sx, 600.0); // 400 + 100*2
  }

  #[test]
  fn visible_bounds_at_origin() {
    let cam = Camera::new(800, 600);
    let bounds = cam.visible_bounds();

    assert_eq!(bounds.left, -400.0);
    assert_eq!(bounds.right, 400.0);
    assert_eq!(bounds.top, -300.0);
    assert_eq!(bounds.bottom, 300.0);
    assert_eq!(bounds.width(), 800.0);
    assert_eq!(bounds.height(), 600.0);
  }

  #[test]
  fn visible_bounds_with_zoom() {
    let mut cam = Camera::new(800, 600);
    cam.set_zoom(2.0);
    let bounds = cam.visible_bounds();

    // Zoomed in 2x means we see half the world area
    assert_eq!(bounds.width(), 400.0);
    assert_eq!(bounds.height(), 300.0);
  }

  #[test]
  fn is_visible_onscreen() {
    let cam = Camera::new(800, 600);
    assert!(cam.is_visible(0.0, 0.0, 32.0, 32.0));
    assert!(cam.is_visible(-300.0, -200.0, 50.0, 50.0));
  }

  #[test]
  fn is_visible_offscreen() {
    let cam = Camera::new(800, 600);
    // Completely off to the left
    assert!(!cam.is_visible(-500.0, 0.0, 32.0, 32.0));
    // Completely below
    assert!(!cam.is_visible(0.0, 400.0, 32.0, 32.0));
  }

  #[test]
  fn follow_moves_toward_target() {
    let mut cam = Camera::new(800, 600);
    cam.follow(100.0, 100.0, 0.5);

    assert_eq!(cam.x, 50.0);
    assert_eq!(cam.y, 50.0);
  }

  #[test]
  fn snap_to_instant() {
    let mut cam = Camera::new(800, 600);
    cam.snap_to(500.0, 250.0);

    assert_eq!(cam.x, 500.0);
    assert_eq!(cam.y, 250.0);
  }

  #[test]
  fn zoom_clamps_to_range() {
    let mut cam = Camera::new(800, 600);
    cam.set_zoom(0.01);
    assert_eq!(cam.zoom, 0.1);

    cam.set_zoom(100.0);
    assert_eq!(cam.zoom, 10.0);
  }

  #[test]
  fn bounds_contains() {
    let bounds = CameraBounds {
      left: 0.0,
      right: 100.0,
      top: 0.0,
      bottom: 100.0,
    };

    assert!(bounds.contains(50.0, 50.0));
    assert!(bounds.contains(0.0, 0.0));
    assert!(!bounds.contains(-1.0, 50.0));
    assert!(!bounds.contains(50.0, 101.0));
  }
}
