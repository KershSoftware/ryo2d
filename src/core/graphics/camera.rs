/// Camera shake state for impact feedback.
///
/// Uses a trauma-based system where trauma accumulates from impacts
/// and decays over time. Shake intensity is trauma² for better feel.
#[derive(Debug, Clone)]
pub struct CameraShake {
  /// Current trauma level (0.0 to 1.0, clamped)
  trauma: f32,
  /// Maximum pixel offset at full trauma (default: 10.0)
  max_offset: f32,
  /// Trauma decay rate per second (default: 1.0)
  decay_rate: f32,
  /// Time accumulator for smooth noise sampling
  noise_time: f32,
  /// Current shake offset in X (pixels)
  offset_x: f32,
  /// Current shake offset in Y (pixels)
  offset_y: f32,
}

impl Default for CameraShake {
  fn default() -> Self {
    Self {
      trauma: 0.0,
      max_offset: 10.0,
      decay_rate: 1.0,
      noise_time: 0.0,
      offset_x: 0.0,
      offset_y: 0.0,
    }
  }
}

impl CameraShake {
  /// Create shake with custom parameters.
  pub fn new(max_offset: f32, decay_rate: f32) -> Self {
    Self {
      max_offset,
      decay_rate,
      ..Default::default()
    }
  }

  /// Smooth noise function using layered sine waves.
  /// Returns value in range [-1.0, 1.0].
  fn noise(t: f32, seed: f32) -> f32 {
    // Layer multiple frequencies for more organic feel
    let n1 = (t * 7.0 + seed).sin();
    let n2 = (t * 13.0 + seed * 2.3).sin() * 0.5;
    let n3 = (t * 23.0 + seed * 5.7).sin() * 0.25;
    (n1 + n2 + n3) / 1.75 // Normalize to [-1, 1]
  }

