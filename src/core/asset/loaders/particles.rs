//! Particle effects from `.particles.ron` text

use std::fmt;

use glam::Vec2;
use ron::extensions::Extensions;
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};

use crate::core::asset::{
  Asset, AssetError, AssetLoader, AssetPath, AssetServer, Assets, CheckContext, Handle, LoadContext,
};
use crate::core::ecs::SpriteEffect;
use crate::core::graphics::Image;
use crate::core::particles::{
  Align, Burst, CollisionConfig, ColorRange, Curve, CurveKey, DrawOrder, EmitShape, Flipbook,
  Gradient, NoiseForce, ParticleEffect, Space, SubEmitter, ValueRange,
};
use crate::core::render::batch::{BlendMode, Filter};
use crate::core::ui::Color;
use crate::core::ui::file::build::parse_hex;

/// Loads [`ParticleEffect`]s from `.particles.ron` files. Every field but
/// `image` may be left out, taking [`ParticleEffect::new`]'s value:
///
/// ```ron
/// (
///   image: "fx/spark.png",
///   // Emission
///   rate: 40,                             // 10 per second
///   rate_per_distance: 0.5,               // 0 per pixel the emitter moves
///   bursts: [(time: 0.1, count: (2, 4)), (count: 8, cycles: 3, interval: 0.25)],
///   duration: 2.0,                        // 1 s
///   looping: true,                        // true
///   start_delay: 0.25,                    // 0 s
///   prewarm: true,                        // false (needs looping)
///   max_particles: 500,                   // 1000
///   // Shape
///   shape: Circle(radius: 12, arc_deg: 180, thickness: 0.5), // Point
///   direction_deg: -90,                   // 0: right, 90: down
///   spread_deg: 40,                       // 360
///   outward: false,                       // false
///   // Spawn values: a number, or a random (min, max)
///   lifetime: (0.4, 0.9),                 // (1, 2) s
///   speed: (80, 160),                     // (0, 50)
///   size: 6,                              // (8, 16)
///   rotation_deg: (0, 360),               // 0
///   angular_velocity_deg: 30,             // 0 per second
///   inherit_velocity: 0.25,               // 0
///   color: ("#ffe08a", "#ff7a1a"),        // white; one colour, or a random pick between two
///   // Over life: keys in order, each `t` within 0 to 1
///   color_over_life: [(0.0, "#ffe08a"), (0.6, "#ff7a1a"), (1.0, "#ff000000")], // white to transparent
///   size_over_life: [(0.0, 1.0), (1.0, 0.2, InQuad)], // 1 throughout
///   speed_over_life: [(0.0, 1.0), (1.0, 0.5)],        // 1 throughout
///   // Motion
///   gravity: (0, 300),                    // (0, 0)
///   drag: 0.5,                            // 0
///   radial_acceleration: (-10, 10),       // 0
///   tangential_acceleration: 5,           // 0
///   noise: (strength: 40, frequency: 0.02, speed: 2), // none (speed: 1)
///   space: World,                         // World or Local
///   // Look
///   blend: Additive,                      // Alpha, Premultiplied, Additive, Multiply, Screen
///   filter: Nearest,                      // Default, Nearest, Linear
///   lit: false,                           // true
///   emission: 2.5,                        // 1
///   effect: Flash(color: (r: 255, g: 0, b: 0, a: 255), intensity: 0.5), // none
///   z: 7,                                 // 0
///   draw_order: OldestOnTop,              // NewestOnTop
///   frames: (columns: 4, rows: 2, count: 6, mode: Fps(12.0), random_start: true), // none
///   align: Velocity(stretch: 0.02),       // None
///   // Collision and sub-effects
///   collision: (radius: 2, mask: 3, response: Bounce(0.6), events: true), // none
///   on_death: (effect: "fx/puff.particles.ron", count: 3, inherit_velocity: 0.5), // none
///   on_collision: (effect: "fx/puff.particles.ron", count: (2, 4)),               // none
///   layer: 2,                             // 0
///   seed: 77,                             // none
/// )
/// ```
///
/// A colour is written `"#rrggbb"`, `"#rrggbbaa"` or `(r:, g:, b:, a:)`, its
/// `a` 255 when left out (in `color`, a colour pair and a gradient's keys).
/// A burst's `time`, `cycles` and `interval` default to 0, 1 and 0. A
/// collision's fields are all optional: `radius` is 4, `mask` every layer,
/// `response` `Die`, `Bounce(0.6)` (the restitution, 0 to 1), `Stick` or
/// `Event`, and `events` false. An option is written without `Some(..)`. Unknown
/// fields are refused, so a misspelt one fails rather than doing nothing.
///
/// `image` and the sub-effects are asset paths, and the effect's
/// dependencies: it is ready once they are, one that does not load fails it
/// (`AssetError::Dependency`), and saving one reloads this effect. A
/// sub-effect that has sub-emitters of its own loads with a warning, those
/// being ignored. A mistake fails the load with `AssetError::Decode`: a RON
/// error at its line and column, a bad value (`ParticleEffect`'s rules, and
/// curve and gradient keys out of range or order) naming its field. A failed
/// reload keeps the last good effect. Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct ParticleEffectLoader;

