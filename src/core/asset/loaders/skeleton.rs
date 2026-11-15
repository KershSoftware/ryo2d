//! Skeletons from `.skeleton.ron` text

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use ron::extensions::Extensions;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::particles::{ColorFile, dependency};
use crate::core::asset::{AssetError, AssetLoader, Assets, CheckContext, Handle, LoadContext};
use crate::core::ecs::Vec2;
use crate::core::graphics::Image;
use crate::core::skeleton::{
  Animation, Attachment, BoneSetup, BoundingBoxAttachment, ConstraintId, Curve, EventAudio,
  EventData, EventValues, Frame, IkBend, IkSetup, IkValues, Influence, Inherit, MeshAttachment,
  MeshVertices, PointAttachment, Region, RegionAttachment, SkeletonData, SkeletonError, SlotBlend,
  SlotId, SlotSetup, Timeline, TransformMix, TransformOffset, TransformSetup, WeightedVertices,
};
use crate::core::ui::Color;

/// Loads [`SkeletonData`] from `.skeleton.ron` files. Every field may be left
/// out, taking the value in the comment; a file that names nothing is an empty
/// skeleton. Angles are degrees (y down, positive clockwise), lengths are in
/// the bones' units, times are seconds, and colours are written as the
/// particle files write them (`"#rrggbb"`, `"#rrggbbaa"` or `(r:, g:, b:, a:)`,
/// the `a` 255 when left out). An option is written without `Some(..)`:
///
/// ```ron
/// (
///   // Images by name: paths from the asset root, as every other asset file writes them
///   images: { "parts": "rigs/hero.png", "gear": "rigs/gear/extra.png" },
///   // Regions by name: an image and its pixel rectangle (x, y, width, height)
///   regions: {
///     "torso": (image: "parts", rect: (64, 0, 48, 80)),
///     "gear": (image: "gear", rect: (0, 0, 16, 16)),
///   },
///   // Bones, parents first
///   bones: [
///     (name: "root"),
///     (name: "torso", parent: "root", length: 40,             // no parent, 0
///      x: 0, y: -40, rotation: 90,                            // 0, 0, 0
///      scale_x: 1, scale_y: 1, shear_x: 0, shear_y: 0,        // 1, 1, 0, 0
///      inherit: Normal),                                      // Normal, OnlyTranslation,
///                                                             // NoRotationOrReflection,
///                                                             // NoScale, NoScaleOrReflection
///     (name: "arm", parent: "torso", length: 30),
///     (name: "aim", parent: "root"),                          // a target for the constraints
///   ],
///   // Slots in setup draw order, back to front
///   slots: [
///     (name: "torso", bone: "torso", attachment: "torso",     // no attachment
///      color: "#ffffff", blend: Normal),                      // white; Normal, Additive,
///                                                             // Multiply, Screen
///     (name: "gear", bone: "arm"),
///   ],
///   // Skins, each slot's attachments by name; "default" is always there
///   skins: {
///     "default": {
///       "torso": {
///         "torso": Region(region: "torso",
///                         x: 0, y: -20, rotation: 0,          // 0, 0, 0
///                         scale_x: 1, scale_y: 1,             // 1, 1
///                         width: 48, height: 80,              // the region's size
///                         color: "#ffffff"),                  // white
///       },
///       "gear": {
///         "muzzle": Point(x: 3, y: 4, rotation: 90),
///         "hit": BoundingBox(vertices: [(0, 0), (10, 0), (10, 10)]),
///         // Hull is how many of the first vertices outline the mesh (0 for none).
///         // UVs run 0 to 1 across the region; triangles index the vertices
///         "cape": Mesh(
///           region: "torso",
///           uvs: [(0, 0), (1, 0), (1, 1), (0, 1)],
///           triangles: [0, 1, 2, 0, 2, 3],
///           hull: 4,                                          // 0
///           // A vertex is weighted over bones, each (bone, x, y, weight): x and y are
///           // where it sits in that bone's space, and a vertex's weights are scaled to
///           // sum to 1. Or unweighted: positions in the slot's bone
///           vertices: Weighted([
///             [("torso", -20, 0, 1)],
///             [("torso", 20, 0, 1)],
///             [("arm", 20, 0, 0.75), ("torso", 20, 60, 0.25)],
///             [("arm", -20, 0, 1)],
///           ]),
///           color: "#ffffff"),                                // white
///         "flag": Mesh(region: "torso", uvs: [(0, 0), (1, 0), (1, 1)], triangles: [0, 1, 2],
///                      vertices: Unweighted([(0, 0), (64, 0), (64, 40)])),
///       },
///     },
///     "red": { "gear": { "gear": Region(region: "gear") } },
///   },
///   // Constraints, in the order they apply: a bone's rule for where it ends up after
///   // animation. Each names its bones (an IK chain runs from parent to child), a target
///   // bone that is not one of them nor below one, and settings, each with its default
///   constraints: [
///     // IK turns one bone, two bones or a chain (up to 16) so the last one's tip reaches the
///     // target. The last bone needs a length, and so does a lone bone that stretches or compresses
///     Ik(name: "reach", bones: ["torso", "arm"], target: "aim",
///        mix: 1,                                             // 0 (animated) to 1 (solved)
///        softness: 0,                                        // two bones, a distance
///        bend: Positive,                                     // two bones: Positive or
///                                                            // Negative; a chain: those
///                                                            // or Pose (its default)
///        compress: false, stretch: false, uniform: false,    // compress: one bone; the
///                                                            // others: not a chain
///        max_stretch: 1.5),                                  // stretch up to this times the
///                                                            // reach (at least 1; left out:
///                                                            // no limit); not a chain
///     // A transform constraint copies the target's rotation, place, scale and shear by a
///     // mix each (0 leaves it, 1 copies it; outside 0 to 1 is allowed) plus an offset
///     Transform(name: "level", bones: ["arm"], target: "aim",
///               mix: (rotate: 1, x: 0, y: 0, scale_x: 0, scale_y: 0, shear_y: 0),   // all 0
///               offset: (rotation: 0, x: 0, y: 0, scale_x: 0, scale_y: 0, shear_y: 0), // all 0
///               local: false, relative: false),              // false, false
///   ],
///   // Events with default values, and a sound the game plays
///   events: {
///     "step": (int: 1, float: 2.5, string: "left",            // 0, 0, ""
///              audio: (path: "audio/step.ogg",                // no audio
///                      volume: 0.8, balance: -0.5)),          // 1, 0
///     "hit": (),
///   },
///   animations: {
///     "walk": (
///       duration: 1.0,
///       bones: {
///         "torso": (
///           // A key is (time, value) or (time, value, curve), the curve
///           // easing to the next key: Linear (the default), Stepped, or
///           // Bezier(x1, y1, x2, y2) with x1 and x2 within 0 to 1
///           rotate: [(0.0, 0.0), (0.5, 20.0, Stepped), (1.0, 0.0, Bezier(0.25, 0.1, 0.75, 0.9))],
///           // Values are changes from the setup pose, but scale is a
///           // factor of it. Translate, scale and shear key both axes
///           translate: [(0.0, (0, 0)), (1.0, (4, -2))],
///           scale: [(0.0, (1, 1)), (1.0, (1.5, 1.5))],
///           shear: [(0.0, (0, 0)), (1.0, (10, -10))],
///         ),
///         "arm": (
///           // Or one axis at a time (not both on one bone)
///           translate_x: [(0.0, 0), (1.0, 3)],
///           translate_y: [(0.0, 0), (1.0, -3)],
///           scale_x: [(0.0, 1), (1.0, 2)],
///           scale_y: [(0.0, 1), (1.0, 0.5)],
///           shear_x: [(0.0, 0), (1.0, 20)],
///           shear_y: [(0.0, 0), (1.0, -20)],
///         ),
///       },
///       slots: { "gear": (
///         attachment: [(0.0, "gear"), (0.5, None)],           // None hides the slot
///         color: [(0.0, "#ffffff"), (1.0, "#ff000000", Stepped)],
///         // The shape of a mesh the slot shows, by its attachment's name. A key
///         // holds an offset from the setup shape, x then y, for each position
///         // of the mesh: a vertex each when unweighted (in the slot's bone), an
///         // influence each when weighted (in that influence's bone). [] is no
///         // offset, the setup shape
///         deform: { "flag": [(0.0, []), (0.5, [0, -6, 0, -6, 0, 6], Bezier(0.25, 0, 0.75, 1)), (1.0, [])] },
///       ) },
///       // Constraints by name: an IK constraint's `ik` keys (mix, softness, bend,
///       // compress, stretch) or a transform constraint's `transform` keys (its six mixes).
///       // A value a key leaves out is the constraint's setup value. The numbers ease to
///       // the next key by its curve; the switches change at their key
///       constraints: {
///         "reach": (ik: [(0.0, (mix: 1)), (0.5, (mix: 0.5, softness: 4, bend: Negative), Stepped)]),
///         "level": (transform: [(0.0, (rotate: 1)), (1.0, (rotate: 0.5, x: 0.25))]),
///       },
///       // Every slot, back to front, or None for the setup order
///       draw_order: [(0.25, ["gear", "torso"]), (0.75, None)],
///       // (time, event) or (time, event, values) with any of
///       // int, float, string, volume and balance
///       events: [(0.5, "step"), (0.75, "step", (int: 9, volume: 0.3))],
///     ),
///     "run": (duration: 0.5),
///   },
///   // Seconds to mix between animations: the default, and by pair
///   mixes: { default: 0.1, pairs: [("walk", "run", 0.2)] },
/// )
/// ```
///
/// A region attachment's `width` and `height` are its region's size when left
/// out. A mesh's `hull` is 0 when left out, its positions are in pixels (no
/// angles occur) and a vertex's weights are normalised as it loads. A
/// deform names an attachment some skin has in its slot, and its keys are
/// each empty or one length, an x and a y for each position; that they fit
/// a mesh is found when the animation plays, since the skins worn decide
/// what a name shows, and a deform that does not fit the attachment of
/// its name is skipped with one warning. A draw-order key lists the slots
/// by name. A constraint's timeline keys a constraint of its own kind, and
/// a key sets no switch its constraint could not have (`compress` on two
/// bones, say), as a setup sets none. A bone
/// or slot of an animation, a region's image, an attachment's region, a
/// mesh influence's bone, a deform's attachment (in its slot, in some
/// skin), an animation's constraint and an event key's event are names the
/// file declares, and so are a constraint's bones and target. Unknown
/// fields are refused, so a misspelt one fails rather than doing nothing,
/// and so is a name written twice in one map.
///
/// The images are the skeleton's dependencies: it is ready once they are, one
/// that does not load fails it (`AssetError::Dependency`), and saving one
/// checks the skeleton again (a region's rectangle must lie within its
/// image). A mistake fails the load with `AssetError::Decode`: a RON error at
/// its line and column, or `SkeletonError`'s item and problem
/// (`bone "arm": ...`, `animation "walk" timeline 0 (rotate): ...`). A failed
/// reload keeps the last good skeleton. Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct SkeletonLoader;

impl AssetLoader for SkeletonLoader {
  type Asset = SkeletonData;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["skeleton.ron"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    _: &(),
    cx: &mut LoadContext<'_>,
  ) -> Result<SkeletonData, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    let file: SkeletonFile = ron::Options::default()
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
    let mut images = HashMap::new();
    for (name, written) in &file.images.0 {
      let place = format!("images: {name:?}");
      images.insert(
        name.as_str(),
        dependency::<Image>(cx, &path, &place, written)?,
      );
    }
    file
      .build(&images)
      .map_err(|error| AssetError::decode(&path, error))
  }

  /// Keeps each region's rectangle inside its image, now the images are in
  /// (and again when one reloads)
  fn check(&self, data: &SkeletonData, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    let Some(images) = cx.world().get_resource::<Assets<Image>>() else {
      return Ok(());
    };
    for region in data.regions() {
      let Some(image) = images.get(&region.image) else {
        continue;
      };
      let (width, height) = (u64::from(image.width()), u64::from(image.height()));
      let right = u64::from(region.x) + u64::from(region.width);
      let bottom = u64::from(region.y) + u64::from(region.height);
      if right > width || bottom > height {
        return Err(AssetError::decode(
          cx.path(),
          problem(
            format!("region {:?}", &*region.name),
            format!(
              "its rect ({}, {}, {}, {}) lies outside its {width} by {height} image",
              region.x, region.y, region.width, region.height
            ),
          ),
        ));
      }
    }
    Ok(())
  }
}

