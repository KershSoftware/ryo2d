# Skeleton module notes

Skeletal animation is being rebuilt from scratch. The design and its decisions are in `docs/specs/2026-11-12-skeleton-design.md`.

## Data (`data.rs`, `attachment.rs`, `skin.rs`, `curve.rs`, `animation.rs`)

- `SkeletonData` is immutable once built. Everything goes through `SkeletonDataBuilder` (`builder.rs`), which checks it in `build` (`validate.rs`); the loader builds through it too.
- The builder refers to bones, slots and skins by name. Animations hold `BoneId`/`SlotId` and event indices, and region attachments hold region indices: places in the order added, which `bone_id`, `slot_id`, `region_index` and `event_index` look up.
- `SkeletonError { item, problem }` names the item as `bone "arm"`, `slot "head"`, `region "torso"`, `event "step"`, `skin "red"`, `skin "red" slot "head" attachment "hat"`, `animation "walk"`, `animation "walk" timeline 2 (rotate)`, `mix "idle" to "run"` or `default mix`. The first problem found is reported.
- In the prelude, `Animation`, `Curve` and `Skin` are `SkeletonClip`, `SkeletonCurve` and `SkeletonSkin` (sprites, particles and the UI have the plain names).

## Validation rules

- Names are unique per kind (bones, slots, regions, events, skins, animations), and so is an attachment name within a skin's slot, and a mix pair.
- Parents come before children. At most 255 bones and 255 slots.
- References resolve: parents, slots' bones, attachments' skins, slots and regions, timelines' bones, slots and events, mixes' animations.
- Values are finite: bone setups, attachments, event floats and audio, key times and values, curves, durations and mixes. Durations and mixes are not negative.
- Rules we chose where Spine's docs are silent (each pinned by a test in `validate.rs`):
  - The default skin (`SkeletonData::DEFAULT_SKIN`, "default") always exists, first in `skins()`, and may be declared once without being a duplicate.
  - Every timeline has at least one key. Key times are from 0, strictly increasing, except event keys, which may share a time (several events at one moment).
  - A bezier's x controls lie in 0..=1, so its time never runs backwards.
  - A draw-order key lists every slot exactly once (or is `None`, the setup order).
  - A region is not empty; a bounding box has at least three vertices.
  - Slots' setup attachments and attachment keys are names looked up in the skin at run time, so they are not checked.
