//! Checking a skeleton as it is built.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use super::animation::{EMPTY_NAME, Target, Timeline};
use super::attachment::Attachment;
use super::builder::SkeletonDataBuilder;
use super::curve::Curve;
use super::data::{AnimationCatalogue, BoneData, BoneId, SkeletonData, SlotData, SlotId};
use super::skin::Skin;
use crate::core::ecs::Vec2;

/// Why a skeleton could not be built: the item at fault (`bone "arm"`,
/// `skin "default" slot "head" attachment "hat"`, ...) and the problem
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonError {
  pub item: String,
  pub problem: String,
}

impl fmt::Display for SkeletonError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}: {}", self.item, self.problem)
  }
}

impl std::error::Error for SkeletonError {}

/// Bones and slots are indexed by a `u8`, so a skeleton has at most this
/// many of each
pub(super) const MAX_BONES: usize = 255;
pub(super) const MAX_SLOTS: usize = 255;

fn error(item: impl Into<String>, problem: impl Into<String>) -> SkeletonError {
  SkeletonError {
    item: item.into(),
    problem: problem.into(),
  }
}

/// Fails on the first of `values` (named) that is NaN or infinite
fn finite(item: impl Fn() -> String, values: &[(&str, f32)]) -> Result<(), SkeletonError> {
  match values.iter().find(|(_, value)| !value.is_finite()) {
    Some((name, value)) => Err(error(
      item(),
      format!("{name} is {value}, not a finite number"),
    )),
    None => Ok(()),
  }
}

fn finite_vec(item: impl Fn() -> String, name: &str, value: Vec2) -> Result<(), SkeletonError> {
  finite(item, &[(name, value.x), (name, value.y)])
}

fn curve(item: impl Fn() -> String, frame: usize, curve: &Curve) -> Result<(), SkeletonError> {
  if let Curve::Bezier { cx1, cy1, cx2, cy2 } = *curve {
    let name = format!("key {frame}'s curve");
    finite(
      &item,
      &[(&name, cx1), (&name, cy1), (&name, cx2), (&name, cy2)],
    )?;
    if !(0.0..=1.0).contains(&cx1) || !(0.0..=1.0).contains(&cx2) {
      return Err(error(
        item(),
        format!("key {frame}'s curve has x controls {cx1} and {cx2}; both must be within 0 to 1"),
      ));
    }
  }
  Ok(())
}

