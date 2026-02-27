//! Trails Demo - Ryo2D Example
//!
//! Six discrete demo pages, each showcasing one trail feature clearly.
//! Navigate with arrow keys or number keys 1-6.
//!
//! ## Pages
//!
//! 1. **Presets** — Mouse trail + 4 orbiting trails, 1-6 switches preset
//! 2. **Fade Modes** — 4 orbiting trails with Position/Time/Distance/None fade
//! 3. **Styles** — 3 trails showing Solid/Soft/Glow rendering styles
//! 4. **Caps & Joins** — Line2D polylines and polygons with all cap/join combos
//! 5. **Textured** — Stretch vs Tile texture modes on orbiting trails
//! 6. **GPU Effects** — Lightning/Noise/Ribbon displacement effects

use ryo2d::prelude::*;

// =============================================================================
// Demo Page Enum
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoPage {
  Presets,
  FadeModes,
  Styles,
  CapsJoins,
  Textured,
  GpuEffects,
}

impl DemoPage {
  const ALL: [DemoPage; 6] = [
    DemoPage::Presets,
    DemoPage::FadeModes,
    DemoPage::Styles,
    DemoPage::CapsJoins,
    DemoPage::Textured,
    DemoPage::GpuEffects,
  ];

  fn index(&self) -> usize {
    Self::ALL.iter().position(|p| p == self).unwrap_or(0)
  }

  fn from_index(idx: usize) -> Self {
    Self::ALL.get(idx).copied().unwrap_or(DemoPage::Presets)
  }

  fn name(&self) -> &'static str {
    match self {
      DemoPage::Presets => "Presets",
      DemoPage::FadeModes => "Fade Modes",
      DemoPage::Styles => "Styles & Visual Quality",
      DemoPage::CapsJoins => "Caps & Joins",
      DemoPage::Textured => "Textured Trails",
      DemoPage::GpuEffects => "GPU Effects",
    }
  }

  fn description(&self) -> &'static str {
    match self {
      DemoPage::Presets => "1-6: Switch preset | Space: Pause | R: Reset",
      DemoPage::FadeModes => "4 trails side-by-side | Up/Down: Adjust lifetime & distance",
      DemoPage::Styles => "Solid / Soft / Glow rendering styles (wider = more visible difference)",
      DemoPage::CapsJoins => {
        "Caps (left) + Joins (right) | Mouse trail: C to cycle cap"
      }
      DemoPage::Textured => "Stretch (left) vs Tile (right) texture mapping on orbiting trails",
      DemoPage::GpuEffects => "Lightning / Noise / Ribbon displacement effects",
    }
  }

  fn next(&self) -> Self {
    Self::from_index((self.index() + 1) % Self::ALL.len())
  }

  fn prev(&self) -> Self {
    let idx = if self.index() == 0 {
      Self::ALL.len() - 1
    } else {
      self.index() - 1
    };
    Self::from_index(idx)
  }
}

// =============================================================================
// Marker Components
// =============================================================================

#[derive(Debug)]
struct MouseTrail;

#[derive(Debug)]
struct OrbitTrail {
  angle: f32,
  radius: f32,
  speed: f32,
  center_x: f32,
  center_y: f32,
}

// =============================================================================
// Trail Preset Enum (for Page 1)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrailPreset {
  Bullet,
  Laser,
  Swipe,
  Lightning,
  Fire,
  Energy,
}

impl TrailPreset {
  fn name(&self) -> &'static str {
    match self {
      TrailPreset::Bullet => "Bullet",
      TrailPreset::Laser => "Laser",
      TrailPreset::Swipe => "Swipe",
      TrailPreset::Lightning => "Lightning",
      TrailPreset::Fire => "Fire",
      TrailPreset::Energy => "Energy",
    }
  }

  fn create(&self, color: Color, width: f32) -> Trail2D {
    match self {
      TrailPreset::Bullet => Trail2D::bullet(color),
      TrailPreset::Laser => Trail2D::laser(color, width),
      TrailPreset::Swipe => Trail2D::swipe(color, width),
      TrailPreset::Lightning => Trail2D::lightning(color, width),
      TrailPreset::Fire => Trail2D::fire(color, width),
      TrailPreset::Energy => Trail2D::energy(color, width),
    }
  }
}