fn problem(item: impl Into<String>, problem: impl Into<String>) -> SkeletonError {
  SkeletonError {
    item: item.into(),
    problem: problem.into(),
  }
}

/// A map read in the file's order, a name written twice failing at its line
struct Ordered<T>(Vec<(String, T)>);

impl<T> Default for Ordered<T> {
  fn default() -> Self {
    Self(Vec::new())
  }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Ordered<T> {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct OrderedVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for OrderedVisitor<T> {
      type Value = Ordered<T>;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a map of names")
      }

      fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered<T>, A::Error> {
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        while let Some(name) = map.next_key::<String>()? {
          if !seen.insert(name.clone()) {
            return Err(de::Error::custom(format!("`{name}` is written twice")));
          }
          entries.push((name, map.next_value()?));
        }
        Ok(Ordered(entries))
      }
    }

    deserializer.deserialize_map(OrderedVisitor(std::marker::PhantomData))
  }
}

/// How a key eases to the next, as a file writes it
#[derive(Deserialize, Default, Clone, Copy)]
enum CurveFile {
  #[default]
  Linear,
  Stepped,
  Bezier(f32, f32, f32, f32),
}

impl From<CurveFile> for Curve {
  fn from(curve: CurveFile) -> Self {
    match curve {
      CurveFile::Linear => Curve::Linear,
      CurveFile::Stepped => Curve::Stepped,
      CurveFile::Bezier(cx1, cy1, cx2, cy2) => Curve::Bezier { cx1, cy1, cx2, cy2 },
    }
  }
}

/// A key written `(time, value)` or `(time, value, curve)`
struct Key<T> {
  time: f32,
  value: T,
  curve: CurveFile,
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Key<T> {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct KeyVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for KeyVisitor<T> {
      type Value = Key<T>;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a key: (time, value) or (time, value, curve)")
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Key<T>, A::Error> {
        let time = seq
          .next_element()?
          .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let value = seq
          .next_element()?
          .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let curve = seq.next_element()?.unwrap_or_default();
        Ok(Key { time, value, curve })
      }
    }

    deserializer.deserialize_tuple(3, KeyVisitor(std::marker::PhantomData))
  }
}

/// An event key's overrides of its event's values
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct EventValuesFile {
  int: Option<i32>,
  float: Option<f32>,
  string: Option<String>,
  volume: Option<f32>,
  balance: Option<f32>,
}

/// An event key written `(time, event)` or `(time, event, values)`
struct EventKey {
  time: f32,
  event: String,
  values: EventValuesFile,
}

impl<'de> Deserialize<'de> for EventKey {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct EventKeyVisitor;

    impl<'de> Visitor<'de> for EventKeyVisitor {
      type Value = EventKey;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an event key: (time, event) or (time, event, values)")
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<EventKey, A::Error> {
        let time = seq
          .next_element()?
          .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let event = seq
          .next_element()?
          .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let values = seq.next_element()?.unwrap_or_default();
        Ok(EventKey {
          time,
          event,
          values,
        })
      }
    }

    deserializer.deserialize_tuple(3, EventKeyVisitor)
  }
}

fn one() -> f32 {
  1.0
}

fn white() -> ColorFile {
  ColorFile(Color::WHITE)
}

#[derive(Deserialize, Default, Clone, Copy)]
enum InheritFile {
  #[default]
  Normal,
  OnlyTranslation,
  NoRotationOrReflection,
  NoScale,
  NoScaleOrReflection,
}

#[derive(Deserialize, Default, Clone, Copy)]
enum BlendFile {
  #[default]
  Normal,
  Additive,
  Multiply,
  Screen,
}

/// An IK constraint's bend, as written
#[derive(Deserialize, Clone, Copy)]
enum IkBendFile {
  Positive,
  Negative,
  Pose,
}

impl IkBendFile {
  fn bend(self) -> IkBend {
    match self {
      Self::Positive => IkBend::Positive,
      Self::Negative => IkBend::Negative,
      Self::Pose => IkBend::Pose,
    }
  }
}

/// A constraint, as written: `Ik(..)` or `Transform(..)`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum ConstraintFile {
  Ik {
    name: String,
    bones: Vec<String>,
    target: String,
    #[serde(default = "one")]
    mix: f32,
    #[serde(default)]
    softness: f32,
    #[serde(default)]
    bend: Option<IkBendFile>,
    #[serde(default)]
    compress: bool,
    #[serde(default)]
    stretch: bool,
    #[serde(default)]
    max_stretch: Option<f32>,
    #[serde(default)]
    uniform: bool,
  },
  Transform {
    name: String,
    bones: Vec<String>,
    target: String,
    #[serde(default)]
    mix: TransformMixFile,
    #[serde(default)]
    offset: TransformOffsetFile,
    #[serde(default)]
    local: bool,
    #[serde(default)]
    relative: bool,
  },
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct TransformMixFile {
  rotate: f32,
  x: f32,
  y: f32,
  scale_x: f32,
  scale_y: f32,
  shear_y: f32,
}

impl TransformMixFile {
  fn mix(&self) -> TransformMix {
    TransformMix {
      rotate: self.rotate,
      x: self.x,
      y: self.y,
      scale_x: self.scale_x,
      scale_y: self.scale_y,
      shear_y: self.shear_y,
    }
  }
}

impl ConstraintFile {
  fn name(&self) -> &str {
    match self {
      Self::Ik { name, .. } | Self::Transform { name, .. } => name,
    }
  }
}