pub(super) fn build(builder: SkeletonDataBuilder) -> Result<SkeletonData, SkeletonError> {
  let SkeletonDataBuilder {
    bones: pending_bones,
    slots: pending_slots,
    regions,
    skins: declared_skins,
    attachments,
    animations,
    events,
    mixes: pending_mixes,
    default_mix,
  } = builder;

  // Bones: parents first, unique, finite, at most 255
  let mut bones = Vec::with_capacity(pending_bones.len());
  let mut bone_names: HashMap<Arc<str>, BoneId> = HashMap::new();
  for (index, bone) in pending_bones.iter().enumerate() {
    let item = || format!("bone {:?}", &*bone.name);
    let id = match u8::try_from(index) {
      Ok(index) if usize::from(index) < MAX_BONES => BoneId(index),
      _ => {
        return Err(error(
          item(),
          format!("a skeleton has at most {MAX_BONES} bones"),
        ));
      }
    };
    if bone_names.contains_key(&bone.name) {
      return Err(error(item(), "another bone has the same name"));
    }
    let parent = match &bone.parent {
      None => None,
      Some(parent) => match bone_names.get(parent) {
        Some(&parent) => Some(parent),
        None
          if pending_bones[index..]
            .iter()
            .any(|later| later.name == *parent) =>
        {
          return Err(error(
            item(),
            format!(
              "its parent {:?} comes after it; parents must come first",
              &**parent
            ),
          ));
        }
        None => {
          return Err(error(
            item(),
            format!("its parent {:?} is not a bone", &**parent),
          ));
        }
      },
    };
    let setup = &bone.setup;
    finite(
      item,
      &[
        ("length", setup.length),
        ("x", setup.x),
        ("y", setup.y),
        ("rotation", setup.rotation),
        ("scale_x", setup.scale_x),
        ("scale_y", setup.scale_y),
        ("shear_x", setup.shear_x),
        ("shear_y", setup.shear_y),
      ],
    )?;
    bone_names.insert(bone.name.clone(), id);
    bones.push(BoneData {
      name: bone.name.clone(),
      parent,
      length: setup.length,
      x: setup.x,
      y: setup.y,
      rotation: setup.rotation,
      scale_x: setup.scale_x,
      scale_y: setup.scale_y,
      shear_x: setup.shear_x,
      shear_y: setup.shear_y,
      inherit: setup.inherit,
    });
  }

  // Slots: unique, on a bone, at most 255
  let mut slots = Vec::with_capacity(pending_slots.len());
  let mut slot_names: HashMap<Arc<str>, SlotId> = HashMap::new();
  for (index, slot) in pending_slots.into_iter().enumerate() {
    let item = || format!("slot {:?}", &*slot.name);
    let id = match u8::try_from(index) {
      Ok(index) if usize::from(index) < MAX_SLOTS => SlotId(index),
      _ => {
        return Err(error(
          item(),
          format!("a skeleton has at most {MAX_SLOTS} slots"),
        ));
      }
    };
    if slot_names.contains_key(&slot.name) {
      return Err(error(item(), "another slot has the same name"));
    }
    let Some(&bone) = bone_names.get(&slot.bone) else {
      return Err(error(
        item(),
        format!("its bone {:?} is not a bone", &*slot.bone),
      ));
    };
    slot_names.insert(slot.name.clone(), id);
    slots.push(SlotData {
      name: slot.name,
      bone,
      color: slot.setup.color,
      attachment: slot.setup.attachment,
      blend: slot.setup.blend,
    });
  }

  // Regions: unique, not empty
  let mut region_names: HashSet<&str> = HashSet::new();
  for region in &regions {
    let item = || format!("region {:?}", &*region.name);
    if !region_names.insert(&region.name) {
      return Err(error(item(), "another region has the same name"));
    }
    if region.width == 0 || region.height == 0 {
      return Err(error(
        item(),
        format!(
          "it is {} by {} pixels; it must not be empty",
          region.width, region.height
        ),
      ));
    }
  }

  // Events: unique, finite
  let mut event_names: HashSet<&str> = HashSet::new();
  for event in &events {
    let item = || format!("event {:?}", &*event.name);
    if !event_names.insert(&event.name) {
      return Err(error(item(), "another event has the same name"));
    }
    finite(item, &[("float", event.float)])?;
    if let Some(audio) = &event.audio {
      finite(
        item,
        &[("volume", audio.volume), ("balance", audio.balance)],
      )?;
    }
  }

  // Skins: unique, the default first whether declared or not
  let mut skins = vec![Skin::new(
    Arc::from(SkeletonData::DEFAULT_SKIN),
    slots.len(),
  )];
  let mut skin_names: HashMap<Arc<str>, usize> = HashMap::new();
  skin_names.insert(Arc::from(SkeletonData::DEFAULT_SKIN), 0);
  let mut default_declared = false;
  for name in declared_skins {
    let item = || format!("skin {:?}", &*name);
    if &*name == SkeletonData::DEFAULT_SKIN && !default_declared {
      default_declared = true;
      continue;
    }
    if skin_names.contains_key(&name) {
      return Err(error(item(), "another skin has the same name"));
    }
    skin_names.insert(name.clone(), skins.len());
    skins.push(Skin::new(name, slots.len()));
  }

  // Attachments: into a skin and slot, unique there, with good values
  for pending in attachments {
    let item = || {
      format!(
        "skin {:?} slot {:?} attachment {:?}",
        &*pending.skin, &*pending.slot, &*pending.name
      )
    };
    let Some(&skin) = skin_names.get(&pending.skin) else {
      return Err(error(item(), format!("{:?} is not a skin", &*pending.skin)));
    };
    let Some(&slot) = slot_names.get(&pending.slot) else {
      return Err(error(item(), format!("{:?} is not a slot", &*pending.slot)));
    };
    match &pending.attachment {
      Attachment::Region(region) => {
        if region.region >= regions.len() {
          return Err(error(
            item(),
            format!(
              "its region {} is out of range; there are {} regions",
              region.region,
              regions.len()
            ),
          ));
        }
        finite(
          item,
          &[
            ("x", region.x),
            ("y", region.y),
            ("rotation", region.rotation),
            ("scale_x", region.scale_x),
            ("scale_y", region.scale_y),
            ("width", region.width),
            ("height", region.height),
          ],
        )?;
      }
      Attachment::Point(point) => {
        finite(
          item,
          &[("x", point.x), ("y", point.y), ("rotation", point.rotation)],
        )?;
      }
      Attachment::BoundingBox(bounds) => {
        if bounds.vertices.len() < 3 {
          return Err(error(
            item(),
            format!(
              "it has {} vertices; a polygon needs at least 3",
              bounds.vertices.len()
            ),
          ));
        }
        for &vertex in &bounds.vertices {
          finite_vec(item, "a vertex", vertex)?;
        }
      }
    }
    let map = &mut skins[skin].slots[slot.index()];
    if map.contains_key(&pending.name) {
      return Err(error(
        item(),
        "the slot already has an attachment of that name in this skin",
      ));
    }
    map.insert(pending.name, pending.attachment);
  }

  // Animations: unique, finite, keys in time order, references resolve
  let mut animation_names: HashMap<Arc<str>, usize> = HashMap::new();
  for (index, animation) in animations.iter().enumerate() {
    let item = || format!("animation {:?}", &*animation.name);
    if animation_names.contains_key(&animation.name) {
      return Err(error(item(), "another animation has the same name"));
    }
    if &*animation.name == EMPTY_NAME {
      return Err(error(
        item(),
        "the name is reserved for the animation state's empty animation",
      ));
    }
    finite(item, &[("duration", animation.duration)])?;
    if animation.duration < 0.0 {
      return Err(error(item(), "its duration is negative"));
    }
    let mut keyed: HashSet<(Target, &'static str)> = HashSet::new();
    for (number, timeline) in animation.timelines.iter().enumerate() {
      let item = || {
        format!(
          "animation {:?} timeline {number} ({})",
          &*animation.name,
          timeline.kind()
        )
      };
      let counts = Counts {
        bones: &bones,
        slots: &slots,
        events: events.len(),
      };
      timeline_ok(item, timeline, animation.duration, counts, &mut keyed)?;
    }
    animation_names.insert(animation.name.clone(), index);
  }

  // Mixes: between animations, finite, each pair once
  let mut mixes: HashMap<(usize, usize), f32> = HashMap::new();
  for (from, to, duration) in pending_mixes {
    let item = || format!("mix {:?} to {:?}", &*from, &*to);
    let (Some(&from_index), Some(&to_index)) =
      (animation_names.get(&from), animation_names.get(&to))
    else {
      let missing = if animation_names.contains_key(&from) {
        &to
      } else {
        &from
      };
      return Err(error(
        item(),
        format!("{:?} is not an animation", &**missing),
      ));
    };
    mix_duration(item, duration)?;
    if mixes.insert((from_index, to_index), duration).is_some() {
      return Err(error(item(), "the pair has another mix"));
    }
  }
  mix_duration(|| "default mix".to_owned(), default_mix)?;

  let catalogue = Arc::new(AnimationCatalogue {
    names: animation_names,
    durations: animations
      .iter()
      .map(|animation| animation.duration)
      .collect(),
    mixes,
    default_mix,
  });
  Ok(SkeletonData {
    bones,
    slots,
    regions,
    skins,
    animations,
    events,
    bone_names,
    slot_names,
    skin_names,
    catalogue,
  })
}

fn mix_duration(item: impl Fn() -> String, duration: f32) -> Result<(), SkeletonError> {
  finite(&item, &[("duration", duration)])?;
  if duration < 0.0 {
    return Err(error(item(), "its duration is negative"));
  }
  Ok(())
}

/// The bones and slots a timeline may refer to, and how many events
#[derive(Clone, Copy)]
struct Counts<'a> {
  bones: &'a [BoneData],
  slots: &'a [SlotData],
  events: usize,
}