impl AssetLoader for ParticleEffectLoader {
  type Asset = ParticleEffect;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["particles.ron"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    _: &(),
    cx: &mut LoadContext<'_>,
  ) -> Result<ParticleEffect, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    let mut file: EffectFile = ron::Options::default()
      .with_default_extension(Extensions::IMPLICIT_SOME)
      .from_str(&text)
      .map_err(|error| {
        AssetError::decode(
          &path,
          format!(
            "line {}, column {}: {}",
            error.position.line, error.position.col, error.code
          ),
        )
      })?;
    file
      .check()
      .map_err(|error| AssetError::decode(&path, error))?;
    let image = dependency::<Image>(cx, &path, "image", &file.image)?;
    let on_death = sub_emitter(cx, &path, "on_death", file.on_death.take())?;
    let on_collision = sub_emitter(cx, &path, "on_collision", file.on_collision.take())?;
    let effect = file.into_effect(image, on_death, on_collision);
    let problems = effect.problems();
    if !problems.is_empty() {
      return Err(AssetError::decode(&path, problems.join("; ")));
    }
    Ok(effect)
  }

  /// Warns about a sub-effect with sub-emitters of its own, which are ignored
  /// (sub-effects go one level deep)
  fn check(&self, effect: &ParticleEffect, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    let Some(effects) = cx.world().get_resource::<Assets<ParticleEffect>>() else {
      return Ok(());
    };
    let server = cx.world().get_resource::<AssetServer>();
    let named = |sub: &SubEmitter| {
      server
        .and_then(|server| server.path(&sub.effect))
        .map_or_else(|| format!("{:?}", sub.effect.id()), |path| path.to_string())
    };
    for (field, sub) in [
      ("on_death", &effect.on_death),
      ("on_collision", &effect.on_collision),
    ] {
      let Some(sub) = sub else { continue };
      let Some(played) = effects.get(&sub.effect) else {
        continue;
      };
      if played.on_death.is_some() || played.on_collision.is_some() {
        crate::engine_warn!(
          module: "Particles",
          "{}: {field} plays \"{}\", which has sub-emitters of its own: those are ignored (sub-effects go one level deep)",
          cx.path(),
          named(sub)
        );
      }
    }
    Ok(())
  }
}

/// The dependency at `file`, which the effect at `path` names in `field`
fn dependency<T: Asset>(
  cx: &mut LoadContext<'_>,
  path: &AssetPath,
  field: &str,
  file: &str,
) -> Result<Handle<T>, AssetError> {
  cx.load::<T>(file).map_err(|error| match error {
    AssetError::Dependency { .. } => error,
    // A path the registry refused (one that leaves the asset root, or no
    // loader for it) is the file's mistake
    refused => AssetError::decode(path, format!("{field}: {file:?}: {}", refused.report())),
  })
}

fn sub_emitter(
  cx: &mut LoadContext<'_>,
  path: &AssetPath,
  field: &str,
  file: Option<SubEmitterFile>,
) -> Result<Option<SubEmitter>, AssetError> {
  let Some(file) = file else {
    return Ok(None);
  };
  let effect = dependency::<ParticleEffect>(cx, path, field, &file.effect)?;
  Ok(Some(
    SubEmitter::new(effect, file.count).with_inherit_velocity(file.inherit_velocity),
  ))
}

/// A `.particles.ron` file, as written: one field per field of
/// [`ParticleEffect`], with its defaults
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename = "ParticleEffect")]
struct EffectFile {
  image: String,
  #[serde(default = "defaults::rate")]
  rate: f32,
  #[serde(default)]
  rate_per_distance: f32,
  #[serde(default)]
  bursts: Vec<Burst>,
  #[serde(default = "defaults::duration")]
  duration: f32,
  #[serde(default = "defaults::yes")]
  looping: bool,
  #[serde(default)]
  start_delay: f32,
  #[serde(default)]
  prewarm: bool,
  #[serde(default = "defaults::max_particles")]
  max_particles: u32,
  #[serde(default)]
  shape: EmitShape,
  #[serde(default)]
  direction_deg: f32,
  #[serde(default = "defaults::spread_deg")]
  spread_deg: f32,
  #[serde(default)]
  outward: bool,
  #[serde(default = "defaults::lifetime")]
  lifetime: ValueRange,
  #[serde(default = "defaults::speed")]
  speed: ValueRange,
  #[serde(default = "defaults::size")]
  size: ValueRange,
  #[serde(default = "defaults::zero")]
  rotation_deg: ValueRange,
  #[serde(default = "defaults::zero")]
  angular_velocity_deg: ValueRange,
  #[serde(default)]
  inherit_velocity: f32,
  #[serde(default = "defaults::color")]
  color: ColorsFile,
  #[serde(default = "defaults::color_over_life")]
  color_over_life: Vec<(f32, ColorFile)>,
  #[serde(default = "defaults::flat_curve")]
  size_over_life: Vec<CurveKey>,
  #[serde(default = "defaults::flat_curve")]
  speed_over_life: Vec<CurveKey>,
  #[serde(default)]
  gravity: Vec2,
  #[serde(default)]
  drag: f32,
  #[serde(default = "defaults::zero")]
  radial_acceleration: ValueRange,
  #[serde(default = "defaults::zero")]
  tangential_acceleration: ValueRange,
  #[serde(default)]
  noise: Option<NoiseForce>,
  #[serde(default)]
  space: Space,
  #[serde(default)]
  blend: BlendMode,
  #[serde(default)]
  filter: Filter,
  #[serde(default = "defaults::yes")]
  lit: bool,
  #[serde(default = "defaults::emission")]
  emission: f32,
  #[serde(default)]
  effect: Option<SpriteEffect>,
  #[serde(default)]
  z: i32,
  #[serde(default)]
  draw_order: DrawOrder,
  #[serde(default)]
  frames: Option<Flipbook>,
  #[serde(default)]
  align: Align,
  #[serde(default)]
  collision: Option<CollisionConfig>,
  #[serde(default)]
  on_death: Option<SubEmitterFile>,
  #[serde(default)]
  on_collision: Option<SubEmitterFile>,
  #[serde(default)]
  layer: u32,
  #[serde(default)]
  seed: Option<u64>,
}

