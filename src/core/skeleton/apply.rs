//! Applying one animation to a pose at a time, by an alpha and a blend, and
//! finding the event keys a step crosses. The module notes give the rule for
//! each timeline kind.

use std::f32::consts::TAU;

use glam::{Vec2, Vec4};

use super::animation::{Animation, EventValues, Frame, Timeline, deform_width};
use super::attachment::Attachment;
use super::constraint::{Constraint, ConstraintId, SpringValues, TransformMix};
use super::data::{BoneData, BoneId, SkeletonData, SlotId};
use super::ik::IkValues;
use super::pose::{BonePose, ConstraintValues, Pose, setup_order, srgb};
use crate::core::ui::Color;

/// How an animation's values combine with the pose under it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MixBlend {
  /// Moves the pose toward the animation's values by the alpha
  #[default]
  Replace,
  /// Adds the animation's change from the setup pose, times the alpha
  Add,
}

/// Whether an animation's stepped keys apply. Attachment and draw-order
/// keys apply only under `In`, which the animation state gives to the one
/// entry of a track that owns them; the others get `Out`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MixDirection {
  In,
  Out,
}

/// Blends `animation` at `time` (its own time: the caller wraps a looping
/// track's time) into `pose`, by `alpha` (0 leaves bone, colour and deform
/// values, 1 takes them fully) and `blend`. Before a timeline's first key
/// it leaves the pose; after its last key the last value holds. Its deform
/// keys move the meshes of the default skin that the slots show once its
/// other keys have applied. Event timelines do nothing here (see
/// [`events_between`])
#[cfg(test)]
pub(crate) fn apply(
  animation: &Animation,
  data: &SkeletonData,
  pose: &mut Pose,
  time: f32,
  alpha: f32,
  blend: MixBlend,
  direction: MixDirection,
) {
  let mut deforms = Vec::new();
  apply_layer(
    animation,
    data,
    pose,
    Layer {
      animation: 0,
      time,
      alpha,
      fades: &[],
      blend,
      direction,
    },
    &mut [],
    &mut deforms,
  );
  for step in deforms {
    let worn = |slot, name: &str| data.default_skin().attachment(slot, name);
    apply_deform(animation, step, pose, worn);
  }
}

/// How one entry applies in a step: its animation's place in the data (for
/// the deform keys it leaves to be applied later), its animation time, its
/// alpha, each timeline's fade (a factor on the alpha, by timeline index;
/// missing ones are 1), its blend and its direction
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layer<'a> {
  pub(crate) animation: usize,
  pub(crate) time: f32,
  pub(crate) alpha: f32,
  pub(crate) fades: &'a [f32],
  pub(crate) blend: MixBlend,
  pub(crate) direction: MixDirection,
}

/// As [`apply`], with a fade per timeline, and `turns` to remember which
/// way each rotate timeline (by index) blends toward the pose: given one,
/// the blend takes the turn nearest the one it took last step (the short
/// way on the first, while it is NaN) and records it, so a turn that would
/// cross half a circle mid-mix keeps going the way it started. Timelines
/// past the end of `turns` blend the short way every step.
///
/// A deform timeline is not applied here: what its slot shows is not
/// settled until every track has applied and the game's attachments are
/// over them. Each one that would move anything is pushed onto `deforms`,
/// in the order applied, for [`apply_deform`] then
pub(crate) fn apply_layer(
  animation: &Animation,
  data: &SkeletonData,
  pose: &mut Pose,
  layer: Layer<'_>,
  turns: &mut [f32],
  deforms: &mut Vec<DeformStep>,
) {
  let Layer {
    animation: place,
    time,
    alpha: base,
    fades,
    blend,
    direction,
  } = layer;
  for (index, timeline) in animation.timelines.iter().enumerate() {
    let alpha = base * fades.get(index).copied().unwrap_or(1.0);
    match timeline {
      Timeline::Rotate { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          let target = setup.rotation + value;
          local.rotation += match blend {
            MixBlend::Replace => {
              let difference = target - local.rotation;
              let turn = match turns.get_mut(index) {
                Some(last) if !last.is_nan() => {
                  difference - TAU * ((difference - *last) / TAU).round()
                }
                _ => shortest(difference),
              };
              if let Some(last) = turns.get_mut(index) {
                *last = turn;
              }
              turn * alpha
            }
            MixBlend::Add => value * alpha,
          };
        }
      }
      Timeline::Translate { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_vec2(frames, time), bone_mut(data, pose, *bone))
        {
          mix(&mut local.x, setup.x, setup.x + value.x, alpha, blend);
          mix(&mut local.y, setup.y, setup.y + value.y, alpha, blend);
        }
      }
      Timeline::Scale { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_vec2(frames, time), bone_mut(data, pose, *bone))
        {
          mix(
            &mut local.scale_x,
            setup.scale_x,
            setup.scale_x * value.x,
            alpha,
            blend,
          );
          mix(
            &mut local.scale_y,
            setup.scale_y,
            setup.scale_y * value.y,
            alpha,
            blend,
          );
        }
      }
      Timeline::Shear { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_vec2(frames, time), bone_mut(data, pose, *bone))
        {
          mix(
            &mut local.shear_x,
            setup.shear_x,
            setup.shear_x + value.x,
            alpha,
            blend,
          );
          mix(
            &mut local.shear_y,
            setup.shear_y,
            setup.shear_y + value.y,
            alpha,
            blend,
          );
        }
      }
      Timeline::TranslateX { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          mix(&mut local.x, setup.x, setup.x + value, alpha, blend);
        }
      }
      Timeline::TranslateY { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          mix(&mut local.y, setup.y, setup.y + value, alpha, blend);
        }
      }
      Timeline::ScaleX { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          mix(
            &mut local.scale_x,
            setup.scale_x,
            setup.scale_x * value,
            alpha,
            blend,
          );
        }
      }
      Timeline::ScaleY { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          mix(
            &mut local.scale_y,
            setup.scale_y,
            setup.scale_y * value,
            alpha,
            blend,
          );
        }
      }
      Timeline::ShearX { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          mix(
            &mut local.shear_x,
            setup.shear_x,
            setup.shear_x + value,
            alpha,
            blend,
          );
        }
      }
      Timeline::ShearY { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          mix(
            &mut local.shear_y,
            setup.shear_y,
            setup.shear_y + value,
            alpha,
            blend,
          );
        }
      }
      Timeline::Color { slot, frames } => {
        if let (Some(target), Some(color)) = (
          sample_color(frames, time),
          pose.slot_colors.get_mut(slot.index()),
        ) {
          // Colours do not add: both blends move toward the key by alpha
          *color += (target - *color) * alpha;
        }
      }
      Timeline::Attachment { slot, frames } => {
        if direction == MixDirection::In
          && let Some(index) = stepped_key(frames, time)
          && let Some(attachment) = pose.slot_attachments.get_mut(slot.index())
        {
          attachment.clone_from(&frames[index].1);
        }
      }
      Timeline::DrawOrder { frames } => {
        if direction == MixDirection::In
          && let Some(index) = stepped_key(frames, time)
        {
          pose.draw_order.clear();
          match &frames[index].1 {
            Some(order) => pose.draw_order.extend_from_slice(order),
            None => pose.draw_order.extend(setup_order(data)),
          }
        }
      }
      Timeline::Ik { constraint, frames } => {
        if let Some((key, mix_key, softness_key)) = sample_ik(frames, time)
          && let Some((Constraint::Ik(ik), ConstraintValues::Ik(values))) =
            constraint_mut(data, pose, *constraint)
        {
          let setup = &ik.setup;
          mix(&mut values.mix, setup.mix, mix_key, alpha, blend);
          mix(
            &mut values.softness,
            setup.softness,
            softness_key,
            alpha,
            blend,
          );
          // The switches are stepped, as attachment keys are
          if direction == MixDirection::In {
            values.bend = key.bend;
            values.compress = key.compress;
            values.stretch = key.stretch;
          }
        }
      }
      Timeline::TransformMix { constraint, frames } => {
        if let Some(key) = sample_transform_mix(frames, time)
          && let Some((Constraint::Transform(transform), ConstraintValues::Transform(values))) =
            constraint_mut(data, pose, *constraint)
        {
          let setup = &transform.setup.mix;
          mix(&mut values.rotate, setup.rotate, key.rotate, alpha, blend);
          mix(&mut values.x, setup.x, key.x, alpha, blend);
          mix(&mut values.y, setup.y, key.y, alpha, blend);
          mix(
            &mut values.scale_x,
            setup.scale_x,
            key.scale_x,
            alpha,
            blend,
          );
          mix(
            &mut values.scale_y,
            setup.scale_y,
            key.scale_y,
            alpha,
            blend,
          );
          mix(
            &mut values.shear_y,
            setup.shear_y,
            key.shear_y,
            alpha,
            blend,
          );
        }
      }
      Timeline::Spring { constraint, frames } => {
        if let Some(key) = sample_spring(frames, time)
          && let Some((Constraint::Spring(spring), ConstraintValues::Spring(values))) =
            constraint_mut(data, pose, *constraint)
        {
          let setup = &spring.setup;
          mix(&mut values.mix, setup.mix, key.mix, alpha, blend);
          mix(&mut values.wind, setup.wind, key.wind, alpha, blend);
          mix(
            &mut values.gravity,
            setup.gravity,
            key.gravity,
            alpha,
            blend,
          );
        }
      }
      Timeline::Deform { .. } => {
        if alpha > 0.0 {
          deforms.push(DeformStep {
            animation: place,
            timeline: index,
            time,
            alpha,
            blend,
          });
        }
      }
      Timeline::Event { .. } => {}
    }
  }
}