// =============================================================================
// Helpers
// =============================================================================

fn cap_name(cap: LineCap) -> &'static str {
  match cap {
    LineCap::Butt => "Butt",
    LineCap::Round => "Round",
    LineCap::Square => "Square",
  }
}

fn circle_sprite(radius: u32, color: Color) -> SpriteHandle {
  let sprite = Sprite::solid_circle(radius, color.r, color.g, color.b, color.a);
  SpriteHandle::from_sprite(sprite, format!("circle_{}_{}", radius, color.r))
}

/// Create a gradient texture for textured trails
fn create_trail_texture() -> SpriteHandle {
  let width = 64u32;
  let height = 32u32;
  let mut data = vec![0u8; (width * height * 4) as usize];

  for y in 0..height {
    for x in 0..width {
      let idx = ((y * width + x) * 4) as usize;
      let t = x as f32 / (width - 1) as f32;

      let (r, g, b) = if t < 0.33 {
        let lt = t / 0.33;
        ((255.0 * (1.0 - lt)) as u8, (255.0 * lt) as u8, 255u8)
      } else if t < 0.66 {
        let lt = (t - 0.33) / 0.33;
        ((255.0 * lt) as u8, 255u8, (255.0 * (1.0 - lt)) as u8)
      } else {
        let lt = (t - 0.66) / 0.34;
        (255u8, (255.0 * (1.0 - lt)) as u8, (255.0 * lt) as u8)
      };

      let vy = 1.0 - ((y as f32 / (height - 1) as f32 - 0.5).abs() * 2.0).powf(2.0);
      let brightness = 0.5 + 0.5 * vy;

      data[idx] = (r as f32 * brightness) as u8;
      data[idx + 1] = (g as f32 * brightness) as u8;
      data[idx + 2] = (b as f32 * brightness) as u8;
      data[idx + 3] = 255;
    }
  }

  let sprite = Sprite::from_rgba(data, width, height);
  SpriteHandle::from_sprite(sprite, "trail_gradient")
}


// =============================================================================
// Main Demo Scene
// =============================================================================

struct TrailsDemoScene {
  current_page: DemoPage,
  entities: Vec<Entity>,
  nav_cooldown: f32,
  paused: bool,
  screen_width: f32,
  screen_height: f32,

  // Page 1: Presets
  current_preset: TrailPreset,

  // Page 2: Fade Modes
  lifetime: f32,
  fade_distance: f32,

  // Page 4: Caps & Joins (mouse trail cycling)
  current_cap: LineCap,
  current_join: LineJoin,

  // Page 5: Textured
  texture_sprite: Option<SpriteHandle>,
}

impl TrailsDemoScene {
  fn new() -> Self {
    Self {
      current_page: DemoPage::Presets,
      entities: Vec::new(),
      nav_cooldown: 0.0,
      paused: false,
      screen_width: 0.0,
      screen_height: 0.0,
      current_preset: TrailPreset::Bullet,
      lifetime: 0.5,
      fade_distance: 100.0,
      current_cap: LineCap::Round,
      current_join: LineJoin::Round,
      texture_sprite: None,
    }
  }

  // ===========================================================================
  // Page lifecycle
  // ===========================================================================

  fn clear_page(&mut self, world: &mut World) {
    for entity in self.entities.drain(..) {
      let _ = world.despawn(entity);
    }
  }

  fn setup_page(&mut self, ctx: &mut SceneContext) {
    match self.current_page {
      DemoPage::Presets => self.setup_presets(ctx),
      DemoPage::FadeModes => self.setup_fade_modes(ctx),
      DemoPage::Styles => self.setup_styles(ctx),
      DemoPage::CapsJoins => self.setup_caps_joins(ctx),
      DemoPage::Textured => self.setup_textured(ctx),
      DemoPage::GpuEffects => self.setup_gpu_effects(ctx),
    }
  }