/// A sub-emitter as a file writes it, the effect a path
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename = "SubEmitter")]
struct SubEmitterFile {
  effect: String,
  count: ValueRange,
  #[serde(default)]
  inherit_velocity: f32,
}

/// [`ParticleEffect::new`]'s values, for the fields a file leaves out
mod defaults {
  use super::{ColorFile, ColorsFile};
  use crate::core::ecs::Ease;
  use crate::core::particles::{CurveKey, ValueRange};
  use crate::core::ui::Color;

  pub(super) fn rate() -> f32 {
    10.0
  }

  pub(super) fn duration() -> f32 {
    1.0
  }

  pub(super) fn yes() -> bool {
    true
  }

  pub(super) fn max_particles() -> u32 {
    1000
  }

  pub(super) fn spread_deg() -> f32 {
    360.0
  }

  pub(super) fn lifetime() -> ValueRange {
    ValueRange::new(1.0, 2.0)
  }

  pub(super) fn speed() -> ValueRange {
    ValueRange::new(0.0, 50.0)
  }

  pub(super) fn size() -> ValueRange {
    ValueRange::new(8.0, 16.0)
  }

  pub(super) fn zero() -> ValueRange {
    ValueRange::constant(0.0)
  }

  pub(super) fn color() -> ColorsFile {
    ColorsFile::One(ColorFile(Color::WHITE))
  }

  pub(super) fn color_over_life() -> Vec<(f32, ColorFile)> {
    vec![
      (0.0, ColorFile(Color::WHITE)),
      (1.0, ColorFile(Color::TRANSPARENT)),
    ]
  }

  pub(super) fn flat_curve() -> Vec<CurveKey> {
    vec![CurveKey {
      t: 0.0,
      value: 1.0,
      ease: Ease::Linear,
    }]
  }

  pub(super) fn emission() -> f32 {
    1.0
  }
}

impl EffectFile {
  /// What neither RON nor [`ParticleEffect::problems`] can say is wrong: a
  /// curve's or gradient's keys, which the effect would otherwise clamp and
  /// sort, must be in order within 0 to 1
  fn check(&self) -> Result<(), String> {
    let mut problems = Vec::new();
    let curves = [
      ("size_over_life", &self.size_over_life),
      ("speed_over_life", &self.speed_over_life),
    ];
    for (field, keys) in curves {
      problems.extend(check_keys(field, keys.iter().map(|key| key.t)));
      if let Some(key) = keys.iter().find(|key| !key.value.is_finite()) {
        problems.push(format!(
          "{field}: a key's value {} is not a number",
          key.value
        ));
      }
    }
    let gradient = self.color_over_life.iter().map(|(t, _)| *t);
    problems.extend(check_keys("color_over_life", gradient));
    if problems.is_empty() {
      Ok(())
    } else {
      Err(problems.join("; "))
    }
  }

  /// The effect the file describes, with its loaded dependencies
  fn into_effect(
    self,
    image: Handle<Image>,
    on_death: Option<SubEmitter>,
    on_collision: Option<SubEmitter>,
  ) -> ParticleEffect {
    let color = match self.color {
      ColorsFile::One(color) => ColorRange::constant(color.0),
      ColorsFile::Pair(a, b) => ColorRange::new(a.0, b.0),
    };
    let color_over_life =
      Gradient::new(self.color_over_life.iter().map(|(t, color)| (*t, color.0)));
    ParticleEffect {
      rate: self.rate,
      rate_per_distance: self.rate_per_distance,
      bursts: self.bursts,
      duration: self.duration,
      looping: self.looping,
      start_delay: self.start_delay,
      prewarm: self.prewarm,
      max_particles: self.max_particles,
      shape: self.shape,
      direction_deg: self.direction_deg,
      spread_deg: self.spread_deg,
      outward: self.outward,
      lifetime: self.lifetime,
      speed: self.speed,
      size: self.size,
      rotation_deg: self.rotation_deg,
      angular_velocity_deg: self.angular_velocity_deg,
      inherit_velocity: self.inherit_velocity,
      color,
      color_over_life,
      size_over_life: Curve::from(self.size_over_life),
      speed_over_life: Curve::from(self.speed_over_life),
      gravity: self.gravity,
      drag: self.drag,
      radial_acceleration: self.radial_acceleration,
      tangential_acceleration: self.tangential_acceleration,
      noise: self.noise,
      space: self.space,
      image,
      blend: self.blend,
      filter: self.filter,
      lit: self.lit,
      emission: self.emission,
      effect: self.effect,
      z: self.z,
      draw_order: self.draw_order,
      frames: self.frames,
      align: self.align,
      collision: self.collision,
      on_death,
      on_collision,
      layer: self.layer,
      seed: self.seed,
    }
  }
}