/// A deform timeline as one entry applies it in a step: the animation's
/// place in the data and the timeline's in the animation, the animation's
/// time, and the alpha (the entry's, times the timeline's fade) and blend
/// its key is taken by
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DeformStep {
  pub(crate) animation: usize,
  pub(crate) timeline: usize,
  pub(crate) time: f32,
  pub(crate) alpha: f32,
  pub(crate) blend: MixBlend,
}

/// Why a deform timeline moved nothing though its slot shows the
/// attachment it names
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Misfit {
  /// What the slot shows under the name is not a mesh
  NotAMesh,
  /// The keys hold another count of positions than the mesh
  Positions { keys: usize, mesh: usize },
}

/// Blends timeline `step.timeline` of `animation`, a deform timeline, into
/// its slot's offsets in `pose`, whose slots show what the step leaves them
/// showing. `worn` finds an attachment by its slot and name in the skins
/// the skeleton wears.
///
/// It moves the mesh the slot shows under the timeline's name when that
/// mesh holds as many positions as the keys (keys that are all empty fit
/// any mesh): replace moves the offsets toward the key's by the alpha, add
/// adds the key's times the alpha, the key between two keys being eased by
/// the first one's curve. Before its first key it does nothing.
///
/// Otherwise nothing moves. A slot that shows another name, or nothing
/// (hidden, or a name no skin worn has in it), is passed over: that is
/// ordinary play. One that shows the timeline's name, where the attachment
/// is not a mesh or is a mesh of another count of positions, is a
/// [`Misfit`] for the caller to tell
pub(crate) fn apply_deform<'a>(
  animation: &Animation,
  step: DeformStep,
  pose: &mut Pose,
  worn: impl FnOnce(SlotId, &str) -> Option<&'a Attachment>,
) -> Option<Misfit> {
  let Some(Timeline::Deform {
    slot,
    attachment,
    frames,
  }) = animation.timelines.get(step.timeline)
  else {
    return None;
  };
  let sample = locate(frames, step.time)?;
  let shown = pose.slot_attachments.get(slot.index())?.as_deref()?;
  if shown != &**attachment {
    return None;
  }
  let Attachment::Mesh(mesh) = worn(*slot, attachment)? else {
    return Some(Misfit::NotAMesh);
  };
  let (keys, width) = (deform_width(frames), mesh.position_count() * 2);
  if keys != 0 && keys != width {
    return Some(Misfit::Positions {
      keys: keys / 2,
      mesh: mesh.position_count(),
    });
  }
  let (from, to, eased) = match sample {
    Sample::Hold(index) => (frames.get(index)?, frames.get(index)?, 0.0),
    Sample::Between(index, eased) => (frames.get(index)?, frames.get(index + 1)?, eased),
  };
  let offsets = pose.deform_mut(slot.index())?;
  mix_deform(
    offsets,
    width,
    (&from.1, &to.1, eased),
    step.alpha,
    step.blend,
  );
  None
}

/// Blends a deform key into a slot's offsets, `width` floats that are all 0
/// while the list is empty. The key is `eased` of the way from one list to
/// another, each `width` floats or empty (all 0)
fn mix_deform(
  offsets: &mut Vec<f32>,
  width: usize,
  (from, to, eased): (&[f32], &[f32], f32),
  alpha: f32,
  blend: MixBlend,
) {
  if offsets.len() != width {
    // Nothing has moved them in this step, and a key of no offsets moves
    // them nowhere: the list stays empty
    offsets.clear();
    if from.is_empty() && to.is_empty() {
      return;
    }
    offsets.resize(width, 0.0);
  }
  let value = |list: &[f32], index: usize| list.get(index).copied().unwrap_or(0.0);
  for (index, offset) in offsets.iter_mut().enumerate() {
    let (from, to) = (value(from, index), value(to, index));
    let key = from + (to - from) * eased;
    *offset += match blend {
      MixBlend::Replace => (key - *offset) * alpha,
      MixBlend::Add => key * alpha,
    };
  }
}

/// The most loops one span visits. A step over more loops than this fires
/// only the keys of the last [`MAX_LOOPS`] loops, those ending at the one
/// holding `now`
pub(super) const MAX_LOOPS: i64 = 1000;

/// An event key crossed: its time unwrapped (in the same time as the span),
/// the event's index, the key's values and the key's own time in the
/// animation
pub(crate) type CrossedKey = (f32, usize, EventValues, f32);

/// Pushes onto `out` the event keys in (`last`, `now`], in time order, with
/// their times unwrapped (in the same time as `last` and `now`). A looping
/// animation's keys repeat every duration, from time 0; a zero-length one
/// fires its keys once, as if not looping. The first step passes a negative
/// `last`, so a key at 0 fires on it. Nothing fires if `now <= last`
#[cfg(test)]
pub(crate) fn events_between(
  animation: &Animation,
  last: f32,
  now: f32,
  looping: bool,
  out: &mut Vec<CrossedKey>,
) {
  events_crossed(animation, last, now, looping, false, out);
}

/// As [`events_between`], playing the animation backward when `reverse`:
/// a key at `k` is crossed at `duration - k` into each loop
pub(crate) fn events_crossed(
  animation: &Animation,
  last: f32,
  now: f32,
  looping: bool,
  reverse: bool,
  out: &mut Vec<CrossedKey>,
) {
  if now.is_nan() || last.is_nan() || now <= last {
    return;
  }
  let Some(frames) = animation
    .timelines
    .iter()
    .find_map(|timeline| match timeline {
      Timeline::Event { frames } => Some(frames),
      _ => None,
    })
  else {
    return;
  };
  let duration = animation.duration;
  let start = out.len();
  let fire = |offset: f32, out: &mut Vec<_>| {
    for (time, event, values) in frames {
      let into = if reverse { duration - time } else { *time };
      let at = offset + into;
      if at > last && at <= now {
        out.push((at, *event, values.clone(), *time));
      }
    }
  };
  // Durations are checked finite and not negative
  if !looping || duration <= 0.0 {
    fire(0.0, out);
  } else {
    // Loops start at multiples of the duration. Division can round the
    // loop holding `last` or `now` across a boundary (2.7 / 0.9 is just
    // under 3 in f32), so one more loop is visited on each side; the
    // (last, now] test drops what they do not hold. Each key's unwrapped
    // time is computed the same way every step, so contiguous steps fire
    // it exactly once. Float-to-int casts saturate, and the sums saturate,
    // so an infinite or huge `now` cannot overflow
    let first = (last.max(0.0) / duration).floor() as i64;
    let end = (now / duration).floor() as i64;
    let first = if end.saturating_sub(first) >= MAX_LOOPS {
      end - (MAX_LOOPS - 1)
    } else {
      first.saturating_sub(1).max(0)
    };
    // The range is at most MAX_LOOPS + 2 long; the bound holds it there
    // whatever the arithmetic above gives
    for index in (first..=end.saturating_add(1)).take(MAX_LOOPS as usize + 2) {
      fire(index as f32 * duration, out);
    }
  }
  // A key at the duration and the next loop's key at 0 can come out one ulp
  // apart in either order (and reversed keys come in reverse); a stable
  // insertion pass puts them in time order without allocating
  let pushed = &mut out[start..];
  for index in 1..pushed.len() {
    let mut at = index;
    while at > 0 && pushed[at - 1].0 > pushed[at].0 {
      pushed.swap(at - 1, at);
      at -= 1;
    }
  }
}

