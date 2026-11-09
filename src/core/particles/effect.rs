//! Particle effects: the asset that describes how one kind of particle behaves.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::collision::CollisionConfig;
use super::value::{ColorRange, Curve, Gradient, ValueRange};
use crate::core::asset::{Asset, Handle};
use crate::core::ecs::SpriteEffect;
use crate::core::graphics::Image;
use crate::core::render::batch::{BlendMode, Filter};
use crate::core::ui::Color;

/// Where in the emitter's space particles are born.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum EmitShape {
  /// The emitter's position
  #[default]
  Point,
  /// A circle, or an arc of one
  Circle {
    /// Radius
    radius: f32,
    /// Angle covered, 0 to 360 degrees
    arc_deg: f32,
    /// 0 is the rim only, 1 fills the disc
    thickness: f32,
  },
  /// A rectangle centred on the emitter
  Rect {
    /// Width and height
    size: Vec2,
    /// 0 is the outline only, 1 fills the rectangle
    thickness: f32,
  },
  /// A horizontal line centred on the emitter
  Line {
    /// Length
    length: f32,
  },
}

/// Particles released at one moment, and optionally again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Burst {
  /// Seconds into the cycle
  pub time: f32,
  /// How many particles
  pub count: ValueRange,
  /// How many times it fires; 0 repeats until the emitter stops
  pub cycles: u32,
  /// Seconds between repeats (above 0 unless `cycles` is 1)
  pub interval: f32,
}

impl Burst {
  /// One burst of `count` particles at `time`.
  pub fn at(time: f32, count: impl Into<ValueRange>) -> Self {
    Self {
      time,
      count: count.into(),
      cycles: 1,
      interval: 0.0,
    }
  }

  /// Repeats the burst `cycles` times (0 for until the emitter stops), `interval` seconds apart.
  pub fn with_cycles(mut self, cycles: u32, interval: f32) -> Self {
    self.cycles = cycles;
    self.interval = interval;
    self
  }
}

/// Where an effect's particles live once born.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Space {
  /// In the world: they stay where they were born as the emitter moves on
  #[default]
  World,
  /// In the emitter's own frame: they move and turn with it, and their
  /// forces act in that frame. They don't collide or feel attractors
  Local,
}

/// A sprite sheet the particles' image is cut into, and how a particle
/// plays through its frames. Frames run left to right, then down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Flipbook {
  /// Frames across the sheet
  pub columns: u32,
  /// Frames down the sheet
  pub rows: u32,
  /// Frames played; 0 plays them all (`columns` x `rows`)
  pub count: u32,
  /// How the frame follows time
  pub mode: FrameMode,
  /// Each particle starts at a frame of its own, fixed for its life
  pub random_start: bool,
}

impl Flipbook {
  /// A sheet of `columns` by `rows` frames, played once over each
  /// particle's life.
  pub fn new(columns: u32, rows: u32) -> Self {
    Self {
      columns,
      rows,
      count: 0,
      mode: FrameMode::OverLife,
      random_start: false,
    }
  }

  /// Plays only the first `count` frames of the sheet.
  pub fn with_count(mut self, count: u32) -> Self {
    self.count = count;
    self
  }

  /// Plays `fps` frames per second of the particle's age, looping.
  pub fn at_fps(mut self, fps: f32) -> Self {
    self.mode = FrameMode::Fps(fps);
    self
  }

  /// Starts each particle at a frame of its own.
  pub fn random_start(mut self) -> Self {
    self.random_start = true;
    self
  }

  /// The frames played: `count`, or the whole sheet when it is 0 or past it
  /// (at least 1)
  pub(crate) fn frames(&self) -> u32 {
    let cells = u64::from(self.columns.max(1)) * u64::from(self.rows.max(1));
    let cells = u32::try_from(cells).unwrap_or(u32::MAX);
    if self.count == 0 || self.count > cells {
      cells
    } else {
      self.count
    }
  }
}

/// How a [`Flipbook`]'s frame follows a particle's time.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum FrameMode {
  /// Once through the frames over the particle's life
  #[default]
  OverLife,
  /// This many frames per second of the particle's age, looping
  Fps(f32),
}

/// How a particle turns.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Align {
  /// By its own rotation and spin
  #[default]
  None,
  /// To face where it moves, ignoring rotation and spin, stretched along
  /// its travel: its length is its size plus its speed times `stretch`
  Velocity {
    /// Pixels of length per pixel per second of speed
    stretch: f32,
  },
}

