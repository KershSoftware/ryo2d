//! Checking a skeleton as it is built.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use super::animation::{EMPTY_NAME, Target, Timeline};
use super::attachment::{Attachment, MeshAttachment, MeshVertices};
use super::builder::{PendingConstraint, PendingKind, SkeletonDataBuilder};
use super::constrain::ConstraintBones;
use super::constraint::{
  Constraint, ConstraintId, IkConstraint, IkSetup, MAX_CONSTRAINTS, MAX_IK_BONES,
  TransformConstraint,
};
use super::curve::Curve;
use super::data::{
  AnimationCatalogue, BoneData, BoneId, Inherit, SkeletonData, SkeletonIndex, SlotData, SlotId,
};
use super::ik::IkValues;
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
    constraints: pending_constraints,
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
      Attachment::Mesh(mesh) => {
        if mesh.region >= regions.len() {
          return Err(error(
            item(),
            format!(
              "its region {} is out of range; there are {} regions",
              mesh.region,
              regions.len()
            ),
          ));
        }
        mesh_ok(item, mesh, bones.len())?;
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

  // Constraints: unique, on bones that exist, with settings that can apply
  let mut constraints = Vec::with_capacity(pending_constraints.len());
  let mut constraint_names: HashMap<Arc<str>, ConstraintId> = HashMap::new();
  for (index, pending) in pending_constraints.iter().enumerate() {
    let item = || format!("constraint {:?}", &*pending.name);
    let id = match u8::try_from(index) {
      Ok(index) if usize::from(index) < MAX_CONSTRAINTS => ConstraintId(index),
      _ => {
        return Err(error(
          item(),
          format!("a skeleton has at most {MAX_CONSTRAINTS} constraints"),
        ));
      }
    };
    if constraint_names.contains_key(&pending.name) {
      return Err(error(item(), "another constraint has the same name"));
    }
    let constraint = constraint_ok(item, pending, &bones, &bone_names)?;
    constraint_names.insert(pending.name.clone(), id);
    constraints.push(constraint);
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
    let mut keyed: HashSet<(Target<'_>, &'static str)> = HashSet::new();
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
        skins: &skins,
        constraints: &constraints,
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
  let index = Arc::new(SkeletonIndex {
    bone_names,
    slot_names,
    constraint_names,
    skin_names,
    skins,
    slot_bones: slots.iter().map(|slot| slot.bone).collect(),
    constraint_bones: ConstraintBones::of(&bones, &constraints),
  });
  Ok(SkeletonData {
    bones,
    slots,
    regions,
    constraints,
    animations,
    events,
    index,
    catalogue,
  })
}

/// Checks a constraint's bones, target and settings, and resolves them
fn constraint_ok(
  item: impl Fn() -> String,
  pending: &PendingConstraint,
  bones: &[BoneData],
  bone_names: &HashMap<Arc<str>, BoneId>,
) -> Result<Constraint, SkeletonError> {
  let mut ids = Vec::with_capacity(pending.bones.len());
  for name in &pending.bones {
    let Some(&id) = bone_names.get(name) else {
      return Err(error(
        item(),
        format!("its bone {:?} is not a bone", &**name),
      ));
    };
    if ids.contains(&id) {
      return Err(error(item(), format!("it lists bone {:?} twice", &**name)));
    }
    ids.push(id);
  }
  let Some(&target) = bone_names.get(&pending.target) else {
    return Err(error(
      item(),
      format!("its target {:?} is not a bone", &*pending.target),
    ));
  };
  let name_of = |bone: BoneId| bones.get(bone.index()).map_or("", |bone| &*bone.name);
  if ids.contains(&target) {
    return Err(error(
      item(),
      format!("its target {:?} is one of its bones", name_of(target)),
    ));
  }
  for &bone in &ids {
    if descends_from(bones, target, bone) {
      return Err(error(
        item(),
        format!(
          "its target {:?} is below its bone {:?}; a bone cannot follow what it carries",
          name_of(target),
          name_of(bone)
        ),
      ));
    }
  }
  match &pending.kind {
    PendingKind::Ik(setup) => ik_ok(&item, &ids, setup, bones)?,
    PendingKind::Transform(setup) => {
      if ids.is_empty() {
        return Err(error(item(), "it has no bones; it needs at least one"));
      }
      let mix = &setup.mix;
      finite(
        &item,
        &[
          ("mix rotate", mix.rotate),
          ("mix x", mix.x),
          ("mix y", mix.y),
          ("mix scale_x", mix.scale_x),
          ("mix scale_y", mix.scale_y),
          ("mix shear_y", mix.shear_y),
        ],
      )?;
      let offset = &setup.offset;
      finite(
        &item,
        &[
          ("offset rotation", offset.rotation),
          ("offset x", offset.x),
          ("offset y", offset.y),
          ("offset scale_x", offset.scale_x),
          ("offset scale_y", offset.scale_y),
          ("offset shear_y", offset.shear_y),
        ],
      )?;
    }
  }
  let name = pending.name.clone();
  Ok(match pending.kind {
    PendingKind::Ik(setup) => Constraint::Ik(IkConstraint {
      name,
      bones: ids,
      target,
      setup,
    }),
    PendingKind::Transform(setup) => Constraint::Transform(TransformConstraint {
      name,
      bones: ids,
      target,
      setup,
    }),
  })
}

/// Whether `bone` is below `ancestor`: walking its parents reaches it
fn descends_from(bones: &[BoneData], bone: BoneId, ancestor: BoneId) -> bool {
  let mut at = bones.get(bone.index()).and_then(|bone| bone.parent);
  while let Some(parent) = at {
    if parent == ancestor {
      return true;
    }
    at = bones.get(parent.index()).and_then(|bone| bone.parent);
  }
  false
}

/// Checks an IK constraint's chain and settings
fn ik_ok(
  item: &impl Fn() -> String,
  ids: &[BoneId],
  setup: &IkSetup,
  bones: &[BoneData],
) -> Result<(), SkeletonError> {
  let count = ids.len();
  if count == 0 {
    return Err(error(item(), "it has no bones; it needs at least one"));
  }
  if count > MAX_IK_BONES {
    return Err(error(
      item(),
      format!("it has {count} bones; at most {MAX_IK_BONES}"),
    ));
  }
  let bone = |id: BoneId| bones.get(id.index());
  for pair in ids.windows(2) {
    if let (Some(before), Some(after)) = (bone(pair[0]), bone(pair[1]))
      && after.parent != Some(pair[0])
    {
      return Err(error(
        item(),
        format!(
          "bone {:?} is not a child of {:?}; each bone after the first must be an immediate child of the one before",
          &*after.name, &*before.name
        ),
      ));
    }
  }
  if let Some(bone) = ids
    .iter()
    .filter_map(|&id| bone(id))
    .find(|bone| bone.inherit != Inherit::Normal)
  {
    return Err(error(
      item(),
      format!(
        "bone {:?} does not inherit normally; an IK constraint turns only bones that do",
        &*bone.name
      ),
    ));
  }
  ik_values_ok(item, "", ids, bones, &IkValues::of(setup), setup.uniform)
}

/// Checks an IK constraint's keyed values, a setup's or a key's (`whose`
/// begins each reason: empty for a setup, `key 2's ` for a key): `mix` 0 to
/// 1 and `softness` finite and not negative, and no setting its bones could
/// not have. `uniform` is the setup's alone, and is checked only for one
fn ik_values_ok(
  item: &impl Fn() -> String,
  whose: &str,
  ids: &[BoneId],
  bones: &[BoneData],
  values: &IkValues,
  uniform: bool,
) -> Result<(), SkeletonError> {
  let count = ids.len();
  let bone = |id: BoneId| bones.get(id.index());
  finite(
    item,
    &[
      (&format!("{whose}mix"), values.mix),
      (&format!("{whose}softness"), values.softness),
    ],
  )?;
  if !(0.0..=1.0).contains(&values.mix) {
    return Err(error(
      item(),
      format!("{whose}mix is {}; it must be within 0 to 1", values.mix),
    ));
  }
  if values.softness < 0.0 {
    return Err(error(
      item(),
      format!(
        "{whose}softness is {}; it must not be negative",
        values.softness
      ),
    ));
  }
  if count != 2 && !values.bend_positive {
    return Err(error(
      item(),
      format!("{whose}bend_positive false applies only to two bones"),
    ));
  }
  if count != 2 && values.softness > 0.0 {
    return Err(error(
      item(),
      format!("{whose}softness applies only to two bones"),
    ));
  }
  if count != 1 && values.compress {
    return Err(error(
      item(),
      format!("{whose}compress applies only to one bone"),
    ));
  }
  if count > 2 && values.stretch {
    return Err(error(
      item(),
      format!("{whose}stretch applies only to one or two bones"),
    ));
  }
  if count > 2 && uniform {
    return Err(error(item(), "uniform applies only to one or two bones"));
  }
  if let Some(&last) = ids
    .last()
    .filter(|_| count > 1 || values.stretch || values.compress)
    && let Some(tip) = bone(last)
    && tip.length <= 0.0
  {
    let reason = if count > 1 {
      "its tip is what reaches the target, so the length must be above 0"
    } else {
      "stretch and compress need a length above 0"
    };
    let switched = if count == 1 && !whose.is_empty() {
      format!("{whose}stretch or compress is on, and ")
    } else {
      String::new()
    };
    return Err(error(
      item(),
      format!(
        "{switched}{} {:?} has a length of {}; {reason}",
        if count > 1 {
          "its last bone"
        } else {
          "its bone"
        },
        &*tip.name,
        tip.length
      ),
    ));
  }
  Ok(())
}

/// The most vertices a mesh has: its triangle indices are `u16`
const MAX_MESH_VERTICES: usize = 65_535;

/// Checks a mesh against `bone_count` bones: its counts, indices, hull and
/// values, and its weights
fn mesh_ok(
  item: impl Fn() -> String,
  mesh: &MeshAttachment,
  bone_count: usize,
) -> Result<(), SkeletonError> {
  let vertices = mesh.vertex_count();
  if vertices > MAX_MESH_VERTICES {
    return Err(error(
      item(),
      format!("it has {vertices} vertices; at most {MAX_MESH_VERTICES}"),
    ));
  }
  if mesh.uvs.len() != vertices {
    return Err(error(
      item(),
      format!("it has {} UVs for {vertices} vertices", mesh.uvs.len()),
    ));
  }
  if mesh.triangles.is_empty() {
    return Err(error(item(), "it has no triangles; it needs at least one"));
  }
  if !mesh.triangles.len().is_multiple_of(3) {
    return Err(error(
      item(),
      format!(
        "it has {} triangle indices; they come in threes",
        mesh.triangles.len()
      ),
    ));
  }
  if let Some(&index) = mesh.triangles.iter().find(|&&i| usize::from(i) >= vertices) {
    return Err(error(
      item(),
      format!("triangle index {index} is out of range; there are {vertices} vertices"),
    ));
  }
  let hull = usize::from(mesh.hull);
  if hull != 0 && !(3..=vertices).contains(&hull) {
    return Err(error(
      item(),
      format!("it has a hull of {hull} vertices; it is 0 or 3 to {vertices}"),
    ));
  }
  for &uv in &mesh.uvs {
    finite_vec(&item, "a UV", uv)?;
  }
  match &mesh.vertices {
    MeshVertices::Unweighted(positions) => {
      for &position in positions {
        finite_vec(&item, "a vertex", position)?;
      }
    }
    MeshVertices::Weighted(weighted) => {
      for vertex in 0..vertices {
        let influences = weighted.influences(vertex);
        for influence in influences {
          finite(
            &item,
            &[
              ("an influence", influence.x),
              ("an influence", influence.y),
              ("an influence", influence.weight),
            ],
          )?;
          if influence.bone.index() >= bone_count {
            return Err(error(
              item(),
              format!(
                "vertex {vertex} has an influence of bone {}, which is not a bone; there are {bone_count}",
                influence.bone.0
              ),
            ));
          }
          if influence.weight < 0.0 {
            return Err(error(
              item(),
              format!(
                "vertex {vertex} has an influence with weight {} that is negative",
                influence.weight
              ),
            ));
          }
        }
        let sum = influences.iter().map(|i| i.weight).sum::<f32>();
        if sum <= 0.0 {
          return Err(error(
            item(),
            format!("vertex {vertex}'s weights sum to 0; they need to sum above 0"),
          ));
        }
        // Weights that sum past what a float holds were left as given, and
        // would skin the vertex to no finite place
        if !sum.is_finite() {
          return Err(error(
            item(),
            format!("vertex {vertex}'s weights sum to {sum}; they need a finite sum"),
          ));
        }
      }
    }
  }
  Ok(())
}

fn mix_duration(item: impl Fn() -> String, duration: f32) -> Result<(), SkeletonError> {
  finite(&item, &[("duration", duration)])?;
  if duration < 0.0 {
    return Err(error(item(), "its duration is negative"));
  }
  Ok(())
}

/// The bones, slots and constraints a timeline may refer to, the skins
/// (whose attachments a deform names), and how many events
#[derive(Clone, Copy)]
struct Counts<'a> {
  bones: &'a [BoneData],
  slots: &'a [SlotData],
  skins: &'a [Skin],
  constraints: &'a [Constraint],
  events: usize,
}

