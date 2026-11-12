//! Checking a skeleton as it is built.

use std::fmt;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::animation::Timeline;
use super::attachment::Attachment;
use super::builder::SkeletonDataBuilder;
use super::curve::Curve;
use super::data::{BoneData, BoneId, SkeletonData, SlotData, SlotId};
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
const MAX_BONES: usize = 255;
const MAX_SLOTS: usize = 255;

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
    let name = format!("frame {frame} curve");
    finite(
      &item,
      &[(&name, cx1), (&name, cy1), (&name, cx2), (&name, cy2)],
    )?;
    if !(0.0..=1.0).contains(&cx1) || !(0.0..=1.0).contains(&cx2) {
      return Err(error(
        item(),
        format!("frame {frame} curve has x controls {cx1} and {cx2}; both must be within 0 to 1"),
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
    finite(item, &[("duration", animation.duration)])?;
    if animation.duration < 0.0 {
      return Err(error(item(), "its duration is negative"));
    }
    for (number, timeline) in animation.timelines.iter().enumerate() {
      let item = || {
        format!(
          "animation {:?} timeline {number} ({})",
          &*animation.name,
          timeline.kind()
        )
      };
      timeline_ok(item, timeline, bones.len(), slots.len(), events.len())?;
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
    animation_names,
    mixes,
    default_mix,
  })
}

fn mix_duration(item: impl Fn() -> String, duration: f32) -> Result<(), SkeletonError> {
  finite(&item, &[("duration", duration)])?;
  if duration < 0.0 {
    return Err(error(item(), "its duration is negative"));
  }
  Ok(())
}

/// Checks one timeline: it has keys, at finite times from 0 in order
/// (strictly, but event keys may share a time), with finite values, good
/// curves, and bones, slots and events that exist
fn timeline_ok(
  item: impl Fn() -> String,
  timeline: &Timeline,
  bone_count: usize,
  slot_count: usize,
  event_count: usize,
) -> Result<(), SkeletonError> {
  let times = timeline.times();
  if times.is_empty() {
    return Err(error(item(), "it has no keys"));
  }
  let shared_times = matches!(timeline, Timeline::Event { .. });
  for (frame, pair) in times.windows(2).enumerate() {
    let ordered = if shared_times {
      pair[0] <= pair[1]
    } else {
      pair[0] < pair[1]
    };
    if pair[1].is_finite() && pair[0].is_finite() && !ordered {
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
    finite(&item, &[(&format!("key {frame}'s time"), time)])?;
    if time < 0.0 {
      return Err(error(
        item(),
        format!("key {frame} is at {time}s, before 0"),
      ));
    }
  }

  let bone_ok = |bone: BoneId| {
    if bone.index() < bone_count {
      Ok(())
    } else {
      Err(error(
        item(),
        format!(
          "bone {} is out of range; there are {bone_count} bones",
          bone.0
        ),
      ))
    }
  };
  let slot_ok = |slot: SlotId| {
    if slot.index() < slot_count {
      Ok(())
    } else {
      Err(error(
        item(),
        format!(
          "slot {} is out of range; there are {slot_count} slots",
          slot.0
        ),
      ))
    }
  };

  match timeline {
    Timeline::Rotate { bone, frames } => {
      bone_ok(*bone)?;
      for (frame, (_, value, key_curve)) in frames.iter().enumerate() {
        finite(&item, &[(&format!("key {frame}'s value"), *value)])?;
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Translate { bone, frames }
    | Timeline::Scale { bone, frames }
    | Timeline::Shear { bone, frames } => {
      bone_ok(*bone)?;
      for (frame, (_, value, key_curve)) in frames.iter().enumerate() {
        finite_vec(&item, &format!("key {frame}'s value"), *value)?;
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Color { slot, frames } => {
      slot_ok(*slot)?;
      for (frame, (_, _, key_curve)) in frames.iter().enumerate() {
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Attachment { slot, .. } => slot_ok(*slot)?,
    Timeline::DrawOrder { frames } => {
      for (frame, (_, order)) in frames.iter().enumerate() {
        let Some(order) = order else { continue };
        let mut seen = vec![false; slot_count];
        let complete = order.len() == slot_count
          && order.iter().all(|slot| {
            seen
              .get_mut(slot.index())
              .is_some_and(|seen| !std::mem::replace(seen, true))
          });
        if !complete {
          return Err(error(
            item(),
            format!("key {frame} must list each of the {slot_count} slots once"),
          ));
        }
      }
    }
    Timeline::Event { frames } => {
      for (frame, (_, event, values)) in frames.iter().enumerate() {
        if *event >= event_count {
          return Err(error(
            item(),
            format!("key {frame}'s event {event} is out of range; there are {event_count} events"),
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

  use super::super::animation::{Animation, Timeline};
  use super::super::builder::{BoneSetup, SlotSetup};
  use super::super::curve::Curve;
  use super::super::data::{BoneId, SkeletonData};

  fn rotate(times: &[f32]) -> Animation {
    Animation {
      name: Arc::from("wave"),
      duration: 1.0,
      timelines: vec![Timeline::Rotate {
        bone: BoneId(1),
        frames: times.iter().map(|&t| (t, 0.5, Curve::Linear)).collect(),
      }],
    }
  }

  fn arm() -> super::SkeletonDataBuilder {
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("arm", Some("root"), BoneSetup::default())
  }

  #[test]
  fn a_built_skeleton_resolves_names() {
    let data = arm()
      .slot("hand", "arm", SlotSetup::default())
      .animation(Animation {
        name: Arc::from("idle"),
        duration: 1.0,
        timelines: Vec::new(),
      })
      .animation(Animation {
        name: Arc::from("run"),
        duration: 1.0,
        timelines: Vec::new(),
      })
      .mix("idle", "run", 0.3)
      .default_mix(0.1)
      .build()
      .expect("valid skeleton");

    assert_eq!(data.bone("arm"), Some(BoneId(1)));
    assert_eq!(data.bones()[1].parent, Some(BoneId(0)));
    assert_eq!(data.slot("hand").map(|slot| slot.0), Some(0));
    assert!(data.animation("run").is_some());
    assert_eq!(data.mix("idle", "run"), 0.3);
    assert_eq!(data.mix("run", "idle"), 0.1);
    assert_eq!(data.mix("idle", "missing"), 0.1);
    assert_eq!(&*data.default_skin().name, SkeletonData::DEFAULT_SKIN);
  }

  #[test]
  fn a_duplicate_bone_name_fails() {
    let error = arm()
      .bone("arm", Some("root"), BoneSetup::default())
      .build()
      .expect_err("duplicate bone");
    assert_eq!(error.item, r#"bone "arm""#);
  }

  #[test]
  fn a_child_before_its_parent_fails() {
    let error = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("hand", Some("arm"), BoneSetup::default())
      .bone("arm", Some("root"), BoneSetup::default())
      .build()
      .expect_err("child before parent");
    assert_eq!(error.item, r#"bone "hand""#);
  }

  #[test]
  fn an_unknown_slot_bone_fails() {
    let error = arm()
      .slot("hat", "head", SlotSetup::default())
      .build()
      .expect_err("unknown bone");
    assert_eq!(error.item, r#"slot "hat""#);
    assert!(error.problem.contains("head"), "{error}");
  }

  #[test]
  fn frames_out_of_order_fail() {
    let error = arm()
      .animation(rotate(&[0.0, 0.5, 0.25]))
      .build()
      .expect_err("frames out of order");
    assert!(error.item.starts_with(r#"animation "wave""#), "{error}");
  }

  #[test]
  fn a_non_finite_value_fails() {
    let error = arm()
      .bone(
        "leg",
        Some("root"),
        BoneSetup {
          rotation: f32::NAN,
          ..BoneSetup::default()
        },
      )
      .build()
      .expect_err("non-finite rotation");
    assert_eq!(error.item, r#"bone "leg""#);
    assert!(error.problem.contains("rotation"), "{error}");
  }

  #[test]
  fn two_hundred_fifty_six_bones_fail() {
    let mut builder = SkeletonData::builder().bone("b0", None, BoneSetup::default());
    for index in 1..255 {
      builder = builder.bone(format!("b{index}"), Some("b0"), BoneSetup::default());
    }
    assert!(builder.clone().build().is_ok(), "255 bones are allowed");

    let error = builder
      .bone("b255", Some("b0"), BoneSetup::default())
      .build()
      .expect_err("256 bones");
    assert_eq!(error.item, r#"bone "b255""#);
  }

  #[test]
  fn an_attachment_error_names_its_skin_slot_and_name() {
    use super::super::attachment::{Attachment, RegionAttachment};

    let builder = arm()
      .slot("head", "arm", SlotSetup::default())
      .skin("red")
      .attachment(
        "red",
        "head",
        "hat",
        Attachment::Region(RegionAttachment::new(3, 8.0, 8.0)),
      );
    let error = builder.build().expect_err("no regions");
    assert_eq!(error.item, r#"skin "red" slot "head" attachment "hat""#);
    assert!(error.problem.contains("region 3"), "{error}");
  }

  #[test]
  fn attachments_resolve_through_their_skin() {
    use super::super::attachment::{Attachment, PointAttachment};

    let point = Attachment::Point(PointAttachment {
      x: 4.0,
      ..PointAttachment::default()
    });
    let data = arm()
      .slot("hand", "arm", SlotSetup::default())
      .skin(SkeletonData::DEFAULT_SKIN)
      .skin("red")
      .attachment("default", "hand", "muzzle", point.clone())
      .build()
      .expect("valid skeleton");
    let hand = data.slot("hand").expect("slot");
    assert_eq!(data.default_skin().attachment(hand, "muzzle"), Some(&point));
    assert_eq!(data.skins().len(), 2);
    assert_eq!(
      data
        .skin("red")
        .and_then(|skin| skin.attachment(hand, "muzzle")),
      None
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
    let error = arm()
      .animation(animation)
      .build()
      .expect_err("bezier x past 1");
    assert_eq!(error.item, r#"animation "wave" timeline 0 (rotate)"#);
  }

  #[test]
  fn event_keys_may_share_a_time_but_not_run_backwards() {
    use super::super::animation::EventValues;
    use super::super::data::EventData;

    let events = |times: &[f32]| Animation {
      name: Arc::from("step"),
      duration: 1.0,
      timelines: vec![Timeline::Event {
        frames: times
          .iter()
          .map(|&t| (t, 0, EventValues::default()))
          .collect(),
      }],
    };
    let builder = arm().event(EventData::new("footstep"));
    assert!(
      builder
        .clone()
        .animation(events(&[0.5, 0.5]))
        .build()
        .is_ok()
    );
    assert!(builder.animation(events(&[0.5, 0.25])).build().is_err());
  }
}