  fn switch_page(&mut self, ctx: &mut SceneContext, page: DemoPage) {
    if page == self.current_page {
      return;
    }
    self.clear_page(ctx.world);
    self.current_page = page;
    self.paused = false;
    self.setup_page(ctx);
  }

  // ===========================================================================
  // Shared orbit spawn helper
  // ===========================================================================

  #[allow(clippy::too_many_arguments)]
  fn spawn_orbit(
    &mut self,
    ctx: &mut SceneContext,
    cx: f32,
    cy: f32,
    radius: f32,
    speed: f32,
    start_angle: f32,
    color: Color,
    trail: Trail2D,
  ) -> Entity {
    let x = cx + start_angle.cos() * radius;
    let y = cy + start_angle.sin() * radius;

    let entity = ctx.world.spawn((
      Transform::new(x, y).with_pivot(0.5, 0.5),
      Renderable::sprite(circle_sprite(6, color)),
      trail,
      OrbitTrail {
        angle: start_angle,
        radius,
        speed,
        center_x: cx,
        center_y: cy,
      },
    ));
    self.entities.push(entity);
    entity
  }

  // ===========================================================================
  // Page 1: Presets
  // ===========================================================================

  fn setup_presets(&mut self, ctx: &mut SceneContext) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Mouse-controlled trail — uses preset's natural parameters
    let (mx, my) = ctx.input.mouse_position();
    let mouse_color = Color::rgb(100, 200, 255);
    let trail = self.current_preset.create(mouse_color, 8.0);

    let entity = ctx.world.spawn((
      Transform::new(mx, my).with_pivot(0.5, 0.5),
      Renderable::sprite(circle_sprite(8, mouse_color)),
      trail,
      MouseTrail,
    ));
    self.entities.push(entity);

    // 4 orbiting trails — wide spacing to avoid visual overlap with effect presets
    let configs: [(f32, f32, Color); 4] = [
      (150.0, 1.5, Color::rgb(255, 100, 100)),
      (220.0, -1.0, Color::rgb(100, 255, 100)),
      (290.0, 0.7, Color::rgb(255, 200, 50)),
      (360.0, -0.5, Color::rgb(200, 100, 255)),
    ];