/// The bone's setup data and its local pose
fn bone_mut<'a>(
  data: &'a SkeletonData,
  pose: &'a mut Pose,
  bone: BoneId,
) -> Option<(&'a BoneData, &'a mut BonePose)> {
  Some((
    data.bones().get(bone.index())?,
    pose.bones.get_mut(bone.index())?,
  ))
}

/// A constraint's data and its values in the pose
fn constraint_mut<'a>(
  data: &'a SkeletonData,
  pose: &'a mut Pose,
  constraint: ConstraintId,
) -> Option<(&'a Constraint, &'a mut ConstraintValues)> {
  Some((
    data.constraints().get(constraint.index())?,
    pose.constraints.get_mut(constraint.index())?,
  ))
}

/// Blends one bone or constraint value: replace moves it toward `target` by
/// alpha, add adds the target's change from setup, times alpha
fn mix(value: &mut f32, setup: f32, target: f32, alpha: f32, blend: MixBlend) {
  *value += match blend {
    MixBlend::Replace => (target - *value) * alpha,
    MixBlend::Add => (target - setup) * alpha,
  };
}

/// An angle difference wrapped to -pi..=pi: the short way round
pub(super) fn shortest(difference: f32) -> f32 {
  difference - TAU * (difference / TAU).round()
}

/// Where `time` falls among value keys: holding one key's value, or between
/// a key and the next at an eased fraction. `None` before the first key
enum Sample {
  Hold(usize),
  Between(usize, f32),
}

fn locate<T>(frames: &[Frame<T>], time: f32) -> Option<Sample> {
  let after = frames.partition_point(|frame| frame.0 <= time);
  let index = after.checked_sub(1)?;
  let (Some(key), Some(next)) = (frames.get(index), frames.get(after)) else {
    return Some(Sample::Hold(index));
  };
  // Key times strictly increase, so the span is positive
  let fraction = (time - key.0) / (next.0 - key.0);
  Some(Sample::Between(index, key.2.value(fraction)))
}

fn sample_f32(frames: &[Frame<f32>], time: f32) -> Option<f32> {
  Some(match locate(frames, time)? {
    Sample::Hold(index) => frames[index].1,
    Sample::Between(index, eased) => {
      let (from, to) = (frames[index].1, frames[index + 1].1);
      from + (to - from) * eased
    }
  })
}

fn sample_vec2(frames: &[Frame<Vec2>], time: f32) -> Option<Vec2> {
  Some(match locate(frames, time)? {
    Sample::Hold(index) => frames[index].1,
    Sample::Between(index, eased) => frames[index].1.lerp(frames[index + 1].1, eased),
  })
}

/// An IK key at `time`: the key at or before it, whose switches hold until
/// the next key, and its `mix` and `softness` eased toward the next key's
fn sample_ik(frames: &[Frame<IkValues>], time: f32) -> Option<(IkValues, f32, f32)> {
  Some(match locate(frames, time)? {
    Sample::Hold(index) => {
      let key = frames[index].1;
      (key, key.mix, key.softness)
    }
    Sample::Between(index, eased) => {
      let (from, to) = (frames[index].1, frames[index + 1].1);
      let mix = from.mix + (to.mix - from.mix) * eased;
      let softness = from.softness + (to.softness - from.softness) * eased;
      (from, mix, softness)
    }
  })
}

/// A transform constraint's mixes at `time`, each eased toward the next
/// key's
fn sample_transform_mix(frames: &[Frame<TransformMix>], time: f32) -> Option<TransformMix> {
  Some(match locate(frames, time)? {
    Sample::Hold(index) => frames[index].1,
    Sample::Between(index, eased) => {
      let (from, to) = (frames[index].1, frames[index + 1].1);
      let lerp = |a: f32, b: f32| a + (b - a) * eased;
      TransformMix {
        rotate: lerp(from.rotate, to.rotate),
        x: lerp(from.x, to.x),
        y: lerp(from.y, to.y),
        scale_x: lerp(from.scale_x, to.scale_x),
        scale_y: lerp(from.scale_y, to.scale_y),
        shear_y: lerp(from.shear_y, to.shear_y),
      }
    }
  })
}

/// A spring constraint's values at `time`, each eased toward the next key's
fn sample_spring(frames: &[Frame<SpringValues>], time: f32) -> Option<SpringValues> {
  Some(match locate(frames, time)? {
    Sample::Hold(index) => frames[index].1,
    Sample::Between(index, eased) => {
      let (from, to) = (frames[index].1, frames[index + 1].1);
      let lerp = |a: f32, b: f32| a + (b - a) * eased;
      SpringValues {
        mix: lerp(from.mix, to.mix),
        wind: lerp(from.wind, to.wind),
        gravity: lerp(from.gravity, to.gravity),
      }
    }
  })
}

/// A colour key's value, 0 to 1 per component, interpolated in sRGB
fn sample_color(frames: &[Frame<Color>], time: f32) -> Option<Vec4> {
  Some(match locate(frames, time)? {
    Sample::Hold(index) => srgb(frames[index].1),
    Sample::Between(index, eased) => srgb(frames[index].1).lerp(srgb(frames[index + 1].1), eased),
  })
}

/// The last stepped key at or before `time`, or `None` before the first
fn stepped_key<T>(frames: &[(f32, T)], time: f32) -> Option<usize> {
  frames
    .partition_point(|frame| frame.0 <= time)
    .checked_sub(1)
}

#[cfg(test)]
mod tests {
  use crate::core::skeleton::IkBend;
  use std::f32::consts::{PI, TAU};
  use std::sync::Arc;

  use super::*;
  use crate::core::ecs::Vec2;
  use crate::core::skeleton::{
    BoneSetup, Curve, EventData, IkSetup, SlotId, SlotSetup, SpringSetup, Timeline, TransformSetup,
  };
  use crate::core::ui::Color;

  /// A root bone at (1, 2), turned 0.1, scaled (2, 1), sheared (0.2, 0.3),
  /// and three slots on it: "back" showing "hand", "middle" and "front"
  fn data() -> SkeletonData {
    SkeletonData::builder()
      .bone(
        "root",
        None,
        BoneSetup {
          x: 1.0,
          y: 2.0,
          rotation: 0.1,
          scale_x: 2.0,
          shear_x: 0.2,
          shear_y: 0.3,
          ..BoneSetup::default()
        },
      )
      .slot(
        "back",
        "root",
        SlotSetup {
          attachment: Some(Arc::from("hand")),
          ..SlotSetup::default()
        },
      )
      .slot("middle", "root", SlotSetup::default())
      .slot("front", "root", SlotSetup::default())
      .event(EventData::new("step"))
      .build()
      .expect("a valid skeleton")
  }

  fn animation(duration: f32, timelines: Vec<Timeline>) -> Animation {
    Animation {
      name: Arc::from("test"),
      duration,
      timelines,
    }
  }

  const ROOT: crate::core::skeleton::BoneId = crate::core::skeleton::BoneId(0);

  fn linear<T>(keys: Vec<(f32, T)>) -> Vec<(f32, T, Curve)> {
    keys
      .into_iter()
      .map(|(time, value)| (time, value, Curve::Linear))
      .collect()
  }

  fn run(
    animation: &Animation,
    pose: &mut Pose,
    time: f32,
    alpha: f32,
    blend: MixBlend,
    direction: MixDirection,
  ) {
    apply(animation, &data(), pose, time, alpha, blend, direction);
  }

