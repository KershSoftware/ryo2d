//! Trails Demo - Ryo2D
//!
//! Six pages, each showcasing one trail feature: presets, fade modes,
//! rendering styles, caps and joins, textured trails and GPU effects.
//! Runs on the demo-kit shell; switch pages with `[` / `]`.

use demo_kit::prelude::*;

// =============================================================================
// Demo Page Enum
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
  Presets,
  FadeModes,
  Styles,
  CapsJoins,
  Textured,
  GpuEffects,
}

impl Variant {
  fn name(&self) -> &'static str {
    match self {
      Variant::Presets => "Presets",
      Variant::FadeModes => "Fade Modes",
      Variant::Styles => "Styles",
      Variant::CapsJoins => "Caps & Joins",
      Variant::Textured => "Textured",
      Variant::GpuEffects => "GPU Effects",
    }
  }

  fn description(&self) -> &'static str {
    match self {
      Variant::Presets => "A mouse trail and four orbiting trails using one trail preset",
      Variant::FadeModes => "Four trails side by side: position, time, distance and no fade",
      Variant::Styles => "Solid, Soft and Glow rendering styles on extra-wide trails",
      Variant::CapsJoins => {
        "Line caps (left) and joins (right) on Line2D shapes; the zigzag takes the chosen cap and join, the mouse trail the cap"
      }
      Variant::Textured => "Stretch (left) vs Tile (right) texture mapping on orbiting trails",
      Variant::GpuEffects => "Lightning, Noise and Ribbon displacement effects",
    }
  }
}

// =============================================================================
// Marker Components
// =============================================================================

#[derive(Component, Debug)]
struct MouseTrail;

#[derive(Component, Debug)]
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
  const ALL: [TrailPreset; 6] = [
    TrailPreset::Bullet,
    TrailPreset::Laser,
    TrailPreset::Swipe,
    TrailPreset::Lightning,
    TrailPreset::Fire,
    TrailPreset::Energy,
  ];

  const NAMES: [&str; 6] = ["Bullet", "Laser", "Swipe", "Lightning", "Fire", "Energy"];

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

const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Round, LineCap::Square];
const CAP_NAMES: [&str; 3] = ["Butt", "Round", "Square"];
const DEFAULT_CAP: usize = 1;

const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
const JOIN_NAMES: [&str; 3] = ["Miter", "Bevel", "Round"];
const DEFAULT_JOIN: usize = 2;

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
// Page
// =============================================================================

struct TrailsPage {
  variant: Variant,
  entities: Vec<Entity>,
  paused: bool,
  screen_width: f32,
  screen_height: f32,

  // Presets: index into `TrailPreset::ALL`
  preset_index: usize,
  applied_preset: usize,

  // Fade Modes
  lifetime: f32,
  fade_distance: f32,
  applied_fade: (f32, f32),

  // Caps & Joins (mouse trail and Line2D shapes)
  cap_index: usize,
  join_index: usize,
  applied_cap_join: (usize, usize),

  // Textured
  texture_sprite: Option<SpriteHandle>,
}

impl TrailsPage {
  fn new(variant: Variant) -> Self {
    Self {
      variant,
      entities: Vec::new(),
      paused: false,
      screen_width: 0.0,
      screen_height: 0.0,
      preset_index: 0,
      applied_preset: 0,
      lifetime: 0.5,
      fade_distance: 100.0,
      applied_fade: (0.5, 100.0),
      cap_index: DEFAULT_CAP,
      join_index: DEFAULT_JOIN,
      applied_cap_join: (DEFAULT_CAP, DEFAULT_JOIN),
      texture_sprite: None,
    }
  }

  fn preset(&self) -> TrailPreset {
    TrailPreset::ALL[self.preset_index.min(TrailPreset::ALL.len() - 1)]
  }

  fn cap(&self) -> LineCap {
    CAPS[self.cap_index.min(CAPS.len() - 1)]
  }

  fn join(&self) -> LineJoin {
    JOINS[self.join_index.min(JOINS.len() - 1)]
  }

  fn clear_page(&mut self, world: &mut World) {
    for entity in self.entities.drain(..) {
      world.despawn(entity);
    }
  }

  fn setup_page(&mut self, ctx: &mut SceneContext) {
    match self.variant {
      Variant::Presets => self.setup_presets(ctx),
      Variant::FadeModes => self.setup_fade_modes(ctx),
      Variant::Styles => self.setup_styles(ctx),
      Variant::CapsJoins => self.setup_caps_joins(ctx),
      Variant::Textured => self.setup_textured(ctx),
      Variant::GpuEffects => self.setup_gpu_effects(ctx),
    }
  }