  /// Update shake state and calculate new offsets.
  fn update(&mut self, dt: f32) {
    if self.trauma <= 0.0 {
      self.offset_x = 0.0;
      self.offset_y = 0.0;
      return;
    }

    // Advance noise time
    self.noise_time += dt * 10.0;

    // Calculate intensity (trauma squared for better feel)
    let intensity = self.trauma * self.trauma;

    // Calculate offsets using smooth noise
    self.offset_x = Self::noise(self.noise_time, 0.0) * self.max_offset * intensity;
    self.offset_y = Self::noise(self.noise_time, 100.0) * self.max_offset * intensity;

    // Decay trauma
    self.trauma = (self.trauma - self.decay_rate * dt).max(0.0);
  }
}

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
  /// Camera shake state
  shake: CameraShake,
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
      shake: CameraShake::default(),
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
      shake: CameraShake::default(),
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
  ///
  /// Camera shake offset is automatically applied.
  pub fn world_to_screen(&self, world_x: f32, world_y: f32) -> (f32, f32) {
    let half_w = self.viewport_width / 2.0;
    let half_h = self.viewport_height / 2.0;

    // Apply shake offset to camera position
    let screen_x = (world_x - self.x) * self.zoom + half_w + self.shake.offset_x;
    let screen_y = (world_y - self.y) * self.zoom + half_h + self.shake.offset_y;

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

  // ===========================================================================
  // Camera Shake
  // ===========================================================================

  /// Add trauma to trigger camera shake.
  ///
  /// Trauma values are clamped to 0.0-1.0 and accumulate.
  /// Use small values (0.1-0.3) for minor impacts, larger values (0.5-0.8) for
  /// explosions or significant events.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // On player hit
  /// ctx.camera.add_trauma(0.3);
  ///
  /// // On explosion
  /// ctx.camera.add_trauma(0.7);
  /// ```
  pub fn add_trauma(&mut self, amount: f32) {
    self.shake.trauma = (self.shake.trauma + amount).clamp(0.0, 1.0);
  }

  /// Configure camera shake parameters.
  ///
  /// # Arguments
  ///
  /// * `max_offset` - Maximum pixel offset at full trauma (default: 10.0)
  /// * `decay_rate` - How fast trauma decays per second (default: 1.0)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Stronger shake that decays slowly
  /// ctx.camera.configure_shake(20.0, 0.5);
  /// ```
  pub fn configure_shake(&mut self, max_offset: f32, decay_rate: f32) {
    self.shake.max_offset = max_offset;
    self.shake.decay_rate = decay_rate;
  }

  /// Get the current trauma level (0.0 to 1.0).
  pub fn trauma(&self) -> f32 {
    self.shake.trauma
  }

  /// Get the current shake offset in pixels.
  ///
  /// Returns (offset_x, offset_y). Useful for custom rendering that needs
  /// to account for shake.
  pub fn shake_offset(&self) -> (f32, f32) {
    (self.shake.offset_x, self.shake.offset_y)
  }

  /// Update camera shake state.
  ///
  /// Called internally by the engine each frame with real (unscaled) delta time.
  /// Game code should not call this directly.
  pub(crate) fn update_shake(&mut self, dt: f32) {
    self.shake.update(dt);
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

  // ===========================================================================
  // Camera Shake Tests
  // ===========================================================================

  #[test]
  fn shake_starts_at_zero() {
    let cam = Camera::new(800, 600);
    assert_eq!(cam.trauma(), 0.0);
    assert_eq!(cam.shake_offset(), (0.0, 0.0));
  }

  #[test]
  fn add_trauma_accumulates() {
    let mut cam = Camera::new(800, 600);
    cam.add_trauma(0.3);
    assert!((cam.trauma() - 0.3).abs() < 0.001);

    cam.add_trauma(0.4);
    assert!((cam.trauma() - 0.7).abs() < 0.001);
  }

  #[test]
  fn trauma_clamps_to_one() {
    let mut cam = Camera::new(800, 600);
    cam.add_trauma(0.8);
    cam.add_trauma(0.5);
    assert_eq!(cam.trauma(), 1.0);
  }

  #[test]
  fn shake_decays_over_time() {
    let mut cam = Camera::new(800, 600);
    cam.add_trauma(1.0);
    assert_eq!(cam.trauma(), 1.0);

    // Decay at default rate (1.0 per second), 0.5 seconds
    cam.update_shake(0.5);
    assert!((cam.trauma() - 0.5).abs() < 0.001);

    // Another 0.5 seconds should bring it to zero
    cam.update_shake(0.5);
    assert_eq!(cam.trauma(), 0.0);
  }

  #[test]
  fn shake_produces_offset_when_active() {
    let mut cam = Camera::new(800, 600);
    cam.add_trauma(1.0);
    cam.update_shake(0.1);

    let (ox, oy) = cam.shake_offset();
    // Offset should be non-zero when trauma is active
    assert!(ox.abs() > 0.001 || oy.abs() > 0.001);
  }

  #[test]
  fn shake_offset_zero_when_no_trauma() {
    let mut cam = Camera::new(800, 600);
    cam.update_shake(0.1);

    assert_eq!(cam.shake_offset(), (0.0, 0.0));
  }

  #[test]
  fn configure_shake_changes_parameters() {
    let mut cam = Camera::new(800, 600);
    cam.configure_shake(20.0, 2.0);
    cam.add_trauma(1.0);

    // With decay_rate 2.0, 0.5 seconds should fully decay
    cam.update_shake(0.5);
    assert_eq!(cam.trauma(), 0.0);
  }

  #[test]
  fn world_to_screen_includes_shake_offset() {
    let mut cam = Camera::new(800, 600);
    cam.add_trauma(1.0);
    cam.update_shake(0.05);

    let (ox, oy) = cam.shake_offset();
    let (sx, sy) = cam.world_to_screen(0.0, 0.0);

    // Screen position should include shake offset
    // Without shake: center is (400, 300)
    assert!((sx - 400.0 - ox).abs() < 0.001);
    assert!((sy - 300.0 - oy).abs() < 0.001);
  }
}