/// Which particle is drawn over which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawOrder {
  /// Younger particles cover older ones
  #[default]
  NewestOnTop,
  /// Older particles cover younger ones
  OldestOnTop,
}

/// How a kind of particle is born, moves, looks and ends.
///
/// Angles are in degrees. Build one with [`ParticleEffect::new`] and the `with_*`
/// builders, or start from a preset such as [`ParticleEffect::fire`].
#[derive(Debug, Clone)]
pub struct ParticleEffect {
  /// Particles per second, on top of the bursts
  pub rate: f32,
  /// Particles per pixel the emitter moves, on top of the rate, spread
  /// along its move (none across a teleport)
  pub rate_per_distance: f32,
  /// Particles released at set moments of each cycle
  pub bursts: Vec<Burst>,
  /// Seconds in one cycle
  pub duration: f32,
  /// Whether the cycle repeats
  pub looping: bool,
  /// Seconds before the first cycle starts
  pub start_delay: f32,
  /// Starts a looping effect as if it had played a cycle already (at most
  /// 10 s of it): the first step its emitter plays it in runs that much
  /// first, its start delay skipped
  pub prewarm: bool,
  /// Most live particles one emitter keeps
  pub max_particles: u32,
  /// Where particles are born
  pub shape: EmitShape,
  /// Direction of travel in degrees (0 is right, 90 is down)
  pub direction_deg: f32,
  /// Width of the cone around the direction, 0 to 360 degrees
  pub spread_deg: f32,
  /// Travel away from the shape's centre instead of along the direction
  pub outward: bool,
  /// Seconds a particle lives
  pub lifetime: ValueRange,
  /// Starting speed
  pub speed: ValueRange,
  /// Starting size
  pub size: ValueRange,
  /// Starting rotation in degrees
  pub rotation_deg: ValueRange,
  /// Spin in degrees per second
  pub angular_velocity_deg: ValueRange,
  /// The share of the emitter's velocity a particle starts with, on top of
  /// its own (a `World` effect's)
  pub inherit_velocity: f32,
  /// Tint picked per particle
  pub color: ColorRange,
  /// Tint multiplier over the particle's life
  pub color_over_life: Gradient,
  /// Size multiplier over the particle's life
  pub size_over_life: Curve,
  /// Speed multiplier over the particle's life
  pub speed_over_life: Curve,
  /// Acceleration in pixels per second squared
  pub gravity: Vec2,
  /// Fraction of speed lost per second
  pub drag: f32,
  /// Acceleration away from the emitter, in pixels per second squared (a
  /// negative value pulls in): each particle keeps one value from the range
  pub radial_acceleration: ValueRange,
  /// Acceleration round the emitter, counter-clockwise on screen when
  /// positive: each particle keeps one value from the range
  pub tangential_acceleration: ValueRange,
  /// A smooth, changing force field, if any
  pub noise: Option<NoiseForce>,
  /// Whether particles live in the world or in the emitter's own frame
  pub space: Space,
  /// The image each particle draws
  pub image: Handle<Image>,
  /// How particles blend with what is behind them
  pub blend: BlendMode,
  /// Texture filtering
  pub filter: Filter,
  /// Whether lights affect the particles
  pub lit: bool,
  /// Brightness for the lighting's glow pass
  pub emission: f32,
  /// A sprite effect applied to each particle
  pub effect: Option<SpriteEffect>,
  /// Draw order among sprites
  pub z: i32,
  /// Which particle covers which
  pub draw_order: DrawOrder,
  /// The image as a sprite sheet the particle plays through, if any
  pub frames: Option<Flipbook>,
  /// How a particle turns
  pub align: Align,
  /// Collision with the world, if any
  pub collision: Option<CollisionConfig>,
  /// Render layer
  pub layer: u32,
  /// Fixed random seed, so the effect repeats
  pub seed: Option<u64>,
}

/// A smooth force field that changes with time, pushing each particle by
/// the noise at its place: the particles of one seed move the same way
/// each run
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoiseForce {
  /// Acceleration at the field's strongest, in pixels per second squared
  pub strength: f32,
  /// How fast the field changes over space: the swirls are about
  /// `1 / frequency` pixels across
  pub frequency: f32,
  /// How fast the field changes over a particle's age
  pub speed: f32,
}

impl NoiseForce {
  /// A field of `strength` and `frequency` that changes at speed 1.
  pub fn new(strength: f32, frequency: f32) -> Self {
    Self {
      strength,
      frequency,
      speed: 1.0,
    }
  }