/// Keys at times `ts`: within 0 to 1, in order
fn check_keys(field: &str, ts: impl Iterator<Item = f32>) -> Vec<String> {
  let mut problems = Vec::new();
  let mut last = f32::NEG_INFINITY;
  for t in ts {
    if !(0.0..=1.0).contains(&t) {
      problems.push(format!("{field}: the key at {t} is not within 0 to 1"));
    } else if t < last {
      problems.push(format!(
        "{field}: the key at {t} comes after the key at {last}"
      ));
    }
    if t.is_finite() {
      last = t;
    }
  }
  problems
}

/// A colour as a file writes it: `"#rrggbb"`, `"#rrggbbaa"` or
/// `(r:, g:, b:, a:)`, its `a` 255 when left out
#[derive(Clone, Copy)]
struct ColorFile(Color);

const COLOR_FORMS: &str = "a colour (\"#rrggbb\", \"#rrggbbaa\" or (r:, g:, b:, a:))";

/// A colour read from a hex string
fn hex_color<E: de::Error>(text: &str, expected: &dyn de::Expected) -> Result<Color, E> {
  parse_hex(text).ok_or_else(|| E::invalid_value(Unexpected::Str(text), expected))
}

/// A colour read from its fields, its `a` 255 when left out
fn field_color<'de, A: MapAccess<'de>>(mut map: A) -> Result<Color, A::Error> {
  const FIELDS: [&str; 4] = ["r", "g", "b", "a"];
  let mut channels = [None; 4];
  while let Some(key) = map.next_key::<String>()? {
    let Some(index) = FIELDS.iter().position(|field| *field == key) else {
      return Err(de::Error::custom(format!(
        "a colour has the fields r, g, b and a, not `{key}`"
      )));
    };
    if channels[index].is_some() {
      return Err(de::Error::custom(format!(
        "a colour's `{key}` is written twice"
      )));
    }
    channels[index] = Some(map.next_value::<u8>()?);
  }
  let channel = |index: usize| -> Result<u8, A::Error> {
    channels[index].ok_or_else(|| {
      de::Error::custom(format!(
        "a colour needs `{}` (its fields are r, g, b and a)",
        FIELDS[index]
      ))
    })
  };
  Ok(Color::new(
    channel(0)?,
    channel(1)?,
    channel(2)?,
    channels[3].unwrap_or(255),
  ))
}

impl<'de> Deserialize<'de> for ColorFile {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct ColorVisitor;

    impl<'de> Visitor<'de> for ColorVisitor {
      type Value = ColorFile;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(COLOR_FORMS)
      }

      fn visit_str<E: de::Error>(self, text: &str) -> Result<ColorFile, E> {
        hex_color(text, &self).map(ColorFile)
      }

      fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ColorFile, A::Error> {
        field_color(map).map(ColorFile)
      }
    }

    deserializer.deserialize_any(ColorVisitor)
  }
}

/// A `color`: one colour, or a random pick between two
#[derive(Clone, Copy)]
enum ColorsFile {
  One(ColorFile),
  Pair(ColorFile, ColorFile),
}

impl<'de> Deserialize<'de> for ColorsFile {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct ColorsVisitor;

    impl<'de> Visitor<'de> for ColorsVisitor {
      type Value = ColorsFile;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(COLOR_FORMS)?;
        f.write_str(", or a pair of them")
      }

      fn visit_str<E: de::Error>(self, text: &str) -> Result<ColorsFile, E> {
        hex_color(text, &self).map(|color| ColorsFile::One(ColorFile(color)))
      }

      fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ColorsFile, A::Error> {
        field_color(map).map(|color| ColorsFile::One(ColorFile(color)))
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ColorsFile, A::Error> {
        let a = seq
          .next_element::<ColorFile>()?
          .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let b = seq
          .next_element::<ColorFile>()?
          .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        if seq.next_element::<IgnoredAny>()?.is_some() {
          return Err(de::Error::invalid_length(3, &self));
        }
        Ok(ColorsFile::Pair(a, b))
      }
    }

    deserializer.deserialize_any(ColorsVisitor)
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use crate::core::asset::{
    AssetEvent, AssetServer, AssetSource, Assets, AssetsView, MemorySource, insert_registry, pump,
  };
  use crate::core::dev_log;
  use crate::core::ecs::kernel::World;
  use crate::core::ecs::{Ease, Messages, SpriteEffect, Vec2};
  use crate::core::graphics::Image;
  use crate::core::particles::{
    Align, Burst, CollisionConfig, CollisionResponse, ColorRange, Curve, DrawOrder, EmitShape,
    Flipbook, FrameMode, Gradient, NoiseForce, ParticleEffect, Space, SubEmitter, ValueRange,
  };
  use crate::core::render::batch::{BlendMode, Filter};
  use crate::core::ui::Color;

  const PNG: &str = "fx/spark.png";
  const PATH: &str = "fx/test.particles.ron";