    for (i, (radius, speed, color)) in configs.iter().enumerate() {
      let angle = i as f32 * std::f32::consts::FRAC_PI_2;
      let trail = self.current_preset.create(*color, 8.0);
      self.spawn_orbit(ctx, cx, cy, *radius, *speed, angle, *color, trail);
    }
  }

  fn recreate_presets(&mut self, ctx: &mut SceneContext) {
    self.clear_page(ctx.world);
    self.setup_presets(ctx);
  }

  // ===========================================================================
  // Page 2: Fade Modes
  // ===========================================================================

  fn setup_fade_modes(&mut self, ctx: &mut SceneContext) {
    let cy = self.screen_height / 2.0;
    let spacing = self.screen_width / 4.0;
    let width = 8.0;

    // Position fade: long trail, alpha gradient from head to tail
    let trail_pos = Trail2D::new(Color::rgb(100, 200, 255), width)
      .with_length(60)
      .with_fade(true);
    self.spawn_orbit(
      ctx,
      spacing * 0.5,
      cy,
      80.0,
      1.5,
      0.0,
      Color::rgb(100, 200, 255),
      trail_pos,
    );

    // Time fade: points expire after lifetime — trail is visibly shorter
    let trail_time = Trail2D::new(Color::rgb(255, 200, 80), width)
      .with_length(60)
      .with_fade(false)
      .with_lifetime(self.lifetime);
    self.spawn_orbit(
      ctx,
      spacing * 1.5,
      cy,
      80.0,
      1.5,
      std::f32::consts::FRAC_PI_2,
      Color::rgb(255, 200, 80),
      trail_time,
    );

    // Distance fade: fades over a set pixel distance from head
    let trail_dist = Trail2D::new(Color::rgb(200, 100, 255), width)
      .with_length(60)
      .with_fade(true)
      .with_fade_distance(self.fade_distance);
    self.spawn_orbit(
      ctx,
      spacing * 2.5,
      cy,
      80.0,
      1.5,
      std::f32::consts::PI,
      Color::rgb(200, 100, 255),
      trail_dist,
    );

    // No fade: fully opaque the entire length, no transparency change
    let trail_none = Trail2D::new(Color::rgb(100, 255, 130), width)
      .with_length(60)
      .with_fade(false);
    self.spawn_orbit(
      ctx,
      spacing * 3.5,
      cy,
      80.0,
      1.5,
      std::f32::consts::FRAC_PI_2 * 3.0,
      Color::rgb(100, 255, 130),
      trail_none,
    );
  }

  // ===========================================================================
  // Page 3: Styles & Visual Quality
  // ===========================================================================

  fn setup_styles(&mut self, ctx: &mut SceneContext) {
    let cy = self.screen_height / 2.0;
    let spacing = self.screen_width / 3.0;
    // Extra-wide trails (60px) so the 30% soft-edge fade zone (~9px each side) is obvious
    let width = 60.0;
    let color = Color::rgb(100, 200, 255);

    let styles = [TrailStyle::Solid, TrailStyle::Soft, TrailStyle::Glow];

    for (i, style) in styles.iter().enumerate() {
      let center_x = spacing * (i as f32 + 0.5);

      // No fade — constant opacity so the edge shape is the only variable
      // Slow orbit so the trail is steady and easy to compare
      let trail = Trail2D::new(color, width)
        .with_length(50)
        .with_fade(false)
        .with_style(*style)
        .with_cap(LineCap::Round);

      let angle = i as f32 * std::f32::consts::TAU / 3.0;
      self.spawn_orbit(ctx, center_x, cy, 100.0, 0.8, angle, color, trail);
    }
  }

  // ===========================================================================
  // Page 4: Caps & Joins
  // ===========================================================================

  fn setup_caps_joins(&mut self, ctx: &mut SceneContext) {
    let sw = self.screen_width;
    let sh = self.screen_height;

    // ---- LEFT HALF: Caps stacked vertically with aligned endpoints ----
    // All three lines share the same start/end x, so the cap extension is directly comparable.
    // 40px wide so the Square cap's 20px extension is clearly visible.
    let cap_width = 40.0;
    let cap_len = 120.0;
    let cap_x0 = sw * 0.08;
    let cap_x1 = cap_x0 + cap_len;
    let caps = [
      (LineCap::Butt, sh * 0.20),
      (LineCap::Round, sh * 0.35),
      (LineCap::Square, sh * 0.50),
    ];

    for (cap, y) in &caps {
      let pts = vec![Vec2::new(cap_x0, *y), Vec2::new(cap_x1, *y)];
      let line = Line2D::new(pts, Color::rgb(100, 200, 255), cap_width)
        .with_cap(*cap)
        .with_style(TrailStyle::Solid);
      let entity = ctx.world.spawn((Transform::new(0.0, 0.0), line));
      self.entities.push(entity);
    }

    // Bright vertical reference lines (3px, magenta) at endpoint positions
    // so the user can see how Round/Square extend past Butt
    for &ref_x in &[cap_x0, cap_x1] {
      let pts = vec![
        Vec2::new(ref_x, sh * 0.12),
        Vec2::new(ref_x, sh * 0.58),
      ];
      let line = Line2D::new(pts, Color::rgb(255, 50, 255), 3.0)
        .with_cap(LineCap::Butt)
        .with_style(TrailStyle::Solid);
      let entity = ctx.world.spawn((Transform::new(0.0, 0.0), line));
      self.entities.push(entity);
    }

    // ---- RIGHT HALF: Joins with acute angle (35°) ----
    // At 35°, miter extends ~3.3x the half-width = a huge visible spike.
    // Bevel/Round stay compact. The difference is unmistakable.
    let join_width = 30.0;
    let arm = 120.0;
    let angle_rad = 35.0_f32.to_radians();
    let joins = [
      (LineJoin::Miter, sh * 0.20),
      (LineJoin::Bevel, sh * 0.45),
      (LineJoin::Round, sh * 0.70),
    ];

    for (join, base_y) in &joins {
      let cx = sw * 0.65;
      // Two arms meeting at an acute angle (opening upward)
      let dx = (angle_rad / 2.0).sin() * arm;
      let dy = (angle_rad / 2.0).cos() * arm;
      let pts = vec![
        Vec2::new(cx - dx, base_y - dy),
        Vec2::new(cx, *base_y),
        Vec2::new(cx + dx, base_y - dy),
      ];

      let line = Line2D::new(pts, Color::rgb(255, 180, 80), join_width)
        .with_join(*join)
        .with_cap(LineCap::Butt)
        .with_style(TrailStyle::Solid);
      let entity = ctx.world.spawn((Transform::new(0.0, 0.0), line));
      self.entities.push(entity);
    }

    // --- Mouse trail with cyclable cap/join ---
    let (mx, my) = ctx.input.mouse_position();
    let mouse_color = Color::rgb(200, 255, 200);
    let trail = Trail2D::new(mouse_color, 12.0)
      .with_length(40)
      .with_cap(self.current_cap)
      .with_join(self.current_join)
      .with_style(TrailStyle::Solid);

    let entity = ctx.world.spawn((
      Transform::new(mx, my).with_pivot(0.5, 0.5),
      Renderable::sprite(circle_sprite(6, mouse_color)),
      trail,
      MouseTrail,
    ));
    self.entities.push(entity);
  }

  // ===========================================================================
  // Page 5: Textured Trails
  // ===========================================================================

  fn setup_textured(&mut self, ctx: &mut SceneContext) {
    if self.texture_sprite.is_none() {
      self.texture_sprite = Some(create_trail_texture());
    }
    let texture = self.texture_sprite.clone().unwrap();

    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;
    let color = Color::WHITE;

    // Left orbit: Stretch mode
    let stretch_trail = Trail2D::new(color, 12.0)
      .with_length(50)
      .with_style(TrailStyle::Solid)
      .with_cap(LineCap::Round)
      .with_texture(texture.clone())
      .with_texture_mode(TextureMode::Stretch);

    self.spawn_orbit(ctx, cx - 180.0, cy, 100.0, 1.2, 0.0, color, stretch_trail);

    // Right orbit: Tile mode
    let tile_trail = Trail2D::new(color, 12.0)
      .with_length(50)
      .with_style(TrailStyle::Solid)
      .with_cap(LineCap::Round)
      .with_texture(texture)
      .with_texture_mode(TextureMode::Tile)
      .with_tile_length(32.0);

    self.spawn_orbit(
      ctx,
      cx + 180.0,
      cy,
      100.0,
      -1.2,
      std::f32::consts::PI,
      color,
      tile_trail,
    );
  }

  // ===========================================================================
  // Page 6: GPU Effects
  // ===========================================================================

  fn setup_gpu_effects(&mut self, ctx: &mut SceneContext) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Wide horizontal spacing so displacement effects don't visually overlap
    let col = self.screen_width / 3.0;

    // Lightning: fast orbit, cyan, jitter displacement
    let lightning_trail = Trail2D::new(Color::CYAN, 6.0)
      .with_length(40)
      .with_style(TrailStyle::Glow)
      .with_cap(LineCap::Round)
      .with_effect(TrailEffect::Lightning {
        jitter: 12.0,
        speed: 3.0,
        detail: 3,
      });

    self.spawn_orbit(
      ctx,
      col * 0.5,
      cy,
      80.0,
      2.5,
      0.0,
      Color::CYAN,
      lightning_trail,
    );

    // Noise: medium orbit, orange, organic distortion
    let noise_color = Color::rgb(255, 140, 40);
    let noise_trail = Trail2D::new(noise_color, 10.0)
      .with_length(40)
      .with_style(TrailStyle::Soft)
      .with_cap(LineCap::Round)
      .with_effect(TrailEffect::Noise {
        frequency: 3.0,
        amplitude: 10.0,
        speed: 2.0,
      });

    self.spawn_orbit(
      ctx,
      cx,
      cy,
      80.0,
      -1.5,
      std::f32::consts::FRAC_PI_3,
      noise_color,
      noise_trail,
    );

    // Ribbon: wide orbit, green, sine wave
    let ribbon_color = Color::rgb(80, 255, 120);
    let ribbon_trail = Trail2D::new(ribbon_color, 8.0)
      .with_length(50)
      .with_style(TrailStyle::Solid)
      .with_cap(LineCap::Round)
      .with_effect(TrailEffect::Ribbon {
        amplitude: 12.0,
        frequency: 4.0,
        speed: 3.0,
      });

    self.spawn_orbit(
      ctx,
      col * 2.5,
      cy,
      80.0,
      1.0,
      std::f32::consts::FRAC_PI_6,
      ribbon_color,
      ribbon_trail,
    );
  }

  // ===========================================================================
  // Update helpers
  // ===========================================================================

  fn update_orbits(&self, ctx: &mut SceneContext, dt: f32) {
    if self.paused {
      return;
    }

    for &entity in &self.entities {
      if let Some(mut orbit) = ctx.world.get_mut::<OrbitTrail>(entity) {
        orbit.angle += orbit.speed * dt;
        let new_x = orbit.center_x + orbit.angle.cos() * orbit.radius;
        let new_y = orbit.center_y + orbit.angle.sin() * orbit.radius;
        drop(orbit);

        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          transform.x = new_x;
          transform.y = new_y;
        }
        if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
          trail.update(new_x, new_y, dt);
        }
      }
    }
  }

  fn update_mouse_trail(&self, ctx: &mut SceneContext, dt: f32) {
    let (mx, my) = ctx.input.mouse_position();

    for &entity in &self.entities {
      if ctx.world.get::<MouseTrail>(entity).is_some() {
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          transform.x = mx;
          transform.y = my;
        }
        if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
          trail.update(mx, my, dt);
        }
      }
    }
  }

  // ===========================================================================
  // Page-specific input handling
  // ===========================================================================

  fn handle_presets_input(&mut self, ctx: &mut SceneContext) {
    let mut changed = false;

    // Preset switching
    let presets = [
      (KeyCode::Digit1, TrailPreset::Bullet),
      (KeyCode::Digit2, TrailPreset::Laser),
      (KeyCode::Digit3, TrailPreset::Swipe),
      (KeyCode::Digit4, TrailPreset::Lightning),
      (KeyCode::Digit5, TrailPreset::Fire),
      (KeyCode::Digit6, TrailPreset::Energy),
    ];
    for (key, preset) in &presets {
      if ctx.input.key_pressed(*key) {
        self.current_preset = *preset;
        changed = true;
      }
    }

    // Pause
    if ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
    }

    // Reset
    if ctx.input.key_pressed(KeyCode::KeyR) {
      for &entity in &self.entities {
        if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
          trail.clear();
        }
      }
    }

    if changed {
      self.recreate_presets(ctx);
    }
  }

  fn handle_fade_modes_input(&mut self, ctx: &mut SceneContext) {
    let mut changed = false;

    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      self.lifetime = (self.lifetime + 0.1).min(2.0);
      self.fade_distance = (self.fade_distance + 20.0).min(500.0);
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      self.lifetime = (self.lifetime - 0.1).max(0.1);
      self.fade_distance = (self.fade_distance - 20.0).max(20.0);
      changed = true;
    }

    if ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
    }

    if changed {
      self.clear_page(ctx.world);
      self.setup_fade_modes(ctx);
    }
  }

  fn handle_caps_joins_input(&mut self, ctx: &mut SceneContext) {
    let mut changed = false;

    if ctx.input.key_pressed(KeyCode::KeyC) {
      self.current_cap = match self.current_cap {
        LineCap::Butt => LineCap::Round,
        LineCap::Round => LineCap::Square,
        LineCap::Square => LineCap::Butt,
      };
      changed = true;
    }

    if ctx.input.key_pressed(KeyCode::KeyJ) {
      self.current_join = match self.current_join {
        LineJoin::None => LineJoin::Miter,
        LineJoin::Miter => LineJoin::Bevel,
        LineJoin::Bevel => LineJoin::Round,
        LineJoin::Round => LineJoin::None,
      };
      changed = true;
    }

    if changed {
      self.clear_page(ctx.world);
      self.setup_caps_joins(ctx);
    }
  }

  fn handle_textured_input(&mut self, ctx: &mut SceneContext) {
    if ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
    }
  }

  fn handle_gpu_effects_input(&mut self, ctx: &mut SceneContext) {
    if ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
    }
  }

  // ===========================================================================
  // Navigation
  // ===========================================================================

  fn handle_navigation(&mut self, ctx: &mut SceneContext) -> bool {
    // Left/Right arrows always switch pages
    let new_page = if ctx.input.key_pressed(KeyCode::ArrowRight) {
      Some(self.current_page.next())
    } else if ctx.input.key_pressed(KeyCode::ArrowLeft) {
      Some(self.current_page.prev())
    } else {
      None
    };

    if let Some(page) = new_page {
      self.switch_page(ctx, page);
      return true;
    }
    false
  }
}