/// Checks one timeline: its bone, slot, constraint or events exist, a
/// constraint's timeline is of its kind, and a deform's attachment is a
/// name some skin has in its slot; no earlier timeline of
/// the animation keys the same property of the same target (`keyed`
/// collects them); it has keys, at finite times from 0 to the animation's
/// duration, in order (strictly, but event keys may share a time); and its
/// values and curves are good
fn timeline_ok<'a>(
  item: impl Fn() -> String,
  timeline: &'a Timeline,
  duration: f32,
  counts: Counts<'_>,
  keyed: &mut HashSet<(Target<'a>, &'static str)>,
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
    Target::Slot(slot) | Target::Attachment(slot, _) if slot.index() >= counts.slots.len() => {
      return Err(error(
        item(),
        format!(
          "slot {} is out of range; there are {} slots",
          slot.0,
          counts.slots.len()
        ),
      ));
    }
    Target::Constraint(constraint) if constraint.index() >= counts.constraints.len() => {
      return Err(error(
        item(),
        format!(
          "constraint {} is out of range; there are {} constraints",
          constraint.0,
          counts.constraints.len()
        ),
      ));
    }
    // A deform moves what its slot shows under a name, and a slot can only
    // show what a skin has in it: under a name no skin has there, the
    // deform could never apply
    Target::Attachment(slot, name)
      if !counts
        .skins
        .iter()
        .any(|skin| skin.attachment(slot, name).is_some()) =>
    {
      return Err(error(
        item(),
        format!(
          "no skin has an attachment {name:?} in slot {:?}; a deform moves the attachment its slot shows under that name",
          &*counts.slots[slot.index()].name
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
        Target::Attachment(slot, name) => {
          format!("slot {:?}'s {name:?}", &*counts.slots[slot.index()].name)
        }
        Target::Constraint(constraint) => {
          format!(
            "constraint {:?}'s",
            &**counts.constraints[constraint.index()].name()
          )
        }
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
    Timeline::Ik { constraint, frames } => {
      let Some(Constraint::Ik(ik)) = counts.constraints.get(constraint.index()) else {
        return Err(error(
          item(),
          format!(
            "constraint {:?} is a transform constraint; an ik timeline keys an IK constraint",
            name_of_constraint(counts.constraints, *constraint)
          ),
        ));
      };
      for (frame, (_, values, key_curve)) in frames.iter().enumerate() {
        let whose = format!("key {frame}'s ");
        ik_values_ok(&item, &whose, &ik.bones, counts.bones, values, false)?;
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::TransformMix { constraint, frames } => {
      let Some(Constraint::Transform(_)) = counts.constraints.get(constraint.index()) else {
        return Err(error(
          item(),
          format!(
            "constraint {:?} is an IK constraint; a transform mix timeline keys a transform constraint",
            name_of_constraint(counts.constraints, *constraint)
          ),
        ));
      };
      for (frame, (_, mix, key_curve)) in frames.iter().enumerate() {
        let name = |what: &str| format!("key {frame}'s {what}");
        finite(
          &item,
          &[
            (&name("rotate"), mix.rotate),
            (&name("x"), mix.x),
            (&name("y"), mix.y),
            (&name("scale_x"), mix.scale_x),
            (&name("scale_y"), mix.scale_y),
            (&name("shear_y"), mix.shear_y),
          ],
        )?;
        curve(&item, frame, key_curve)?;
      }
    }
    Timeline::Deform { frames, .. } => {
      // The first key that holds offsets sets the length of the rest
      let mut first: Option<(usize, usize)> = None;
      for (frame, (_, offsets, key_curve)) in frames.iter().enumerate() {
        let length = offsets.len();
        if !length.is_multiple_of(2) {
          return Err(error(
            item(),
            format!(
              "key {frame} holds {length} values; a key holds an x and a y for each position"
            ),
          ));
        }
        match first {
          Some((key, held)) if length != 0 && length != held => {
            return Err(error(
              item(),
              format!(
                "key {frame} holds {length} values where key {key} holds {held}; every key is empty or as long as the others"
              ),
            ));
          }
          None if length != 0 => first = Some((frame, length)),
          _ => {}
        }
        if let Some(index) = offsets.iter().position(|value| !value.is_finite()) {
          let name = format!("key {frame}'s value {index}");
          finite(&item, &[(&name, offsets[index])])?;
        }
        curve(&item, frame, key_curve)?;
      }
    }
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

/// A constraint's name, for messages
fn name_of_constraint(constraints: &[Constraint], constraint: ConstraintId) -> &str {
  constraints
    .get(constraint.index())
    .map_or("", |constraint| constraint.name())
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::super::animation::{Animation, EventValues, Timeline};
  use super::super::attachment::{
    Attachment, BoundingBoxAttachment, Influence, MeshAttachment, MeshVertices, PointAttachment,
    RegionAttachment, WeightedVertices,
  };
  use super::super::builder::{BoneSetup, SkeletonDataBuilder, SlotSetup};
  use super::super::constraint::{
    Constraint, ConstraintId, IkSetup, MAX_CONSTRAINTS, MAX_IK_BONES, TransformMix,
    TransformOffset, TransformSetup,
  };
  use super::super::curve::Curve;
  use super::super::data::{BoneId, EventData, Inherit, Region, SkeletonData, SlotId};
  use super::super::ik::IkValues;
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

  // Meshes

  const CAPE: &str = r#"skin "default" slot "hat" attachment "cape""#;

  /// A square of four unweighted vertices on region 0, with every part
  /// valid; each test spoils one
  fn square() -> MeshAttachment {
    MeshAttachment {
      hull: 4,
      ..MeshAttachment::new(
        0,
        MeshVertices::Unweighted(vec![
          Vec2::new(0.0, 0.0),
          Vec2::new(8.0, 0.0),
          Vec2::new(8.0, 8.0),
          Vec2::new(0.0, 8.0),
        ]),
        vec![
          Vec2::new(0.0, 0.0),
          Vec2::new(1.0, 0.0),
          Vec2::new(1.0, 1.0),
          Vec2::new(0.0, 1.0),
        ],
        vec![0, 1, 2, 0, 2, 3],
      )
    }
  }

  fn weighted(vertices: Vec<Vec<Influence>>) -> MeshAttachment {
    MeshAttachment {
      vertices: MeshVertices::Weighted(WeightedVertices::new(vertices)),
      ..square()
    }
  }

  fn influence(bone: u8, weight: f32) -> Influence {
    Influence::new(BoneId(bone), 1.0, 2.0, weight)
  }

  /// `rig` with a region and `mesh` as the hat's "cape"
  fn with_mesh(mesh: MeshAttachment) -> SkeletonDataBuilder {
    rig()
      .region(region("torso", 8, 8))
      .attachment("default", "hat", "cape", Attachment::Mesh(mesh))
  }

  #[test]
  fn a_valid_mesh_builds_weighted_and_unweighted() {
    let two = |bone| vec![influence(bone, 3.0), influence(1, 1.0)];
    let data = with_mesh(weighted(vec![two(0), two(1), two(0), two(1)]))
      .attachment("default", "hand", "flag", Attachment::Mesh(square()))
      .build()
      .expect("builds");
    let skin = &data.skins()[0];
    let Some(Attachment::Mesh(cape)) = skin.attachment(SlotId(1), "cape") else {
      panic!("no cape");
    };
    assert_eq!((cape.vertex_count(), cape.position_count()), (4, 8));
    let MeshVertices::Weighted(vertices) = &cape.vertices else {
      panic!("not weighted");
    };
    let sum: f32 = vertices.influences(0).iter().map(|i| i.weight).sum();
    assert!((sum - 1.0).abs() < 1e-6, "weights are normalised: {sum}");
    assert_eq!(vertices.influences(0)[0].weight, 0.75);
    let Some(Attachment::Mesh(flag)) = skin.attachment(SlotId(0), "flag") else {
      panic!("no flag");
    };
    assert_eq!((flag.vertex_count(), flag.position_count()), (4, 4));
  }

  #[test]
  fn a_mesh_region_outside_the_regions_fails() {
    fails(
      with_mesh(MeshAttachment {
        region: 1,
        ..square()
      }),
      CAPE,
      "region 1 is out of range",
    );
  }

  #[test]
  fn a_mesh_with_a_uv_count_other_than_its_vertices_fails() {
    let mut mesh = square();
    mesh.uvs.pop();
    fails(with_mesh(mesh), CAPE, "3 UVs for 4 vertices");
  }

  #[test]
  fn a_mesh_with_no_triangles_fails() {
    fails(
      with_mesh(MeshAttachment {
        triangles: Vec::new(),
        ..square()
      }),
      CAPE,
      "it needs at least one",
    );
  }

  #[test]
  fn a_mesh_with_triangle_indices_not_in_threes_fails() {
    fails(
      with_mesh(MeshAttachment {
        triangles: vec![0, 1, 2, 3],
        ..square()
      }),
      CAPE,
      "4 triangle indices; they come in threes",
    );
  }

  #[test]
  fn a_mesh_triangle_index_past_the_vertices_fails() {
    fails(
      with_mesh(MeshAttachment {
        triangles: vec![0, 1, 4],
        ..square()
      }),
      CAPE,
      "triangle index 4 is out of range; there are 4 vertices",
    );
  }

  #[test]
  fn a_mesh_hull_of_one_or_two_or_past_the_vertices_fails() {
    for hull in [1, 2, 5] {
      fails(
        with_mesh(MeshAttachment { hull, ..square() }),
        CAPE,
        &format!("hull of {hull} vertices; it is 0 or 3 to 4"),
      );
    }
    for hull in [0, 3, 4] {
      with_mesh(MeshAttachment { hull, ..square() })
        .build()
        .expect("a good hull builds");
    }
  }

  #[test]
  fn a_mesh_of_more_than_65535_vertices_fails() {
    let count = 65_536;
    let mesh = MeshAttachment::new(
      0,
      MeshVertices::Unweighted(vec![Vec2::ZERO; count]),
      vec![Vec2::ZERO; count],
      vec![0, 1, 2],
    );
    fails(with_mesh(mesh), CAPE, "65536 vertices; at most 65535");
    let mesh = MeshAttachment::new(
      0,
      MeshVertices::Unweighted(vec![Vec2::ZERO; count - 1]),
      vec![Vec2::ZERO; count - 1],
      vec![0, 1, 65_534],
    );
    with_mesh(mesh).build().expect("65535 vertices build");
  }

  #[test]
  fn a_non_finite_mesh_value_fails() {
    let mut mesh = square();
    mesh.uvs[2].y = f32::NAN;
    fails(with_mesh(mesh), CAPE, "a UV is NaN, not a finite number");
    let mut mesh = square();
    mesh.vertices = MeshVertices::Unweighted(vec![Vec2::new(f32::INFINITY, 0.0); 4]);
    fails(
      with_mesh(mesh),
      CAPE,
      "a vertex is inf, not a finite number",
    );
    let mut bad = influence(0, 1.0);
    bad.y = f32::NEG_INFINITY;
    fails(
      with_mesh(weighted(vec![vec![bad]; 4])),
      CAPE,
      "an influence is -inf, not a finite number",
    );
    let mut bad = influence(0, 1.0);
    bad.weight = f32::INFINITY;
    fails(
      with_mesh(weighted(vec![vec![bad]; 4])),
      CAPE,
      "an influence is inf, not a finite number",
    );
  }

  #[test]
  fn a_mesh_influence_of_an_unknown_bone_fails() {
    fails(
      with_mesh(weighted(vec![vec![influence(2, 1.0)]; 4])),
      CAPE,
      "influence of bone 2, which is not a bone; there are 2",
    );
  }

  #[test]
  fn a_mesh_influence_with_a_negative_weight_fails() {
    let vertex = vec![influence(0, 2.0), influence(1, -1.0)];
    fails(
      with_mesh(weighted(vec![vertex; 4])),
      CAPE,
      "weight -1 that is negative",
    );
  }

  #[test]
  fn a_mesh_vertex_whose_weights_sum_to_zero_fails() {
    let good = vec![influence(0, 1.0)];
    let zero = vec![influence(0, 0.0), influence(1, 0.0)];
    fails(
      with_mesh(weighted(vec![good.clone(), good.clone(), zero, good])),
      CAPE,
      "vertex 2's weights sum to 0",
    );
    fails(
      with_mesh(weighted(vec![Vec::new(); 4])),
      CAPE,
      "vertex 0's weights sum to 0",
    );
    // Weights that are each finite and sum past what a float holds cannot
    // be made to sum to 1 either: left as given, they would skin the
    // vertex to no finite place
    let good = vec![influence(0, 1.0)];
    let huge = vec![influence(0, f32::MAX), influence(1, f32::MAX)];
    fails(
      with_mesh(weighted(vec![good.clone(), huge, good.clone(), good])),
      CAPE,
      "vertex 1's weights sum to inf",
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

  // Constraints

  /// A root with a thigh, a shin (1 below the thigh, 10 long) and a foot
  /// target, for the limb's constraints
  fn limb() -> SkeletonDataBuilder {
    let long = BoneSetup {
      length: 10.0,
      ..BoneSetup::default()
    };
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("thigh", Some("root"), long)
      .bone("shin", Some("thigh"), BoneSetup { x: 10.0, ..long })
      .bone("foot_target", Some("root"), BoneSetup::default())
  }

  fn leg(setup: IkSetup) -> SkeletonDataBuilder {
    limb().ik("leg", &["thigh", "shin"], "foot_target", setup)
  }

  #[test]
  fn constraints_keep_the_order_they_were_added_in() {
    let data = limb()
      .transform_constraint(
        "pelvis",
        &["thigh"],
        "foot_target",
        TransformSetup::default(),
      )
      .ik("leg", &["thigh", "shin"], "foot_target", IkSetup::default())
      .build()
      .expect("valid skeleton");
    let names: Vec<&str> = data.constraints().iter().map(|c| &**c.name()).collect();
    assert_eq!(names, ["pelvis", "leg"]);
    assert_eq!(data.constraint("leg"), Some(ConstraintId(1)));
    assert_eq!(data.constraint("pelvis").map(ConstraintId::index), Some(0));
    assert_eq!(data.constraint("nothing"), None);
    let leg = &data.constraints()[1];
    assert_eq!(leg.bones(), [BoneId(1), BoneId(2)]);
    assert_eq!(leg.target(), BoneId(3));
    assert!(matches!(leg, Constraint::Ik(_)));
  }

  #[test]
  fn a_constraint_name_used_twice_fails() {
    fails(
      leg(IkSetup::default()).transform_constraint(
        "leg",
        &["root"],
        "foot_target",
        TransformSetup::default(),
      ),
      r#"constraint "leg""#,
      "another constraint has the same name",
    );
  }

  #[test]
  fn a_constraint_s_unknown_bone_or_target_fails() {
    fails(
      limb().ik("leg", &["thigh", "calf"], "foot_target", IkSetup::default()),
      r#"constraint "leg""#,
      r#"its bone "calf" is not a bone"#,
    );
    fails(
      limb().ik("leg", &["thigh", "shin"], "floor", IkSetup::default()),
      r#"constraint "leg""#,
      r#"its target "floor" is not a bone"#,
    );
    fails(
      limb().transform_constraint("pelvis", &["hips"], "root", TransformSetup::default()),
      r#"constraint "pelvis""#,
      r#"its bone "hips" is not a bone"#,
    );
    fails(
      limb().transform_constraint("pelvis", &["root"], "torso", TransformSetup::default()),
      r#"constraint "pelvis""#,
      r#"its target "torso" is not a bone"#,
    );
  }

  #[test]
  fn a_constraint_s_bones_must_be_distinct() {
    fails(
      limb().transform_constraint(
        "pelvis",
        &["thigh", "shin", "thigh"],
        "foot_target",
        TransformSetup::default(),
      ),
      r#"constraint "pelvis""#,
      r#"it lists bone "thigh" twice"#,
    );
    // A bone and its own descendant are two bones
    limb()
      .transform_constraint(
        "pelvis",
        &["shin", "thigh"],
        "foot_target",
        TransformSetup::default(),
      )
      .build()
      .expect("a bone and its descendant in one transform constraint");
  }

  #[test]
  fn a_constraint_s_target_among_or_below_its_bones_fails() {
    fails(
      limb().ik("leg", &["thigh", "shin"], "shin", IkSetup::default()),
      r#"constraint "leg""#,
      r#"its target "shin" is one of its bones"#,
    );
    // A bone below a constrained bone, not itself constrained
    let below = limb().bone("toe", Some("shin"), BoneSetup::default());
    fails(
      below.ik("leg", &["thigh", "shin"], "toe", IkSetup::default()),
      r#"constraint "leg""#,
      r#"its target "toe" is below its bone "thigh""#,
    );
    fails(
      limb().transform_constraint("pelvis", &["thigh"], "shin", TransformSetup::default()),
      r#"constraint "pelvis""#,
      r#"its target "shin" is below its bone "thigh""#,
    );
    // Above its bones is allowed: a bone follows what carries it
    limb()
      .ik("leg", &["thigh", "shin"], "root", IkSetup::default())
      .build()
      .expect("a target that is an ancestor of the bones");
  }

  #[test]
  fn an_ik_bone_that_is_not_its_predecessor_s_child_fails() {
    fails(
      limb().ik("leg", &["shin", "thigh"], "foot_target", IkSetup::default()),
      r#"constraint "leg""#,
      r#"bone "thigh" is not a child of "shin"; each bone after the first must be an immediate child of the one before"#,
    );
    // A grandchild is not an immediate child
    let long = BoneSetup {
      length: 10.0,
      ..BoneSetup::default()
    };
    fails(
      limb().bone("toe", Some("shin"), long).ik(
        "leg",
        &["thigh", "toe"],
        "foot_target",
        IkSetup::default(),
      ),
      r#"constraint "leg""#,
      r#"bone "toe" is not a child of "thigh""#,
    );
  }

  #[test]
  fn an_ik_bone_that_does_not_inherit_normally_fails() {
    let rig = |inherit| {
      let long = BoneSetup {
        length: 10.0,
        ..BoneSetup::default()
      };
      SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .bone("thigh", Some("root"), long)
        .bone("shin", Some("thigh"), BoneSetup { inherit, ..long })
        .bone("foot_target", Some("root"), BoneSetup::default())
        .ik("leg", &["thigh", "shin"], "foot_target", IkSetup::default())
    };
    fails(
      rig(Inherit::NoScale),
      r#"constraint "leg""#,
      r#"bone "shin" does not inherit normally"#,
    );
    rig(Inherit::Normal).build().expect("valid skeleton");
  }

  #[test]
  fn an_ik_mix_or_softness_out_of_range_fails() {
    for (mix, reason) in [
      (-0.1, "mix is -0.1; it must be within 0 to 1"),
      (1.5, "mix is 1.5; it must be within 0 to 1"),
      (f32::NAN, "mix is NaN, not a finite number"),
    ] {
      fails(
        leg(IkSetup {
          mix,
          ..IkSetup::default()
        }),
        r#"constraint "leg""#,
        reason,
      );
    }
    fails(
      leg(IkSetup {
        mix: 2.0,
        ..IkSetup::default()
      }),
      r#"constraint "leg""#,
      "mix is 2; it must be within 0 to 1",
    );
    fails(
      leg(IkSetup {
        softness: -1.0,
        ..IkSetup::default()
      }),
      r#"constraint "leg""#,
      "softness is -1; it must not be negative",
    );
    fails(
      leg(IkSetup {
        softness: f32::INFINITY,
        ..IkSetup::default()
      }),
      r#"constraint "leg""#,
      "softness is inf, not a finite number",
    );
  }

  #[test]
  fn an_ik_setting_that_cannot_apply_fails() {
    let one = |setup| limb().ik("look", &["thigh"], "foot_target", setup);
    let chain = |setup| {
      limb()
        .bone(
          "foot",
          Some("shin"),
          BoneSetup {
            length: 5.0,
            ..BoneSetup::default()
          },
        )
        .ik("tail", &["thigh", "shin", "foot"], "foot_target", setup)
    };
    let item = |name| format!("constraint {name:?}");
    fails(
      one(IkSetup {
        bend_positive: false,
        ..IkSetup::default()
      }),
      &item("look"),
      "bend_positive false applies only to two bones",
    );
    fails(
      one(IkSetup {
        softness: 2.0,
        ..IkSetup::default()
      }),
      &item("look"),
      "softness applies only to two bones",
    );
    fails(
      chain(IkSetup {
        bend_positive: false,
        ..IkSetup::default()
      }),
      &item("tail"),
      "bend_positive false applies only to two bones",
    );
    fails(
      chain(IkSetup {
        softness: 2.0,
        ..IkSetup::default()
      }),
      &item("tail"),
      "softness applies only to two bones",
    );
    fails(
      leg(IkSetup {
        compress: true,
        ..IkSetup::default()
      }),
      &item("leg"),
      "compress applies only to one bone",
    );
    fails(
      chain(IkSetup {
        compress: true,
        ..IkSetup::default()
      }),
      &item("tail"),
      "compress applies only to one bone",
    );
    fails(
      chain(IkSetup {
        stretch: true,
        ..IkSetup::default()
      }),
      &item("tail"),
      "stretch applies only to one or two bones",
    );
    fails(
      chain(IkSetup {
        uniform: true,
        ..IkSetup::default()
      }),
      &item("tail"),
      "uniform applies only to one or two bones",
    );
    // What is allowed
    leg(IkSetup {
      bend_positive: false,
      softness: 3.0,
      stretch: true,
      uniform: true,
      ..IkSetup::default()
    })
    .build()
    .expect("valid skeleton");
    one(IkSetup {
      compress: true,
      stretch: true,
      uniform: true,
      ..IkSetup::default()
    })
    .build()
    .expect("valid skeleton");
    chain(IkSetup {
      mix: 0.5,
      ..IkSetup::default()
    })
    .build()
    .expect("valid skeleton");
  }

  #[test]
  fn an_ik_tip_without_a_length_fails() {
    let rig = |last: f32, setup| {
      SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
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
            length: last,
            ..BoneSetup::default()
          },
        )
        .bone("foot_target", Some("root"), BoneSetup::default())
        .ik("leg", &["thigh", "shin"], "foot_target", setup)
    };
    fails(
      rig(0.0, IkSetup::default()),
      r#"constraint "leg""#,
      r#"its last bone "shin" has a length of 0; its tip is what reaches the target, so the length must be above 0"#,
    );
    rig(0.5, IkSetup::default())
      .build()
      .expect("valid skeleton");

    // One bone: pointing needs no length, stretch and compress do
    let one = |setup| {
      SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .bone("head", Some("root"), BoneSetup::default())
        .bone("look_target", Some("root"), BoneSetup::default())
        .ik("look", &["head"], "look_target", setup)
    };
    one(IkSetup::default()).build().expect("valid skeleton");
    fails(
      one(IkSetup {
        stretch: true,
        ..IkSetup::default()
      }),
      r#"constraint "look""#,
      r#"its bone "head" has a length of 0; stretch and compress need a length above 0"#,
    );
    fails(
      one(IkSetup {
        compress: true,
        ..IkSetup::default()
      }),
      r#"constraint "look""#,
      "stretch and compress need a length above 0",
    );
  }

  #[test]
  fn a_transform_constraint_without_bones_or_with_a_value_not_finite_fails() {
    let transform = |setup| limb().transform_constraint("pelvis", &["thigh"], "root", setup);
    fails(
      limb().transform_constraint("pelvis", &[], "root", TransformSetup::default()),
      r#"constraint "pelvis""#,
      "it has no bones",
    );
    fails(
      transform(TransformSetup {
        mix: TransformMix {
          rotate: f32::NAN,
          ..TransformMix::default()
        },
        ..TransformSetup::default()
      }),
      r#"constraint "pelvis""#,
      "mix rotate is NaN, not a finite number",
    );
    fails(
      transform(TransformSetup {
        offset: TransformOffset {
          shear_y: f32::INFINITY,
          ..TransformOffset::default()
        },
        ..TransformSetup::default()
      }),
      r#"constraint "pelvis""#,
      "offset shear_y is inf, not a finite number",
    );
    // Mixes outside 0 to 1 are allowed
    transform(TransformSetup {
      mix: TransformMix {
        rotate: -1.0,
        x: 2.0,
        ..TransformMix::default()
      },
      ..TransformSetup::default()
    })
    .build()
    .expect("valid skeleton");
  }

  #[test]
  fn too_many_constraints_or_ik_bones_fail() {
    let mut most = limb();
    for number in 0..MAX_CONSTRAINTS {
      most = most.transform_constraint(
        format!("c{number}"),
        &["thigh"],
        "foot_target",
        TransformSetup::default(),
      );
    }
    let data = most.build().expect("exactly 255 constraints build");
    assert_eq!(data.constraints().len(), MAX_CONSTRAINTS);
    let mut many = limb();
    for number in 0..=MAX_CONSTRAINTS {
      many = many.transform_constraint(
        format!("c{number}"),
        &["thigh"],
        "foot_target",
        TransformSetup::default(),
      );
    }
    fails(
      many,
      r#"constraint "c255""#,
      "a skeleton has at most 255 constraints",
    );

    let chain = |count: usize| {
      let mut builder = SkeletonData::builder()
        .bone("root", None, BoneSetup::default())
        .bone("target", Some("root"), BoneSetup::default());
      let mut names = Vec::new();
      let mut parent = String::from("root");
      for number in 0..count {
        let name = format!("b{number}");
        builder = builder.bone(
          name.as_str(),
          Some(parent.as_str()),
          BoneSetup {
            length: 5.0,
            ..BoneSetup::default()
          },
        );
        names.push(name.clone());
        parent = name;
      }
      let bones: Vec<&str> = names.iter().map(String::as_str).collect();
      builder.ik("tail", &bones, "target", IkSetup::default())
    };
    fails(
      chain(MAX_IK_BONES + 1),
      r#"constraint "tail""#,
      "it has 17 bones; at most 16",
    );
    chain(MAX_IK_BONES).build().expect("valid skeleton");
    fails(
      limb().ik("leg", &[], "foot_target", IkSetup::default()),
      r#"constraint "leg""#,
      "it has no bones",
    );
  }

  /// [`leg`] at its setup, a one-bone constraint "look" on the thigh, a
  /// chain "tail" of three bones, a transform constraint "level" and a
  /// one-bone constraint "gaze" on a bone of no length, with the animation
  /// "walk" holding `timelines`
  fn keyed(timelines: Vec<Timeline>) -> SkeletonDataBuilder {
    let long = BoneSetup {
      length: 5.0,
      ..BoneSetup::default()
    };
    leg(IkSetup::default())
      .bone("flat", Some("root"), BoneSetup::default())
      .bone("pelvis", Some("root"), BoneSetup::default())
      .bone("t0", Some("root"), long)
      .bone("t1", Some("t0"), long)
      .bone("t2", Some("t1"), long)
      .ik("look", &["thigh"], "foot_target", IkSetup::default())
      .ik(
        "tail",
        &["t0", "t1", "t2"],
        "foot_target",
        IkSetup::default(),
      )
      .transform_constraint(
        "level",
        &["pelvis"],
        "foot_target",
        TransformSetup::default(),
      )
      .ik("gaze", &["flat"], "foot_target", IkSetup::default())
      .animation(animation("walk", 1.0, timelines))
  }

  const LEG: ConstraintId = ConstraintId(0);
  const LOOK: ConstraintId = ConstraintId(1);
  const TAIL: ConstraintId = ConstraintId(2);
  const LEVEL: ConstraintId = ConstraintId(3);
  const GAZE: ConstraintId = ConstraintId(4);

  fn ik_keys(constraint: ConstraintId, keys: Vec<IkValues>) -> Timeline {
    Timeline::Ik {
      constraint,
      frames: keys
        .into_iter()
        .enumerate()
        .map(|(at, key)| (at as f32 * 0.5, key, Curve::Linear))
        .collect(),
    }
  }

  fn ik_value(change: impl FnOnce(&mut IkValues)) -> IkValues {
    let mut values = IkValues::of(&IkSetup::default());
    change(&mut values);
    values
  }

  fn mix_keys(constraint: ConstraintId, mix: TransformMix) -> Timeline {
    Timeline::TransformMix {
      constraint,
      frames: vec![(0.0, mix, Curve::Linear)],
    }
  }

  const WALK_0_IK: &str = r#"animation "walk" timeline 0 (ik)"#;

  #[test]
  fn a_constraint_timeline_for_an_unknown_constraint_or_the_other_kind_fails() {
    keyed(vec![
      ik_keys(LEG, vec![ik_value(|_| {})]),
      mix_keys(LEVEL, TransformMix::ALL),
    ])
    .build()
    .expect("each timeline keys a constraint of its kind");
    fails(
      keyed(vec![ik_keys(ConstraintId(5), vec![ik_value(|_| {})])]),
      WALK_0_IK,
      "constraint 5 is out of range; there are 5 constraints",
    );
    fails(
      keyed(vec![ik_keys(LEVEL, vec![ik_value(|_| {})])]),
      WALK_0_IK,
      r#"constraint "level" is a transform constraint; an ik timeline keys an IK constraint"#,
    );
    fails(
      keyed(vec![mix_keys(LEG, TransformMix::ALL)]),
      r#"animation "walk" timeline 0 (transform mix)"#,
      r#"constraint "leg" is an IK constraint; a transform mix timeline keys a transform constraint"#,
    );
  }

  #[test]
  fn two_timelines_for_one_constraint_fail() {
    fails(
      keyed(vec![
        ik_keys(LEG, vec![ik_value(|_| {})]),
        ik_keys(LOOK, vec![ik_value(|_| {})]),
        ik_keys(LEG, vec![ik_value(|_| {})]),
      ]),
      r#"animation "walk" timeline 2 (ik)"#,
      r#"another timeline already keys constraint "leg"'s values"#,
    );
    fails(
      keyed(vec![
        mix_keys(LEVEL, TransformMix::ALL),
        mix_keys(LEVEL, TransformMix::default()),
      ]),
      r#"animation "walk" timeline 1 (transform mix)"#,
      r#"another timeline already keys constraint "level"'s values"#,
    );
  }

  #[test]
  fn a_constraint_key_out_of_range_or_with_a_switch_it_cannot_have_fails() {
    // Every switch each constraint can have, at its edges, builds
    keyed(vec![
      ik_keys(
        LEG,
        vec![
          ik_value(|key| key.mix = 0.0),
          ik_value(|key| {
            key.mix = 1.0;
            key.softness = 5.0;
            key.bend_positive = false;
            key.stretch = true;
          }),
        ],
      ),
      ik_keys(
        LOOK,
        vec![ik_value(|key| {
          key.compress = true;
          key.stretch = true;
        })],
      ),
      ik_keys(TAIL, vec![ik_value(|key| key.mix = 0.5)]),
      mix_keys(
        LEVEL,
        TransformMix {
          rotate: -1.0,
          x: 2.0,
          ..TransformMix::default()
        },
      ),
    ])
    .build()
    .expect("keys within range");
    let one_key = |constraint, change: fn(&mut IkValues)| {
      keyed(vec![ik_keys(
        constraint,
        vec![ik_value(|_| {}), ik_value(change)],
      )])
    };
    type Case = (ConstraintId, fn(&mut IkValues), &'static str);
    let cases: [Case; 10] = [
      (
        LEG,
        |key| key.mix = 1.5,
        "key 1's mix is 1.5; it must be within 0 to 1",
      ),
      (
        LEG,
        |key| key.mix = -0.5,
        "key 1's mix is -0.5; it must be within 0 to 1",
      ),
      (
        LEG,
        |key| key.mix = f32::NAN,
        "key 1's mix is NaN, not a finite number",
      ),
      (
        LEG,
        |key| key.softness = -1.0,
        "key 1's softness is -1; it must not be negative",
      ),
      (
        LEG,
        |key| key.compress = true,
        "key 1's compress applies only to one bone",
      ),
      (
        LOOK,
        |key| key.bend_positive = false,
        "key 1's bend_positive false applies only to two bones",
      ),
      (
        LOOK,
        |key| key.softness = 1.0,
        "key 1's softness applies only to two bones",
      ),
      (
        TAIL,
        |key| key.stretch = true,
        "key 1's stretch applies only to one or two bones",
      ),
      (
        GAZE,
        |key| key.stretch = true,
        r#"key 1's stretch or compress is on, and its bone "flat" has a length of 0; stretch and compress need a length above 0"#,
      ),
      (
        GAZE,
        |key| key.compress = true,
        r#"key 1's stretch or compress is on, and its bone "flat" has a length of 0"#,
      ),
    ];
    for (constraint, change, reason) in cases {
      fails(one_key(constraint, change), WALK_0_IK, reason);
    }
    fails(
      keyed(vec![mix_keys(
        LEVEL,
        TransformMix {
          shear_y: f32::INFINITY,
          ..TransformMix::default()
        },
      )]),
      r#"animation "walk" timeline 0 (transform mix)"#,
      "key 0's shear_y is inf, not a finite number",
    );
    // Its keys' curves are checked as every timeline's are
    let mut bent = ik_keys(LEG, vec![ik_value(|_| {}), ik_value(|_| {})]);
    if let Timeline::Ik { frames, .. } = &mut bent {
      frames[0].2 = Curve::Bezier {
        cx1: 1.5,
        cy1: 0.0,
        cx2: 0.5,
        cy2: 1.0,
      };
    }
    fails(
      keyed(vec![bent]),
      WALK_0_IK,
      "key 0's curve has x controls 1.5 and 0.5",
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

  // Deform timelines

  const WAVE_DEFORM: &str = r#"animation "wave" timeline 0 (deform)"#;

  /// [`rig`] with what the deform tests' timelines name, in the default
  /// skin: a "flag" and a "pennant" in the hat's slot, and a "flag" in the
  /// hand's. They are points: a deform's attachment is checked by name
  fn flagged() -> SkeletonDataBuilder {
    rig()
      .attachment("default", "hat", "flag", point(1.0))
      .attachment("default", "hat", "pennant", point(2.0))
      .attachment("default", "hand", "flag", point(3.0))
  }

  /// A deform timeline of `attachment` in slot `slot`, keyed linearly
  fn deform(slot: u8, attachment: &str, keys: &[(f32, &[f32])]) -> Timeline {
    Timeline::Deform {
      slot: SlotId(slot),
      attachment: Arc::from(attachment),
      frames: keys
        .iter()
        .map(|(time, offsets)| (*time, offsets.to_vec(), Curve::Linear))
        .collect(),
    }
  }

  #[test]
  fn two_deform_timelines_for_one_slot_and_name_fail() {
    let flag = |slot: u8, name: &str| deform(slot, name, &[(0.0, &[1.0, 2.0])]);
    fails(
      flagged().animation(wave(vec![flag(1, "flag"), flag(1, "flag")])),
      r#"animation "wave" timeline 1 (deform)"#,
      "another timeline already keys slot \"hat\"'s \"flag\" deform",
    );
    // What a deform keys is its slot's attachment by name. Another name in
    // the slot is another property, and so is the same name in another
    // slot; and none of them is the slot's attachment or colour, which
    // timelines of their own key beside it
    let beside = vec![
      flag(1, "flag"),
      flag(1, "pennant"),
      flag(0, "flag"),
      Timeline::Attachment {
        slot: SlotId(1),
        frames: vec![(0.0, Some(Arc::from("flag")))],
      },
      Timeline::Color {
        slot: SlotId(1),
        frames: vec![(0.0, crate::core::ui::Color::WHITE, Curve::Linear)],
      },
    ];
    let data = flagged().animation(wave(beside)).build();
    assert!(data.is_ok(), "{data:?}");
  }

  #[test]
  fn deform_keys_of_different_lengths_fail() {
    // Every key is empty (no offset: the setup shape) or as long as the
    // others, wherever the empty ones come
    let wavy: &[(f32, &[f32])] = &[
      (0.0, &[]),
      (0.25, &[1.0, 2.0, 3.0, 4.0]),
      (0.5, &[]),
      (1.0, &[4.0, 3.0, 2.0, 1.0]),
    ];
    let still: &[(f32, &[f32])] = &[(0.0, &[]), (1.0, &[])];
    for keys in [wavy, still] {
      let data = flagged()
        .animation(wave(vec![deform(1, "flag", keys)]))
        .build();
      assert!(data.is_ok(), "{data:?}");
    }
    let uneven: &[(f32, &[f32])] = &[
      (0.0, &[]),
      (0.25, &[1.0, 2.0, 3.0, 4.0]),
      (0.5, &[]),
      (1.0, &[1.0, 2.0]),
    ];
    fails(
      flagged().animation(wave(vec![deform(1, "flag", uneven)])),
      WAVE_DEFORM,
      "key 3 holds 2 values where key 1 holds 4",
    );
  }

  #[test]
  fn a_deform_key_of_an_odd_count_fails() {
    // A key holds an x and a y for each position: three numbers are no
    // whole count of them, whatever mesh the slot shows
    fails(
      flagged().animation(wave(vec![deform(1, "flag", &[(0.0, &[1.0, 2.0, 3.0])])])),
      WAVE_DEFORM,
      "key 0 holds 3 values",
    );
  }

  #[test]
  fn a_deform_timeline_is_checked_as_every_timeline_is() {
    let key: &[f32] = &[1.0, 2.0];
    let failing = |timeline: Timeline, reason: &str| {
      fails(
        flagged().animation(wave(vec![timeline])),
        WAVE_DEFORM,
        reason,
      );
    };
    // Its slot exists
    failing(deform(2, "flag", &[(0.0, key)]), "slot 2 is out of range");
    // It has keys, from 0 to the animation's end, in time order
    failing(deform(1, "flag", &[]), "no keys");
    failing(deform(1, "flag", &[(-0.5, key)]), "before 0");
    failing(deform(1, "flag", &[(0.5, key), (0.25, key)]), "time order");
    failing(deform(1, "flag", &[(0.5, key), (0.5, key)]), "time order");
    failing(
      deform(1, "flag", &[(0.0, key), (1.5, key)]),
      "past the animation's duration",
    );
    failing(deform(1, "flag", &[(f32::NAN, key)]), "key 0's time is NaN");
    // Its values are finite
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
      failing(
        deform(1, "flag", &[(0.0, key), (0.5, &[3.0, bad])]),
        &format!("key 1's value 1 is {bad}"),
      );
    }
    // And its curves are good
    let mut curved = deform(1, "flag", &[(0.0, key), (1.0, key)]);
    if let Timeline::Deform { frames, .. } = &mut curved {
      frames[0].2 = Curve::Bezier {
        cx1: 0.25,
        cy1: 0.0,
        cx2: 1.5,
        cy2: 1.0,
      };
    }
    failing(curved, "within 0 to 1");
  }

  #[test]
  fn a_deform_naming_an_attachment_no_skin_has_in_its_slot_fails() {
    let key: &[f32] = &[1.0, 2.0];
    let naming = |slot: u8, name: &str| wave(vec![deform(slot, name, &[(0.0, key)])]);
    // A deform moves what its slot shows under a name. A name no skin has
    // in that slot can never be shown there, so the deform could never
    // apply: a misspelt name, which would otherwise do nothing in silence
    let error = flagged()
      .animation(naming(1, "flagg"))
      .build()
      .expect_err("a name no skin has");
    assert_eq!(error.item, WAVE_DEFORM);
    assert_eq!(
      error.problem,
      "no skin has an attachment \"flagg\" in slot \"hat\"; a deform moves the attachment its slot shows under that name"
    );
    // A name that exists only under another slot: the pennant is the
    // hat's, not the hand's
    fails(
      flagged().animation(naming(0, "pennant")),
      WAVE_DEFORM,
      "no skin has an attachment \"pennant\" in slot \"hand\"",
    );
    // Keys that are all empty would fit any mesh, and must name one all
    // the same
    let empty = wave(vec![deform(1, "flagg", &[(0.0, &[])])]);
    fails(
      flagged().animation(empty),
      WAVE_DEFORM,
      "no skin has an attachment \"flagg\" in slot \"hat\"",
    );
    // And a skeleton whose skins hold nothing
    fails(
      rig().animation(naming(1, "flag")),
      WAVE_DEFORM,
      "no skin has an attachment \"flag\" in slot \"hat\"",
    );
    // A name only a skin other than the default has in the slot is one
    // the slot can show
    let dressed = rig()
      .skin("red")
      .attachment("red", "hat", "plume", point(1.0))
      .animation(naming(1, "plume"))
      .build();
    assert!(dressed.is_ok(), "{dressed:?}");
    // Only the name is checked. What is found under it (here a point),
    // and whether a mesh there holds the keys' count, is the skins' to
    // decide as the deform plays
    let named = flagged().animation(naming(1, "flag")).build();
    assert!(named.is_ok(), "{named:?}");
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