  /// Sets how fast the field changes over a particle's age.
  pub fn with_speed(mut self, speed: f32) -> Self {
    self.speed = speed;
    self
  }
}

impl Asset for ParticleEffect {}

impl ParticleEffect {
  /// An effect drawing `image`, with plain defaults.
  pub fn new(image: Handle<Image>) -> Self {
    Self {
      rate: 10.0,
      rate_per_distance: 0.0,
      bursts: Vec::new(),
      duration: 1.0,
      looping: true,
      start_delay: 0.0,
      prewarm: false,
      max_particles: 1000,
      shape: EmitShape::Point,
      direction_deg: 0.0,
      spread_deg: 360.0,
      outward: false,
      lifetime: ValueRange::new(1.0, 2.0),
      speed: ValueRange::new(0.0, 50.0),
      size: ValueRange::new(8.0, 16.0),
      rotation_deg: ValueRange::constant(0.0),
      angular_velocity_deg: ValueRange::constant(0.0),
      inherit_velocity: 0.0,
      color: ColorRange::constant(Color::WHITE),
      color_over_life: Gradient::new([(0.0, Color::WHITE), (1.0, Color::TRANSPARENT)]),
      size_over_life: Curve::constant(1.0),
      speed_over_life: Curve::constant(1.0),
      gravity: Vec2::ZERO,
      drag: 0.0,
      radial_acceleration: ValueRange::constant(0.0),
      tangential_acceleration: ValueRange::constant(0.0),
      noise: None,
      space: Space::World,
      image,
      blend: BlendMode::Alpha,
      filter: Filter::Default,
      lit: true,
      emission: 1.0,
      effect: None,
      z: 0,
      draw_order: DrawOrder::NewestOnTop,
      frames: None,
      align: Align::None,
      collision: None,
      layer: 0,
      seed: None,
    }
  }

  /// Sets the particles per second.
  pub fn with_rate(mut self, rate: f32) -> Self {
    self.rate = rate;
    self
  }

  /// Sets the particles per pixel the emitter moves.
  pub fn with_rate_per_distance(mut self, rate_per_distance: f32) -> Self {
    self.rate_per_distance = rate_per_distance;
    self
  }

  /// Adds a burst.
  pub fn with_burst(mut self, burst: Burst) -> Self {
    self.bursts.push(burst);
    self
  }

  /// Sets the seconds in one cycle.
  pub fn with_duration(mut self, duration: f32) -> Self {
    self.duration = duration;
    self
  }

  /// Sets whether the cycle repeats.
  pub fn with_looping(mut self, looping: bool) -> Self {
    self.looping = looping;
    self
  }

  /// Sets the seconds before the first cycle.
  pub fn with_start_delay(mut self, start_delay: f32) -> Self {
    self.start_delay = start_delay;
    self
  }

  /// Starts a looping effect as if it had played a cycle already.
  pub fn with_prewarm(mut self, prewarm: bool) -> Self {
    self.prewarm = prewarm;
    self
  }

  /// Sets the most live particles per emitter.
  pub fn with_max_particles(mut self, max_particles: u32) -> Self {
    self.max_particles = max_particles;
    self
  }

  /// Sets where particles are born.
  pub fn with_shape(mut self, shape: EmitShape) -> Self {
    self.shape = shape;
    self
  }

  /// Sets the direction of travel and the cone's width, in degrees.
  pub fn with_direction(mut self, direction_deg: f32, spread_deg: f32) -> Self {
    self.direction_deg = direction_deg;
    self.spread_deg = spread_deg;
    self
  }

  /// Makes particles travel away from the shape's centre.
  pub fn outward(mut self) -> Self {
    self.outward = true;
    self
  }

  /// Sets the seconds a particle lives.
  pub fn with_lifetime(mut self, lifetime: impl Into<ValueRange>) -> Self {
    self.lifetime = lifetime.into();
    self
  }

  /// Sets the starting speed.
  pub fn with_speed(mut self, speed: impl Into<ValueRange>) -> Self {
    self.speed = speed.into();
    self
  }

  /// Sets the starting size.
  pub fn with_size(mut self, size: impl Into<ValueRange>) -> Self {
    self.size = size.into();
    self
  }

  /// Sets the starting rotation, in degrees.
  pub fn with_rotation(mut self, rotation_deg: impl Into<ValueRange>) -> Self {
    self.rotation_deg = rotation_deg.into();
    self
  }