// =============================================================================
// Scene Implementation
// =============================================================================

impl Scene for TrailsDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_width = w;
    self.screen_height = h;

    println!("=== TRAILS DEMO ===");
    println!("Left/Right: Switch pages");
    println!("ESC: Exit");

    self.setup_page(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Navigation cooldown
    if self.nav_cooldown > 0.0 {
      self.nav_cooldown -= dt;
    }

    if self.nav_cooldown <= 0.0 && self.handle_navigation(ctx) {
      self.nav_cooldown = 0.2;
      return SceneAction::Continue;
    }

    // Page-specific input
    match self.current_page {
      DemoPage::Presets => self.handle_presets_input(ctx),
      DemoPage::FadeModes => self.handle_fade_modes_input(ctx),
      DemoPage::Styles => {
        if ctx.input.key_pressed(KeyCode::Space) {
          self.paused = !self.paused;
        }
      }
      DemoPage::CapsJoins => self.handle_caps_joins_input(ctx),
      DemoPage::Textured => self.handle_textured_input(ctx),
      DemoPage::GpuEffects => self.handle_gpu_effects_input(ctx),
    }

    // Update trails
    self.update_orbits(ctx, dt);

    // Mouse trail only on pages that have one
    if matches!(self.current_page, DemoPage::Presets | DemoPage::CapsJoins) {
      self.update_mouse_trail(ctx, dt);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (sw, sh) = ui.screen_size();
    let sw = sw as f32;
    let sh = sh as f32;
    let white = Color::WHITE;
    let gray = Color::rgb(200, 200, 200);
    let muted = Color::rgb(150, 150, 150);
    let yellow = Color::rgb(255, 255, 100);

    // Page title
    ui.draw_text(
      &format!(
        "Page {}/6: {}",
        self.current_page.index() + 1,
        self.current_page.name()
      ),
      10.0,
      10.0,
      white,
    );

    // Description
    ui.draw_text(self.current_page.description(), 10.0, 30.0, yellow);

    // Navigation hint at bottom
    ui.draw_text(
      "Left/Right: Switch Page | ESC: Exit",
      10.0,
      sh - 20.0,
      muted,
    );

    // Page-specific UI
    match self.current_page {
      DemoPage::Presets => {
        ui.draw_text(
          &format!("Preset: {}", self.current_preset.name()),
          10.0,
          50.0,
          gray,
        );
        if self.paused {
          ui.draw_text("PAUSED", sw / 2.0 - 30.0, sh / 2.0, yellow);
        }
      }
      DemoPage::FadeModes => {
        let spacing = sw / 4.0;
        let label_y = 55.0;
        let colors = [
          Color::rgb(100, 200, 255),
          Color::rgb(255, 200, 80),
          Color::rgb(200, 100, 255),
          Color::rgb(100, 255, 130),
        ];
        let labels = [
          ("Position Fade", "Head-to-tail gradient"),
          ("Time Fade", &format!("Expires after {:.1}s", self.lifetime)),
          (
            "Distance Fade",
            &format!("Fades over {:.0}px", self.fade_distance),
          ),
          ("No Fade", "Fully opaque, no alpha change"),
        ];
        for (i, (name, detail)) in labels.iter().enumerate() {
          let x = spacing * (i as f32 + 0.5) - 50.0;
          ui.draw_text(name, x, label_y, colors[i]);
          ui.draw_text(detail, x, label_y + 16.0, gray);
        }
        if self.paused {
          ui.draw_text("PAUSED", sw / 2.0 - 30.0, sh / 2.0, yellow);
        }
      }
      DemoPage::Styles => {
        let spacing = sw / 3.0;
        let label_y = self.screen_height * 0.25 - 80.0;
        let labels = ["Solid", "Soft", "Glow"];
        for (i, label) in labels.iter().enumerate() {
          let x = spacing * (i as f32 + 0.5) - 15.0;
          ui.draw_text(label, x, label_y, white);
        }
        if self.paused {
          ui.draw_text("PAUSED", sw / 2.0 - 30.0, sh / 2.0, yellow);
        }
      }
      DemoPage::CapsJoins => {
        // LEFT HALF: Cap labels beside each line
        ui.draw_text("CAPS", 10.0, 55.0, gray);
        ui.draw_text("(magenta lines = true endpoints)", 50.0, 55.0, muted);
        let cap_labels = [
          ("Butt", sh * 0.20),
          ("Round", sh * 0.35),
          ("Square", sh * 0.50),
        ];
        for (label, y) in &cap_labels {
          ui.draw_text(label, sw * 0.05, y - 20.0, white);
        }

        // RIGHT HALF: Join labels beside each shape
        ui.draw_text("JOINS (35° angle)", sw * 0.50, 55.0, gray);
        let join_labels = [
          ("Miter", sh * 0.20),
          ("Bevel", sh * 0.45),
          ("Round", sh * 0.70),
        ];
        for (label, y) in &join_labels {
          ui.draw_text(label, sw * 0.50, y - 20.0, white);
        }

        // Mouse trail info
        ui.draw_text(
          &format!(
            "Mouse trail: Cap={} | C to cycle cap  (Joins apply to Line2D only, not Trail2D)",
            cap_name(self.current_cap),
          ),
          10.0,
          sh - 40.0,
          gray,
        );
      }
      DemoPage::Textured => {
        let cx = sw / 2.0;
        ui.draw_text("Stretch", cx - 180.0 - 25.0, sh * 0.25 - 60.0, white);
        ui.draw_text(
          "Texture spans entire trail",
          cx - 180.0 - 70.0,
          sh * 0.25 - 44.0,
          gray,
        );
        ui.draw_text("Tile", cx + 180.0 - 15.0, sh * 0.25 - 60.0, white);
        ui.draw_text(
          "Texture repeats every 32px",
          cx + 180.0 - 75.0,
          sh * 0.25 - 44.0,
          gray,
        );
        if self.paused {
          ui.draw_text("PAUSED", sw / 2.0 - 30.0, sh / 2.0, yellow);
        }
      }
      DemoPage::GpuEffects => {
        let col = sw / 3.0;
        let cx = sw / 2.0;
        ui.draw_text("Lightning", col * 0.5 - 30.0, sh * 0.25 - 60.0, Color::CYAN);
        ui.draw_text(
          "Noise",
          cx - 15.0,
          sh * 0.25 - 60.0,
          Color::rgb(255, 140, 40),
        );
        ui.draw_text(
          "Ribbon",
          col * 2.5 - 20.0,
          sh * 0.25 - 60.0,
          Color::rgb(80, 255, 120),
        );
        ui.draw_text(
          "Jitter displacement",
          col * 0.5 - 55.0,
          sh * 0.25 - 44.0,
          gray,
        );
        ui.draw_text("Organic distortion", cx - 55.0, sh * 0.25 - 44.0, gray);
        ui.draw_text("Sine wave", col * 2.5 - 30.0, sh * 0.25 - 44.0, gray);
        if self.paused {
          ui.draw_text("PAUSED", sw / 2.0 - 30.0, sh / 2.0, yellow);
        }
      }
    }

    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Trails Demo - Ryo2D")
    .resolution(1920, 1080)
    .window_mode(WindowMode::ResizableAspectRatio)
    .target_fps(144)
    .physics_hz(144)
    .start_scene(TrailsDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
