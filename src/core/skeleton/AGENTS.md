# Skeleton module notes

Skeletal animation is being rebuilt from scratch. The design and its decisions are in `docs/specs/2026-11-12-skeleton-design.md`.

## Data (`data.rs`, `attachment.rs`, `skin.rs`, `curve.rs`, `animation.rs`)

- `SkeletonData` is immutable once built. Everything goes through `SkeletonDataBuilder` (`builder.rs`), which checks it in `build` (`validate.rs`); the loader builds through it too.
- The builder refers to bones, slots and skins by name. Animations hold `BoneId`/`SlotId` and event indices, and region attachments hold region indices: places in the order added, which `bone_id`, `slot_id`, `region_index` and `event_index` look up.
- `SkeletonError { item, problem }` names the item as `bone "arm"`, `slot "head"`, `region "torso"`, `event "step"`, `skin "red"`, `skin "red" slot "head" attachment "hat"`, `animation "walk"`, `animation "walk" timeline 2 (rotate)`, `mix "idle" to "run"` or `default mix`. The first problem found is reported.
- In the prelude, the module's generic names are prefixed (aliases; the types keep their short names in `ryo2d::core::skeleton`): `SkeletonClip` (`Animation`), `SkeletonCurve`, `SkeletonSkin`, `SkeletonTimeline`, `SkeletonAttachment`, `SkeletonRegion`, `SkeletonEventData`, `SkeletonEventValues`, `SkeletonEventAudio` and `BoneInherit` (`Inherit`).
- Bone timelines key rotate, and translate, scale and shear either combined (`Vec2` keys) or per axis (`TranslateX` … `ShearY`, `f32` keys).
- Colours (`SlotData::color`, `RegionAttachment::color`, colour keys) are 8-bit sRGB `Color`s. Applying a colour timeline interpolates each component in float, in sRGB, as Spine does, and rounds only when the pose's colour is stored.

## Validation rules

Every rule below has its own test in `validate.rs`, which asserts the error's full item and that the problem names the reason.

- Names are unique per kind (bones, slots, regions, events, skins, animations), and so is an attachment name within a skin's slot, and a mix pair.
- Parents come before children. At most 255 bones and 255 slots (`bone_id` and `slot_id` give `None` past the 255th).
- References resolve: parents, slots' bones, attachments' skins, slots and regions, timelines' bones and slots, event keys' events, mixes' animations.
- Values are finite: bone setups, attachments, event defaults and overrides, key times and values, curves, durations and mixes. Durations and mixes are not negative.
- Rules we chose where Spine's docs are silent:
  - The default skin (`SkeletonData::DEFAULT_SKIN`, "default") always exists, first in `skins()`, and may be declared once without being a duplicate.
  - Every timeline has at least one key. Key times run from 0 to the animation's duration, strictly increasing, except event keys, which may share a time (several events at one moment).
  - An animation has at most one timeline per property of a bone or slot: two rotate timelines on one bone fail, and so do a combined translate and a translate x on one bone (the combined one keys both axes); translate x and translate y together are fine. At most one draw-order and one event timeline an animation.
  - A bezier's x controls lie in 0..=1, so its time never runs backwards.
  - A draw-order key lists every slot exactly once (or is `None`, the setup order).
  - A region is not empty; a bounding box has at least three vertices.
- Not checked: slots' setup attachments and attachment keys are names looked up in the skin at run time, so a name no skin has is allowed (it shows nothing).

## Pose and world transforms (`pose.rs`)

- `BonePose` is a bone's local transform (setup, or as animations leave it); the crate-internal `Pose` holds one per bone, the world transforms, and each slot's colour and attachment and the draw order, all set from the setup by `Pose::setup`.
- A bone's local linear part: x axis `scale_x * (cos(rotation + shear_x), sin(rotation + shear_x))`, y axis `scale_y * (-sin(rotation + shear_y), cos(rotation + shear_y))`. With y down this turns clockwise, as the engine does.
- `compute_world` is one pass over the parent-first bones. A child's origin is always its parent's full world transform applied to `(x, y)`: translation is inherited in every mode. Its linear part is `M * L`, where `L` is its local linear part and `M` comes from the parent's world linear part `P` (axes `p_x`, `p_y`) by mode:
  - Normal: `M = P`.
  - Only translation: `M = I`.
  - No rotation or reflection: `M = R(θ) * diag(s, u) * R(-θ)`, where `θ` is the parent bone's direction (the angle of `p_x`), `s = |p_x|` its length scale along itself and `u = |det(P)| / s` its area per unit length across itself. The child keeps its own world angle and is stretched along the parent bone's own axes, wherever the parent points: a parent turned 90 degrees and scaled (2, 1) stretches the child vertically. The parent's shear is dropped: written as `P * R(-θ)` it would skew the child's axes, which is a rotation the mode removes, and a mirrored parent's shear has no single unmirrored form. The absolute determinant takes out reflection and keeps the parent's area. If `p_x` has collapsed (under 1e-6), `θ` comes from `p_y` turned back a quarter, with `s = 0` and `u = |p_y|`, the limit of the normal path; with both axes collapsed `M` is zero.
  - No scale: the child is rigid, pointing where normal inheritance points it. Its world x direction is `d = normalise(P * (cos(r + shear_x), sin(r + shear_x)))`, its y direction `d.perp()` (a quarter turn clockwise), negated when `det(P) < 0` so the parent's mirroring stays. Then the child's own scale and shear apply in that frame (its linear part with the rotation taken as `-shear_x`). Taking the direction from `P` applied to the child's own direction, not from `p_x`, makes it match normal inheritance exactly under a skewed parent (a chain of turns under non-uniform scale), and the perpendicular unit frame takes out every scale and skew. If `P` squashes that direction flat (a zero-scale parent), the child is turned by the parent's rotation (from `p_x`, else from `p_y` turned back a quarter) at full size; with both axes zero it keeps its own axes.
  - No scale or reflection: as no scale, but the y direction is always `d.perp()`, so a mirrored parent leaves the child pointing as normal inheritance does but unmirrored.
- Rule we chose: the inherit modes act in skeleton space. `root` (the entity's transform) is applied to every bone after the pass, whatever its mode, so turning, scaling or flipping the entity turns, scales or flips an only-translation or no-rotation bone too (Spine's own flip flips every bone).