  /// Sets the spin, in degrees per second.
  pub fn with_angular_velocity(mut self, angular_velocity_deg: impl Into<ValueRange>) -> Self {
    self.angular_velocity_deg = angular_velocity_deg.into();
    self
  }

  /// Sets the share of the emitter's velocity a particle starts with.
  pub fn with_inherit_velocity(mut self, inherit_velocity: f32) -> Self {
    self.inherit_velocity = inherit_velocity;
    self
  }

  /// Sets the tint picked per particle.
  pub fn with_color(mut self, color: impl Into<ColorRange>) -> Self {
    self.color = color.into();
    self
  }

  /// Sets the tint multiplier over a particle's life.
  pub fn with_color_over_life(mut self, gradient: Gradient) -> Self {
    self.color_over_life = gradient;
    self
  }

  /// Sets the size multiplier over a particle's life.
  pub fn with_size_over_life(mut self, curve: Curve) -> Self {
    self.size_over_life = curve;
    self
  }

  /// Sets the speed multiplier over a particle's life.
  pub fn with_speed_over_life(mut self, curve: Curve) -> Self {
    self.speed_over_life = curve;
    self
  }

  /// Sets the acceleration.
  pub fn with_gravity(mut self, x: f32, y: f32) -> Self {
    self.gravity = Vec2::new(x, y);
    self
  }

  /// Sets the fraction of speed lost per second.
  pub fn with_drag(mut self, drag: f32) -> Self {
    self.drag = drag;
    self
  }

  /// Sets the acceleration away from the emitter.
  pub fn with_radial_acceleration(mut self, acceleration: impl Into<ValueRange>) -> Self {
    self.radial_acceleration = acceleration.into();
    self
  }

  /// Sets the acceleration round the emitter.
  pub fn with_tangential_acceleration(mut self, acceleration: impl Into<ValueRange>) -> Self {
    self.tangential_acceleration = acceleration.into();
    self
  }

  /// Sets the noise force.
  pub fn with_noise(mut self, noise: NoiseForce) -> Self {
    self.noise = Some(noise);
    self
  }

  /// Sets whether particles live in the world or in the emitter's frame.
  pub fn with_space(mut self, space: Space) -> Self {
    self.space = space;
    self
  }

  /// Sets the blend mode.
  pub fn with_blend(mut self, blend: BlendMode) -> Self {
    self.blend = blend;
    self
  }

  /// Sets the texture filter.
  pub fn with_filter(mut self, filter: Filter) -> Self {
    self.filter = filter;
    self
  }

  /// Sets whether lights affect the particles.
  pub fn with_lighting(mut self, lit: bool) -> Self {
    self.lit = lit;
    self
  }

  /// Sets the brightness for the glow pass.
  pub fn with_emission(mut self, emission: f32) -> Self {
    self.emission = emission;
    self
  }

  /// Applies a sprite effect to each particle.
  pub fn with_effect(mut self, effect: SpriteEffect) -> Self {
    self.effect = Some(effect);
    self
  }

  /// Sets the draw order among sprites.
  pub fn with_z(mut self, z: i32) -> Self {
    self.z = z;
    self
  }

  /// Cuts the image into a sprite sheet each particle plays through.
  pub fn with_frames(mut self, frames: Flipbook) -> Self {
    self.frames = Some(frames);
    self
  }

  /// Sets how particles turn.
  pub fn with_align(mut self, align: Align) -> Self {
    self.align = align;
    self
  }

  /// Sets which particle covers which.
  pub fn with_draw_order(mut self, draw_order: DrawOrder) -> Self {
    self.draw_order = draw_order;
    self
  }

  /// Makes particles collide with the world.
  pub fn with_collision(mut self, collision: CollisionConfig) -> Self {
    self.collision = Some(collision);
    self
  }

  /// Sets the render layer.
  pub fn with_layer(mut self, layer: u32) -> Self {
    self.layer = layer;
    self
  }

  /// Fixes the random seed so the effect repeats.
  pub fn with_seed(mut self, seed: u64) -> Self {
    self.seed = Some(seed);
    self
  }

  /// Whether its particles meet colliders: it has a collision and lives in
  /// the world
  pub(crate) fn collides(&self) -> bool {
    self.collision.is_some() && self.space == Space::World
  }