  /// Rebuild the page's entities when a setting changed
  fn rebuild(&mut self, ctx: &mut SceneContext) {
    self.clear_page(ctx.world);
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

    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(circle_sprite(6, color)),
        trail,
        OrbitTrail {
          angle: start_angle,
          radius,
          speed,
          center_x: cx,
          center_y: cy,
        },
      ))
      .id();
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
    let trail = self.preset().create(mouse_color, 8.0);

    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(mx, my),
        Renderable::sprite(circle_sprite(8, mouse_color)),
        trail,
        MouseTrail,
      ))
      .id();
    self.entities.push(entity);

    // 4 orbiting trails — wide spacing to avoid visual overlap with effect presets
    let configs: [(f32, f32, Color); 4] = [
      (150.0, 1.5, Color::rgb(255, 100, 100)),
      (220.0, -1.0, Color::rgb(100, 255, 100)),
      (290.0, 0.7, Color::rgb(255, 200, 50)),
      (360.0, -0.5, Color::rgb(200, 100, 255)),
    ];

    // The radii suit a 1080-tall canvas: scale them to this one, so the
    // outer orbit clears the top bar
    let scale = self.screen_height / 1080.0;
    for (i, (radius, speed, color)) in configs.iter().enumerate() {
      let angle = i as f32 * std::f32::consts::FRAC_PI_2;
      let trail = self.preset().create(*color, 8.0);
      self.spawn_orbit(ctx, cx, cy, radius * scale, *speed, angle, *color, trail);
    }
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
    // Rows sit at fractions of the canvas; the sizes suit a 1080-tall one, so
    // they scale with it
    let unit = sh / 1080.0;

    // ---- LEFT HALF: Caps stacked vertically with aligned endpoints ----
    // All three lines share the same start/end x, so the cap extension is directly comparable.
    // 40px wide so the Square cap's 20px extension is clearly visible.
    let cap_width = 40.0 * unit;
    let cap_len = 120.0 * unit;
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
      let entity = ctx.world.spawn((Transform::IDENTITY, line)).id();
      self.entities.push(entity);
    }

    // Bright vertical reference lines (3px, magenta) at endpoint positions
    // so the user can see how Round/Square extend past Butt
    for &ref_x in &[cap_x0, cap_x1] {
      let pts = vec![Vec2::new(ref_x, sh * 0.12), Vec2::new(ref_x, sh * 0.58)];
      let line = Line2D::new(pts, Color::rgb(255, 50, 255), 3.0)
        .with_cap(LineCap::Butt)
        .with_style(TrailStyle::Solid);
      let entity = ctx.world.spawn((Transform::IDENTITY, line)).id();
      self.entities.push(entity);
    }

    // ---- RIGHT HALF: Joins with acute angle (35°) ----
    // At 35°, miter extends ~3.3x the half-width = a huge visible spike.
    // Bevel/Round stay compact. The difference is unmistakable.
    let join_width = 30.0 * unit;
    let arm = 120.0 * unit;
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
      let entity = ctx.world.spawn((Transform::IDENTITY, line)).id();
      self.entities.push(entity);
    }

    // --- Zigzag with the chosen cap and join (joins apply to Line2D) ---
    let (zig_x, zig_y, zig_w) = (sw * 0.08, sh * 0.82, sw * 0.32);
    let zigzag: Vec<Vec2> = (0..5)
      .map(|i| {
        let rise = if i % 2 == 0 { 0.0 } else { -150.0 * unit };
        Vec2::new(zig_x + zig_w * i as f32 / 4.0, zig_y + rise)
      })
      .collect();
    let line = Line2D::new(zigzag, Color::rgb(200, 255, 200), 24.0 * unit)
      .with_cap(self.cap())
      .with_join(self.join())
      .with_style(TrailStyle::Solid);
    let entity = ctx.world.spawn((Transform::IDENTITY, line)).id();
    self.entities.push(entity);

    // --- Mouse trail with the chosen cap (trails draw no joins) ---
    let (mx, my) = ctx.input.mouse_position();
    let mouse_color = Color::rgb(200, 255, 200);
    let trail = Trail2D::new(mouse_color, 12.0)
      .with_length(40)
      .with_cap(self.cap())
      .with_style(TrailStyle::Solid);

    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(mx, my),
        Renderable::sprite(circle_sprite(6, mouse_color)),
        trail,
        MouseTrail,
      ))
      .id();
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

  /// Move each orbiting head; the engine records its trail point after
  /// `update`, in world space
  fn update_orbits(&self, ctx: &mut SceneContext, dt: f32) {
    if self.paused {
      return;
    }

    for (mut orbit, mut transform) in ctx
      .world
      .query::<(&mut OrbitTrail, &mut Transform)>()
      .iter_mut(ctx.world)
    {
      orbit.angle += orbit.speed * dt;
      transform.translation = Vec2::new(
        orbit.center_x + orbit.angle.cos() * orbit.radius,
        orbit.center_y + orbit.angle.sin() * orbit.radius,
      );
    }
  }

  fn update_mouse_trail(&self, ctx: &mut SceneContext) {
    let (mx, my) = ctx.input.mouse_position();

    for mut transform in ctx
      .world
      .query_filtered::<&mut Transform, With<MouseTrail>>()
      .iter_mut(ctx.world)
    {
      transform.translation = Vec2::new(mx, my);
    }
  }
}

// =============================================================================
// Page Implementation
// =============================================================================

impl DemoPage for TrailsPage {
  fn name(&self) -> &str {
    self.variant.name()
  }