  fn close(actual: f32, expected: f32) {
    assert!(
      (actual - expected).abs() < 1e-5,
      "expected {expected}, got {actual}"
    );
  }

  fn close_colour(actual: Vec4, expected: Vec4) {
    assert!(
      actual.abs_diff_eq(expected, 1e-5),
      "expected {expected}, got {actual}"
    );
  }

  fn events(animation: &Animation, last: f32, now: f32, looping: bool) -> Vec<f32> {
    let mut out = Vec::new();
    events_between(animation, last, now, looping, &mut out);
    out.iter().map(|event| event.0).collect()
  }

  fn event_keys(times: &[f32]) -> Timeline {
    Timeline::Event {
      frames: times
        .iter()
        .map(|&time| (time, 0, EventValues::default()))
        .collect(),
    }
  }

  #[test]
  fn linear_stepped_and_bezier_sample_known_values() {
    close(Curve::Linear.value(0.25), 0.25);
    close(Curve::Stepped.value(0.0), 0.0);
    close(Curve::Stepped.value(0.999), 0.0);
    close(Curve::Stepped.value(1.0), 1.0);
    // CSS's "ease"
    let ease = Curve::Bezier {
      cx1: 0.25,
      cy1: 0.1,
      cx2: 0.25,
      cy2: 1.0,
    };
    assert!(
      (ease.value(0.5) - 0.8024).abs() < 1e-3,
      "{}",
      ease.value(0.5)
    );
    assert_eq!(ease.value(0.0), 0.0);
    assert_eq!(ease.value(1.0), 1.0);
    // Overshooting y controls, a flat-x start: still exact at the ends
    let steep = Curve::Bezier {
      cx1: 0.0,
      cy1: -0.5,
      cx2: 1.0,
      cy2: 1.5,
    };
    assert_eq!(steep.value(0.0), 0.0);
    assert_eq!(steep.value(1.0), 1.0);
    close(steep.value(0.5), 0.5);

    // In a timeline: the curve eases from a key to the next, and each key's
    // value is hit exactly
    let walk = animation(
      2.0,
      vec![Timeline::Rotate {
        bone: ROOT,
        frames: vec![
          (0.0, 0.0, ease),
          (1.0, 1.0, Curve::Stepped),
          (2.0, 3.0, Curve::Linear),
        ],
      }],
    );
    let data = data();
    let at = |time: f32| {
      let mut pose = Pose::setup(&data);
      apply(
        &walk,
        &data,
        &mut pose,
        time,
        1.0,
        MixBlend::Replace,
        MixDirection::In,
      );
      pose.bones[0].rotation - 0.1
    };
    assert!((at(0.5) - 0.8024).abs() < 1e-3);
    close(at(0.0), 0.0);
    close(at(1.0), 1.0);
    close(at(1.5), 1.0);
    close(at(2.0), 3.0);
  }