  /// Every value the effect has that is not allowed, each starting with its field's name.
  pub(crate) fn problems(&self) -> Vec<String> {
    let mut out = Vec::new();
    let mut report = |field: &str, bad: bool, what: &str| {
      if bad {
        out.push(format!("{field}: {what}"));
      }
    };
    let finite = |v: f32| v.is_finite();
    let range_finite = |r: &ValueRange| finite(r.min) && finite(r.max);
    let range_negative = |r: &ValueRange| r.min < 0.0 || r.max < 0.0;

    report("rate", !finite(self.rate), "is not a number");
    report("rate", self.rate < 0.0, "is negative");
    report(
      "rate_per_distance",
      !finite(self.rate_per_distance),
      "is not a number",
    );
    report(
      "rate_per_distance",
      self.rate_per_distance < 0.0,
      "is negative",
    );
    report("duration", !finite(self.duration), "is not a number");
    report("duration", self.duration <= 0.0, "must be above 0");
    report("start_delay", !finite(self.start_delay), "is not a number");
    report("start_delay", self.start_delay < 0.0, "is negative");
    report(
      "prewarm",
      self.prewarm && !self.looping,
      "needs a looping effect",
    );
    report("max_particles", self.max_particles == 0, "must be above 0");
    report(
      "direction_deg",
      !finite(self.direction_deg),
      "is not a number",
    );
    report("spread_deg", !finite(self.spread_deg), "is not a number");
    report(
      "spread_deg",
      self.spread_deg < 0.0 || self.spread_deg > 360.0,
      "must be within 0 to 360",
    );
    report("lifetime", !range_finite(&self.lifetime), "is not a number");
    report("lifetime", self.lifetime.min <= 0.0, "must be above 0");
    report("speed", !range_finite(&self.speed), "is not a number");
    report("speed", range_negative(&self.speed), "is negative");
    report("size", !range_finite(&self.size), "is not a number");
    report("size", range_negative(&self.size), "is negative");
    report(
      "rotation_deg",
      !range_finite(&self.rotation_deg),
      "is not a number",
    );
    report(
      "angular_velocity_deg",
      !range_finite(&self.angular_velocity_deg),
      "is not a number",
    );
    report(
      "inherit_velocity",
      !finite(self.inherit_velocity),
      "is not a number",
    );
    report(
      "gravity",
      !(finite(self.gravity.x) && finite(self.gravity.y)),
      "is not a number",
    );
    report("drag", !finite(self.drag), "is not a number");
    report("drag", self.drag < 0.0, "is negative");
    report(
      "radial_acceleration",
      !range_finite(&self.radial_acceleration),
      "is not a number",
    );
    report(
      "tangential_acceleration",
      !range_finite(&self.tangential_acceleration),
      "is not a number",
    );
    if let Some(noise) = &self.noise {
      report(
        "noise",
        !(finite(noise.strength) && finite(noise.frequency) && finite(noise.speed)),
        "has a value that is not a number",
      );
      report(
        "noise",
        noise.frequency < 0.0 || noise.speed < 0.0,
        "frequency and speed must not be negative",
      );
    }
    if let Some(frames) = &self.frames {
      report(
        "frames",
        frames.columns == 0 || frames.rows == 0,
        "columns and rows must be above 0",
      );
      report(
        "frames",
        u64::from(frames.count) > u64::from(frames.columns) * u64::from(frames.rows),
        "count is more than columns times rows",
      );
      if let FrameMode::Fps(fps) = frames.mode {
        report("frames", !finite(fps) || fps <= 0.0, "fps must be above 0");
      }
    }
    if let Align::Velocity { stretch } = self.align {
      report("align", !finite(stretch), "stretch is not a number");
      report("align", stretch < 0.0, "stretch is negative");
    }
    report("emission", !finite(self.emission), "is not a number");
    report("emission", self.emission < 0.0, "is negative");
    report(
      "collision",
      self.collision.is_some() && self.space == Space::Local,
      "a Local effect does not collide",
    );

    for burst in &self.bursts {
      report(
        "bursts",
        !(finite(burst.time) && finite(burst.interval) && range_finite(&burst.count)),
        "has a value that is not a number",
      );
      report(
        "bursts",
        burst.cycles != 1 && burst.interval <= 0.0,
        "a repeating burst needs an interval above 0",
      );
    }

    let unit = |v: f32| (0.0..=1.0).contains(&v);
    let arc = |v: f32| (0.0..=360.0).contains(&v);
    match self.shape {
      EmitShape::Point => {}
      EmitShape::Circle {
        radius,
        arc_deg,
        thickness,
      } => {
        report(
          "shape",
          !(finite(radius) && finite(arc_deg) && finite(thickness)),
          "has a value that is not a number",
        );
        report("shape", radius < 0.0, "radius is negative");
        report("shape", !arc(arc_deg), "arc_deg must be within 0 to 360");
        report("shape", !unit(thickness), "thickness must be within 0 to 1");
      }
      EmitShape::Rect { size, thickness } => {
        report(
          "shape",
          !(finite(size.x) && finite(size.y) && finite(thickness)),
          "has a value that is not a number",
        );
        report("shape", size.x < 0.0 || size.y < 0.0, "size is negative");
        report("shape", !unit(thickness), "thickness must be within 0 to 1");
      }
      EmitShape::Line { length } => {
        report("shape", !finite(length), "has a value that is not a number");
        report("shape", length < 0.0, "length is negative");
      }
    }
    out
  }

