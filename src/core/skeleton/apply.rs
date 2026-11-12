//! Applying one animation to a pose at a time, by an alpha and a blend, and
//! finding the event keys a step crosses. The module notes give the rule for
//! each timeline kind.

use std::f32::consts::TAU;

use glam::{Vec2, Vec4};

use super::animation::{Animation, EventValues, Frame, Timeline};
use super::data::{BoneData, BoneId, SkeletonData, SlotId};
use super::pose::{BonePose, Pose, srgb};
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

/// Whether an animation is mixing in or out. Attachment and draw-order keys
/// apply only when mixing in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MixDirection {
  In,
  Out,
}

/// Blends `animation` at `time` (its own time: the caller wraps a looping
/// track's time) into `pose`, by `alpha` (0 leaves bone and colour values,
/// 1 takes them fully) and `blend`. Before a timeline's first key it leaves
/// the pose; after its last key the last value holds. Event timelines do
/// nothing here (see [`events_between`])
pub(crate) fn apply(
  animation: &Animation,
  data: &SkeletonData,
  pose: &mut Pose,
  time: f32,
  alpha: f32,
  blend: MixBlend,
  direction: MixDirection,
) {
  for timeline in &animation.timelines {
    match timeline {
      Timeline::Rotate { bone, frames } => {
        if let (Some(value), Some((setup, local))) =
          (sample_f32(frames, time), bone_mut(data, pose, *bone))
        {
          let target = setup.rotation + value;
          local.rotation += match blend {
            MixBlend::Replace => shortest(target - local.rotation) * alpha,
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
            None => pose.draw_order.extend(
              (0..data.slots().len()).filter_map(|index| u8::try_from(index).ok().map(SlotId)),
            ),
          }
        }
      }
      Timeline::Event { .. } => {}
    }
  }
}

/// The most loops one span visits. A step over more loops than this fires
/// only the keys of the last [`MAX_LOOPS`] loops, those ending at the one
/// holding `now`
const MAX_LOOPS: i64 = 1000;

/// Pushes onto `out` the event keys in (`last`, `now`], in time order, with
/// their times unwrapped (in the same time as `last` and `now`). A looping
/// animation's keys repeat every duration, from time 0; a zero-length one
/// fires its keys once, as if not looping. The first step passes a negative
/// `last`, so a key at 0 fires on it. Nothing fires if `now <= last`
pub(crate) fn events_between(
  animation: &Animation,
  last: f32,
  now: f32,
  looping: bool,
  out: &mut Vec<(f32, usize, EventValues)>,
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
      let at = offset + time;
      if at > last && at <= now {
        out.push((at, *event, values.clone()));
      }
    }
  };
  // Durations are checked finite and not negative
  if !looping || duration <= 0.0 {
    fire(0.0, out);
    return;
  }
  // Loops start at multiples of the duration. Division can round the loop
  // holding `last` or `now` across a boundary (2.7 / 0.9 is just under 3 in
  // f32), so one more loop is visited on each side; the (last, now] test
  // drops what they do not hold. Each key's unwrapped time is computed the
  // same way every step, so contiguous steps fire it exactly once
  let first = (last.max(0.0) / duration).floor() as i64;
  let end = (now / duration).floor() as i64;
  let capped = end - first >= MAX_LOOPS;
  let first = if capped {
    end - (MAX_LOOPS - 1)
  } else {
    (first - 1).max(0)
  };
  for index in first..=end + 1 {
    fire(index as f32 * duration, out);
  }
  // A key at the duration and the next loop's key at 0 can come out one ulp
  // apart in either order; a stable insertion pass puts them in time order
  // without allocating (anything else is already in order)
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

/// Blends one bone property: replace moves it toward `target` by alpha, add
/// adds the target's change from setup, times alpha
fn mix(value: &mut f32, setup: f32, target: f32, alpha: f32, blend: MixBlend) {
  *value += match blend {
    MixBlend::Replace => (target - *value) * alpha,
    MixBlend::Add => (target - setup) * alpha,
  };
}

/// An angle difference wrapped to -pi..=pi: the short way round
fn shortest(difference: f32) -> f32 {
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
  use std::f32::consts::{PI, TAU};
  use std::sync::Arc;

  use super::*;
  use crate::core::ecs::Vec2;
  use crate::core::skeleton::{BoneSetup, Curve, EventData, SlotId, SlotSetup, Timeline};
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
    assert_eq!(out, vec![(0.25, 0, EventValues::default())]);
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
}