  #[test]
  fn a_rotate_timeline_blends_by_alpha() {
    let turn = animation(
      1.0,
      vec![Timeline::Rotate {
        bone: ROOT,
        frames: linear(vec![(0.0, 0.0), (1.0, 1.0)]),
      }],
    );
    let mut pose = Pose::setup(&data());
    // Target: setup 0.1 + key 0.5 = 0.6; half way from 0.1 is 0.35
    run(
      &turn,
      &mut pose,
      0.5,
      0.5,
      MixBlend::Replace,
      MixDirection::In,
    );
    close(pose.bones[0].rotation, 0.35);
    // Alpha 1 reaches it; alpha 0 leaves the pose
    run(
      &turn,
      &mut pose,
      0.5,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    close(pose.bones[0].rotation, 0.6);
    pose.bones[0].rotation = 0.2;
    run(
      &turn,
      &mut pose,
      0.5,
      0.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    close(pose.bones[0].rotation, 0.2);
  }

  #[test]
  fn rotation_blends_the_short_way_round() {
    let degrees = |d: f32| d.to_radians();
    // A key at -170 degrees from setup (0.1 rad), from a pose at 170: the
    // short way is through 180, not through 0
    let turn = animation(
      1.0,
      vec![Timeline::Rotate {
        bone: ROOT,
        frames: linear(vec![(0.0, degrees(-170.0) - 0.1)]),
      }],
    );
    let mut pose = Pose::setup(&data());
    pose.bones[0].rotation = degrees(170.0);
    run(
      &turn,
      &mut pose,
      0.0,
      0.5,
      MixBlend::Replace,
      MixDirection::In,
    );
    let off = pose.bones[0].rotation - PI;
    close(off - TAU * (off / TAU).round(), 0.0);
    // Keys themselves interpolate as written: 0 to 2 pi is a full turn, so a
    // quarter of the way is a quarter turn
    let spin = animation(
      1.0,
      vec![Timeline::Rotate {
        bone: ROOT,
        frames: linear(vec![(0.0, 0.0), (1.0, TAU)]),
      }],
    );
    let mut pose = Pose::setup(&data());
    run(
      &spin,
      &mut pose,
      0.25,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    close(pose.bones[0].rotation, 0.1 + TAU / 4.0);
  }

  #[test]
  fn translate_scale_and_shear_blend_by_alpha_combined_and_per_axis() {
    let combined = animation(
      1.0,
      vec![
        Timeline::Translate {
          bone: ROOT,
          frames: linear(vec![(0.0, Vec2::new(4.0, -2.0))]),
        },
        Timeline::Scale {
          bone: ROOT,
          frames: linear(vec![(0.0, Vec2::new(1.5, 3.0))]),
        },
        Timeline::Shear {
          bone: ROOT,
          frames: linear(vec![(0.0, Vec2::new(0.4, -0.2))]),
        },
      ],
    );
    let per_axis = animation(
      1.0,
      vec![
        Timeline::TranslateX {
          bone: ROOT,
          frames: linear(vec![(0.0, 4.0)]),
        },
        Timeline::TranslateY {
          bone: ROOT,
          frames: linear(vec![(0.0, -2.0)]),
        },
        Timeline::ScaleX {
          bone: ROOT,
          frames: linear(vec![(0.0, 1.5)]),
        },
        Timeline::ScaleY {
          bone: ROOT,
          frames: linear(vec![(0.0, 3.0)]),
        },
        Timeline::ShearX {
          bone: ROOT,
          frames: linear(vec![(0.0, 0.4)]),
        },
        Timeline::ShearY {
          bone: ROOT,
          frames: linear(vec![(0.0, -0.2)]),
        },
      ],
    );
    for clip in [&combined, &per_axis] {
      let mut pose = Pose::setup(&data());
      run(
        clip,
        &mut pose,
        0.0,
        0.5,
        MixBlend::Replace,
        MixDirection::In,
      );
      let bone = pose.bones[0];
      // Translate and shear are offsets from setup, scale a factor of it:
      // targets (5, 0), (3, 3), (0.6, 0.1), half way from (1, 2), (2, 1),
      // (0.2, 0.3)
      close(bone.x, 3.0);
      close(bone.y, 1.0);
      close(bone.scale_x, 2.5);
      close(bone.scale_y, 2.0);
      close(bone.shear_x, 0.4);
      close(bone.shear_y, 0.2);
    }
  }

  #[test]
  fn an_add_blend_adds_to_the_pose() {
    let clip = animation(
      1.0,
      vec![
        Timeline::Rotate {
          bone: ROOT,
          frames: linear(vec![(0.0, 0.2)]),
        },
        Timeline::Translate {
          bone: ROOT,
          frames: linear(vec![(0.0, Vec2::new(4.0, -2.0))]),
        },
        Timeline::ScaleX {
          bone: ROOT,
          frames: linear(vec![(0.0, 1.5)]),
        },
        Timeline::ShearY {
          bone: ROOT,
          frames: linear(vec![(0.0, 0.4)]),
        },
      ],
    );
    let mut pose = Pose::setup(&data());
    // As a lower track left it
    pose.bones[0].rotation = 0.3;
    pose.bones[0].x = 5.0;
    pose.bones[0].y = 5.0;
    pose.bones[0].scale_x = 3.0;
    pose.bones[0].shear_y = 1.0;
    run(&clip, &mut pose, 0.0, 0.5, MixBlend::Add, MixDirection::In);
    let bone = pose.bones[0];
    // Each adds its change from setup, times alpha 0.5:
    // rotate 0.2, translate (4, -2), scale 2 * 1.5 - 2 = 1, shear 0.4
    close(bone.rotation, 0.4);
    close(bone.x, 7.0);
    close(bone.y, 4.0);
    close(bone.scale_x, 3.5);
    close(bone.shear_y, 1.2);
    // Untouched properties stay
    close(bone.scale_y, 1.0);
  }

  #[test]
  fn keys_hold_after_the_last_and_do_nothing_before_the_first() {
    let late = animation(
      2.0,
      vec![
        Timeline::Rotate {
          bone: ROOT,
          frames: linear(vec![(0.5, 1.0), (1.0, 2.0)]),
        },
        Timeline::Color {
          slot: SlotId(1),
          frames: linear(vec![(0.5, Color::RED)]),
        },
      ],
    );
    let mut pose = Pose::setup(&data());
    pose.bones[0].rotation = 5.0;
    run(
      &late,
      &mut pose,
      0.25,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    close(pose.bones[0].rotation, 5.0);
    assert_eq!(pose.slot_colors[1], Vec4::ONE);
    run(
      &late,
      &mut pose,
      1.75,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    close(pose.bones[0].rotation, 2.1);
    assert_eq!(pose.slot_colors[1], srgb(Color::RED));
  }

  #[test]
  fn value_timelines_ignore_the_mix_direction() {
    let clip = animation(
      1.0,
      vec![
        Timeline::Rotate {
          bone: ROOT,
          frames: linear(vec![(0.0, 1.0)]),
        },
        Timeline::Color {
          slot: SlotId(0),
          frames: linear(vec![(0.0, Color::BLACK)]),
        },
      ],
    );
    let mut pose = Pose::setup(&data());
    run(
      &clip,
      &mut pose,
      0.0,
      1.0,
      MixBlend::Replace,
      MixDirection::Out,
    );
    close(pose.bones[0].rotation, 1.1);
    assert_eq!(pose.slot_colors[0], srgb(Color::BLACK));
  }

  #[test]
  fn attachment_keys_apply_only_when_mixing_in() {
    let clip = animation(
      1.0,
      vec![Timeline::Attachment {
        slot: SlotId(0),
        frames: vec![(0.0, Some(Arc::from("fist"))), (0.5, None)],
      }],
    );
    let hand = Some(Arc::<str>::from("hand"));
    let fist = Some(Arc::<str>::from("fist"));
    let mut pose = Pose::setup(&data());
    run(
      &clip,
      &mut pose,
      0.25,
      1.0,
      MixBlend::Replace,
      MixDirection::Out,
    );
    assert_eq!(pose.slot_attachments[0], hand);
    // Mixing in, a key applies whatever the alpha and blend: keys are
    // stepped and cannot be part-applied
    run(
      &clip,
      &mut pose,
      0.25,
      0.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    assert_eq!(pose.slot_attachments[0], fist);
    run(&clip, &mut pose, 0.75, 0.5, MixBlend::Add, MixDirection::In);
    assert_eq!(pose.slot_attachments[0], None);
    // Before the first key a key does nothing
    let late = animation(
      1.0,
      vec![Timeline::Attachment {
        slot: SlotId(0),
        frames: vec![(0.5, None)],
      }],
    );
    let mut pose = Pose::setup(&data());
    run(
      &late,
      &mut pose,
      0.25,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    assert_eq!(pose.slot_attachments[0], hand);
  }

  #[test]
  fn draw_order_keys_reorder_slots() {
    let clip = animation(
      1.0,
      vec![Timeline::DrawOrder {
        frames: vec![
          (0.0, Some(vec![SlotId(2), SlotId(0), SlotId(1)])),
          (1.0, None),
        ],
      }],
    );
    let setup = vec![SlotId(0), SlotId(1), SlotId(2)];
    let mut pose = Pose::setup(&data());
    run(
      &clip,
      &mut pose,
      0.5,
      1.0,
      MixBlend::Replace,
      MixDirection::Out,
    );
    assert_eq!(pose.draw_order, setup);
    run(
      &clip,
      &mut pose,
      0.5,
      0.25,
      MixBlend::Replace,
      MixDirection::In,
    );
    assert_eq!(pose.draw_order, vec![SlotId(2), SlotId(0), SlotId(1)]);
    // A `None` key is the setup order
    run(
      &clip,
      &mut pose,
      1.0,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    assert_eq!(pose.draw_order, setup);
  }

  #[test]
  fn color_keys_tint_the_slot() {
    let clip = animation(
      1.0,
      vec![Timeline::Color {
        slot: SlotId(0),
        frames: linear(vec![
          (0.0, Color::new(255, 0, 0, 255)),
          (1.0, Color::new(0, 0, 255, 0)),
        ]),
      }],
    );
    // Half way, each component interpolated in sRGB (not linear light),
    // kept in float
    let mut pose = Pose::setup(&data());
    run(
      &clip,
      &mut pose,
      0.5,
      1.0,
      MixBlend::Replace,
      MixDirection::In,
    );
    close_colour(pose.slot_colors[0], Vec4::new(0.5, 0.0, 0.5, 0.5));
    // Alpha blends from the pose's colour (white) toward red
    let mut pose = Pose::setup(&data());
    run(
      &clip,
      &mut pose,
      0.0,
      0.5,
      MixBlend::Replace,
      MixDirection::In,
    );
    close_colour(pose.slot_colors[0], Vec4::new(1.0, 0.5, 0.5, 1.0));
    // Colours do not add: an add blend blends as replace does
    let mut pose = Pose::setup(&data());
    run(&clip, &mut pose, 0.0, 0.5, MixBlend::Add, MixDirection::In);
    close_colour(pose.slot_colors[0], Vec4::new(1.0, 0.5, 0.5, 1.0));
    // Nothing rounds between layers: ten tiny blends toward red move the
    // green from 1 to 0.999^10, which 8-bit storage would stall at 255
    let mut pose = Pose::setup(&data());
    for _ in 0..10 {
      run(
        &clip,
        &mut pose,
        0.0,
        0.001,
        MixBlend::Replace,
        MixDirection::In,
      );
    }
    close_colour(
      pose.slot_colors[0],
      Vec4::new(1.0, 0.999_f32.powi(10), 0.999_f32.powi(10), 1.0),
    );
  }

  #[test]
  fn events_between_crosses_loops_once_each() {
    let clip = animation(1.0, vec![event_keys(&[0.25, 0.75])]);
    let close_all = |actual: Vec<f32>, expected: &[f32]| {
      assert_eq!(actual.len(), expected.len(), "{actual:?}");
      for (a, e) in actual.iter().zip(expected) {
        close(*a, *e);
      }
    };
    close_all(events(&clip, 0.6, 2.3, true), &[0.75, 1.25, 1.75, 2.25]);
    // The same span in two steps: each key once
    let mut both = events(&clip, 0.6, 1.25, true);
    both.extend(events(&clip, 1.25, 2.3, true));
    close_all(both, &[0.75, 1.25, 1.75, 2.25]);
    // Not looping, only the animation's own keys
    close_all(events(&clip, 0.6, 2.3, false), &[0.75]);
    // Values and indices come with the times
    let mut out = Vec::new();
    events_between(&clip, 0.0, 0.5, true, &mut out);
    assert_eq!(out, vec![(0.25, 0, EventValues::default(), 0.25)]);
    // A step that goes nowhere fires nothing
    close_all(events(&clip, 0.25, 0.25, true), &[]);
  }

  #[test]
  fn an_event_at_zero_fires_on_the_first_step_and_each_loop() {
    let clip = animation(1.0, vec![event_keys(&[0.0])]);
    // The first step passes a negative `last`, so (last, now] holds 0
    assert_eq!(events(&clip, -1.0, 0.0, true), vec![0.0]);
    assert_eq!(events(&clip, 0.0, 0.5, true), Vec::<f32>::new());
    assert_eq!(events(&clip, 0.5, 1.0, true), vec![1.0]);
    assert_eq!(events(&clip, 1.0, 3.5, true), vec![2.0, 3.0]);
    // Not looping, once
    assert_eq!(events(&clip, -1.0, 0.1, false), vec![0.0]);
    assert_eq!(events(&clip, 0.1, 3.0, false), Vec::<f32>::new());
    // A zero-length looping animation fires its key once, not forever
    let instant = animation(0.0, vec![event_keys(&[0.0])]);
    assert_eq!(events(&instant, -1.0, 5.0, true), vec![0.0]);
  }

  /// The float one ulp below `value` (a positive float)
  fn ulp_below(value: f32) -> f32 {
    f32::from_bits(value.to_bits() - 1)
  }

  #[test]
  fn a_step_ending_exactly_on_a_loop_boundary_fires_its_key() {
    // 2.7 / 0.9 rounds to just under 3 in f32, so the loop holding `now`
    // must not be found by division alone
    let clip = animation(0.9, vec![event_keys(&[0.0])]);
    let now = 3.0 * 0.9;
    assert_eq!(events(&clip, 2.0, now, true), vec![now]);
    assert_eq!(events(&clip, now, now + 0.1, true), Vec::<f32>::new());
  }

  #[test]
  fn a_step_starting_just_below_a_loop_boundary_keeps_the_key_at_the_duration() {
    // A key at the duration, ending loop 8 at 9 x 0.1. `last` one ulp below
    // that rounds up to loop 9 by division; across the two steps the key
    // fires once
    let duration = 0.1;
    let clip = animation(duration, vec![event_keys(&[duration])]);
    let last = ulp_below(9.0 * duration);
    let mut fired = events(&clip, 0.75, last, true);
    fired.extend(events(&clip, last, 1.0, true));
    let near_nine_tenths = fired
      .iter()
      .filter(|&&time| (time - 0.9).abs() < 1e-4)
      .count();
    assert_eq!(near_nine_tenths, 1, "{fired:?}");
  }

  #[test]
  fn small_steps_across_twenty_loops_fire_every_key_once() {
    for duration in [0.1_f32, 0.3, 0.7, 0.9] {
      let keys = [0.0, duration * 0.5, duration];
      let clip = animation(duration, vec![event_keys(&keys)]);
      // Steps landing exactly on loop boundaries, and steps that never do
      let on_boundaries: Vec<f32> = (0..=80).map(|i| i as f32 * duration / 4.0).collect();
      let mut drifting = vec![0.0_f32];
      while *drifting.last().unwrap_or(&0.0) < 20.0 * duration {
        let next = drifting.last().unwrap_or(&0.0) + duration / 7.0;
        drifting.push(next);
      }
      for nows in [on_boundaries, drifting] {
        let mut fired = Vec::new();
        let mut last = -1.0;
        for &now in &nows {
          events_between(&clip, last, now, true, &mut fired);
          last = now;
        }
        let end = last;
        // Every (loop, key) at or before the end, each once
        for repeat in 0..=21 {
          for (key, &time) in keys.iter().enumerate() {
            let at = repeat as f32 * duration + time;
            let count = fired.iter().filter(|event| event.0 == at).count();
            let expected = usize::from(at <= end);
            // A key at the duration and the next loop's key at 0 may land
            // on one float; count both there
            let twin = (key == 2 && (repeat + 1) as f32 * duration == at)
              || (key == 0 && repeat > 0 && (repeat - 1) as f32 * duration + duration == at);
            let expected = if twin { expected * 2 } else { expected };
            assert_eq!(
              count, expected,
              "duration {duration} loop {repeat} key {key} at {at}"
            );
          }
        }
        assert!(
          fired.windows(2).all(|pair| pair[0].0 <= pair[1].0),
          "{duration}: out of order"
        );
      }
    }
  }

  #[test]
  fn a_span_past_the_loop_cap_fires_only_its_last_thousand_loops() {
    let clip = animation(1.0, vec![event_keys(&[0.5])]);
    let fired = events(&clip, 0.0, 5000.75, true);
    assert_eq!(fired.len(), 1000);
    assert_eq!(fired.first(), Some(&4001.5));
    assert_eq!(fired.last(), Some(&5000.5));
    // A span of exactly the cap fires every loop
    assert_eq!(events(&clip, 0.0, 999.75, true).len(), 1000);
  }

  #[test]
  fn an_infinite_or_huge_span_neither_overflows_nor_runs_long() {
    let clip = animation(1.0, vec![event_keys(&[0.5])]);
    for now in [f32::INFINITY, f32::MAX, 1e30] {
      let fired = events(&clip, 0.0, now, true);
      assert!(fired.len() <= 1000, "{now}: {}", fired.len());
    }
    let tiny = animation(1e-30, vec![event_keys(&[0.0])]);
    assert!(events(&tiny, 0.0, 1.0, true).len() <= 1001);
  }

  /// A root bone and a slot "banner" on it showing "flag": a mesh of three
  /// unweighted vertices, so of three positions
  fn banner() -> SkeletonData {
    use crate::core::skeleton::{MeshAttachment, MeshVertices, Region};
    let corners = vec![Vec2::ZERO, Vec2::X, Vec2::Y];
    let flag = MeshAttachment::new(
      0,
      MeshVertices::Unweighted(corners.clone()),
      corners,
      vec![0, 1, 2],
    );
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .slot(
        "banner",
        "root",
        SlotSetup {
          attachment: Some(Arc::from("flag")),
          ..SlotSetup::default()
        },
      )
      .region(Region {
        name: Arc::from("cloth"),
        image: crate::core::asset::Handle::detached(),
        x: 0,
        y: 0,
        width: 4,
        height: 4,
      })
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        "banner",
        "flag",
        Attachment::Mesh(flag),
      )
      .build()
      .expect("a valid skeleton")
  }

  /// A deform of the banner's flag with these keys
  fn deform(frames: Vec<(f32, Vec<f32>, Curve)>) -> Animation {
    animation(
      1.0,
      vec![Timeline::Deform {
        slot: SlotId(0),
        attachment: Arc::from("flag"),
        frames,
      }],
    )
  }

  /// The banner's six offsets in `pose`: all 0 while nothing has moved them
  fn offsets(pose: &Pose) -> [f32; 6] {
    let moved = pose
      .deforms
      .first()
      .map_or(&[][..], |deform| &deform.offsets);
    if moved.is_empty() {
      return [0.0; 6];
    }
    <[f32; 6]>::try_from(moved).expect("an x and a y for each of the flag's three positions")
  }

  fn close_all(actual: [f32; 6], expected: [f32; 6]) {
    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
      assert!(
        (got - want).abs() < 1e-5,
        "offset {index}: expected {expected:?}, got {actual:?}"
      );
    }
  }

  const KEY: [f32; 6] = [2.0, -4.0, 6.0, 8.0, -10.0, 12.0];
  const NEXT: [f32; 6] = [4.0, 0.0, 6.0, -8.0, 10.0, 0.0];

  #[test]
  fn a_deform_key_moves_its_vertices_by_alpha() {
    let data = banner();
    let wave = deform(vec![
      (0.25, KEY.to_vec(), Curve::Linear),
      (0.75, NEXT.to_vec(), Curve::Linear),
    ]);
    let at = |pose: &mut Pose, time: f32, alpha: f32| {
      let (blend, direction) = (MixBlend::Replace, MixDirection::In);
      apply(&wave, &data, pose, time, alpha, blend, direction);
      offsets(pose)
    };
    // On the setup shape, at full strength: the key's own offsets
    close_all(at(&mut Pose::setup(&data), 0.25, 1.0), KEY);
    // Replace moves each offset toward the key's by the alpha: from none,
    // half of it; and from what a lower layer left, part of the way
    close_all(at(&mut Pose::setup(&data), 0.25, 0.5), KEY.map(|k| k * 0.5));
    let mut pose = Pose::setup(&data);
    let under = [1.0, 1.0, -2.0, 0.0, 30.0, 8.0];
    pose
      .deform_mut(0)
      .expect("the banner")
      .extend_from_slice(&under);
    let quarter: [f32; 6] = std::array::from_fn(|i| under[i] + (KEY[i] - under[i]) * 0.25);
    close_all(at(&mut pose, 0.25, 0.25), quarter);
    // No alpha leaves them as they were, and makes no list in a pose
    // nothing has moved
    close_all(at(&mut pose, 0.25, 0.0), quarter);
    let mut pose = Pose::setup(&data);
    close_all(at(&mut pose, 0.25, 0.0), [0.0; 6]);
    assert!(pose.deforms.is_empty());
    // Between two keys the offsets go from one key's to the next's
    let between = |t: f32| -> [f32; 6] { std::array::from_fn(|i| KEY[i] + (NEXT[i] - KEY[i]) * t) };
    close_all(at(&mut Pose::setup(&data), 0.5, 1.0), between(0.5));
    close_all(at(&mut Pose::setup(&data), 0.375, 1.0), between(0.25));
    // After the last key its offsets hold; before the first nothing moves
    close_all(at(&mut Pose::setup(&data), 0.9, 1.0), NEXT);
    let mut pose = Pose::setup(&data);
    close_all(at(&mut pose, 0.1, 1.0), [0.0; 6]);
    assert!(pose.deforms.iter().all(|deform| deform.offsets.is_empty()));

    // The first key's curve eases the way to the next: stepped holds the
    // first key until the next, and a bezier (CSS's "ease") is 0.8024 of
    // the way at half time
    let eased = |curve: Curve, time: f32| {
      let wave = deform(vec![
        (0.0, KEY.to_vec(), curve),
        (1.0, NEXT.to_vec(), Curve::Linear),
      ]);
      let mut pose = Pose::setup(&data);
      let (blend, direction) = (MixBlend::Replace, MixDirection::In);
      apply(&wave, &data, &mut pose, time, 1.0, blend, direction);
      offsets(&pose)
    };
    close_all(eased(Curve::Stepped, 0.99), KEY);
    close_all(eased(Curve::Stepped, 1.0), NEXT);
    let ease = Curve::Bezier {
      cx1: 0.25,
      cy1: 0.1,
      cx2: 0.25,
      cy2: 1.0,
    };
    let half = eased(ease, 0.5);
    for index in 0..6 {
      let want = KEY[index] + (NEXT[index] - KEY[index]) * 0.8024;
      assert!(
        (half[index] - want).abs() < 0.02,
        "offset {index}: {half:?}"
      );
    }
    assert_ne!(half[0], between(0.5)[0], "not the straight line's value");
  }

  #[test]
  fn an_empty_key_is_the_setup_shape() {
    let data = banner();
    let wave = deform(vec![
      (0.0, Vec::new(), Curve::Linear),
      (0.5, KEY.to_vec(), Curve::Linear),
      (1.0, Vec::new(), Curve::Linear),
    ]);
    let at = |pose: &mut Pose, time: f32, alpha: f32, blend: MixBlend| {
      apply(&wave, &data, pose, time, alpha, blend, MixDirection::In);
      offsets(pose)
    };
    let fresh = || Pose::setup(&data);
    // At an empty key nothing is moved; toward and from one the offsets
    // grow from nothing and shrink to nothing
    close_all(at(&mut fresh(), 0.0, 1.0, MixBlend::Replace), [0.0; 6]);
    close_all(
      at(&mut fresh(), 0.25, 1.0, MixBlend::Replace),
      KEY.map(|k| k * 0.5),
    );
    close_all(at(&mut fresh(), 0.5, 1.0, MixBlend::Replace), KEY);
    close_all(
      at(&mut fresh(), 0.875, 1.0, MixBlend::Replace),
      KEY.map(|k| k * 0.25),
    );
    // Held at the last key, an empty one, it leaves a pose nothing moved
    // with nothing in its list
    let mut pose = fresh();
    close_all(at(&mut pose, 1.0, 1.0, MixBlend::Replace), [0.0; 6]);
    assert!(pose.deforms.iter().all(|deform| deform.offsets.is_empty()));

    // An empty key is offsets of 0, not no key: replace moves what a lower
    // layer left toward the setup shape by the alpha, and add adds nothing
    let under = [1.0, 1.0, -2.0, 0.0, 30.0, 8.0];
    let lower = || {
      let mut pose = fresh();
      pose
        .deform_mut(0)
        .expect("the banner")
        .extend_from_slice(&under);
      pose
    };
    close_all(
      at(&mut lower(), 1.0, 0.25, MixBlend::Replace),
      under.map(|u| u * 0.75),
    );
    close_all(at(&mut lower(), 1.0, 1.0, MixBlend::Replace), [0.0; 6]);
    close_all(at(&mut lower(), 1.0, 1.0, MixBlend::Add), under);
    // Keys that are all empty hold no count of positions, so they fit the
    // flag as they fit any mesh
    let still = deform(vec![(0.0, Vec::new(), Curve::Linear)]);
    let mut pose = lower();
    let (blend, direction) = (MixBlend::Replace, MixDirection::In);
    apply(&still, &data, &mut pose, 0.5, 0.5, blend, direction);
    close_all(offsets(&pose), under.map(|u| u * 0.5));
  }

  #[test]
  fn an_add_blend_adds_a_deform_key_times_its_alpha() {
    let data = banner();
    let wave = deform(vec![(0.0, KEY.to_vec(), Curve::Linear)]);
    let mut pose = Pose::setup(&data);
    let under = [1.0, 1.0, -2.0, 0.0, 30.0, 8.0];
    pose
      .deform_mut(0)
      .expect("the banner")
      .extend_from_slice(&under);
    let (blend, direction) = (MixBlend::Add, MixDirection::Out);
    apply(&wave, &data, &mut pose, 0.0, 0.5, blend, direction);
    close_all(
      offsets(&pose),
      std::array::from_fn(|i| under[i] + KEY[i] * 0.5),
    );
    // Over the setup shape it adds to nothing
    let mut pose = Pose::setup(&data);
    apply(&wave, &data, &mut pose, 0.0, 0.5, blend, direction);
    close_all(offsets(&pose), KEY.map(|k| k * 0.5));
  }

  /// A limb of two bones, "thigh" and "shin", whose IK constraint "leg"
  /// (setup: mix 0.5, softness 2, bend positive) reaches for "foot", and a
  /// "pelvis" whose transform constraint "level" (setup: a rotate mix of
  /// 0.2, the other mixes 0) copies the foot
  fn constrained() -> SkeletonData {
    let long = |x: f32| BoneSetup {
      x,
      length: 10.0,
      ..BoneSetup::default()
    };
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("thigh", Some("root"), long(0.0))
      .bone("shin", Some("thigh"), long(10.0))
      .bone("foot", Some("root"), BoneSetup::default())
      .bone("pelvis", Some("root"), BoneSetup::default())
      .ik(
        "leg",
        &["thigh", "shin"],
        "foot",
        IkSetup {
          mix: 0.5,
          softness: 2.0,
          ..IkSetup::default()
        },
      )
      .transform_constraint(
        "level",
        &["pelvis"],
        "foot",
        TransformSetup {
          mix: TransformMix {
            rotate: 0.2,
            ..TransformMix::default()
          },
          ..TransformSetup::default()
        },
      )
      .build()
      .expect("a valid skeleton")
  }

  const LEG: ConstraintId = ConstraintId(0);
  const LEVEL: ConstraintId = ConstraintId(1);

  fn ik_values(pose: &Pose) -> IkValues {
    match pose.constraints[LEG.index()] {
      ConstraintValues::Ik(values) => values,
      other => panic!("{other:?}"),
    }
  }

  fn mixes(pose: &Pose) -> TransformMix {
    match pose.constraints[LEVEL.index()] {
      ConstraintValues::Transform(mix) => mix,
      other => panic!("{other:?}"),
    }
  }

  fn ik_key(mix: f32, softness: f32, positive: bool, stretch: bool) -> IkValues {
    IkValues {
      mix,
      softness,
      bend: if positive {
        IkBend::Positive
      } else {
        IkBend::Negative
      },
      compress: false,
      stretch,
      max_stretch: None,
    }
  }

  #[test]
  fn an_ik_key_sets_its_constraint_s_values_by_alpha() {
    let data = constrained();
    // From mix 1, softness 4, bent the other way and stretching, to mix 0,
    // softness 0, bent the positive way and not stretching, over a second
    let reach = animation(
      1.0,
      vec![Timeline::Ik {
        constraint: LEG,
        frames: linear(vec![
          (0.0, ik_key(1.0, 4.0, false, true)),
          (1.0, ik_key(0.0, 0.0, true, false)),
        ]),
      }],
    );
    let apply_at = |time: f32, alpha: f32, direction: MixDirection| {
      let mut pose = Pose::setup(&data);
      apply(
        &reach,
        &data,
        &mut pose,
        time,
        alpha,
        MixBlend::Replace,
        direction,
      );
      ik_values(&pose)
    };
    // A quarter of the way: the key is mix 0.75 and softness 3, and the
    // switches are the first key's until the second
    let full = apply_at(0.25, 1.0, MixDirection::In);
    close(full.mix, 0.75);
    close(full.softness, 3.0);
    assert!(full.bend == IkBend::Negative && full.stretch && !full.compress);
    // At half alpha the numbers move half way from the setup's (0.5 and 2)
    let half = apply_at(0.25, 0.5, MixDirection::In);
    close(half.mix, 0.625);
    close(half.softness, 2.5);
    assert!(half.bend == IkBend::Negative && half.stretch);
    // The switches change at their key and not before it
    let before = apply_at(0.999, 1.0, MixDirection::In);
    assert!(before.bend == IkBend::Negative && before.stretch);
    let at = apply_at(1.0, 1.0, MixDirection::In);
    assert!(at.bend == IkBend::Positive && !at.stretch);
    close(at.mix, 0.0);
    // Mixing out, the switches stay the setup's
    let out = apply_at(0.25, 1.0, MixDirection::Out);
    close(out.mix, 0.75);
    assert!(out.bend == IkBend::Positive && !out.stretch);
  }

  #[test]
  fn a_transform_key_sets_its_mixes_by_alpha() {
    let data = constrained();
    let key = TransformMix {
      rotate: 1.0,
      x: 0.5,
      y: -1.0,
      scale_x: 2.0,
      scale_y: 0.0,
      shear_y: 0.4,
    };
    // A stepped key holds until the next, which sets every mix to 0
    let lean = animation(
      1.0,
      vec![Timeline::TransformMix {
        constraint: LEVEL,
        frames: vec![
          (0.0, key, Curve::Stepped),
          (1.0, TransformMix::default(), Curve::Linear),
        ],
      }],
    );
    let apply_at = |time: f32, alpha: f32| {
      let mut pose = Pose::setup(&data);
      apply(
        &lean,
        &data,
        &mut pose,
        time,
        alpha,
        MixBlend::Replace,
        MixDirection::Out,
      );
      mixes(&pose)
    };
    assert_eq!(apply_at(0.5, 1.0), key);
    // At half alpha each mix moves half way from the setup's
    let half = apply_at(0.5, 0.5);
    for (actual, expected) in [
      (half.rotate, 0.6),
      (half.x, 0.25),
      (half.y, -0.5),
      (half.scale_x, 1.0),
      (half.scale_y, 0.0),
      (half.shear_y, 0.2),
    ] {
      close(actual, expected);
    }
    assert_eq!(apply_at(1.0, 1.0), TransformMix::default());
  }

  #[test]
  fn an_add_layer_adds_a_constraint_s_difference_from_setup() {
    let data = constrained();
    let keys = animation(
      1.0,
      vec![
        Timeline::Ik {
          constraint: LEG,
          frames: linear(vec![(0.0, ik_key(1.0, 4.0, false, false))]),
        },
        Timeline::TransformMix {
          constraint: LEVEL,
          frames: linear(vec![(
            0.0,
            TransformMix {
              rotate: 1.0,
              x: -0.5,
              ..TransformMix::default()
            },
          )]),
        },
      ],
    );
    // What the layers below left: mix 0.1 and softness 1, a rotate mix of 0
    let mut pose = Pose::setup(&data);
    pose.constraints[LEG.index()] = ConstraintValues::Ik(ik_key(0.1, 1.0, true, false));
    pose.constraints[LEVEL.index()] = ConstraintValues::Transform(TransformMix::default());
    apply(
      &keys,
      &data,
      &mut pose,
      0.0,
      0.5,
      MixBlend::Add,
      MixDirection::In,
    );
    // Each adds half its difference from the setup (mix 0.5 to 1, softness
    // 2 to 4, rotate 0.2 to 1, x 0 to -0.5)
    let values = ik_values(&pose);
    close(values.mix, 0.35);
    close(values.softness, 2.0);
    let mix = mixes(&pose);
    close(mix.rotate, 0.4);
    close(mix.x, -0.25);
    // The switches are stepped whatever the blend
    assert_eq!(values.bend, IkBend::Negative);
  }

  /// A "hair" bone with a spring "sway" (setup: mix 0.5, wind 2, gravity
  /// 10)
  fn sprung() -> SkeletonData {
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone(
        "hair",
        Some("root"),
        BoneSetup {
          length: 10.0,
          ..BoneSetup::default()
        },
      )
      .spring(
        "sway",
        "hair",
        SpringSetup {
          rotate: 1.0,
          mix: 0.5,
          wind: 2.0,
          gravity: 10.0,
          ..SpringSetup::default()
        },
      )
      .build()
      .expect("a valid skeleton")
  }

  fn spring_values(pose: &Pose) -> SpringValues {
    match pose.constraints[0] {
      ConstraintValues::Spring(values) => values,
      other => panic!("{other:?}"),
    }
  }

  fn spring_key(mix: f32, wind: f32, gravity: f32) -> SpringValues {
    SpringValues { mix, wind, gravity }
  }

  #[test]
  fn a_spring_key_sets_its_values_by_curve_and_alpha() {
    let data = sprung();
    // An eased key from mix 1, wind 6, gravity 0 to mix 0, wind -2, gravity
    // 20 over a second, then a stepped key held to the end
    let ease = Curve::Bezier {
      cx1: 0.5,
      cy1: 0.0,
      cx2: 1.0,
      cy2: 0.5,
    };
    let gust = animation(
      2.0,
      vec![Timeline::Spring {
        constraint: ConstraintId(0),
        frames: vec![
          (0.0, spring_key(1.0, 6.0, 0.0), ease),
          (1.0, spring_key(0.0, -2.0, 20.0), Curve::Stepped),
          (2.0, spring_key(1.0, 0.0, 0.0), Curve::Linear),
        ],
      }],
    );
    let apply_at = |time: f32, alpha: f32, blend: MixBlend| {
      let mut pose = Pose::setup(&data);
      apply(
        &gust,
        &data,
        &mut pose,
        time,
        alpha,
        blend,
        MixDirection::In,
      );
      spring_values(&pose)
    };
    // Half way through the eased key, at its curve's value
    let eased = ease.value(0.5);
    let full = apply_at(0.5, 1.0, MixBlend::Replace);
    close(full.mix, 1.0 - eased);
    close(full.wind, 6.0 - 8.0 * eased);
    close(full.gravity, 20.0 * eased);
    // At half alpha each moves half way from the setup's (0.5, 2, 10)
    let half = apply_at(0.5, 0.5, MixBlend::Replace);
    close(half.mix, 0.5 + (1.0 - eased - 0.5) * 0.5);
    close(half.wind, 2.0 + (6.0 - 8.0 * eased - 2.0) * 0.5);
    close(half.gravity, 10.0 + (20.0 * eased - 10.0) * 0.5);
    // The stepped key holds until the next
    assert_eq!(
      apply_at(1.9, 1.0, MixBlend::Replace),
      spring_key(0.0, -2.0, 20.0)
    );
    // Added, each adds its difference from the setup, times alpha
    let added = apply_at(1.5, 0.5, MixBlend::Add);
    close(added.mix, 0.5 + (0.0 - 0.5) * 0.5);
    close(added.wind, 2.0 + (-2.0 - 2.0) * 0.5);
    close(added.gravity, 10.0 + (20.0 - 10.0) * 0.5);
  }
}