  /// A copy with every problem clamped to the nearest allowed value.
  pub(crate) fn sanitized(&self) -> ParticleEffect {
    let mut fx = self.clone();
    let num = |v: f32, default: f32| if v.is_finite() { v } else { default };
    let non_negative = |v: f32, default: f32| num(v, default).max(0.0);
    let range = |r: ValueRange, default: ValueRange| {
      if r.min.is_finite() && r.max.is_finite() {
        r
      } else {
        default
      }
    };
    let non_negative_range = |r: ValueRange, default: ValueRange| {
      let r = range(r, default);
      ValueRange::new(r.min.max(0.0), r.max.max(0.0))
    };

    fx.rate = non_negative(fx.rate, 10.0);
    fx.rate_per_distance = non_negative(fx.rate_per_distance, 0.0);
    fx.duration = num(fx.duration, 1.0).max(0.01);
    fx.start_delay = non_negative(fx.start_delay, 0.0);
    fx.prewarm = fx.prewarm && fx.looping;
    fx.max_particles = fx.max_particles.max(1);
    fx.direction_deg = num(fx.direction_deg, 0.0);
    fx.spread_deg = num(fx.spread_deg, 360.0).clamp(0.0, 360.0);
    let lifetime = range(fx.lifetime, ValueRange::new(1.0, 2.0));
    fx.lifetime = ValueRange::new(lifetime.min.max(0.01), lifetime.max);
    fx.speed = non_negative_range(fx.speed, ValueRange::new(0.0, 50.0));
    fx.size = non_negative_range(fx.size, ValueRange::new(8.0, 16.0));
    fx.rotation_deg = range(fx.rotation_deg, ValueRange::constant(0.0));
    fx.angular_velocity_deg = range(fx.angular_velocity_deg, ValueRange::constant(0.0));
    fx.inherit_velocity = num(fx.inherit_velocity, 0.0);
    fx.gravity = Vec2::new(num(fx.gravity.x, 0.0), num(fx.gravity.y, 0.0));
    fx.drag = non_negative(fx.drag, 0.0);
    fx.radial_acceleration = range(fx.radial_acceleration, ValueRange::constant(0.0));
    fx.tangential_acceleration = range(fx.tangential_acceleration, ValueRange::constant(0.0));
    if let Some(noise) = &mut fx.noise {
      noise.strength = num(noise.strength, 0.0);
      noise.frequency = non_negative(noise.frequency, 0.0);
      noise.speed = non_negative(noise.speed, 0.0);
    }
    if let Some(frames) = &mut fx.frames {
      frames.columns = frames.columns.max(1);
      frames.rows = frames.rows.max(1);
      if frames.count > frames.frames() {
        frames.count = 0;
      }
      if let FrameMode::Fps(fps) = frames.mode
        && !(fps.is_finite() && fps > 0.0)
      {
        frames.mode = FrameMode::OverLife;
      }
    }
    if let Align::Velocity { stretch } = &mut fx.align {
      *stretch = non_negative(*stretch, 0.0);
    }
    fx.emission = non_negative(fx.emission, 1.0);
    if fx.space == Space::Local {
      fx.collision = None;
    }

    for burst in &mut fx.bursts {
      burst.time = num(burst.time, 0.0);
      burst.count = range(burst.count, ValueRange::constant(0.0));
      burst.interval = num(burst.interval, 0.0);
      if burst.cycles != 1 && burst.interval <= 0.0 {
        burst.interval = 0.01;
      }
    }

    fx.shape = match fx.shape {
      EmitShape::Point => EmitShape::Point,
      EmitShape::Circle {
        radius,
        arc_deg,
        thickness,
      } => EmitShape::Circle {
        radius: non_negative(radius, 0.0),
        arc_deg: num(arc_deg, 360.0).clamp(0.0, 360.0),
        thickness: num(thickness, 1.0).clamp(0.0, 1.0),
      },
      EmitShape::Rect { size, thickness } => EmitShape::Rect {
        size: Vec2::new(non_negative(size.x, 0.0), non_negative(size.y, 0.0)),
        thickness: num(thickness, 1.0).clamp(0.0, 1.0),
      },
      EmitShape::Line { length } => EmitShape::Line {
        length: non_negative(length, 0.0),
      },
    };
    fx
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::Assets;

  fn image() -> Handle<Image> {
    Assets::<Image>::new().add(Image::solid_rect(8, 8, 255, 255, 255, 255))
  }

  fn starts_with(problems: &[String], prefix: &str) -> bool {
    problems.iter().any(|p| p.starts_with(prefix))
  }

  #[test]
  fn defaults_match_the_table() {
    let fx = ParticleEffect::new(image());
    assert_eq!(fx.rate, 10.0);
    assert_eq!(fx.rate_per_distance, 0.0);
    assert!(fx.bursts.is_empty());
    assert_eq!(fx.duration, 1.0);
    assert!(fx.looping);
    assert_eq!(fx.start_delay, 0.0);
    assert!(!fx.prewarm);
    assert_eq!(fx.max_particles, 1000);
    assert_eq!(fx.shape, EmitShape::Point);
    assert_eq!(fx.direction_deg, 0.0);
    assert_eq!(fx.spread_deg, 360.0);
    assert!(!fx.outward);
    assert_eq!(fx.lifetime, ValueRange::new(1.0, 2.0));
    assert_eq!(fx.speed, ValueRange::new(0.0, 50.0));
    assert_eq!(fx.size, ValueRange::new(8.0, 16.0));
    assert_eq!(fx.rotation_deg, ValueRange::constant(0.0));
    assert_eq!(fx.angular_velocity_deg, ValueRange::constant(0.0));
    assert_eq!(fx.inherit_velocity, 0.0);
    assert_eq!(fx.color, ColorRange::constant(Color::WHITE));
    assert_eq!(
      fx.color_over_life,
      Gradient::new([(0.0, Color::WHITE), (1.0, Color::TRANSPARENT)])
    );
    assert_eq!(fx.size_over_life, Curve::constant(1.0));
    assert_eq!(fx.speed_over_life, Curve::constant(1.0));
    assert_eq!(fx.gravity, Vec2::ZERO);
    assert_eq!(fx.drag, 0.0);
    assert_eq!(fx.space, Space::World);
    assert_eq!(fx.blend, BlendMode::Alpha);
    assert_eq!(fx.filter, Filter::Default);
    assert!(fx.lit);
    assert_eq!(fx.emission, 1.0);
    assert!(fx.effect.is_none());
    assert_eq!(fx.z, 0);
    assert_eq!(fx.draw_order, DrawOrder::NewestOnTop);
    assert!(fx.collision.is_none());
    assert_eq!(fx.layer, 0);
    assert!(fx.seed.is_none());
    assert!(fx.problems().is_empty());
  }

  #[test]
  fn problems_name_each_bad_field() {
    let fx = ParticleEffect::new(image()).with_lifetime((f32::NAN, 2.0));
    assert!(starts_with(&fx.problems(), "lifetime:"));
    let fx = ParticleEffect::new(image()).with_rate(-1.0);
    assert!(starts_with(&fx.problems(), "rate:"));
    let fx = ParticleEffect::new(image()).with_duration(0.0);
    assert!(starts_with(&fx.problems(), "duration:"));
    let fx = ParticleEffect::new(image()).with_rate_per_distance(-1.0);
    assert!(starts_with(&fx.problems(), "rate_per_distance:"));
    let fx = ParticleEffect::new(image()).with_inherit_velocity(f32::NAN);
    assert!(starts_with(&fx.problems(), "inherit_velocity:"));
    let fx = ParticleEffect::new(image()).with_burst(Burst::at(0.0, 5.0).with_cycles(0, 0.0));
    assert!(starts_with(&fx.problems(), "bursts:"));
    let fx = ParticleEffect::new(image()).with_shape(EmitShape::Circle {
      radius: 1.0,
      arc_deg: 360.0,
      thickness: 2.0,
    });
    assert!(starts_with(&fx.problems(), "shape:"));
  }

  #[test]
  fn bad_sheets_and_alignment_are_reported_and_clamped() {
    let bad = [
      ParticleEffect::new(image()).with_frames(Flipbook::new(0, 2)),
      ParticleEffect::new(image()).with_frames(Flipbook::new(2, 0)),
      ParticleEffect::new(image()).with_frames(Flipbook::new(2, 2).with_count(5)),
      ParticleEffect::new(image()).with_frames(Flipbook::new(2, 2).at_fps(0.0)),
      ParticleEffect::new(image()).with_frames(Flipbook::new(2, 2).at_fps(f32::NAN)),
    ];
    for fx in bad {
      assert!(starts_with(&fx.problems(), "frames:"), "{:?}", fx.frames);
      let clean = fx.sanitized();
      assert!(clean.problems().is_empty(), "{:?}", clean.problems());
      assert!(clean.frames.is_some_and(|book| book.frames() >= 1));
    }
    for stretch in [-1.0, f32::NAN] {
      let fx = ParticleEffect::new(image()).with_align(Align::Velocity { stretch });
      assert!(starts_with(&fx.problems(), "align:"));
      assert!(fx.sanitized().problems().is_empty());
    }
  }

  #[test]
  fn sanitized_effects_have_no_problems() {
    let fx = ParticleEffect::new(image())
      .with_rate(-1.0)
      .with_rate_per_distance(f32::NAN)
      .with_inherit_velocity(f32::INFINITY)
      .with_duration(0.0)
      .with_start_delay(-2.0)
      .with_max_particles(0)
      .with_direction(f32::NAN, 400.0)
      .with_lifetime((0.0, 3.0))
      .with_speed((-5.0, f32::NAN))
      .with_size((-1.0, 4.0))
      .with_rotation((f32::NAN, 1.0))
      .with_angular_velocity(f32::NEG_INFINITY)
      .with_gravity(f32::NAN, 1.0)
      .with_drag(-1.0)
      .with_emission(f32::NAN)
      .with_burst(Burst::at(f32::NAN, (f32::NAN, 1.0)).with_cycles(0, 0.0))
      .with_burst(Burst::at(0.0, 3.0).with_cycles(4, -1.0))
      .with_shape(EmitShape::Rect {
        size: Vec2::new(-1.0, f32::NAN),
        thickness: 2.0,
      });
    assert!(!fx.problems().is_empty());
    let clean = fx.sanitized();
    assert!(clean.problems().is_empty(), "{:?}", clean.problems());
    assert_eq!(clean.lifetime.min, 0.01);
    assert_eq!(clean.bursts[1].interval, 0.01);
    for v in [
      clean.rate,
      clean.rate_per_distance,
      clean.inherit_velocity,
      clean.duration,
      clean.direction_deg,
      clean.speed.min,
      clean.speed.max,
      clean.rotation_deg.min,
      clean.gravity.x,
      clean.emission,
      clean.bursts[0].time,
      clean.bursts[0].count.min,
    ] {
      assert!(v.is_finite());
    }

    let line = ParticleEffect::new(image()).with_shape(EmitShape::Line { length: -3.0 });
    assert!(line.sanitized().problems().is_empty());
    let circle = ParticleEffect::new(image()).with_shape(EmitShape::Circle {
      radius: f32::NAN,
      arc_deg: -4.0,
      thickness: 3.0,
    });
    assert!(circle.sanitized().problems().is_empty());
  }

  #[test]
  fn degrees_are_degrees() {
    let fx = ParticleEffect::new(image()).with_direction(90.0, 10.0);
    assert_eq!(fx.direction_deg, 90.0);
    assert_eq!(fx.spread_deg, 10.0);
  }

  #[test]
  fn force_values_are_checked_and_clamped() {
    let fx = ParticleEffect::new(image())
      .with_radial_acceleration((f32::NAN, 1.0))
      .with_tangential_acceleration(f32::INFINITY)
      .with_noise(NoiseForce::new(f32::NAN, -2.0).with_speed(-1.0));
    assert_eq!(fx.problems().len(), 4);
    let clean = fx.sanitized();
    assert!(clean.problems().is_empty());
    assert_eq!(clean.noise, Some(NoiseForce::new(0.0, 0.0).with_speed(0.0)));

    // Any finite strength is allowed
    let strong = ParticleEffect::new(image()).with_noise(NoiseForce::new(-50.0, 0.1));
    assert!(strong.problems().is_empty());
  }
}