/// Checks one timeline: its bone, slot or events exist; no earlier timeline
/// of the animation keys the same property of the same target (`keyed`
/// collects them); it has keys, at finite times from 0 to the animation's
/// duration, in order (strictly, but event keys may share a time); and its
/// values and curves are good
fn timeline_ok(
  item: impl Fn() -> String,
  timeline: &Timeline,
  duration: f32,
  counts: Counts<'_>,
  keyed: &mut HashSet<(Target, &'static str)>,
) -> Result<(), SkeletonError> {
  let target = timeline.target();
  match target {
    Target::Bone(bone) if bone.index() >= counts.bones.len() => {
      return Err(error(
        item(),
        format!(
          "bone {} is out of range; there are {} bones",
          bone.0,
          counts.bones.len()
        ),
      ));
    }
    Target::Slot(slot) if slot.index() >= counts.slots.len() => {
      return Err(error(
        item(),
        format!(
          "slot {} is out of range; there are {} slots",
          slot.0,
          counts.slots.len()
        ),
      ));
    }
    _ => {}
  }
  for &property in timeline.properties() {
    if !keyed.insert((target, property)) {
      let whose = match target {
        Target::Bone(bone) => format!("bone {:?}'s", &*counts.bones[bone.index()].name),
        Target::Slot(slot) => format!("slot {:?}'s", &*counts.slots[slot.index()].name),
        Target::Skeleton => "the skeleton's".to_owned(),
      };
      return Err(error(
        item(),
        format!("another timeline already keys {whose} {property}"),
      ));
    }
  }

  let times = timeline.times();
  if times.is_empty() {
    return Err(error(item(), "it has no keys"));
  }
  for (frame, &time) in times.iter().enumerate() {
    finite(&item, &[(&format!("key {frame}'s time"), time)])?;
    if time < 0.0 {
      return Err(error(
        item(),
        format!("key {frame} is at {time}s, before 0"),
      ));
    }
  }
  let shared_times = matches!(timeline, Timeline::Event { .. });
  for (frame, pair) in times.windows(2).enumerate() {
    let ordered = if shared_times {
      pair[0] <= pair[1]
    } else {
      pair[0] < pair[1]
    };
    if !ordered {
      return Err(error(
        item(),
        format!(
          "key {} at {}s comes after key {frame} at {}s; keys must be in time order",
          frame + 1,
          pair[1],
          pair[0]
        ),
      ));
    }
  }
  for (frame, &time) in times.iter().enumerate() {
    if time > duration {
      return Err(error(
        item(),
        format!("key {frame} at {time}s is past the animation's duration of {duration}s"),
      ));
    }
  }

  match timeline {
    Timeline::Rotate { frames, .. }
    | Timeline::TranslateX { frames, .. }
    | Timeline::TranslateY { frames, .. }
    | Timeline::ScaleX { frames, .. }
    | Timeline::ScaleY { frames, .. }
    | Timeline::ShearX { frames, .. }
    | Timeline::ShearY { frames, .. } => {
      for (frame, (_, value, key_curve)) in frames.iter().enumerate() {
        finite(&item, &[(&format!("key {frame}'s value"), *value)])?;
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Translate { frames, .. }
    | Timeline::Scale { frames, .. }
    | Timeline::Shear { frames, .. } => {
      for (frame, (_, value, key_curve)) in frames.iter().enumerate() {
        finite_vec(&item, &format!("key {frame}'s value"), *value)?;
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Color { frames, .. } => {
      for (frame, (_, _, key_curve)) in frames.iter().enumerate() {
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Attachment { .. } => {}
    Timeline::DrawOrder { frames } => {
      for (frame, (_, order)) in frames.iter().enumerate() {
        let Some(order) = order else { continue };
        let mut seen = vec![false; counts.slots.len()];
        let complete = order.len() == counts.slots.len()
          && order.iter().all(|slot| {
            seen
              .get_mut(slot.index())
              .is_some_and(|seen| !std::mem::replace(seen, true))
          });
        if !complete {
          return Err(error(
            item(),
            format!(
              "key {frame} must list each of the {} slots once",
              counts.slots.len()
            ),
          ));
        }
      }
    }
    Timeline::Event { frames } => {
      for (frame, (_, event, values)) in frames.iter().enumerate() {
        if *event >= counts.events {
          return Err(error(
            item(),
            format!(
              "key {frame}'s event {event} is out of range; there are {} events",
              counts.events
            ),
          ));
        }
        let overrides = [
          ("float", values.float),
          ("volume", values.volume),
          ("balance", values.balance),
        ];
        for (name, value) in overrides {
          if let Some(value) = value {
            finite(&item, &[(&format!("key {frame}'s {name}"), value)])?;
          }
        }
      }
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::super::animation::{Animation, EventValues, Timeline};
  use super::super::attachment::{
    Attachment, BoundingBoxAttachment, PointAttachment, RegionAttachment,
  };
  use super::super::builder::{BoneSetup, SkeletonDataBuilder, SlotSetup};
  use super::super::curve::Curve;
  use super::super::data::{BoneId, EventData, Region, SkeletonData, SlotId};
  use crate::core::asset::Handle;
  use crate::core::ecs::Vec2;

  const WAVE_0: &str = r#"animation "wave" timeline 0 (rotate)"#;

  /// Builds `builder`, expecting it to fail on `item` for a reason that
  /// mentions `reason`
  fn fails(builder: SkeletonDataBuilder, item: &str, reason: &str) {
    let error = builder.build().expect_err(item);
    assert_eq!(error.item, item, "{error}");
    assert!(
      error.problem.contains(reason),
      "expected {reason:?} in: {error}"
    );
  }

  fn animation(name: &str, duration: f32, timelines: Vec<Timeline>) -> Animation {
    Animation {
      name: Arc::from(name),
      duration,
      timelines,
    }
  }

  fn rotate_keys(times: &[f32]) -> Timeline {
    Timeline::Rotate {
      bone: BoneId(1),
      frames: times.iter().map(|&t| (t, 0.5, Curve::Linear)).collect(),
    }
  }

  /// "wave": one rotate timeline on the arm, keyed at `times`, 1 s long
  fn rotate(times: &[f32]) -> Animation {
    animation("wave", 1.0, vec![rotate_keys(times)])
  }

  fn wave(timelines: Vec<Timeline>) -> Animation {
    animation("wave", 1.0, timelines)
  }

  fn arm() -> SkeletonDataBuilder {
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("arm", Some("root"), BoneSetup::default())
  }

  /// The arm with two slots, "hand" and "hat", and a "footstep" event
  fn rig() -> SkeletonDataBuilder {
    arm()
      .slot("hand", "arm", SlotSetup::default())
      .slot("hat", "root", SlotSetup::default())
      .event(EventData::new("footstep"))
  }

  fn region(name: &str, width: u32, height: u32) -> Region {
    Region {
      name: Arc::from(name),
      image: Handle::detached(),
      x: 0,
      y: 0,
      width,
      height,
    }
  }

  fn point(x: f32) -> Attachment {
    Attachment::Point(PointAttachment {
      x,
      ..PointAttachment::default()
    })
  }

  fn idle_and_run() -> SkeletonDataBuilder {
    arm()
      .animation(animation("idle", 1.0, Vec::new()))
      .animation(animation("run", 1.0, Vec::new()))
  }

  fn event_keys(times: &[f32]) -> Timeline {
    Timeline::Event {
      frames: times
        .iter()
        .map(|&t| (t, 0, EventValues::default()))
        .collect(),
    }
  }

  // Building and resolving

  #[test]
  fn a_built_skeleton_resolves_names() {
    let data = arm()
      .slot("hand", "arm", SlotSetup::default())
      .animation(animation("idle", 1.0, Vec::new()))
      .animation(animation("run", 1.0, Vec::new()))
      .mix("idle", "run", 0.3)
      .default_mix(0.1)
      .build()
      .expect("valid skeleton");

    assert_eq!(data.bone("arm"), Some(BoneId(1)));
    assert_eq!(data.bones()[1].parent, Some(BoneId(0)));
    assert_eq!(data.slot("hand"), Some(SlotId(0)));
    assert!(data.animation("run").is_some());
    assert_eq!(data.mix("idle", "run"), 0.3);
    assert_eq!(data.mix("run", "idle"), 0.1);
    assert_eq!(data.mix("idle", "missing"), 0.1);
    assert_eq!(&*data.default_skin().name, SkeletonData::DEFAULT_SKIN);
  }

  #[test]
  fn attachments_resolve_through_their_skin() {
    let data = arm()
      .slot("hand", "arm", SlotSetup::default())
      .skin(SkeletonData::DEFAULT_SKIN)
      .skin("red")
      .attachment("default", "hand", "muzzle", point(4.0))
      .build()
      .expect("valid skeleton");
    let hand = data.slot("hand").expect("slot");
    assert_eq!(
      data.default_skin().attachment(hand, "muzzle"),
      Some(&point(4.0))
    );
    assert_eq!(data.skins().len(), 2);
    assert_eq!(
      data
        .skin("red")
        .and_then(|skin| skin.attachment(hand, "muzzle")),
      None
    );
  }

  #[test]
  fn per_axis_and_combined_timelines_on_different_properties_build() {
    let timelines = vec![
      Timeline::TranslateX {
        bone: BoneId(1),
        frames: vec![(0.0, 2.0, Curve::Linear)],
      },
      Timeline::TranslateY {
        bone: BoneId(1),
        frames: vec![(0.0, 3.0, Curve::Stepped)],
      },
      Timeline::Scale {
        bone: BoneId(1),
        frames: vec![(0.0, Vec2::ONE, Curve::Linear)],
      },
      Timeline::ShearX {
        bone: BoneId(1),
        frames: vec![(0.0, 0.1, Curve::Linear)],
      },
      Timeline::Rotate {
        bone: BoneId(0),
        frames: vec![(0.0, 0.1, Curve::Linear)],
      },
      rotate_keys(&[0.0]),
    ];
    assert!(arm().animation(wave(timelines)).build().is_ok());
  }

  // Unique names

  #[test]
  fn a_duplicate_bone_name_fails() {
    fails(
      arm().bone("arm", Some("root"), BoneSetup::default()),
      r#"bone "arm""#,
      "same name",
    );
  }

  #[test]
  fn a_duplicate_slot_name_fails() {
    fails(
      rig().slot("hand", "root", SlotSetup::default()),
      r#"slot "hand""#,
      "same name",
    );
  }

  #[test]
  fn a_duplicate_region_name_fails() {
    fails(
      arm()
        .region(region("torso", 8, 8))
        .region(region("torso", 4, 4)),
      r#"region "torso""#,
      "same name",
    );
  }

  #[test]
  fn a_duplicate_event_name_fails() {
    fails(
      rig().event(EventData::new("footstep")),
      r#"event "footstep""#,
      "same name",
    );
  }

  #[test]
  fn a_duplicate_skin_name_fails() {
    fails(arm().skin("red").skin("red"), r#"skin "red""#, "same name");
    fails(
      arm().skin("default").skin("default"),
      r#"skin "default""#,
      "same name",
    );
  }

  #[test]
  fn a_duplicate_animation_name_fails() {
    fails(
      idle_and_run().animation(animation("run", 2.0, Vec::new())),
      r#"animation "run""#,
      "same name",
    );
  }

  #[test]
  fn an_animation_named_as_the_empty_animation_fails() {
    fails(
      idle_and_run().animation(animation("<empty>", 1.0, Vec::new())),
      r#"animation "<empty>""#,
      "reserved",
    );
  }

  #[test]
  fn a_duplicate_attachment_name_in_a_slot_fails() {
    fails(
      rig()
        .attachment("default", "hand", "muzzle", point(1.0))
        .attachment("default", "hand", "muzzle", point(2.0)),
      r#"skin "default" slot "hand" attachment "muzzle""#,
      "already has an attachment of that name",
    );
  }

  #[test]
  fn a_duplicated_mix_pair_fails() {
    fails(
      idle_and_run()
        .mix("idle", "run", 0.2)
        .mix("idle", "run", 0.4),
      r#"mix "idle" to "run""#,
      "another mix",
    );
  }

  // Order and references

  #[test]
  fn a_child_before_its_parent_fails() {
    fails(
      SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .bone("hand", Some("arm"), BoneSetup::default())
        .bone("arm", Some("root"), BoneSetup::default()),
      r#"bone "hand""#,
      "parents must come first",
    );
  }

  #[test]
  fn an_unknown_parent_fails() {
    fails(
      arm().bone("leg", Some("hip"), BoneSetup::default()),
      r#"bone "leg""#,
      r#"parent "hip" is not a bone"#,
    );
  }

  #[test]
  fn an_unknown_slot_bone_fails() {
    fails(
      arm().slot("hat", "head", SlotSetup::default()),
      r#"slot "hat""#,
      r#"bone "head" is not a bone"#,
    );
  }

  #[test]
  fn an_attachment_in_an_unknown_skin_fails() {
    fails(
      rig().attachment("blue", "hand", "muzzle", point(0.0)),
      r#"skin "blue" slot "hand" attachment "muzzle""#,
      r#""blue" is not a skin"#,
    );
  }

  #[test]
  fn an_attachment_in_an_unknown_slot_fails() {
    fails(
      rig().attachment("default", "foot", "muzzle", point(0.0)),
      r#"skin "default" slot "foot" attachment "muzzle""#,
      r#""foot" is not a slot"#,
    );
  }

  #[test]
  fn a_region_attachment_outside_the_regions_fails() {
    fails(
      rig().region(region("torso", 8, 8)).skin("red").attachment(
        "red",
        "hat",
        "cap",
        Attachment::Region(RegionAttachment::new(1, 8.0, 8.0)),
      ),
      r#"skin "red" slot "hat" attachment "cap""#,
      "region 1 is out of range",
    );
  }

  #[test]
  fn a_timeline_on_an_unknown_bone_fails() {
    let timeline = Timeline::Rotate {
      bone: BoneId(9),
      frames: vec![(0.0, 1.0, Curve::Linear)],
    };
    fails(
      arm().animation(wave(vec![timeline])),
      WAVE_0,
      "bone 9 is out of range",
    );
  }

  #[test]
  fn a_timeline_on_an_unknown_slot_fails() {
    let timeline = Timeline::Attachment {
      slot: SlotId(2),
      frames: vec![(0.0, None)],
    };
    fails(
      rig().animation(wave(vec![timeline])),
      r#"animation "wave" timeline 0 (attachment)"#,
      "slot 2 is out of range",
    );
  }

  #[test]
  fn an_event_key_for_an_unknown_event_fails() {
    let timeline = Timeline::Event {
      frames: vec![(0.0, 1, EventValues::default())],
    };
    fails(
      rig().animation(wave(vec![timeline])),
      r#"animation "wave" timeline 0 (event)"#,
      "event 1 is out of range",
    );
  }

  #[test]
  fn a_mix_naming_an_unknown_animation_fails() {
    fails(
      idle_and_run().mix("idle", "walk", 0.2),
      r#"mix "idle" to "walk""#,
      r#""walk" is not an animation"#,
    );
  }

  // Limits

  #[test]
  fn two_hundred_fifty_six_bones_fail() {
    let mut builder = SkeletonData::builder().bone("b0", None, BoneSetup::default());
    for index in 1..255 {
      builder = builder.bone(format!("b{index}"), Some("b0"), BoneSetup::default());
    }
    assert!(builder.clone().build().is_ok(), "255 bones are allowed");
    assert_eq!(builder.bone_id("b254"), Some(BoneId(254)));

    let builder = builder.bone("b255", Some("b0"), BoneSetup::default());
    assert_eq!(builder.bone_id("b255"), None);
    fails(builder, r#"bone "b255""#, "at most 255 bones");
  }

  #[test]
  fn two_hundred_fifty_six_slots_fail() {
    let mut builder = arm();
    for index in 0..255 {
      builder = builder.slot(format!("s{index}"), "arm", SlotSetup::default());
    }
    assert!(builder.clone().build().is_ok(), "255 slots are allowed");
    assert_eq!(builder.slot_id("s254"), Some(SlotId(254)));

    let builder = builder.slot("s255", "arm", SlotSetup::default());
    assert_eq!(builder.slot_id("s255"), None);
    fails(builder, r#"slot "s255""#, "at most 255 slots");
  }

  #[test]
  fn an_empty_region_fails() {
    fails(
      arm().region(region("torso", 0, 8)),
      r#"region "torso""#,
      "must not be empty",
    );
  }

  #[test]
  fn a_bounding_box_with_two_vertices_fails() {
    let bounds = Attachment::BoundingBox(BoundingBoxAttachment {
      vertices: vec![Vec2::ZERO, Vec2::X],
    });
    fails(
      rig().attachment("default", "hand", "hitbox", bounds),
      r#"skin "default" slot "hand" attachment "hitbox""#,
      "at least 3",
    );
  }

  // Keys

  #[test]
  fn a_timeline_with_no_keys_fails() {
    fails(arm().animation(rotate(&[])), WAVE_0, "no keys");
  }

  #[test]
  fn frames_out_of_order_fail() {
    fails(
      arm().animation(rotate(&[0.0, 0.5, 0.25])),
      WAVE_0,
      "time order",
    );
  }

  #[test]
  fn a_per_axis_frame_out_of_order_fails() {
    let timeline = Timeline::ScaleY {
      bone: BoneId(1),
      frames: vec![(0.5, 1.0, Curve::Linear), (0.25, 2.0, Curve::Linear)],
    };
    fails(
      arm().animation(wave(vec![timeline])),
      r#"animation "wave" timeline 0 (scale y)"#,
      "time order",
    );
  }

  #[test]
  fn event_keys_may_share_a_time_but_not_run_backwards() {
    assert!(
      rig()
        .animation(wave(vec![event_keys(&[0.5, 0.5])]))
        .build()
        .is_ok()
    );
    fails(
      rig().animation(wave(vec![event_keys(&[0.5, 0.25])])),
      r#"animation "wave" timeline 0 (event)"#,
      "time order",
    );
  }

  #[test]
  fn a_key_before_zero_fails() {
    fails(arm().animation(rotate(&[-0.5, 0.5])), WAVE_0, "before 0");
  }

  #[test]
  fn a_key_past_the_duration_fails() {
    fails(
      arm().animation(rotate(&[0.0, 1.5])),
      WAVE_0,
      "past the animation's duration",
    );
  }

  #[test]
  fn a_bezier_whose_time_runs_backwards_fails() {
    let mut animation = rotate(&[0.0, 1.0]);
    if let Timeline::Rotate { frames, .. } = &mut animation.timelines[0] {
      frames[0].2 = Curve::Bezier {
        cx1: 0.25,
        cy1: 0.0,
        cx2: 1.5,
        cy2: 1.0,
      };
    }
    fails(arm().animation(animation), WAVE_0, "within 0 to 1");
  }

  #[test]
  fn a_draw_order_key_missing_a_slot_fails() {
    let timeline = Timeline::DrawOrder {
      frames: vec![
        (0.0, None),
        (0.5, Some(vec![SlotId(1), SlotId(0)])),
        (1.0, Some(vec![SlotId(1), SlotId(1)])),
      ],
    };
    fails(
      rig().animation(wave(vec![timeline])),
      r#"animation "wave" timeline 0 (draw order)"#,
      "key 2 must list each of the 2 slots once",
    );
  }

  // One timeline per property

  #[test]
  fn two_timelines_of_one_kind_on_one_bone_or_slot_fail() {
    fails(
      arm().animation(wave(vec![rotate_keys(&[0.0]), rotate_keys(&[0.5])])),
      r#"animation "wave" timeline 1 (rotate)"#,
      "another timeline already keys bone \"arm\"'s rotate",
    );
    let color = || Timeline::Color {
      slot: SlotId(1),
      frames: vec![(0.0, crate::core::ui::Color::WHITE, Curve::Linear)],
    };
    fails(
      rig().animation(wave(vec![color(), color()])),
      r#"animation "wave" timeline 1 (color)"#,
      "another timeline already keys slot \"hat\"'s color",
    );
  }

  #[test]
  fn a_combined_and_a_per_axis_timeline_on_one_bone_fail() {
    let timelines = vec![
      Timeline::TranslateY {
        bone: BoneId(1),
        frames: vec![(0.0, 1.0, Curve::Linear)],
      },
      Timeline::Translate {
        bone: BoneId(1),
        frames: vec![(0.0, Vec2::ONE, Curve::Linear)],
      },
    ];
    fails(
      arm().animation(wave(timelines)),
      r#"animation "wave" timeline 1 (translate)"#,
      "another timeline already keys bone \"arm\"'s translate y",
    );
  }

  #[test]
  fn two_draw_order_or_event_timelines_fail() {
    let order = || Timeline::DrawOrder {
      frames: vec![(0.0, None)],
    };
    fails(
      rig().animation(wave(vec![order(), order()])),
      r#"animation "wave" timeline 1 (draw order)"#,
      "another timeline already keys the skeleton's draw order",
    );
    fails(
      rig().animation(wave(vec![event_keys(&[0.0]), event_keys(&[0.5])])),
      r#"animation "wave" timeline 1 (event)"#,
      "another timeline already keys the skeleton's event",
    );
  }

  // Durations and mixes

  #[test]
  fn a_negative_duration_fails() {
    fails(
      arm().animation(animation("wave", -1.0, Vec::new())),
      r#"animation "wave""#,
      "duration is negative",
    );
  }

  #[test]
  fn a_negative_mix_fails() {
    fails(
      idle_and_run().mix("idle", "run", -0.2),
      r#"mix "idle" to "run""#,
      "duration is negative",
    );
    fails(
      arm().default_mix(-0.2),
      "default mix",
      "duration is negative",
    );
  }

  // Non-finite values

  #[test]
  fn a_non_finite_bone_value_fails() {
    fails(
      arm().bone(
        "leg",
        Some("root"),
        BoneSetup {
          rotation: f32::NAN,
          ..BoneSetup::default()
        },
      ),
      r#"bone "leg""#,
      "rotation is NaN, not a finite number",
    );
  }

  #[test]
  fn a_non_finite_attachment_field_fails() {
    fails(
      rig().attachment("default", "hand", "muzzle", point(f32::INFINITY)),
      r#"skin "default" slot "hand" attachment "muzzle""#,
      "x is inf, not a finite number",
    );
  }

  #[test]
  fn a_non_finite_key_value_fails() {
    let timeline = Timeline::Rotate {
      bone: BoneId(1),
      frames: vec![(0.0, 0.0, Curve::Linear), (0.5, f32::NAN, Curve::Linear)],
    };
    fails(
      arm().animation(wave(vec![timeline])),
      WAVE_0,
      "key 1's value is NaN",
    );
  }

  #[test]
  fn a_non_finite_key_time_fails() {
    fails(
      arm().animation(rotate(&[0.0, f32::NAN])),
      WAVE_0,
      "key 1's time is NaN",
    );
  }

  #[test]
  fn a_non_finite_curve_control_fails() {
    let mut animation = rotate(&[0.0, 1.0]);
    if let Timeline::Rotate { frames, .. } = &mut animation.timelines[0] {
      frames[0].2 = Curve::Bezier {
        cx1: 0.25,
        cy1: f32::NAN,
        cx2: 0.75,
        cy2: 1.0,
      };
    }
    fails(arm().animation(animation), WAVE_0, "key 0's curve is NaN");
  }

  #[test]
  fn a_non_finite_event_override_fails() {
    let timeline = Timeline::Event {
      frames: vec![(
        0.0,
        0,
        EventValues {
          volume: Some(f32::NAN),
          ..EventValues::default()
        },
      )],
    };
    fails(
      rig().animation(wave(vec![timeline])),
      r#"animation "wave" timeline 0 (event)"#,
      "key 0's volume is NaN",
    );
  }

  #[test]
  fn a_non_finite_event_default_fails() {
    let mut event = EventData::new("splash");
    event.float = f32::NEG_INFINITY;
    fails(rig().event(event), r#"event "splash""#, "float is -inf");
  }

  #[test]
  fn a_non_finite_duration_fails() {
    fails(
      arm().animation(animation("wave", f32::INFINITY, Vec::new())),
      r#"animation "wave""#,
      "duration is inf",
    );
  }

  #[test]
  fn a_non_finite_mix_fails() {
    fails(
      idle_and_run().mix("idle", "run", f32::NAN),
      r#"mix "idle" to "run""#,
      "duration is NaN",
    );
  }
}