  fn description(&self) -> &str {
    self.variant.description()
  }

  fn settings(&mut self, s: &mut Settings) {
    match self.variant {
      Variant::Presets => s.choice("Preset", &mut self.preset_index, &TrailPreset::NAMES),
      Variant::FadeModes => {
        s.float("Lifetime", &mut self.lifetime, 0.1..=2.0, 0.1);
        s.float("Distance", &mut self.fade_distance, 20.0..=500.0, 20.0);
      }
      Variant::CapsJoins => {
        s.choice("Cap", &mut self.cap_index, &CAP_NAMES);
        s.choice("Join", &mut self.join_index, &JOIN_NAMES);
      }
      Variant::Styles | Variant::Textured | Variant::GpuEffects => {}
    }
  }

  fn controls(&self) -> Vec<Control> {
    match self.variant {
      Variant::Presets => vec![
        Control::key(KeyCode::Space, "Pause"),
        Control::mouse("Move the trail"),
      ],
      Variant::CapsJoins => vec![Control::mouse("Move the trail")],
      _ => vec![Control::key(KeyCode::Space, "Pause")],
    }
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_width = w;
    self.screen_height = h;
    self.setup_page(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Settings changes rebuild the page's trails
    let changed = match self.variant {
      Variant::Presets => {
        let changed = self.preset_index != self.applied_preset;
        self.applied_preset = self.preset_index;
        changed
      }
      Variant::FadeModes => {
        let now = (self.lifetime, self.fade_distance);
        let changed = now != self.applied_fade;
        self.applied_fade = now;
        changed
      }
      Variant::CapsJoins => {
        let now = (self.cap_index, self.join_index);
        let changed = now != self.applied_cap_join;
        self.applied_cap_join = now;
        changed
      }
      _ => false,
    };
    if changed {
      self.rebuild(ctx);
    }

    if self.variant != Variant::CapsJoins && ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
    }

    self.update_orbits(ctx, dt);

    // Mouse trail only on pages that have one
    if matches!(self.variant, Variant::Presets | Variant::CapsJoins) {
      self.update_mouse_trail(ctx);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let (sw, sh) = ui.screen_size();
    let sw = sw as f32;
    let sh = sh as f32;
    let white = Color::WHITE;
    let gray = Color::rgb(200, 200, 200);
    let muted = Color::rgb(150, 150, 150);
    let yellow = Color::rgb(255, 255, 100);

    // Page-specific UI
    match self.variant {
      Variant::Presets => {
        if self.paused {
          ui.draw_text("PAUSED", sw / 2.0 - 30.0, sh / 2.0, yellow);
        }
      }
      Variant::FadeModes => {
        let spacing = sw / 4.0;
        let label_y = content_area(ui).y + settings_panel_height(ui, 2) + 8.0; // below the settings panel
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
      Variant::Styles => {
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
      Variant::CapsJoins => {
        // LEFT HALF: Cap labels beside each line
        let heading_y = content_area(ui).y + 4.0;
        ui.draw_text("CAPS", 10.0, heading_y, gray);
        ui.draw_text("(magenta lines = true endpoints)", 50.0, heading_y, muted);
        let cap_labels = [
          ("Butt", sh * 0.20),
          ("Round", sh * 0.35),
          ("Square", sh * 0.50),
        ];
        for (label, y) in &cap_labels {
          ui.draw_text(label, sw * 0.035, y - 20.0, white);
        }

        // RIGHT HALF: Join labels beside each shape
        ui.draw_text("JOINS (35° angle)", sw * 0.50, heading_y, gray);
        let join_labels = [
          ("Miter", sh * 0.20),
          ("Bevel", sh * 0.45),
          ("Round", sh * 0.70),
        ];
        for (label, y) in &join_labels {
          ui.draw_text(label, sw * 0.50, y - 20.0, white);
        }

        // What the settings drive
        ui.draw_text(
          &format!(
            "Zigzag: Cap={}, Join={}   Mouse trail: Cap={} (trails draw no joins)",
            CAP_NAMES[self.cap_index.min(CAP_NAMES.len() - 1)],
            JOIN_NAMES[self.join_index.min(JOIN_NAMES.len() - 1)],
            CAP_NAMES[self.cap_index.min(CAP_NAMES.len() - 1)],
          ),
          10.0,
          sh - 40.0,
          gray,
        );
      }
      Variant::Textured => {
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
      Variant::GpuEffects => {
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
  }
}

fn main() {
  // Smooth trails, not pixel art: a canvas that fills any window
  Demo::new("Trails")
    .fit(1280, 720)
    .configure(|b| b.target_fps(144).fixed_hz(144))
    .page(|| TrailsPage::new(Variant::Presets))
    .page(|| TrailsPage::new(Variant::FadeModes))
    .page(|| TrailsPage::new(Variant::Styles))
    .page(|| TrailsPage::new(Variant::CapsJoins))
    .page(|| TrailsPage::new(Variant::Textured))
    .page(|| TrailsPage::new(Variant::GpuEffects))
    .run();
}