/// The offsets' angles are degrees
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct TransformOffsetFile {
  rotation: f32,
  x: f32,
  y: f32,
  scale_x: f32,
  scale_y: f32,
  shear_y: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionFile {
  image: String,
  rect: (u32, u32, u32, u32),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoneFile {
  name: String,
  #[serde(default)]
  parent: Option<String>,
  #[serde(default)]
  length: f32,
  #[serde(default)]
  x: f32,
  #[serde(default)]
  y: f32,
  #[serde(default)]
  rotation: f32,
  #[serde(default = "one")]
  scale_x: f32,
  #[serde(default = "one")]
  scale_y: f32,
  #[serde(default)]
  shear_x: f32,
  #[serde(default)]
  shear_y: f32,
  #[serde(default)]
  inherit: InheritFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlotFile {
  name: String,
  bone: String,
  #[serde(default = "white")]
  color: ColorFile,
  #[serde(default)]
  attachment: Option<String>,
  #[serde(default)]
  blend: BlendFile,
}

/// A mesh's vertices: positions in the slot's bone, or each vertex's
/// influences as (bone, x, y, weight)
#[derive(Deserialize)]
enum MeshVerticesFile {
  Unweighted(Vec<Vec2>),
  Weighted(Vec<Vec<(String, f32, f32, f32)>>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum AttachmentFile {
  Region {
    region: String,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    rotation: f32,
    #[serde(default = "one")]
    scale_x: f32,
    #[serde(default = "one")]
    scale_y: f32,
    #[serde(default)]
    width: Option<f32>,
    #[serde(default)]
    height: Option<f32>,
    #[serde(default = "white")]
    color: ColorFile,
  },
  Mesh {
    region: String,
    uvs: Vec<Vec2>,
    triangles: Vec<u16>,
    #[serde(default)]
    hull: u16,
    vertices: MeshVerticesFile,
    #[serde(default = "white")]
    color: ColorFile,
  },
  Point {
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    rotation: f32,
  },
  BoundingBox {
    vertices: Vec<Vec2>,
  },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioFile {
  path: String,
  #[serde(default = "one")]
  volume: f32,
  #[serde(default)]
  balance: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventFile {
  #[serde(default)]
  int: i32,
  #[serde(default)]
  float: f32,
  #[serde(default)]
  string: String,
  #[serde(default)]
  audio: Option<AudioFile>,
}

/// One bone's timelines, each present only if written
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct BoneTimelinesFile {
  rotate: Option<Vec<Key<f32>>>,
  translate: Option<Vec<Key<Vec2>>>,
  scale: Option<Vec<Key<Vec2>>>,
  shear: Option<Vec<Key<Vec2>>>,
  translate_x: Option<Vec<Key<f32>>>,
  translate_y: Option<Vec<Key<f32>>>,
  scale_x: Option<Vec<Key<f32>>>,
  scale_y: Option<Vec<Key<f32>>>,
  shear_x: Option<Vec<Key<f32>>>,
  shear_y: Option<Vec<Key<f32>>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct SlotTimelinesFile {
  attachment: Option<Vec<(f32, Option<String>)>>,
  color: Option<Vec<Key<ColorFile>>>,
  /// By attachment name, keys of offsets: x then y for each position
  deform: Ordered<Vec<Key<Vec<f32>>>>,
}

/// An IK constraint's key: each value left out is the constraint's setup
/// value
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct IkKeyFile {
  mix: Option<f32>,
  softness: Option<f32>,
  bend: Option<IkBendFile>,
  compress: Option<bool>,
  stretch: Option<bool>,
}

/// A transform constraint's key: each mix left out is the constraint's
/// setup mix
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct TransformKeyFile {
  rotate: Option<f32>,
  x: Option<f32>,
  y: Option<f32>,
  scale_x: Option<f32>,
  scale_y: Option<f32>,
  shear_y: Option<f32>,
}

/// One constraint's timeline: an IK constraint's `ik` keys or a transform
/// constraint's `transform` keys
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct ConstraintTimelinesFile {
  ik: Option<Vec<Key<IkKeyFile>>>,
  transform: Option<Vec<Key<TransformKeyFile>>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnimationFile {
  duration: f32,
  #[serde(default)]
  bones: Ordered<BoneTimelinesFile>,
  #[serde(default)]
  slots: Ordered<SlotTimelinesFile>,
  #[serde(default)]
  constraints: Ordered<ConstraintTimelinesFile>,
  #[serde(default)]
  draw_order: Option<Vec<(f32, Option<Vec<String>>)>>,
  #[serde(default)]
  events: Option<Vec<EventKey>>,
}

/// The mixes, written as a map: `{ default: 0.15, pairs: [("idle", "run", 0.2)] }`
#[derive(Default)]
struct MixesFile {
  default: f32,
  pairs: Vec<(String, String, f32)>,
}

impl<'de> Deserialize<'de> for MixesFile {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    /// The keys of the map
    enum MixKey {
      Default,
      Pairs,
    }

    impl<'de> Deserialize<'de> for MixKey {
      fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;

        impl Visitor<'_> for KeyVisitor {
          type Value = MixKey;

          fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("`default` or `pairs`")
          }

          fn visit_str<E: de::Error>(self, name: &str) -> Result<MixKey, E> {
            match name {
              "default" => Ok(MixKey::Default),
              "pairs" => Ok(MixKey::Pairs),
              other => Err(E::unknown_field(other, &["default", "pairs"])),
            }
          }
        }

        deserializer.deserialize_identifier(KeyVisitor)
      }
    }

    struct MixesVisitor;

    impl<'de> Visitor<'de> for MixesVisitor {
      type Value = MixesFile;

      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a map: { default: seconds, pairs: [(from, to, seconds)] }")
      }

      fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<MixesFile, A::Error> {
        let mut mixes = MixesFile::default();
        let (mut got_default, mut got_pairs) = (false, false);
        while let Some(key) = map.next_key::<MixKey>()? {
          match key {
            MixKey::Default if !got_default => {
              got_default = true;
              mixes.default = map.next_value()?;
            }
            MixKey::Pairs if !got_pairs => {
              got_pairs = true;
              mixes.pairs = map.next_value()?;
            }
            MixKey::Default => return Err(de::Error::duplicate_field("default")),
            MixKey::Pairs => return Err(de::Error::duplicate_field("pairs")),
          }
        }
        Ok(mixes)
      }
    }

    deserializer.deserialize_map(MixesVisitor)
  }
}

/// A `.skeleton.ron` file, as written
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename = "Skeleton")]
struct SkeletonFile {
  #[serde(default)]
  images: Ordered<String>,
  #[serde(default)]
  regions: Ordered<RegionFile>,
  #[serde(default)]
  bones: Vec<BoneFile>,
  #[serde(default)]
  slots: Vec<SlotFile>,
  #[serde(default)]
  constraints: Vec<ConstraintFile>,
  #[serde(default)]
  skins: Ordered<Ordered<Ordered<AttachmentFile>>>,
  #[serde(default)]
  events: Ordered<EventFile>,
  #[serde(default)]
  animations: Ordered<AnimationFile>,
  #[serde(default)]
  mixes: MixesFile,
}

/// A key list as frames, its values converted
fn frames<T, U>(keys: &[Key<T>], convert: impl Fn(&T) -> U) -> Vec<Frame<U>> {
  keys
    .iter()
    .map(|key| (key.time, convert(&key.value), key.curve.into()))
    .collect()
}

fn radians(value: &f32) -> f32 {
  value.to_radians()
}

fn same(value: &f32) -> f32 {
  *value
}

fn vec_same(value: &Vec2) -> Vec2 {
  *value
}

fn vec_radians(value: &Vec2) -> Vec2 {
  Vec2::new(value.x.to_radians(), value.y.to_radians())
}

impl SkeletonFile {
  /// The skeleton the file describes, built and checked through the builder,
  /// with the images the regions name loaded
  fn build(&self, images: &HashMap<&str, Handle<Image>>) -> Result<SkeletonData, SkeletonError> {
    let mut builder = SkeletonData::builder();
    for (name, region) in &self.regions.0 {
      let item = format!("region {name:?}");
      let image = images.get(region.image.as_str()).ok_or_else(|| {
        problem(
          &item,
          format!("image {:?} is not in `images`", region.image),
        )
      })?;
      let (x, y, width, height) = region.rect;
      builder = builder.region(Region {
        name: Arc::from(name.as_str()),
        image: image.clone(),
        x,
        y,
        width,
        height,
      });
    }
    for bone in &self.bones {
      builder = builder.bone(
        bone.name.as_str(),
        bone.parent.as_deref(),
        BoneSetup {
          length: bone.length,
          x: bone.x,
          y: bone.y,
          rotation: bone.rotation.to_radians(),
          scale_x: bone.scale_x,
          scale_y: bone.scale_y,
          shear_x: bone.shear_x.to_radians(),
          shear_y: bone.shear_y.to_radians(),
          inherit: match bone.inherit {
            InheritFile::Normal => Inherit::Normal,
            InheritFile::OnlyTranslation => Inherit::OnlyTranslation,
            InheritFile::NoRotationOrReflection => Inherit::NoRotationOrReflection,
            InheritFile::NoScale => Inherit::NoScale,
            InheritFile::NoScaleOrReflection => Inherit::NoScaleOrReflection,
          },
        },
      );
    }
    for slot in &self.slots {
      builder = builder.slot(
        slot.name.as_str(),
        &slot.bone,
        SlotSetup {
          color: slot.color.0,
          attachment: slot.attachment.as_deref().map(Arc::from),
          blend: match slot.blend {
            BlendFile::Normal => SlotBlend::Normal,
            BlendFile::Additive => SlotBlend::Additive,
            BlendFile::Multiply => SlotBlend::Multiply,
            BlendFile::Screen => SlotBlend::Screen,
          },
        },
      );
    }
    for constraint in &self.constraints {
      builder = match constraint {
        ConstraintFile::Ik {
          name,
          bones,
          target,
          mix,
          softness,
          bend,
          compress,
          stretch,
          max_stretch,
          uniform,
        } => builder.ik(
          name.as_str(),
          &bones.iter().map(String::as_str).collect::<Vec<_>>(),
          target,
          IkSetup {
            mix: *mix,
            softness: *softness,
            bend: bend.map(IkBendFile::bend),
            compress: *compress,
            stretch: *stretch,
            max_stretch: *max_stretch,
            uniform: *uniform,
          },
        ),
        ConstraintFile::Transform {
          name,
          bones,
          target,
          mix,
          offset,
          local,
          relative,
        } => builder.transform_constraint(
          name.as_str(),
          &bones.iter().map(String::as_str).collect::<Vec<_>>(),
          target,
          TransformSetup {
            mix: mix.mix(),
            offset: TransformOffset {
              rotation: offset.rotation.to_radians(),
              x: offset.x,
              y: offset.y,
              scale_x: offset.scale_x,
              scale_y: offset.scale_y,
              shear_y: offset.shear_y.to_radians(),
            },
            local: *local,
            relative: *relative,
          },
        ),
      };
    }
    for (skin, _) in &self.skins.0 {
      if skin != SkeletonData::DEFAULT_SKIN {
        builder = builder.skin(skin.as_str());
      }
    }
    for (skin, slots) in &self.skins.0 {
      for (slot, attachments) in &slots.0 {
        for (name, attachment) in &attachments.0 {
          let item = format!("skin {skin:?} slot {slot:?} attachment {name:?}");
          let attachment = self.attachment(&builder, &item, attachment)?;
          builder = builder.attachment(skin, slot, name.as_str(), attachment);
        }
      }
    }
    for (name, event) in &self.events.0 {
      builder = builder.event(EventData {
        name: Arc::from(name.as_str()),
        int: event.int,
        float: event.float,
        string: Arc::from(event.string.as_str()),
        audio: event.audio.as_ref().map(|audio| EventAudio {
          path: Arc::from(audio.path.as_str()),
          volume: audio.volume,
          balance: audio.balance,
        }),
      });
    }
    for (name, animation) in &self.animations.0 {
      let animation = self.animation(&builder, name, animation)?;
      builder = builder.animation(animation);
    }
    builder = builder.default_mix(self.mixes.default);
    for (from, to, duration) in &self.mixes.pairs {
      builder = builder.mix(from, to, *duration);
    }
    builder.build()
  }

  /// The attachment `file`, its region found by name
  fn attachment(
    &self,
    builder: &crate::core::skeleton::SkeletonDataBuilder,
    item: &str,
    file: &AttachmentFile,
  ) -> Result<Attachment, SkeletonError> {
    Ok(match file {
      AttachmentFile::Region {
        region,
        x,
        y,
        rotation,
        scale_x,
        scale_y,
        width,
        height,
        color,
      } => {
        let index = builder
          .region_index(region)
          .ok_or_else(|| problem(item, format!("region {region:?} is not in `regions`")))?;
        let (_, _, w, h) = self.regions.0[index].1.rect;
        Attachment::Region(RegionAttachment {
          region: index,
          x: *x,
          y: *y,
          rotation: rotation.to_radians(),
          scale_x: *scale_x,
          scale_y: *scale_y,
          width: width.unwrap_or(w as f32),
          height: height.unwrap_or(h as f32),
          color: color.0,
        })
      }
      AttachmentFile::Mesh {
        region,
        uvs,
        triangles,
        hull,
        vertices,
        color,
      } => {
        let index = builder
          .region_index(region)
          .ok_or_else(|| problem(item, format!("region {region:?} is not in `regions`")))?;
        let vertices = match vertices {
          MeshVerticesFile::Unweighted(positions) => MeshVertices::Unweighted(positions.clone()),
          MeshVerticesFile::Weighted(vertices) => {
            let mut weighted = Vec::with_capacity(vertices.len());
            for vertex in vertices {
              let mut influences = Vec::with_capacity(vertex.len());
              for (bone, x, y, weight) in vertex {
                let bone = builder
                  .bone_id(bone)
                  .ok_or_else(|| problem(item, format!("bone {bone:?} is not in `bones`")))?;
                influences.push(Influence::new(bone, *x, *y, *weight));
              }
              weighted.push(influences);
            }
            MeshVertices::Weighted(WeightedVertices::new(weighted))
          }
        };
        Attachment::Mesh(MeshAttachment {
          region: index,
          uvs: uvs.clone(),
          triangles: triangles.clone(),
          hull: *hull,
          vertices,
          color: color.0,
        })
      }
      AttachmentFile::Point { x, y, rotation } => Attachment::Point(PointAttachment {
        x: *x,
        y: *y,
        rotation: rotation.to_radians(),
      }),
      AttachmentFile::BoundingBox { vertices } => Attachment::BoundingBox(BoundingBoxAttachment {
        vertices: vertices.clone(),
      }),
    })
  }

  /// The animation `file` called `name`: bones' timelines, then slots'
  /// (each slot's attachment, colour and deforms), constraints' (each one's
  /// IK keys, then its transform keys), the draw order and the events
  fn animation(
    &self,
    builder: &crate::core::skeleton::SkeletonDataBuilder,
    name: &str,
    file: &AnimationFile,
  ) -> Result<Animation, SkeletonError> {
    let item = format!("animation {name:?}");
    let mut timelines = Vec::new();
    for (bone_name, t) in &file.bones.0 {
      let bone = builder.bone_id(bone_name).ok_or_else(|| {
        problem(
          &item,
          format!("bones: {bone_name:?} is not a bone (or is past the 255th)"),
        )
      })?;
      let mut push = |timeline: Option<Timeline>| timelines.extend(timeline);
      push(t.rotate.as_ref().map(|keys| Timeline::Rotate {
        bone,
        frames: frames(keys, radians),
      }));
      push(t.translate.as_ref().map(|keys| Timeline::Translate {
        bone,
        frames: frames(keys, vec_same),
      }));
      push(t.scale.as_ref().map(|keys| Timeline::Scale {
        bone,
        frames: frames(keys, vec_same),
      }));
      push(t.shear.as_ref().map(|keys| Timeline::Shear {
        bone,
        frames: frames(keys, vec_radians),
      }));
      push(t.translate_x.as_ref().map(|keys| Timeline::TranslateX {
        bone,
        frames: frames(keys, same),
      }));
      push(t.translate_y.as_ref().map(|keys| Timeline::TranslateY {
        bone,
        frames: frames(keys, same),
      }));
      push(t.scale_x.as_ref().map(|keys| Timeline::ScaleX {
        bone,
        frames: frames(keys, same),
      }));
      push(t.scale_y.as_ref().map(|keys| Timeline::ScaleY {
        bone,
        frames: frames(keys, same),
      }));
      push(t.shear_x.as_ref().map(|keys| Timeline::ShearX {
        bone,
        frames: frames(keys, radians),
      }));
      push(t.shear_y.as_ref().map(|keys| Timeline::ShearY {
        bone,
        frames: frames(keys, radians),
      }));
    }
    for (slot_name, t) in &file.slots.0 {
      let slot = builder.slot_id(slot_name).ok_or_else(|| {
        problem(
          &item,
          format!("slots: {slot_name:?} is not a slot (or is past the 255th)"),
        )
      })?;
      if let Some(keys) = &t.attachment {
        timelines.push(Timeline::Attachment {
          slot,
          frames: keys
            .iter()
            .map(|(time, name)| (*time, name.as_deref().map(Arc::from)))
            .collect(),
        });
      }
      if let Some(keys) = &t.color {
        timelines.push(Timeline::Color {
          slot,
          frames: frames(keys, |color| color.0),
        });
      }
      for (attachment, keys) in &t.deform.0 {
        timelines.push(Timeline::Deform {
          slot,
          attachment: Arc::from(attachment.as_str()),
          frames: frames(keys, Vec::clone),
        });
      }
    }
    for (constraint_name, t) in &file.constraints.0 {
      let found = self
        .constraints
        .iter()
        .enumerate()
        .find(|(_, constraint)| constraint.name() == constraint_name);
      // As a bone's or a slot's id, none past the 255th, which the build
      // refuses
      let below = found.filter(|&(index, _)| index < crate::core::skeleton::MAX_CONSTRAINTS);
      let Some((constraint, setup)) = below.and_then(|(index, setup)| {
        u8::try_from(index)
          .ok()
          .map(|index| (ConstraintId(index), setup))
      }) else {
        return Err(problem(
          &item,
          format!("constraints: {constraint_name:?} is not a constraint (or is past the 255th)"),
        ));
      };
      if let Some(keys) = &t.ik {
        // A value a key leaves out is the setup's; keys for a transform
        // constraint fail as validation refuses them
        let setup = match setup {
          ConstraintFile::Ik {
            bones,
            mix,
            softness,
            bend,
            compress,
            stretch,
            max_stretch,
            ..
          } => IkValues {
            mix: *mix,
            softness: *softness,
            bend: bend.map_or(IkBend::default_for(bones.len()), IkBendFile::bend),
            compress: *compress,
            stretch: *stretch,
            max_stretch: *max_stretch,
          },
          ConstraintFile::Transform { .. } => IkValues::of(&IkSetup::default()),
        };
        timelines.push(Timeline::Ik {
          constraint,
          frames: frames(keys, |key| IkValues {
            mix: key.mix.unwrap_or(setup.mix),
            softness: key.softness.unwrap_or(setup.softness),
            bend: key.bend.map_or(setup.bend, IkBendFile::bend),
            compress: key.compress.unwrap_or(setup.compress),
            stretch: key.stretch.unwrap_or(setup.stretch),
            max_stretch: setup.max_stretch,
          }),
        });
      }
      if let Some(keys) = &t.transform {
        let setup = match setup {
          ConstraintFile::Transform { mix, .. } => mix.mix(),
          ConstraintFile::Ik { .. } => TransformMix::default(),
        };
        timelines.push(Timeline::TransformMix {
          constraint,
          frames: frames(keys, |key| TransformMix {
            rotate: key.rotate.unwrap_or(setup.rotate),
            x: key.x.unwrap_or(setup.x),
            y: key.y.unwrap_or(setup.y),
            scale_x: key.scale_x.unwrap_or(setup.scale_x),
            scale_y: key.scale_y.unwrap_or(setup.scale_y),
            shear_y: key.shear_y.unwrap_or(setup.shear_y),
          }),
        });
      }
    }
    if let Some(keys) = &file.draw_order {
      let mut frames = Vec::with_capacity(keys.len());
      for (time, order) in keys {
        let order = match order {
          None => None,
          Some(names) => {
            let ids: Result<Vec<SlotId>, SkeletonError> = names
              .iter()
              .map(|slot| {
                builder.slot_id(slot).ok_or_else(|| {
                  problem(
                    &item,
                    format!("draw_order: the key at {time} names {slot:?}, which is not a slot"),
                  )
                })
              })
              .collect();
            Some(ids?)
          }
        };
        frames.push((*time, order));
      }
      timelines.push(Timeline::DrawOrder { frames });
    }
    if let Some(keys) = &file.events {
      let mut frames = Vec::with_capacity(keys.len());
      for key in keys {
        let event = builder.event_index(&key.event).ok_or_else(|| {
          problem(
            &item,
            format!(
              "events: the key at {} names {:?}, which is not in `events`",
              key.time, key.event
            ),
          )
        })?;
        let values = &key.values;
        frames.push((
          key.time,
          event,
          EventValues {
            int: values.int,
            float: values.float,
            string: values.string.as_deref().map(Arc::from),
            volume: values.volume,
            balance: values.balance,
          },
        ));
      }
      timelines.push(Timeline::Event { frames });
    }
    Ok(Animation {
      name: Arc::from(name),
      duration: file.duration,
      timelines,
    })
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use crate::core::asset::{
    AssetServer, AssetSource, Assets, AssetsView, MemorySource, insert_registry,
  };
  use crate::core::ecs::Vec2;
  use crate::core::ecs::kernel::World;
  use crate::core::graphics::Image;
  use crate::core::skeleton::{
    Animation, Attachment, BoneId, BoneSetup, BoundingBoxAttachment, ConstraintId, Curve,
    EventAudio, EventData, EventValues, IkBend, IkSetup, IkValues, Influence, Inherit,
    MeshAttachment, MeshVertices, PointAttachment, Region, RegionAttachment, SkeletonData,
    SlotBlend, SlotId, SlotSetup, Timeline, TransformMix, TransformOffset, TransformSetup,
    WeightedVertices,
  };
  use crate::core::ui::Color;

  const PATH: &str = "rigs/hero.skeleton.ron";
  const IMAGE: &str = "rigs/hero.png";
  const EXTRA: &str = "rigs/gear/extra.png";

  fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255; 4]));
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

  /// The images, and `text` at [`PATH`]
  fn files(text: &str) -> MemorySource {
    let files = MemorySource::new();
    files.insert(IMAGE, png(128, 128));
    files.insert("hero.png", png(128, 128));
    files.insert("other/pic.png", png(8, 8));
    files.insert(EXTRA, png(16, 16));
    files.insert(PATH, text);
    files
  }

  /// The skeleton at [`PATH`] among `files`, loaded at once
  fn load_in(world: &mut World) -> Result<SkeletonData, String> {
    let mut assets = AssetsView::new(world);
    let handle = assets
      .load_now::<SkeletonData>(PATH)
      .map_err(|error| error.report())?;
    assets
      .get(&handle)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn loaded(text: &str) -> SkeletonData {
    match load_in(&mut registry_over(files(text))) {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    }
  }

  fn failure(text: &str) -> String {
    match load_in(&mut registry_over(files(text))) {
      Ok(_) => panic!("{text} loaded"),
      Err(error) => error,
    }
  }

  /// The file shape the brief gives, with the one animation its mix pair names
  /// (a mix between animations the file lacks fails validation)
  const DOCUMENTED: &str = r#"(
    images: { "parts": "hero.png" },
    regions: { "head": (image: "parts", rect: (0, 0, 64, 64)), "torso": (image: "parts", rect: (64, 0, 48, 80)) },
    bones: [ (name: "root"), (name: "torso", parent: "root", y: -40, length: 40), (name: "head", parent: "torso", y: -40, rotation: 5) ],
    slots: [ (name: "torso", bone: "torso", attachment: "torso"), (name: "head", bone: "head", attachment: "head") ],
    skins: { "default": { "torso": { "torso": Region(region: "torso", y: -20) }, "head": { "head": Region(region: "head", y: -24) } } },
    events: { "step": (audio: Some((path: "audio/step.ogg", volume: 0.8))) },
    animations: { "idle": (duration: 1.0, bones: { "head": (rotate: [(0.0, 5.0), (0.5, -5.0, Bezier(0.25, 0, 0.75, 1)), (1.0, 5.0)]) }, events: [(0.5, "step")]), "run": (duration: 0.5) },
    mixes: { default: 0.15, pairs: [("idle", "run", 0.2)] },
  )"#;

  #[test]
  fn the_documented_file_loads() {
    let mut world = registry_over(files(DOCUMENTED));
    let data = match load_in(&mut world) {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    };
    let names: Vec<_> = data.bones().iter().map(|bone| &*bone.name).collect();
    assert_eq!(names, ["root", "torso", "head"]);
    assert_eq!(data.bones()[2].parent, Some(BoneId(1)));
    assert_eq!(data.bones()[1].y, -40.0);
    assert_eq!(data.bones()[1].length, 40.0);
    assert_eq!(data.slots().len(), 2);
    assert_eq!(data.slots()[0].attachment.as_deref(), Some("torso"));
    // Regions on the image the path names, from the asset root
    let mut assets = AssetsView::new(&mut world);
    let image = match assets.load_now::<Image>("hero.png") {
      Ok(image) => image,
      Err(error) => panic!("{}", error.report()),
    };
    let torso = &data.regions()[1];
    assert_eq!(&*torso.name, "torso");
    assert_eq!(torso.image, image);
    assert_eq!(
      (torso.x, torso.y, torso.width, torso.height),
      (64, 0, 48, 80)
    );
    // The attachment sizes itself from its region
    let slot = data.slot("torso").expect("a torso slot");
    match data.default_skin().attachment(slot, "torso") {
      Some(Attachment::Region(region)) => {
        assert_eq!((region.region, region.y), (1, -20.0));
        assert_eq!((region.width, region.height), (48.0, 80.0));
      }
      other => panic!("{other:?}"),
    }
    let event = &data.events()[0];
    let audio = event.audio.as_ref().expect("audio");
    assert_eq!(
      (&*audio.path, audio.volume, audio.balance),
      ("audio/step.ogg", 0.8, 0.0)
    );
    let idle = data.animation("idle").expect("idle");
    assert_eq!(idle.duration, 1.0);
    match &idle.timelines[0] {
      Timeline::Rotate { bone, frames } => {
        assert_eq!(*bone, BoneId(2));
        assert_eq!(frames[0].2, Curve::Linear);
        assert_eq!(
          frames[1].2,
          Curve::Bezier {
            cx1: 0.25,
            cy1: 0.0,
            cx2: 0.75,
            cy2: 1.0
          }
        );
      }
      other => panic!("{other:?}"),
    }
    assert_eq!(data.default_mix(), 0.15);
    assert_eq!(data.mix("idle", "run"), 0.2);
    assert_eq!(data.mix("run", "idle"), 0.15);
  }

  #[test]
  fn a_typo_in_a_field_fails_with_its_line() {
    let error = failure("(\n  bones: [\n    (name: \"root\", lenght: 4),\n  ],\n)");
    assert!(error.contains(PATH), "{error}");
    assert!(error.contains("line 3, column"), "{error}");
    assert!(error.contains("lenght"), "{error}");
  }

  #[test]
  fn an_unknown_region_fails_naming_the_attachment() {
    let error = failure(
      r#"(
      bones: [(name: "root")],
      slots: [(name: "head", bone: "root")],
      skins: { "default": { "head": { "hat": Region(region: "nowhere") } } },
    )"#,
    );
    assert!(error.contains(PATH), "{error}");
    assert!(
      error.contains(r#"skin "default" slot "head" attachment "hat""#),
      "{error}"
    );
    assert!(error.contains("nowhere"), "{error}");
  }

  #[test]
  fn angles_in_degrees_become_radians() {
    let data = loaded(
      r#"(
      bones: [(name: "root", rotation: 90, shear_x: 45, shear_y: -30), (name: "b", parent: "root")],
      slots: [(name: "s", bone: "root")],
      images: { "i": "rigs/hero.png" },
      regions: { "r": (image: "i", rect: (0, 0, 8, 8)) },
      skins: { "default": { "s": {
        "a": Region(region: "r", rotation: 180),
        "p": Point(rotation: 90),
      } } },
      animations: { "a": (duration: 1.0, bones: { "root": (
        rotate: [(0.0, 180.0)],
        shear: [(0.0, (90, -90))],
      ), "b": (shear_x: [(0.0, 30.0)]) }) },
    )"#,
    );
    let bone = &data.bones()[0];
    assert_eq!(bone.rotation, std::f32::consts::FRAC_PI_2);
    assert_eq!(bone.shear_x, std::f32::consts::FRAC_PI_4);
    assert_eq!(bone.shear_y, -std::f32::consts::FRAC_PI_6);
    let slot = SlotId(0);
    match data.default_skin().attachment(slot, "a") {
      Some(Attachment::Region(region)) => assert_eq!(region.rotation, std::f32::consts::PI),
      other => panic!("{other:?}"),
    }
    match data.default_skin().attachment(slot, "p") {
      Some(Attachment::Point(point)) => assert_eq!(point.rotation, std::f32::consts::FRAC_PI_2),
      other => panic!("{other:?}"),
    }
    let timelines = &data.animation("a").expect("a").timelines;
    match &timelines[0] {
      Timeline::Rotate { frames, .. } => assert_eq!(frames[0].1, std::f32::consts::PI),
      other => panic!("{other:?}"),
    }
    match &timelines[1] {
      Timeline::Shear { frames, .. } => assert_eq!(
        frames[0].1,
        Vec2::new(std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2)
      ),
      other => panic!("{other:?}"),
    }
    match &timelines[2] {
      Timeline::ShearX { frames, .. } => assert_eq!(frames[0].1, std::f32::consts::FRAC_PI_6),
      other => panic!("{other:?}"),
    }
  }

  /// Every feature the data holds, written once
  const EVERYTHING: &str = r##"(
    images: { "parts": "rigs/hero.png", "extra": "rigs/gear/extra.png" },
    regions: {
      "head": (image: "parts", rect: (0, 0, 64, 64)),
      "torso": (image: "parts", rect: (64, 0, 48, 80)),
      "gear": (image: "extra", rect: (0, 0, 16, 16)),
    },
    bones: [
      (name: "root"),
      (name: "torso", parent: "root", length: 40, x: 1, y: -40, rotation: 90,
       scale_x: 2, scale_y: 0.5, shear_x: 45, shear_y: -30, inherit: NoScale),
      (name: "head", parent: "torso", inherit: OnlyTranslation),
      (name: "arm", parent: "torso", inherit: NoRotationOrReflection),
      (name: "leg", parent: "root", inherit: NoScaleOrReflection),
      (name: "thigh", parent: "root", length: 10),
      (name: "shin", parent: "thigh", x: 10, length: 10),
      (name: "aim", parent: "root"),
    ],
    slots: [
      (name: "torso", bone: "torso", attachment: "torso", color: "#ff8000"),
      (name: "head", bone: "head", attachment: "head", color: (r: 10, g: 20, b: 30), blend: Additive),
      (name: "gear", bone: "arm", blend: Multiply),
      (name: "glow", bone: "leg", color: "#ffffff80", blend: Screen),
    ],
    skins: {
      "default": {
        "torso": { "torso": Region(region: "torso", y: -20) },
        "head": { "head": Region(region: "head", x: 1, y: -24, rotation: 180, scale_x: 1.5, scale_y: 2,
                                 width: 32, height: 30, color: "#80808080") },
        "gear": {
          "muzzle": Point(x: 3, y: 4, rotation: 90),
          "hit": BoundingBox(vertices: [(0, 0), (10, 0), (10, 10)]),
          "cape": Mesh(
            region: "torso",
            uvs: [(0, 0), (1, 0), (1, 1), (0, 1)],
            triangles: [0, 1, 2, 0, 2, 3],
            hull: 4,
            vertices: Weighted([
              [("torso", -20, 0, 1)],
              [("torso", 20, 0, 1)],
              [("arm", 20, 0, 0.75), ("torso", 20, 60, 0.25)],
              [("arm", -20, 0, 1)],
            ]),
            color: "#80808080",
          ),
        },
        "glow": { "flag": Mesh(
          region: "head",
          uvs: [(0, 0), (1, 0), (1, 1), (0, 1)],
          triangles: [0, 1, 2, 0, 2, 3],
          vertices: Unweighted([(0, 0), (64, 0), (64, 40), (0, 40)]),
        ) },
      },
      "red": { "gear": { "gear": Region(region: "gear") } },
      "blue": {},
    },
    constraints: [
      Ik(name: "limb", bones: ["thigh", "shin"], target: "aim", mix: 0.75, softness: 2.5,
         bend: Negative, stretch: true, max_stretch: 1.8, uniform: true),
      Transform(name: "copy", bones: ["head", "arm"], target: "aim",
                mix: (rotate: 0.5, x: 0.25, y: -0.5, scale_x: 1, scale_y: 2, shear_y: 0.1),
                offset: (rotation: 45, x: 1, y: 2, scale_x: 0.5, scale_y: -0.5, shear_y: 30),
                local: true, relative: true),
    ],
    events: {
      "step": (int: 1, float: 2.5, string: "left", audio: Some((path: "audio/step.ogg", volume: 0.8, balance: -0.5))),
      "hit": (),
    },
    animations: {
      "walk": (
        duration: 1.0,
        bones: {
          "torso": (
            rotate: [(0.0, 0.0), (0.5, 90.0, Stepped), (1.0, 0.0, Bezier(0.25, 0.1, 0.75, 0.9))],
            translate: [(0.0, (0, 0)), (1.0, (4, -2))],
            scale: [(0.0, (1, 1)), (1.0, (1.5, 1.5), Stepped)],
            shear: [(0.0, (0, 0)), (1.0, (90, -90))],
          ),
          "head": (
            translate_x: [(0.0, 0), (1.0, 3)],
            translate_y: [(0.0, 0), (1.0, -3)],
            scale_x: [(0.0, 1), (1.0, 2)],
            scale_y: [(0.0, 1), (1.0, 0.5)],
            shear_x: [(0.0, 0), (1.0, 180)],
            shear_y: [(0.0, 0), (1.0, -180)],
          ),
        },
        slots: {
          "head": (
            attachment: [(0.0, "head"), (0.5, None)],
            color: [(0.0, "#ffffff"), (1.0, "#ff000000", Stepped)],
          ),
          "glow": (deform: { "flag": [(0.0, []), (0.5, [0, -6, 0, -6, 0, 6, 0, 6], Bezier(0.25, 0, 0.75, 1)), (1.0, [])] }),
        },
        constraints: {
          "copy": (transform: [(0.25, (rotate: -1, shear_y: 3), Bezier(0.25, 0, 0.75, 1)), (1.0, (x: 2))]),
          "limb": (ik: [(0.0, (mix: 0.5)), (0.5, (softness: 1, bend: Positive, stretch: false), Stepped), (1.0, (mix: 0, softness: 0))]),
        },
        draw_order: [(0.25, ["head", "torso", "gear", "glow"]), (0.75, None)],
        events: [
          (0.5, "step"),
          (0.5, "hit"),
          (0.75, "step", (int: 9, float: 0.5, string: "right", volume: 0.3, balance: 1.0)),
        ],
      ),
      "run": (duration: 0.5),
    },
    mixes: { default: 0.1, pairs: [("walk", "run", 0.2), ("run", "walk", 0.3)] },
  )"##;

  fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
  }

  fn animation(name: &str, duration: f32, timelines: Vec<Timeline>) -> Animation {
    Animation {
      name: Arc::from(name),
      duration,
      timelines,
    }
  }

  fn unit_square() -> Vec<Vec2> {
    vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 0.0),
      Vec2::new(1.0, 1.0),
      Vec2::new(0.0, 1.0),
    ]
  }

  fn deg(value: f32) -> f32 {
    value.to_radians()
  }

  /// [`EVERYTHING`], built in code
  fn built(
    parts: crate::core::asset::Handle<Image>,
    extra: crate::core::asset::Handle<Image>,
  ) -> SkeletonData {
    let region = |name: &str, image: &crate::core::asset::Handle<Image>, x, y, w, h| Region {
      name: Arc::from(name),
      image: image.clone(),
      x,
      y,
      width: w,
      height: h,
    };
    let origin = || BoneSetup::default();
    let builder = SkeletonData::builder()
      .region(region("head", &parts, 0, 0, 64, 64))
      .region(region("torso", &parts, 64, 0, 48, 80))
      .region(region("gear", &extra, 0, 0, 16, 16))
      .bone("root", None, origin())
      .bone(
        "torso",
        Some("root"),
        BoneSetup {
          length: 40.0,
          x: 1.0,
          y: -40.0,
          rotation: deg(90.0),
          scale_x: 2.0,
          scale_y: 0.5,
          shear_x: deg(45.0),
          shear_y: deg(-30.0),
          inherit: Inherit::NoScale,
        },
      )
      .bone(
        "head",
        Some("torso"),
        BoneSetup {
          inherit: Inherit::OnlyTranslation,
          ..origin()
        },
      )
      .bone(
        "arm",
        Some("torso"),
        BoneSetup {
          inherit: Inherit::NoRotationOrReflection,
          ..origin()
        },
      )
      .bone(
        "leg",
        Some("root"),
        BoneSetup {
          inherit: Inherit::NoScaleOrReflection,
          ..origin()
        },
      )
      .bone(
        "thigh",
        Some("root"),
        BoneSetup {
          length: 10.0,
          ..origin()
        },
      )
      .bone(
        "shin",
        Some("thigh"),
        BoneSetup {
          x: 10.0,
          length: 10.0,
          ..origin()
        },
      )
      .bone("aim", Some("root"), origin())
      .slot(
        "torso",
        "torso",
        SlotSetup {
          color: rgb(255, 128, 0),
          attachment: Some(Arc::from("torso")),
          blend: SlotBlend::Normal,
        },
      )
      .slot(
        "head",
        "head",
        SlotSetup {
          color: rgb(10, 20, 30),
          attachment: Some(Arc::from("head")),
          blend: SlotBlend::Additive,
        },
      )
      .slot(
        "gear",
        "arm",
        SlotSetup {
          blend: SlotBlend::Multiply,
          ..SlotSetup::default()
        },
      )
      .slot(
        "glow",
        "leg",
        SlotSetup {
          color: Color::new(255, 255, 255, 128),
          blend: SlotBlend::Screen,
          ..SlotSetup::default()
        },
      )
      .skin("red")
      .skin("blue")
      .ik(
        "limb",
        &["thigh", "shin"],
        "aim",
        IkSetup {
          mix: 0.75,
          softness: 2.5,
          bend: Some(IkBend::Negative),
          compress: false,
          stretch: true,
          max_stretch: Some(1.8),
          uniform: true,
        },
      )
      .transform_constraint(
        "copy",
        &["head", "arm"],
        "aim",
        TransformSetup {
          mix: TransformMix {
            rotate: 0.5,
            x: 0.25,
            y: -0.5,
            scale_x: 1.0,
            scale_y: 2.0,
            shear_y: 0.1,
          },
          offset: TransformOffset {
            rotation: deg(45.0),
            x: 1.0,
            y: 2.0,
            scale_x: 0.5,
            scale_y: -0.5,
            shear_y: deg(30.0),
          },
          local: true,
          relative: true,
        },
      )
      .attachment(
        "default",
        "torso",
        "torso",
        Attachment::Region(RegionAttachment {
          y: -20.0,
          ..RegionAttachment::new(1, 48.0, 80.0)
        }),
      )
      .attachment(
        "default",
        "head",
        "head",
        Attachment::Region(RegionAttachment {
          region: 0,
          x: 1.0,
          y: -24.0,
          rotation: deg(180.0),
          scale_x: 1.5,
          scale_y: 2.0,
          width: 32.0,
          height: 30.0,
          color: Color::new(128, 128, 128, 128),
        }),
      )
      .attachment(
        "default",
        "gear",
        "muzzle",
        Attachment::Point(PointAttachment {
          x: 3.0,
          y: 4.0,
          rotation: deg(90.0),
        }),
      )
      .attachment(
        "default",
        "gear",
        "hit",
        Attachment::BoundingBox(BoundingBoxAttachment {
          vertices: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
          ],
        }),
      )
      .attachment(
        "default",
        "gear",
        "cape",
        Attachment::Mesh(MeshAttachment {
          hull: 4,
          color: Color::new(128, 128, 128, 128),
          ..MeshAttachment::new(
            1,
            MeshVertices::Weighted(WeightedVertices::new([
              vec![Influence::new(BoneId(1), -20.0, 0.0, 1.0)],
              vec![Influence::new(BoneId(1), 20.0, 0.0, 1.0)],
              vec![
                Influence::new(BoneId(3), 20.0, 0.0, 0.75),
                Influence::new(BoneId(1), 20.0, 60.0, 0.25),
              ],
              vec![Influence::new(BoneId(3), -20.0, 0.0, 1.0)],
            ])),
            unit_square(),
            vec![0, 1, 2, 0, 2, 3],
          )
        }),
      )
      .attachment(
        "default",
        "glow",
        "flag",
        Attachment::Mesh(MeshAttachment::new(
          0,
          MeshVertices::Unweighted(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(64.0, 0.0),
            Vec2::new(64.0, 40.0),
            Vec2::new(0.0, 40.0),
          ]),
          unit_square(),
          vec![0, 1, 2, 0, 2, 3],
        )),
      )
      .attachment(
        "red",
        "gear",
        "gear",
        Attachment::Region(RegionAttachment::new(2, 16.0, 16.0)),
      )
      .event(EventData {
        int: 1,
        float: 2.5,
        string: Arc::from("left"),
        audio: Some(EventAudio {
          path: Arc::from("audio/step.ogg"),
          volume: 0.8,
          balance: -0.5,
        }),
        ..EventData::new("step")
      })
      .event(EventData::new("hit"));
    let bone = |name: &str| builder.bone_id(name).expect("a bone");
    let (torso, head) = (bone("torso"), bone("head"));
    let head_slot = builder.slot_id("head").expect("a slot");
    let glow_slot = builder.slot_id("glow").expect("a slot");
    let slot_ids: Vec<SlotId> = ["head", "torso", "gear", "glow"]
      .iter()
      .map(|name| builder.slot_id(name).expect("a slot"))
      .collect();
    let (step, hit) = (
      builder.event_index("step").expect("event"),
      builder.event_index("hit").expect("event"),
    );
    let linear = Curve::Linear;
    let copy_mix = TransformMix {
      rotate: 0.5,
      x: 0.25,
      y: -0.5,
      scale_x: 1.0,
      scale_y: 2.0,
      shear_y: 0.1,
    };
    let limb = IkValues {
      mix: 0.75,
      softness: 2.5,
      bend: IkBend::Negative,
      compress: false,
      stretch: true,
      max_stretch: Some(1.8),
    };
    let walk = animation(
      "walk",
      1.0,
      vec![
        Timeline::Rotate {
          bone: torso,
          frames: vec![
            (0.0, 0.0, linear),
            (0.5, deg(90.0), Curve::Stepped),
            (
              1.0,
              0.0,
              Curve::Bezier {
                cx1: 0.25,
                cy1: 0.1,
                cx2: 0.75,
                cy2: 0.9,
              },
            ),
          ],
        },
        Timeline::Translate {
          bone: torso,
          frames: vec![
            (0.0, Vec2::new(0.0, 0.0), linear),
            (1.0, Vec2::new(4.0, -2.0), linear),
          ],
        },
        Timeline::Scale {
          bone: torso,
          frames: vec![
            (0.0, Vec2::new(1.0, 1.0), linear),
            (1.0, Vec2::new(1.5, 1.5), Curve::Stepped),
          ],
        },
        Timeline::Shear {
          bone: torso,
          frames: vec![
            (0.0, Vec2::new(0.0, 0.0), linear),
            (1.0, Vec2::new(deg(90.0), deg(-90.0)), linear),
          ],
        },
        Timeline::TranslateX {
          bone: head,
          frames: vec![(0.0, 0.0, linear), (1.0, 3.0, linear)],
        },
        Timeline::TranslateY {
          bone: head,
          frames: vec![(0.0, 0.0, linear), (1.0, -3.0, linear)],
        },
        Timeline::ScaleX {
          bone: head,
          frames: vec![(0.0, 1.0, linear), (1.0, 2.0, linear)],
        },
        Timeline::ScaleY {
          bone: head,
          frames: vec![(0.0, 1.0, linear), (1.0, 0.5, linear)],
        },
        Timeline::ShearX {
          bone: head,
          frames: vec![(0.0, 0.0, linear), (1.0, deg(180.0), linear)],
        },
        Timeline::ShearY {
          bone: head,
          frames: vec![(0.0, 0.0, linear), (1.0, deg(-180.0), linear)],
        },
        Timeline::Attachment {
          slot: head_slot,
          frames: vec![(0.0, Some(Arc::from("head"))), (0.5, None)],
        },
        Timeline::Color {
          slot: head_slot,
          frames: vec![
            (0.0, rgb(255, 255, 255), linear),
            (1.0, Color::new(255, 0, 0, 0), Curve::Stepped),
          ],
        },
        Timeline::Deform {
          slot: glow_slot,
          attachment: Arc::from("flag"),
          frames: vec![
            (0.0, Vec::new(), linear),
            (
              0.5,
              vec![0.0, -6.0, 0.0, -6.0, 0.0, 6.0, 0.0, 6.0],
              Curve::Bezier {
                cx1: 0.25,
                cy1: 0.0,
                cx2: 0.75,
                cy2: 1.0,
              },
            ),
            (1.0, Vec::new(), linear),
          ],
        },
        // The keys' left-out values are the setups': "copy" mixes rotate
        // 0.5, x 0.25, y -0.5, scale 1 and 2, shear 0.1; "limb" a mix of
        // 0.75, softness 2.5, bent the negative way, stretching
        Timeline::TransformMix {
          constraint: ConstraintId(1),
          frames: vec![
            (
              0.25,
              TransformMix {
                rotate: -1.0,
                shear_y: 3.0,
                ..copy_mix
              },
              Curve::Bezier {
                cx1: 0.25,
                cy1: 0.0,
                cx2: 0.75,
                cy2: 1.0,
              },
            ),
            (1.0, TransformMix { x: 2.0, ..copy_mix }, linear),
          ],
        },
        Timeline::Ik {
          constraint: ConstraintId(0),
          frames: vec![
            (0.0, IkValues { mix: 0.5, ..limb }, linear),
            (
              0.5,
              IkValues {
                softness: 1.0,
                bend: IkBend::Positive,
                stretch: false,
                ..limb
              },
              Curve::Stepped,
            ),
            (
              1.0,
              IkValues {
                mix: 0.0,
                softness: 0.0,
                ..limb
              },
              linear,
            ),
          ],
        },
        Timeline::DrawOrder {
          frames: vec![(0.25, Some(slot_ids)), (0.75, None)],
        },
        Timeline::Event {
          frames: vec![
            (0.5, step, EventValues::default()),
            (0.5, hit, EventValues::default()),
            (
              0.75,
              step,
              EventValues {
                int: Some(9),
                float: Some(0.5),
                string: Some(Arc::from("right")),
                volume: Some(0.3),
                balance: Some(1.0),
              },
            ),
          ],
        },
      ],
    );
    match builder
      .animation(walk)
      .animation(animation("run", 0.5, Vec::new()))
      .default_mix(0.1)
      .mix("walk", "run", 0.2)
      .mix("run", "walk", 0.3)
      .build()
    {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    }
  }

  #[test]
  fn a_file_using_every_feature_loads_as_the_builder_builds_it() {
    let mut world = registry_over(files(EVERYTHING));
    let data = match load_in(&mut world) {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    };
    let mut assets = AssetsView::new(&mut world);
    let parts = assets.load_now::<Image>(IMAGE).expect("parts");
    let extra = assets.load_now::<Image>(EXTRA).expect("extra");
    let expected = built(parts, extra);
    assert_eq!(data.bones(), expected.bones());
    assert_eq!(data.slots(), expected.slots());
    assert_eq!(data.regions(), expected.regions());
    assert_eq!(data.events(), expected.events());
    assert_eq!(data.constraints(), expected.constraints());
    assert_eq!(data.constraints().len(), 2);
    assert_eq!(data.animations(), expected.animations());
    assert_eq!(data.skins(), expected.skins());
    assert_eq!(data.default_mix(), expected.default_mix());
    for (from, to) in [("walk", "run"), ("run", "walk"), ("walk", "walk")] {
      assert_eq!(data.mix(from, to), expected.mix(from, to), "{from} {to}");
    }
    assert_eq!(data.skins().len(), 3, "default, red and blue");
  }

  #[test]
  fn a_stretch_limit_below_one_or_on_a_chain_fails_to_load() {
    let file = |constraint: &str| {
      format!(
        r#"(
          bones: [
            (name: "root"),
            (name: "a", parent: "root", length: 10),
            (name: "b", parent: "a", x: 10, length: 10),
            (name: "c", parent: "b", x: 10, length: 10),
            (name: "target", parent: "root"),
          ],
          constraints: [{constraint}],
        )"#
      )
    };
    let error = failure(&file(
      r#"Ik(name: "arm", bones: ["a", "b"], target: "target", stretch: true, max_stretch: 0.5)"#,
    ));
    assert!(
      error.contains("max_stretch is 0.5; it must be at least 1"),
      "{error}"
    );
    let error = failure(&file(
      r#"Ik(name: "tail", bones: ["a", "b", "c"], target: "target", max_stretch: 2)"#,
    ));
    assert!(
      error.contains("max_stretch applies only to one or two bones"),
      "{error}"
    );
    loaded(&file(
      r#"Ik(name: "arm", bones: ["a", "b"], target: "target", stretch: true, max_stretch: 1)"#,
    ));
  }

  #[test]
  fn a_file_s_constraints_load_and_match_the_builder_s() {
    let data = loaded(
      r#"(
        bones: [
          (name: "root"),
          (name: "pelvis", parent: "root"),
          (name: "thigh", parent: "pelvis", length: 10),
          (name: "shin", parent: "thigh", x: 10, length: 10),
          (name: "head", parent: "pelvis", length: 8),
          (name: "torso", parent: "root"),
          (name: "foot_target", parent: "root"),
          (name: "look_target", parent: "root"),
        ],
        constraints: [
          Ik(name: "leg", bones: ["thigh", "shin"], target: "foot_target", bend: Negative, softness: 6),
          Ik(name: "look", bones: ["head"], target: "look_target", mix: 0.5, stretch: true, max_stretch: 2, uniform: true),
          Transform(name: "level", bones: ["pelvis"], target: "torso", mix: (rotate: 0.6), offset: (rotation: 90), local: true),
        ],
      )"#,
    );
    let long = BoneSetup {
      length: 10.0,
      ..BoneSetup::default()
    };
    let expected = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("pelvis", Some("root"), BoneSetup::default())
      .bone("thigh", Some("pelvis"), long)
      .bone("shin", Some("thigh"), BoneSetup { x: 10.0, ..long })
      .bone(
        "head",
        Some("pelvis"),
        BoneSetup {
          length: 8.0,
          ..BoneSetup::default()
        },
      )
      .bone("torso", Some("root"), BoneSetup::default())
      .bone("foot_target", Some("root"), BoneSetup::default())
      .bone("look_target", Some("root"), BoneSetup::default())
      .ik(
        "leg",
        &["thigh", "shin"],
        "foot_target",
        IkSetup {
          bend: Some(IkBend::Negative),
          softness: 6.0,
          ..IkSetup::default()
        },
      )
      .ik(
        "look",
        &["head"],
        "look_target",
        IkSetup {
          mix: 0.5,
          stretch: true,
          max_stretch: Some(2.0),
          uniform: true,
          ..IkSetup::default()
        },
      )
      .transform_constraint(
        "level",
        &["pelvis"],
        "torso",
        TransformSetup {
          mix: TransformMix {
            rotate: 0.6,
            ..TransformMix::default()
          },
          offset: TransformOffset {
            rotation: deg(90.0),
            ..TransformOffset::default()
          },
          local: true,
          relative: false,
        },
      )
      .build()
      .expect("valid skeleton");
    assert_eq!(data.constraints(), expected.constraints());
    assert_eq!(data.constraints().len(), 3);
    assert_eq!(data.constraint("level"), expected.constraint("level"));
  }

  #[test]
  fn the_bend_setting_loads_keys_and_overrides() {
    use glam::Affine2;

    use crate::core::skeleton::{Constraint, ConstraintOverride, IkOverride, Skeleton};
    // Two bones bent the negative way, a chain and one bone leaving it out;
    // keys that leave it out and keys that set it
    let mut world = registry_over(files(
      r#"(
        bones: [
          (name: "root"),
          (name: "thigh", parent: "root", length: 10),
          (name: "shin", parent: "thigh", x: 10, length: 10),
          (name: "tail_1", parent: "root", x: -10, length: 10),
          (name: "tail_2", parent: "tail_1", x: 10, length: 10),
          (name: "tail_3", parent: "tail_2", x: 10, length: 10),
          (name: "neck", parent: "root", y: 40, length: 5),
          (name: "foot_target", parent: "root", x: 12),
          (name: "tail_target", parent: "root", x: 5),
        ],
        constraints: [
          Ik(name: "leg", bones: ["thigh", "shin"], target: "foot_target", bend: Negative),
          Ik(name: "tail", bones: ["tail_1", "tail_2", "tail_3"], target: "tail_target"),
          Ik(name: "look", bones: ["neck"], target: "foot_target"),
        ],
        animations: { "walk": (
          duration: 1.0,
          constraints: {
            "leg": (ik: [(0.0, (mix: 1)), (0.5, (bend: Positive))]),
            "tail": (ik: [(0.0, (mix: 1)), (0.5, (bend: Negative))]),
          },
        ) },
      )"#,
    ));
    let handle = match AssetsView::new(&mut world).load_now::<SkeletonData>(PATH) {
      Ok(handle) => handle,
      Err(error) => panic!("{}", error.report()),
    };
    let assets = world.resource::<Assets<SkeletonData>>();
    let (Some(data), Some(version)) = (assets.get(&handle), assets.version(&handle)) else {
      panic!("not in the store");
    };
    // The setup's, or the default for its bone count
    let found = |name: &str| {
      data
        .constraint(name)
        .and_then(|id| data.constraints().get(id.index()))
    };
    let bend = |name: &str| match found(name) {
      Some(Constraint::Ik(ik)) => ik.setup.bend,
      other => panic!("{name}: {other:?}"),
    };
    assert_eq!(bend("leg"), Some(IkBend::Negative));
    assert_eq!(bend("tail"), Some(IkBend::Pose));
    assert_eq!(bend("look"), Some(IkBend::Positive));
    // A key that leaves it out takes the setup's
    let Some(walk) = data.animation("walk") else {
      panic!("no walk");
    };
    let keyed: Vec<Vec<IkBend>> = walk
      .timelines
      .iter()
      .filter_map(|timeline| match timeline {
        Timeline::Ik { frames, .. } => Some(frames.iter().map(|frame| frame.1.bend).collect()),
        _ => None,
      })
      .collect();
    assert_eq!(
      keyed,
      vec![
        vec![IkBend::Negative, IkBend::Positive],
        vec![IkBend::Pose, IkBend::Negative]
      ]
    );

    // An override bends it: the knee and the tail's middle joint, on the
    // line along x to their targets, go to the side asked for (y below 0
    // the positive side, above it the negative). A bend the constraint
    // cannot have is passed over
    let mut skeleton = Skeleton::new(handle.clone());
    let sides = |skeleton: &mut Skeleton| {
      skeleton.appear(data, (handle.id(), version), None, Affine2::IDENTITY.into());
      let y = |bone: &str| skeleton.bone_world(bone).map_or(0.0, |at| at.translation.y);
      (y("shin").signum(), y("tail_2").signum())
    };
    assert_eq!(sides(&mut skeleton), (1.0, -1.0));
    let bent = |bend| {
      ConstraintOverride::Ik(IkOverride {
        bend: Some(bend),
        ..IkOverride::default()
      })
    };
    skeleton.override_constraint("leg", bent(IkBend::Positive));
    skeleton.override_constraint("tail", bent(IkBend::Negative));
    assert_eq!(sides(&mut skeleton), (-1.0, 1.0));
    skeleton.override_constraint("leg", bent(IkBend::Pose));
    skeleton.override_constraint("tail", bent(IkBend::Positive));
    assert_eq!(sides(&mut skeleton), (1.0, -1.0));
  }

  #[test]
  fn constraint_timelines_load_and_match_the_builder_s() {
    // The brief's example: a value a key leaves out is the setup's (the
    // leg's mix 0.8, softness 3, bent the positive way; the pelvis's rotate
    // mix 0.5, its x mix 0.1)
    let data = loaded(
      r#"(
        bones: [
          (name: "root"),
          (name: "pelvis", parent: "root"),
          (name: "thigh", parent: "root", length: 10),
          (name: "shin", parent: "thigh", x: 10, length: 10),
          (name: "torso", parent: "root"),
          (name: "foot", parent: "root"),
        ],
        constraints: [
          Ik(name: "leg", bones: ["thigh", "shin"], target: "foot", mix: 0.8, softness: 3),
          Transform(name: "pelvis", bones: ["pelvis"], target: "torso", mix: (rotate: 0.5, x: 0.1)),
        ],
        animations: { "walk": (
          duration: 0.5,
          bones: { "torso": (rotate: [(0.0, 10)]) },
          constraints: {
            "leg": (ik: [(0.0, (mix: 1)), (0.3, (mix: 0, bend: Negative), Bezier(0.3, 0, 0.7, 1))]),
            "pelvis": (transform: [(0.0, (rotate: 0.6)), (0.5, (rotate: 0.9, x: 0.2))]),
          },
        ) },
      )"#,
    );
    let builder = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("pelvis", Some("root"), BoneSetup::default())
      .bone(
        "thigh",
        Some("root"),
        BoneSetup {
          length: 10.0,
          ..BoneSetup::default()
        },
      )
      .bone(
        "shin",
        Some("thigh"),
        BoneSetup {
          x: 10.0,
          length: 10.0,
          ..BoneSetup::default()
        },
      )
      .bone("torso", Some("root"), BoneSetup::default())
      .bone("foot", Some("root"), BoneSetup::default());
    let leg = IkSetup {
      mix: 0.8,
      softness: 3.0,
      ..IkSetup::default()
    };
    let pelvis = TransformMix {
      rotate: 0.5,
      x: 0.1,
      ..TransformMix::default()
    };
    let torso = builder.bone_id("torso").expect("a bone");
    let setup = IkValues::of(&leg);
    let walk = animation(
      "walk",
      0.5,
      vec![
        Timeline::Rotate {
          bone: torso,
          frames: vec![(0.0, deg(10.0), Curve::Linear)],
        },
        Timeline::Ik {
          constraint: ConstraintId(0),
          frames: vec![
            (0.0, IkValues { mix: 1.0, ..setup }, Curve::Linear),
            (
              0.3,
              IkValues {
                mix: 0.0,
                bend: IkBend::Negative,
                ..setup
              },
              Curve::Bezier {
                cx1: 0.3,
                cy1: 0.0,
                cx2: 0.7,
                cy2: 1.0,
              },
            ),
          ],
        },
        Timeline::TransformMix {
          constraint: ConstraintId(1),
          frames: vec![
            (
              0.0,
              TransformMix {
                rotate: 0.6,
                ..pelvis
              },
              Curve::Linear,
            ),
            (
              0.5,
              TransformMix {
                rotate: 0.9,
                x: 0.2,
                ..pelvis
              },
              Curve::Linear,
            ),
          ],
        },
      ],
    );
    let expected = builder
      .ik("leg", &["thigh", "shin"], "foot", leg)
      .transform_constraint(
        "pelvis",
        &["pelvis"],
        "torso",
        TransformSetup {
          mix: pelvis,
          ..TransformSetup::default()
        },
      )
      .animation(walk)
      .build()
      .expect("a valid skeleton");
    assert_eq!(data.animations(), expected.animations());
    assert_eq!(data.constraints(), expected.constraints());
    // The leg's second key keeps the setup's softness, which it leaves out
    match &data.animation("walk").expect("walk").timelines[1] {
      Timeline::Ik { frames, .. } => assert_eq!(frames[1].1.softness, 3.0),
      other => panic!("{other:?}"),
    }
  }

  /// A file with a torso and a head bone, a slot "s" on the torso and the
  /// skin's attachments `attachments`
  fn mesh_file(attachments: &str) -> String {
    format!(
      r#"(
        images: {{ "parts": "hero.png" }},
        regions: {{ "r": (image: "parts", rect: (0, 0, 64, 64)) }},
        bones: [(name: "torso"), (name: "head", parent: "torso")],
        slots: [(name: "s", bone: "torso")],
        skins: {{ "default": {{ "s": {{ {attachments} }} }} }},
      )"#
    )
  }

  #[test]
  fn a_mesh_attachment_loads_weighted_and_unweighted() {
    let data = loaded(&mesh_file(
      r##"
      "cape": Mesh(
        region: "r",
        uvs: [(0, 0), (1, 0), (0.5, 1)],
        triangles: [0, 1, 2],
        hull: 3,
        vertices: Weighted([
          [("torso", -20, 0, 1)],
          [("torso", 20, 0, 3), ("head", 0, 10, 1)],
          [("head", 5, 6, 2)],
        ]),
        color: "#ff000080",
      ),
      "flag": Mesh(
        region: "r",
        uvs: [(0, 0), (1, 0), (1, 1)],
        triangles: [0, 1, 2],
        vertices: Unweighted([(0, 0), (64, 0), (64, 40)]),
      ),
      "##,
    ));
    let skin = &data.skins()[0];
    let Some(Attachment::Mesh(cape)) = skin.attachment(SlotId(0), "cape") else {
      panic!("no cape");
    };
    assert_eq!((cape.region, cape.hull), (0, 3));
    assert_eq!(cape.uvs[2], Vec2::new(0.5, 1.0));
    assert_eq!(cape.triangles, [0, 1, 2]);
    assert_eq!(cape.color, Color::new(255, 0, 0, 128));
    assert_eq!((cape.vertex_count(), cape.position_count()), (3, 4));
    let MeshVertices::Weighted(vertices) = &cape.vertices else {
      panic!("not weighted");
    };
    assert_eq!(
      vertices.influences(1),
      [
        Influence::new(BoneId(0), 20.0, 0.0, 0.75),
        Influence::new(BoneId(1), 0.0, 10.0, 0.25),
      ]
    );
    assert_eq!(
      vertices.influences(2),
      [Influence::new(BoneId(1), 5.0, 6.0, 1.0)],
      "weights are normalised"
    );
    let Some(Attachment::Mesh(flag)) = skin.attachment(SlotId(0), "flag") else {
      panic!("no flag");
    };
    assert_eq!((flag.hull, flag.color), (0, Color::WHITE));
    assert_eq!(
      flag.vertices,
      MeshVertices::Unweighted(vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(64.0, 0.0),
        Vec2::new(64.0, 40.0)
      ])
    );
  }

  /// A pole with a slot "banner" showing "flag", a mesh of four vertices,
  /// which can show "pennant", a region; and a slot "spare" that shows
  /// nothing and can show a point, "flag". `slots` is the slots block of
  /// its one animation, "wave"
  fn banner_file(slots: &str) -> String {
    format!(
      r#"(
        images: {{ "parts": "hero.png" }},
        regions: {{ "r": (image: "parts", rect: (0, 0, 64, 64)) }},
        bones: [(name: "pole")],
        slots: [(name: "banner", bone: "pole", attachment: "flag"), (name: "spare", bone: "pole")],
        skins: {{ "default": {{
          "banner": {{
            "flag": Mesh(region: "r", uvs: [(0, 0), (1, 0), (1, 1), (0, 1)], triangles: [0, 1, 2, 0, 2, 3],
                         vertices: Unweighted([(0, 0), (64, 0), (64, 40), (0, 40)])),
            "pennant": Region(region: "r"),
          }},
          "spare": {{ "flag": Point() }},
        }} }},
        animations: {{ "wave": (duration: 1.0, slots: {{ {slots} }}) }},
      )"#
    )
  }

  #[test]
  fn a_deform_timeline_loads_and_matches_the_builder_s() {
    // A slot's block holds a `deform` map from attachment names to keys:
    // each key a time, a list of offsets (x then y for each position, or
    // none for the setup shape) and, if written, a curve
    let mut world = registry_over(files(&banner_file(
      r#"
      "banner": (
        deform: {
          "flag": [(0.0, []), (0.5, [0, -6, 0, -6, 0, 6, 0, 6], Bezier(0.25, 0, 0.75, 1)), (1.0, [])],
          "pennant": [(0.25, [1.5, 2, 3, -4.25], Stepped)],
        },
        attachment: [(0.0, "flag")],
      ),
      "spare": (deform: { "flag": [(0.0, [])] }),
      "#,
    )));
    let data = match load_in(&mut world) {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    };
    // The same animation built in code: a slot's attachment and colour
    // timelines come first, then its deforms in the file's order
    let mut assets = AssetsView::new(&mut world);
    let image = assets.load_now::<Image>("hero.png").expect("the image");
    let builder = SkeletonData::builder()
      .bone("pole", None, BoneSetup::default())
      .slot(
        "banner",
        "pole",
        SlotSetup {
          attachment: Some(Arc::from("flag")),
          ..SlotSetup::default()
        },
      )
      .slot("spare", "pole", SlotSetup::default())
      .region(Region {
        name: Arc::from("r"),
        image,
        x: 0,
        y: 0,
        width: 64,
        height: 64,
      })
      .attachment(
        "default",
        "banner",
        "flag",
        Attachment::Mesh(MeshAttachment::new(
          0,
          MeshVertices::Unweighted(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(64.0, 0.0),
            Vec2::new(64.0, 40.0),
            Vec2::new(0.0, 40.0),
          ]),
          unit_square(),
          vec![0, 1, 2, 0, 2, 3],
        )),
      )
      .attachment(
        "default",
        "banner",
        "pennant",
        Attachment::Region(RegionAttachment::new(0, 64.0, 64.0)),
      )
      .attachment(
        "default",
        "spare",
        "flag",
        Attachment::Point(PointAttachment::default()),
      );
    let (banner, spare) = (
      builder.slot_id("banner").expect("a slot"),
      builder.slot_id("spare").expect("a slot"),
    );
    let deform = |slot: SlotId, attachment: &str, frames| Timeline::Deform {
      slot,
      attachment: Arc::from(attachment),
      frames,
    };
    let ease = Curve::Bezier {
      cx1: 0.25,
      cy1: 0.0,
      cx2: 0.75,
      cy2: 1.0,
    };
    let wave = animation(
      "wave",
      1.0,
      vec![
        Timeline::Attachment {
          slot: banner,
          frames: vec![(0.0, Some(Arc::from("flag")))],
        },
        deform(
          banner,
          "flag",
          vec![
            (0.0, Vec::new(), Curve::Linear),
            (0.5, vec![0.0, -6.0, 0.0, -6.0, 0.0, 6.0, 0.0, 6.0], ease),
            (1.0, Vec::new(), Curve::Linear),
          ],
        ),
        deform(
          banner,
          "pennant",
          vec![(0.25, vec![1.5, 2.0, 3.0, -4.25], Curve::Stepped)],
        ),
        deform(spare, "flag", vec![(0.0, Vec::new(), Curve::Linear)]),
      ],
    );
    let expected = builder.animation(wave).build().expect("a valid skeleton");
    assert_eq!(data.animations(), expected.animations());
    assert_eq!(data.skins(), expected.skins());
    assert_eq!(data.slots(), expected.slots());

    // That a deform's keys fit a mesh is found when it plays, as the skin
    // worn decides the mesh (the pennant here is a region, and the spare
    // slot's flag a point); what can be checked in the file is, naming the
    // animation and its timeline
    for (slots, item, reason) in [
      (
        r#""banner": (deform: { "flag": [(0.0, [1, 2, 3, 4]), (1.0, [1, 2])] })"#,
        r#"animation "wave" timeline 0 (deform)"#,
        "key 1 holds 2 values where key 0 holds 4",
      ),
      (
        r#""banner": (color: [(0.0, (r: 9, g: 9, b: 9))], deform: { "flag": [(0.5, [1, 2]), (0.25, [1, 2])] })"#,
        r#"animation "wave" timeline 1 (deform)"#,
        "time order",
      ),
      (
        r#""banner": (deform: { "flag": [(0.0, [1, 2])], "flag": [(0.0, [1, 2])] })"#,
        "",
        "`flag` is written twice",
      ),
      (
        r#""banner": (deform: { "flag": [] })"#,
        r#"animation "wave" timeline 0 (deform)"#,
        "no keys",
      ),
      (
        r#""flagpole": (deform: { "flag": [(0.0, [])] })"#,
        r#"animation "wave""#,
        r#""flagpole" is not a slot"#,
      ),
      (
        r#""banner": (deform: { "flagg": [(0.0, [1, 2])] })"#,
        r#"animation "wave" timeline 0 (deform)"#,
        r#"no skin has an attachment "flagg" in slot "banner""#,
      ),
      (
        r#""spare": (deform: { "pennant": [(0.0, [])] })"#,
        r#"animation "wave" timeline 0 (deform)"#,
        r#"no skin has an attachment "pennant" in slot "spare""#,
      ),
    ] {
      let error = failure(&banner_file(slots));
      assert!(error.contains(PATH), "{error}");
      assert!(error.contains(item), "{item} in {error}");
      assert!(error.contains(reason), "{reason} in {error}");
    }
  }

  #[test]
  fn a_mistake_in_a_mesh_names_its_attachment() {
    let uvs = "uvs: [(0, 0), (1, 0), (1, 1)]";
    let flat = "Unweighted([(0, 0), (1, 0), (1, 1)])";
    let item = r#"skin "default" slot "s" attachment "m""#;
    for (mesh, reason) in [
      (
        format!(r#"Mesh(region: "r", {uvs}, triangles: [0, 1, 3], vertices: {flat})"#),
        "triangle index 3 is out of range",
      ),
      (
        format!(
          r#"Mesh(region: "r", {uvs}, triangles: [0, 1, 2], vertices: Weighted([[("torso", 0, 0, 1)], [("torso", 0, 0, 0), ("head", 0, 0, 0)], [("head", 0, 0, 1)]]))"#
        ),
        "vertex 1's weights sum to 0",
      ),
      (
        format!(
          r#"Mesh(region: "r", uvs: [(0, 0), (1, 0)], triangles: [0, 1, 2], vertices: {flat})"#
        ),
        "2 UVs for 3 vertices",
      ),
      (
        format!(
          r#"Mesh(region: "r", {uvs}, triangles: [0, 1, 2], vertices: Weighted([[("torso", 0, 0, 1)], [("tail", 0, 0, 1)], [("head", 0, 0, 1)]]))"#
        ),
        r#""tail""#,
      ),
      (
        format!(r#"Mesh(region: "q", {uvs}, triangles: [0, 1, 2], vertices: {flat})"#),
        r#""q""#,
      ),
    ] {
      let error = failure(&mesh_file(&format!(r#""m": {mesh},"#)));
      assert!(error.contains(PATH), "{error}");
      assert!(error.contains(item), "{item} in {error}");
      assert!(error.contains(reason), "{reason} in {error}");
    }
  }

  #[test]
  fn a_missing_image_fails_as_a_dependency() {
    let error = failure(r#"(images: { "i": "rigs/nowhere.png" }, bones: [(name: "root")])"#);
    assert!(error.contains(PATH), "{error}");
    assert!(error.contains("rigs/nowhere.png"), "{error}");
    let error = failure(r#"(images: { "i": "../../up.png" })"#);
    assert!(error.contains("../../up.png"), "{error}");
  }

  #[test]
  fn a_mistake_in_the_file_names_the_file_and_the_item() {
    for (text, item, reason) in [
      (
        r#"(bones: [(name: "a"), (name: "a")])"#,
        r#"bone "a""#,
        "same name",
      ),
      (
        r#"(bones: [(name: "a", parent: "b")])"#,
        r#"bone "a""#,
        r#""b""#,
      ),
      (
        r#"(bones: [(name: "a")], animations: { "<empty>": (duration: 1.0) })"#,
        r#"animation "<empty>""#,
        "reserved",
      ),
      (
        r#"(bones: [(name: "a")], animations: { "w": (duration: 1.0, bones: { "a": (rotate: [(2.0, 0.0)]) }) })"#,
        r#"animation "w" timeline 0 (rotate)"#,
        "duration",
      ),
      (r#"(bones: [(name: "a", x: inf)])"#, r#"bone "a""#, "finite"),
      (
        r#"(bones: [(name: "a")], animations: { "w": (duration: 1.0, bones: { "z": (rotate: [(0.0, 0.0)]) }) })"#,
        r#"animation "w""#,
        r#""z""#,
      ),
      (
        r#"(bones: [(name: "a")], slots: [(name: "s", bone: "a")], animations: { "w": (duration: 1.0, draw_order: [(0.0, ["q", "s"])]) })"#,
        r#"animation "w""#,
        r#""q""#,
      ),
      (
        r#"(bones: [(name: "a")], animations: { "w": (duration: 1.0, events: [(0.0, "boom")]) })"#,
        r#"animation "w""#,
        r#""boom""#,
      ),
      (
        r#"(images: { "i": "rigs/hero.png" }, regions: { "r": (image: "j", rect: (0, 0, 8, 8)) })"#,
        r#"region "r""#,
        r#""j""#,
      ),
      (
        r#"(bones: [(name: "a")], slots: [(name: "s", bone: "a"), (name: "t", bone: "a")], animations: { "w": (duration: 1.0, draw_order: [(0.5, ["s"])]) })"#,
        r#"animation "w" timeline 0 (draw order)"#,
        "key 0",
      ),
      (
        r#"(bones: [(name: "a")], slots: [(name: "s", bone: "a")], skins: { "default": { "nope": { "x": Point() } } })"#,
        "skin",
        r#""nope""#,
      ),
      (
        r#"(skins: { "default": {} , "red": { "nope": { "x": Point() } } })"#,
        r#"skin "red""#,
        r#""nope""#,
      ),
      (
        r#"(mixes: { pairs: [("x", "y", 0.1)] })"#,
        r#"mix "x" to "y""#,
        "not an animation",
      ),
      (
        r#"(bones: [(name: "a", length: 5), (name: "b", parent: "a", length: 5)], constraints: [Ik(name: "c", bones: ["a", "b"], target: "z")])"#,
        r#"constraint "c""#,
        r#"its target "z" is not a bone"#,
      ),
      (
        r#"(bones: [(name: "a"), (name: "t")], constraints: [Transform(name: "c", bones: ["a"], target: "t", mix: (rotate: inf))])"#,
        r#"constraint "c""#,
        "mix rotate is inf",
      ),
      (
        r#"(bones: [(name: "a")], slots: [(name: "s", bone: "a")], skins: { "default": { "s": { "flag": Point() } } }, animations: { "w": (duration: 1.0, slots: { "s": (deform: { "flagg": [(0.0, [])] }) }) })"#,
        r#"animation "w" timeline 0 (deform)"#,
        r#"no skin has an attachment "flagg" in slot "s""#,
      ),
      (
        r#"(bones: [(name: "a", length: 5), (name: "t")], constraints: [Ik(name: "c", bones: ["a"], target: "t")], animations: { "w": (duration: 1.0, constraints: { "d": (ik: [(0.0, (mix: 1))]) }) })"#,
        r#"animation "w""#,
        r#"constraints: "d" is not a constraint"#,
      ),
      (
        r#"(bones: [(name: "a", length: 5), (name: "t")], constraints: [Ik(name: "c", bones: ["a"], target: "t")], animations: { "w": (duration: 1.0, constraints: { "c": (transform: [(0.0, (rotate: 1))]) }) })"#,
        r#"animation "w" timeline 0 (transform mix)"#,
        r#"constraint "c" is an IK constraint"#,
      ),
      (
        r#"(bones: [(name: "a", length: 5), (name: "b", parent: "a", length: 5), (name: "t")], constraints: [Ik(name: "c", bones: ["a", "b"], target: "t")], animations: { "w": (duration: 1.0, constraints: { "c": (ik: [(0.0, (compress: true))]) }) })"#,
        r#"animation "w" timeline 0 (ik)"#,
        "key 0's compress applies only to one bone",
      ),
    ] {
      let error = failure(text);
      assert!(error.contains(PATH), "{error}");
      assert!(error.contains(item), "{item} in {error}");
      assert!(error.contains(reason), "{reason} in {error}");
    }
    // A misspelt key value too
    let error = failure(
      r#"(bones: [(name: "a", length: 5), (name: "t")], constraints: [Ik(name: "c", bones: ["a"], target: "t")], animations: { "w": (duration: 1.0, constraints: { "c": (ik: [(0.0, (mx: 1))]) }) })"#,
    );
    assert!(error.contains("mx"), "{error}");
    assert!(error.contains("line 1"), "{error}");
    // A misspelt setting is a mistake in the text, at its line and column
    let error = failure(
      r#"(bones: [(name: "a", length: 5), (name: "t")], constraints: [Ik(name: "c", bones: ["a"], target: "t", strech: true)])"#,
    );
    assert!(error.contains(PATH), "{error}");
    assert!(error.contains("strech"), "{error}");
    assert!(error.contains("line 1"), "{error}");
  }

  #[test]
  fn an_image_is_named_from_the_asset_root_not_the_file_s_folder() {
    // The file is in rigs/; the image only exists at other/pic.png
    let mut world = registry_over(files(
      r#"(images: { "i": "other/pic.png" }, regions: { "r": (image: "i", rect: (0, 0, 8, 8)) })"#,
    ));
    let data = match load_in(&mut world) {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    };
    let pic = AssetsView::new(&mut world)
      .load_now::<Image>("other/pic.png")
      .expect("pic");
    assert_eq!(data.regions()[0].image, pic);
  }

  #[test]
  fn a_name_written_twice_in_a_map_fails() {
    let error = failure(r#"(events: { "a": (), "a": () })"#);
    assert!(error.contains("written twice"), "{error}");
    assert!(error.contains("line 1"), "{error}");
  }

  #[test]
  fn a_region_outside_its_image_fails_the_check() {
    let error = failure(
      r#"(images: { "i": "rigs/hero.png" }, regions: { "r": (image: "i", rect: (100, 0, 64, 64)) })"#,
    );
    assert!(error.contains(PATH), "{error}");
    assert!(error.contains(r#"region "r""#), "{error}");
    assert!(error.contains("128"), "{error}");
  }

  /// The loader's rustdoc example, which would otherwise rot unseen
  #[test]
  fn the_documented_example_loads() {
    let source = include_str!("skeleton.rs");
    let start = source.find("/// ```ron\n").expect("the example") + "/// ```ron\n".len();
    let end = start + source[start..].find("/// ```").expect("its end");
    let text: String = source[start..end]
      .lines()
      .map(|line| line.strip_prefix("///").unwrap_or(line))
      .collect::<Vec<_>>()
      .join("\n");
    let data = loaded(&text);
    assert_eq!(data.animations().len(), 2);
    assert_eq!(data.skins().len(), 2);
    assert_eq!(data.mix("walk", "run"), 0.2);
    assert_eq!(data.constraints().len(), 2);
    // Its deform keys are for the flag it declares: three positions
    let walk = data.animation("walk").expect("walk");
    let deforms: Vec<_> = walk
      .timelines
      .iter()
      .filter_map(|timeline| match timeline {
        Timeline::Deform {
          attachment, frames, ..
        } => Some((&**attachment, frames.len(), frames[1].1.len())),
        _ => None,
      })
      .collect();
    assert_eq!(deforms, [("flag", 3, 6)]);
    // Both kinds of constraint timeline, the IK one's second key keeping
    // the setup's switches it leaves out
    let constraints: Vec<_> = walk
      .timelines
      .iter()
      .filter_map(|timeline| match timeline {
        Timeline::Ik { constraint, frames } => {
          let key = frames[1].1;
          Some((constraint.0, frames.len(), key.mix, key.stretch))
        }
        Timeline::TransformMix { constraint, frames } => {
          Some((constraint.0, frames.len(), frames[1].1.rotate, false))
        }
        _ => None,
      })
      .collect();
    assert_eq!(constraints, [(0, 2, 0.5, false), (1, 2, 0.5, false)]);
  }

  #[test]
  fn the_registry_has_the_store() {
    let world = registry_over(MemorySource::new());
    assert!(world.get_resource::<Assets<SkeletonData>>().is_some());
  }
}