  fn png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255; 4]));
    let mut bytes = Vec::new();
    if let Err(error) = image.write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    ) {
      panic!("encoding a png: {error}");
    }
    bytes
  }

  fn registry_over(source: impl AssetSource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    world
  }

  /// The image, and the texts at their paths
  fn files(texts: &[(&str, &str)]) -> MemorySource {
    let files = MemorySource::new();
    files.insert(PNG, png());
    for (path, text) in texts {
      files.insert(path, *text);
    }
    files
  }

  /// The effect at `path` among `texts`, loaded at once
  fn load(path: &str, texts: &[(&str, &str)]) -> Result<ParticleEffect, String> {
    let mut world = registry_over(files(texts));
    let mut assets = AssetsView::new(&mut world);
    let effect = assets
      .load_now::<ParticleEffect>(path)
      .map_err(|e| e.report())?;
    assets
      .get(&effect)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn loaded(text: &str) -> ParticleEffect {
    match load(PATH, &[(PATH, text)]) {
      Ok(effect) => effect,
      Err(error) => panic!("{error}"),
    }
  }

  fn failure(text: &str) -> String {
    match load(PATH, &[(PATH, text)]) {
      Ok(effect) => panic!("{text} loaded: {effect:?}"),
      Err(error) => error,
    }
  }

  /// Sets every field of the file, each away from its default
  const SAMPLE: &str = r##"(
    image: "fx/spark.png",
    rate: 40,
    rate_per_distance: 0.5,
    bursts: [
      (time: 0.1, count: (2, 4)),
      (time: 0.5, count: 8, cycles: 3, interval: 0.25),
    ],
    duration: 2.0,
    looping: true,
    start_delay: 0.25,
    prewarm: true,
    max_particles: 500,
    shape: Circle(radius: 12, arc_deg: 180, thickness: 0.5),
    direction_deg: -90,
    spread_deg: 40,
    outward: true,
    lifetime: (0.4, 0.9),
    speed: (80, 160),
    size: 6,
    rotation_deg: (0, 360),
    angular_velocity_deg: 30,
    inherit_velocity: 0.25,
    color: ("#ffe08a", (r: 255, g: 122, b: 26)),
    color_over_life: [(0.0, "#ffe08a"), (0.6, "#ff7a1a"), (1.0, "#ff000000")],
    size_over_life: [(0.0, 1.0), (1.0, 0.2, InQuad)],
    speed_over_life: [(0.0, 1.0), (1.0, 0.5)],
    gravity: (0, 300),
    drag: 0.5,
    radial_acceleration: (-10, 10),
    tangential_acceleration: 5,
    noise: (strength: 40, frequency: 0.02, speed: 2),
    space: World,
    blend: Additive,
    filter: Nearest,
    lit: false,
    emission: 2.5,
    effect: Flash(color: (r: 255, g: 0, b: 0, a: 255), intensity: 0.5),
    z: 7,
    draw_order: OldestOnTop,
    frames: (columns: 4, rows: 2, count: 6, mode: Fps(12.0), random_start: true),
    align: Velocity(stretch: 0.02),
    collision: (radius: 2, mask: 3, response: Bounce(0.6), events: true),
    on_death: (effect: "fx/puff.particles.ron", count: 3, inherit_velocity: 0.5),
    on_collision: (effect: "fx/puff.particles.ron", count: (2, 4)),
    layer: 2,
    seed: 77,
  )"##;

  const PUFF: &str = r#"(image: "fx/spark.png", lifetime: 0.5)"#;

  #[test]
  fn a_sample_file_loads_every_field() {
    let mut world = registry_over(files(&[(PATH, SAMPLE), ("fx/puff.particles.ron", PUFF)]));
    let mut assets = AssetsView::new(&mut world);
    let effect = match assets.load_now::<ParticleEffect>(PATH) {
      Ok(effect) => effect,
      Err(error) => panic!("{}", error.report()),
    };
    let image = match assets.load_now::<Image>(PNG) {
      Ok(image) => image,
      Err(error) => panic!("{}", error.report()),
    };
    let puff = match assets.load_now::<ParticleEffect>("fx/puff.particles.ron") {
      Ok(puff) => puff,
      Err(error) => panic!("{}", error.report()),
    };
    let expected = ParticleEffect::new(image)
      .with_rate(40.0)
      .with_rate_per_distance(0.5)
      .with_burst(Burst::at(0.1, (2.0, 4.0)))
      .with_burst(Burst::at(0.5, 8.0).with_cycles(3, 0.25))
      .with_duration(2.0)
      .with_looping(true)
      .with_start_delay(0.25)
      .with_prewarm(true)
      .with_max_particles(500)
      .with_shape(EmitShape::Circle {
        radius: 12.0,
        arc_deg: 180.0,
        thickness: 0.5,
      })
      .with_direction(-90.0, 40.0)
      .outward()
      .with_lifetime((0.4, 0.9))
      .with_speed((80.0, 160.0))
      .with_size(6.0)
      .with_rotation((0.0, 360.0))
      .with_angular_velocity(30.0)
      .with_inherit_velocity(0.25)
      .with_color(ColorRange::new(
        Color::rgb(255, 224, 138),
        Color::rgb(255, 122, 26),
      ))
      .with_color_over_life(Gradient::new([
        (0.0, Color::rgb(255, 224, 138)),
        (0.6, Color::rgb(255, 122, 26)),
        (1.0, Color::new(255, 0, 0, 0)),
      ]))
      .with_size_over_life(Curve::eased([
        (0.0, 1.0, Ease::Linear),
        (1.0, 0.2, Ease::InQuad),
      ]))
      .with_speed_over_life(Curve::new([(0.0, 1.0), (1.0, 0.5)]))
      .with_gravity(0.0, 300.0)
      .with_drag(0.5)
      .with_radial_acceleration((-10.0, 10.0))
      .with_tangential_acceleration(5.0)
      .with_noise(NoiseForce::new(40.0, 0.02).with_speed(2.0))
      .with_space(Space::World)
      .with_blend(BlendMode::Additive)
      .with_filter(Filter::Nearest)
      .with_lighting(false)
      .with_emission(2.5)
      .with_effect(SpriteEffect::Flash {
        color: Color::new(255, 0, 0, 255),
        intensity: 0.5,
      })
      .with_z(7)
      .with_draw_order(DrawOrder::OldestOnTop)
      .with_frames(
        Flipbook::new(4, 2)
          .with_count(6)
          .at_fps(12.0)
          .random_start(),
      )
      .with_align(Align::Velocity { stretch: 0.02 })
      .with_collision(
        CollisionConfig::new(2.0)
          .with_mask(3)
          .with_response(CollisionResponse::bounce(0.6))
          .with_events(),
      )
      .on_death(SubEmitter::new(puff.clone(), 3.0).with_inherit_velocity(0.5))
      .on_collision(SubEmitter::new(puff, (2.0, 4.0)))
      .with_layer(2)
      .with_seed(77);
    assert_eq!(assets.get(&effect), Some(&expected));
  }

  #[test]
  fn every_field_but_the_image_has_the_effect_s_default() {
    let mut world = registry_over(files(&[(PATH, r#"(image: "fx/spark.png")"#)]));
    let mut assets = AssetsView::new(&mut world);
    let effect = match assets.load_now::<ParticleEffect>(PATH) {
      Ok(effect) => effect,
      Err(error) => panic!("{}", error.report()),
    };
    let image = match assets.load_now::<Image>(PNG) {
      Ok(image) => image,
      Err(error) => panic!("{}", error.report()),
    };
    assert_eq!(assets.get(&effect), Some(&ParticleEffect::new(image)));
    // The image is the one field a file must give
    assert!(failure("()").contains("image"), "{}", failure("()"));
  }

  /// The form the loader's documentation shows loads as it says
  #[test]
  fn the_documented_form_loads() {
    let source = include_str!("particles.rs");
    let start = source
      .find("/// ```ron")
      .map_or(0, |at| at + "/// ```ron".len());
    let end = source[start..]
      .find("/// ```")
      .map_or(start, |at| start + at);
    let text: String = source[start..end]
      .lines()
      .map(|line| line.trim_start().trim_start_matches("///"))
      .collect::<Vec<_>>()
      .join("\n");
    let effect = match load(
      PATH,
      &[(PATH, text.as_str()), ("fx/puff.particles.ron", PUFF)],
    ) {
      Ok(effect) => effect,
      Err(error) => panic!("{error}\n{text}"),
    };
    assert_eq!(effect.max_particles, 500);
    assert_eq!(effect.frames.map(|frames| frames.columns), Some(4));
  }

  #[test]
  fn colours_read_as_hex_or_fields() {
    let color = |text: &str| loaded(&format!(r#"(image: "fx/spark.png", color: {text})"#)).color;
    assert_eq!(
      color(r##""#ff8000""##),
      ColorRange::constant(Color::new(255, 128, 0, 255))
    );
    assert_eq!(
      color(r##""#FF800080""##),
      ColorRange::constant(Color::new(255, 128, 0, 128)),
      "either case, with alpha"
    );
    assert_eq!(
      color("(r: 1, g: 2, b: 3)"),
      ColorRange::constant(Color::new(1, 2, 3, 255)),
      "alpha defaults to 255"
    );
    assert_eq!(
      color("(r: 1, g: 2, b: 3, a: 4)"),
      ColorRange::constant(Color::new(1, 2, 3, 4))
    );
    assert_eq!(
      color(r##"("#000000", (r: 255, g: 255, b: 255, a: 128))"##),
      ColorRange::new(Color::new(0, 0, 0, 255), Color::new(255, 255, 255, 128))
    );
    let gradient = loaded(
      r##"(image: "fx/spark.png", color_over_life: [(0.0, "#102030"), (1.0, (r: 4, g: 5, b: 6))])"##,
    )
    .color_over_life;
    assert_eq!(
      gradient,
      Gradient::new([
        (0.0, Color::new(16, 32, 48, 255)),
        (1.0, Color::new(4, 5, 6, 255)),
      ])
    );
    for bad in [
      r##""#ff80""##,
      r#""red""#,
      "(r: 1, g: 2)",
      "(r: 1, g: 2, b: 3, c: 4)",
    ] {
      let error = failure(&format!(r#"(image: "fx/spark.png", color: {bad})"#));
      assert!(
        error.starts_with("could not load \"fx/test.particles.ron\""),
        "{error}"
      );
      assert!(error.contains("line 1"), "{bad}: {error}");
    }
  }

  #[test]
  fn values_read_as_numbers_or_ranges() {
    let effect = loaded(
      r#"(image: "fx/spark.png", lifetime: 2, speed: 1.5, size: (3, 4.5), gravity: (0, -9.5))"#,
    );
    assert_eq!(effect.lifetime, ValueRange::constant(2.0));
    assert_eq!(effect.speed, ValueRange::constant(1.5));
    assert_eq!(effect.size, ValueRange::new(3.0, 4.5));
    assert_eq!(effect.gravity, Vec2::new(0.0, -9.5));
    let error = failure(r#"(image: "fx/spark.png", lifetime: "soon")"#);
    assert!(
      error.contains("line 1") && error.contains("a number"),
      "{error}"
    );
  }

  #[test]
  fn shapes_and_nested_fields_take_their_defaults() {
    let effect = loaded(
      r#"(image: "fx/spark.png", shape: Circle(radius: 20), frames: (columns: 2, rows: 3), noise: (strength: 1, frequency: 0.5), align: None)"#,
    );
    assert_eq!(
      effect.shape,
      EmitShape::Circle {
        radius: 20.0,
        arc_deg: 360.0,
        thickness: 1.0,
      }
    );
    assert_eq!(effect.frames, Some(Flipbook::new(2, 3)));
    assert_eq!(effect.noise, Some(NoiseForce::new(1.0, 0.5)));
    assert_eq!(effect.align, Align::None);
    let rect = loaded(
      r#"(image: "fx/spark.png", shape: Rect(size: (4, 8)), frames: (columns: 2, rows: 2, mode: OverLife))"#,
    );
    assert_eq!(
      rect.shape,
      EmitShape::Rect {
        size: Vec2::new(4.0, 8.0),
        thickness: 1.0,
      }
    );
    assert_eq!(rect.frames.map(|book| book.mode), Some(FrameMode::OverLife));
    let burst = loaded(r#"(image: "fx/spark.png", bursts: [(count: 5)])"#);
    assert_eq!(burst.bursts, vec![Burst::at(0.0, 5.0)]);
    let collision = loaded(r#"(image: "fx/spark.png", collision: ())"#);
    assert_eq!(collision.collision, Some(CollisionConfig::default()));
  }

  #[test]
  fn unknown_fields_are_refused_with_their_line() {
    let error = failure(
      r##"(
  image: "fx/spark.png",
  colour: "#ff0000",
)"##,
    );
    assert!(error.contains("colour"), "{error}");
    assert!(error.contains("line 3"), "{error}");
    // Nested forms refuse theirs too
    for nested in [
      r#"(image: "fx/spark.png", bursts: [(count: 1, amount: 2)])"#,
      r#"(image: "fx/spark.png", collision: (bounce: 2))"#,
      r#"(image: "fx/spark.png", on_death: (effect: "fx/puff.particles.ron", count: 1, velocity: 2))"#,
      r#"(image: "fx/spark.png", noise: (strength: 1, frequency: 1, rate: 1))"#,
      r#"(image: "fx/spark.png", frames: (columns: 1, rows: 1, loops: 1))"#,
      r#"(image: "fx/spark.png", shape: Circle(radius: 1, arc: 90))"#,
      r#"(image: "fx/spark.png", align: Velocity(strech: 1))"#,
    ] {
      let error = failure(nested);
      assert!(error.contains("line 1"), "{nested}: {error}");
    }
  }

  #[test]
  fn bad_values_are_refused_naming_the_field() {
    let cases: &[(&str, &str)] = &[
      ("lifetime: -1", "lifetime:"),
      ("lifetime: (0.5, NaN)", "lifetime:"),
      ("rate: -5", "rate:"),
      ("rate: NaN", "rate:"),
      ("duration: 0", "duration:"),
      ("speed: (-1, 2)", "speed:"),
      ("size: inf", "size:"),
      ("spread_deg: 400", "spread_deg:"),
      ("max_particles: 0", "max_particles:"),
      ("drag: -1", "drag:"),
      ("gravity: (NaN, 0)", "gravity:"),
      ("emission: NaN", "emission:"),
      ("prewarm: true, looping: false", "prewarm:"),
      ("frames: (columns: 0, rows: 2)", "frames:"),
      ("frames: (columns: 2, rows: 2, count: 9)", "frames:"),
      ("frames: (columns: 2, rows: 2, mode: Fps(0.0))", "frames:"),
      ("align: Velocity(stretch: -1)", "align:"),
      ("noise: (strength: NaN, frequency: 1)", "noise:"),
      ("shape: Circle(radius: -1)", "shape:"),
      ("shape: Line(length: inf)", "shape:"),
      ("bursts: [(count: NaN)]", "bursts:"),
      ("bursts: [(count: 1, cycles: 0)]", "bursts:"),
      (
        r#"on_death: (effect: "fx/spark.png", count: NaN)"#,
        "on_death:",
      ),
      ("size_over_life: [(1.5, 1.0)]", "size_over_life:"),
      (
        "size_over_life: [(0.8, 1.0), (0.2, 2.0)]",
        "size_over_life:",
      ),
      ("speed_over_life: [(0.0, NaN)]", "speed_over_life:"),
      (
        r##"color_over_life: [(2.0, "#ffffff")]"##,
        "color_over_life:",
      ),
      (
        r##"color_over_life: [(0.5, "#ffffff"), (0.1, "#000000")]"##,
        "color_over_life:",
      ),
      ("collision: (radius: -1)", "collision:"),
      ("collision: (radius: NaN)", "collision:"),
      ("collision: (response: Bounce(1.5))", "collision:"),
      ("collision: (response: Bounce(NaN))", "collision:"),
      ("space: Local, collision: ()", "collision:"),
    ];
    for (field, names) in cases {
      let text = format!(r#"(image: "fx/spark.png", {field})"#);
      let error = failure(&text);
      assert!(
        error.starts_with("could not load \"fx/test.particles.ron\""),
        "{field}: {error}"
      );
      assert!(error.contains(names), "{field}: {error}");
    }
    // A sub-effect's path is a dependency, not its image
    let text = r#"(image: "fx/spark.png", on_death: (effect: "fx/spark.png", count: 1))"#;
    assert!(failure(text).contains("fx/spark.png"), "{}", failure(text));
    // Several problems are named together
    let several = failure(r#"(image: "fx/spark.png", lifetime: -1, rate: -1)"#);
    assert!(
      several.contains("lifetime:") && several.contains("rate:") && several.contains("; "),
      "{several}"
    );
  }

  #[test]
  fn a_ron_error_keeps_its_line_and_column() {
    let error = failure("(\n  image: \"fx/spark.png\",\n  rate: ,\n)");
    assert!(error.contains("line 3, column"), "{error}");
  }

  #[test]
  fn a_missing_image_fails_as_a_dependency() {
    let error = failure(r#"(image: "fx/nowhere.png")"#);
    assert!(error.contains("fx/test.particles.ron"), "{error}");
    assert!(error.contains("fx/nowhere.png"), "{error}");
    // A sub-effect that is missing fails it the same way
    let error =
      failure(r#"(image: "fx/spark.png", on_death: (effect: "fx/lost.particles.ron", count: 1))"#);
    assert!(error.contains("fx/test.particles.ron"), "{error}");
    assert!(error.contains("fx/lost.particles.ron"), "{error}");
    // A path the registry refuses is the file's own mistake
    let error = failure(r#"(image: "../spark.png")"#);
    assert!(error.contains("../spark.png"), "{error}");
  }

  /// Pumps until `done`, for at most 5 s
  fn pump_until(world: &mut World, done: impl Fn(&World) -> bool) {
    let start = Instant::now();
    loop {
      pump(world);
      if done(world) {
        return;
      }
      assert!(
        start.elapsed() < Duration::from_secs(5),
        "timed out waiting"
      );
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  #[test]
  fn a_broken_reload_keeps_the_last_good_effect() {
    let path = "fx/kept.particles.ron";
    let good = r#"(image: "fx/spark.png", rate: 25)"#;
    let source = files(&[(path, good)]);
    let mut world = registry_over(source.clone());
    world.resource::<AssetServer>().start_watching();
    let handle = match AssetsView::new(&mut world).load_now::<ParticleEffect>(path) {
      Ok(handle) => handle,
      Err(error) => panic!("{}", error.report()),
    };
    let version = |world: &World| world.resource::<Assets<ParticleEffect>>().version(&handle);
    let before = version(&world);
    let mut events = world
      .resource::<Messages<AssetEvent<ParticleEffect>>>()
      .get_cursor_current();
    // A broken save, and a save with a bad value: each keeps the last good one
    for broken in [
      "(image: \"fx/spark.png\", rate: ",
      r#"(image: "fx/spark.png", rate: -3)"#,
    ] {
      source.insert(path, broken);
      let mut sent = Vec::new();
      let start = Instant::now();
      while !sent
        .iter()
        .any(|event| matches!(event, AssetEvent::ReloadFailed { .. }))
      {
        assert!(start.elapsed() < Duration::from_secs(5), "{sent:?}");
        std::thread::sleep(Duration::from_millis(1));
        pump(&mut world);
        sent.extend(
          events
            .read(world.resource::<Messages<AssetEvent<ParticleEffect>>>())
            .cloned(),
        );
      }
      assert!(
        matches!(sent.as_slice(), [AssetEvent::ReloadFailed { id, .. }] if *id == handle.id()),
        "ReloadFailed, not Modified: {sent:?}"
      );
      assert_eq!(version(&world), before, "{broken}: the version is kept");
      let kept = world.resource::<Assets<ParticleEffect>>().get(&handle);
      assert_eq!(kept.map(|effect| effect.rate), Some(25.0), "{broken}");
    }
    // Each distinct failure is warned about once, and a good save carries on
    let warned = dev_log::get_entries()
      .into_iter()
      .filter(|entry| entry.message.contains(path))
      .count();
    assert_eq!(warned, 2, "one warning per broken save");
    source.insert(path, r#"(image: "fx/spark.png", rate: 90)"#);
    pump_until(&mut world, |world| version(world) != before);
    let reloaded = world.resource::<Assets<ParticleEffect>>().get(&handle);
    assert_eq!(reloaded.map(|effect| effect.rate), Some(90.0));
  }
}
