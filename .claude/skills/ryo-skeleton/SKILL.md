---
name: ryo-skeleton
description: Skeletal animation - SkeletonData assets read from hot-reloading .skeleton.ron files or built in code (bones, slots, region, point and bounding-box attachments, skins, animations, events), Skeleton entities the engine poses and draws as it draws sprites, SkeletonAnimation tracks (crossfades, layers, additive, queues, start offsets), SkeletonEvent and SkeletonTrackEvent messages and the event audio recipe, BoneSocket children on bones, bone overrides for aiming, skins, the debug:skeletons overlay, limits and testing.
metadata:
  version: 1.0.0
---

# Ryo2D Skeletons

You help users animate characters and creatures made of parts on bones (cutout animation): a rig in a `.skeleton.ron` file or built in code, played, crossfaded and layered on an entity, with events, items on bones, aiming and skins.

**For complete API signatures, see the `ryo-api` skill.** Sprite-sheet animation (`Animator`) is in `ryo-animation`.

## Data and entities

A skeleton's **data** (`SkeletonData`, an asset) is the rig: its bones, the parts on them, its skins, animations and events. It is read from a `.skeleton.ron` file or built in code, checked once, and shared by every entity that shows it. A **skeleton** is an entity that shows one. The engine poses it in every fixed step and draws it every frame; a game calls nothing each frame.

| On the entity | What it does |
|---------------|--------------|
| `Transform` | Where the skeleton's origin stands (put a rig's origin at its feet), its turn and its scale |
| `Skeleton::new(data)` | Shows the data there, in its setup pose and default skin; holds the skins, attachments and bone overrides the game sets |
| `SkeletonAnimation::new()` | Plays the data's animations on tracks. Without one the skeleton holds its setup pose |
| `Renderable::skeleton()` | Draws it, sorted, lit and shadowed as a sprite is. Without one it is posed and not drawn |
| `Interpolated` | Added by `Skeleton`, so nothing to write: the entity, its bones and its sockets are drawn between fixed steps, smooth on any display. **Remove it for a skeleton that animates at the step rate** (60 a second) |

```rust
#[derive(Default)]
struct Level {
  hero: Option<Entity>,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // An asset path, relative to assets/ (see ryo-assets)
    match ctx.assets().load_now::<SkeletonData>("rigs/hero.skeleton.ron") {
      Ok(data) => {
        // Set before the entity exists, so its first frame shows the idle
        let mut state = SkeletonAnimation::new();
        state.set_animation(0, "idle", true);
        let hero = ctx.world.spawn((
          Transform::from_xy(400.0, 500.0),
          Skeleton::new(data),
          state,
          Renderable::skeleton().with_blob_shadow(BlobShadow::ellipse(40.0, 10.0)),
        ));
        self.hero = Some(hero.id());
      }
      Err(error) => warn!("no hero: {error}"),
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let wanted = if ctx.input.key_down(KeyCode::KeyD) { "run" } else { "idle" };
    if let Some(hero) = self.hero
      && let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero)
      && state.animation(0) != Some(wanted)
    {
      // Crossfades from what plays, over the mix the file gives the pair
      state.set_animation(0, wanted, true);
    }
    SceneAction::Continue
  }
}
```

- The data holds its images and a skeleton holds its data: what an entity shows stays loaded while the entity lives. Many entities share one `Handle<SkeletonData>` (it clones cheaply), each with its own pose, skins and animation.
- The animation moves the limbs; the game moves the entity. Turn a character round with its renderable's `flip_x`.
- A skeleton whose data is still loading (`ctx.assets().load`) has no bones, draws nothing and sends nothing, with no warning. What the game sets on it meanwhile is kept by name, and all of it shows in the frame the data lands.
- To give an entity another skeleton, insert a new `Skeleton` over the one it has. Assigning through a mutable reference (`*skeleton = Skeleton::new(..)`) leaves it with no pose until its next step.

## The data model

Coordinates are the engine's: y down, positive rotation clockwise. Lengths are the rig's own units, one world pixel each at an entity scale of 1. **Files give angles in degrees; code gives them in radians.**

- **Bones** make the tree the parts hang on. A bone has a name, a parent (bones are listed parents first), a setup pose relative to its parent (`x`, `y`, `rotation`, `scale_x`, `scale_y`, `shear_x`, `shear_y`), a `length` along its x axis (used by the debug overlay), and an inherit mode (`BoneInherit`): `Normal` (all of its parent's transform), `OnlyTranslation`, `NoRotationOrReflection`, `NoScale` or `NoScaleOrReflection`.
- **Slots** are places on bones where one attachment at a time shows. They are listed in draw order, back to front. A slot has its bone, a colour (multiplied into its attachment's), the name of its setup attachment and a blend (`SlotBlend`: `Normal`, `Additive`, `Multiply`, `Screen`).
- **Attachments** are what a slot can show, each under a name:
  - a **region** (`RegionAttachment`): a quad of an image, centred on its `x`, `y` from the bone, with a rotation, scale, size and colour;
  - a **point** (`PointAttachment`): a place and a direction on the bone, drawn as nothing (a muzzle, a sole);
  - a **bounding box** (`BoundingBoxAttachment`): a polygon on the bone, drawn as nothing, for hit tests.
- **Regions** (`SkeletonRegion`) are named pixel rectangles of images: the parts of an atlas.
- **Skins** (`SkeletonSkin`) say which attachment each name means in each slot. Every skeleton has the skin `"default"` (`SkeletonData::DEFAULT_SKIN`); other skins add or replace attachments over it.
- **Animations** (`SkeletonClip`) have a duration and timelines of keys (`SkeletonTimeline`): a bone's rotation, translation, scale and shear (both axes together, or one axis at a time), a slot's attachment and colour, the draw order, and events. Each value key eases to the next by its curve (`SkeletonCurve`): `Linear`, `Stepped` or `Bezier`.
- **Events** (`SkeletonEventData`) are named moments an animation keys, with an `int`, a `float`, a `string` and an optional sound for the game to play.
- **Mixes** are the seconds a crossfade takes: a default, and one per pair of animations, each way on its own.

Bone keys are changes from the setup pose: rotation, translation and shear add to it, scale multiplies it. Colour keys are absolute. Before a timeline's first key it leaves its property alone, and after its last key the last value holds. Attachment and draw-order keys are stepped: they switch at their time.

## Skeleton files

A `*.skeleton.ron` file under `assets/` holds one skeleton. Saved while the game runs, it applies at the skeletons' next step (hot reload is on by default in debug builds with a window; `EngineBuilder::hot_reload`).

```ron
(
  // Images by name: paths from the asset root, not from this file's folder
  images: { "parts": "rigs/hero.png" },
  // Regions by name: an image and its pixel rectangle (x, y, width, height)
  regions: {
    "head": (image: "parts", rect: (0, 0, 64, 64)),
    "torso": (image: "parts", rect: (64, 0, 48, 80)),
  },
  // Bones, parents first
  bones: [
    (name: "root"),
    (name: "torso", parent: "root", y: -40, length: 40),
    (name: "head", parent: "torso", y: -40, rotation: 5),
  ],
  // Slots in draw order, back to front, each showing an attachment by name
  slots: [
    (name: "torso", bone: "torso", attachment: "torso"),
    (name: "head", bone: "head", attachment: "head"),
  ],
  // What each attachment name means, by skin and slot
  skins: {
    "default": {
      "torso": { "torso": Region(region: "torso", y: -20) },
      "head": { "head": Region(region: "head", y: -24) },
    },
  },
  events: { "step": (audio: (path: "audio/step.sound.ron", volume: 0.8)) },
  animations: {
    "idle": (
      duration: 1.0,
      bones: { "head": (rotate: [(0.0, 5.0), (0.5, -5.0, Bezier(0.25, 0, 0.75, 1)), (1.0, 5.0)]) },
      events: [(0.5, "step")],
    ),
    "run": (duration: 0.5),
  },
  // Seconds to crossfade: between any two, and from idle to run
  mixes: { default: 0.15, pairs: [("idle", "run", 0.2)] },
)
```

Every field with a default may be left out. The whole format, each default in its comment:

```ron
(
  images: { "parts": "rigs/hero.png", "gear": "rigs/gear/extra.png" },
  regions: {
    "torso": (image: "parts", rect: (64, 0, 48, 80)),
    "gear": (image: "gear", rect: (0, 0, 16, 16)),
  },
  bones: [
    (name: "root"),
    (name: "torso", parent: "root", length: 40,             // no parent, 0
     x: 0, y: -40, rotation: 90,                            // 0, 0, 0
     scale_x: 1, scale_y: 1, shear_x: 0, shear_y: 0,        // 1, 1, 0, 0
     inherit: Normal),                                      // Normal, OnlyTranslation,
                                                            // NoRotationOrReflection,
                                                            // NoScale, NoScaleOrReflection
    (name: "arm", parent: "torso"),
  ],
  slots: [
    (name: "torso", bone: "torso", attachment: "torso",     // no attachment
     color: "#ffffff", blend: Normal),                      // white; Normal, Additive,
                                                            // Multiply, Screen
    (name: "gear", bone: "arm"),
  ],
  skins: {
    "default": {
      "torso": {
        "torso": Region(region: "torso",
                        x: 0, y: -20, rotation: 0,          // 0, 0, 0
                        scale_x: 1, scale_y: 1,             // 1, 1
                        width: 48, height: 80,              // the region's size
                        color: "#ffffff"),                  // white
      },
      "gear": {
        "muzzle": Point(x: 3, y: 4, rotation: 90),
        "hit": BoundingBox(vertices: [(0, 0), (10, 0), (10, 10)]),
      },
    },
    "red": { "gear": { "gear": Region(region: "gear") } },
  },
  events: {
    "step": (int: 1, float: 2.5, string: "left",            // 0, 0, ""
             audio: (path: "audio/step.ogg",                // no audio
                     volume: 0.8, balance: -0.5)),          // 1, 0
    "hit": (),
  },
  animations: {
    "walk": (
      duration: 1.0,
      bones: {
        "torso": (
          // A key is (time, value) or (time, value, curve), the curve
          // easing to the next key: Linear (the default), Stepped, or
          // Bezier(x1, y1, x2, y2) with x1 and x2 within 0 to 1
          rotate: [(0.0, 0.0), (0.5, 20.0, Stepped), (1.0, 0.0, Bezier(0.25, 0.1, 0.75, 0.9))],
          // Translate, scale and shear key both axes
          translate: [(0.0, (0, 0)), (1.0, (4, -2))],
          scale: [(0.0, (1, 1)), (1.0, (1.5, 1.5))],
          shear: [(0.0, (0, 0)), (1.0, (10, -10))],
        ),
        "arm": (
          // Or one axis at a time (not both forms on one bone)
          translate_x: [(0.0, 0), (1.0, 3)],
          translate_y: [(0.0, 0), (1.0, -3)],
          scale_x: [(0.0, 1), (1.0, 2)],
          scale_y: [(0.0, 1), (1.0, 0.5)],
          shear_x: [(0.0, 0), (1.0, 20)],
          shear_y: [(0.0, 0), (1.0, -20)],
        ),
      },
      slots: { "gear": (
        attachment: [(0.0, "gear"), (0.5, None)],           // None hides the slot
        color: [(0.0, "#ffffff"), (1.0, "#ff000000", Stepped)],
      ) },
      // Every slot, back to front, or None for the setup order
      draw_order: [(0.25, ["gear", "torso"]), (0.75, None)],
      // (time, event) or (time, event, values) with any of
      // int, float, string, volume and balance
      events: [(0.5, "step"), (0.75, "step", (int: 9, volume: 0.3))],
    ),
    "run": (duration: 0.5),
  },
  mixes: { default: 0.1, pairs: [("walk", "run", 0.2)] },
)
```

- A colour is `"#rrggbb"`, `"#rrggbbaa"` or `(r:, g:, b:, a:)`, its `a` 255 when left out. An option is written without `Some(..)`.
- **Image paths are from the asset root**, as in every other asset file of the engine: a file at `assets/rigs/hero.skeleton.ron` names `"rigs/hero.png"`. The images are the skeleton's dependencies: it is ready once they are, a missing one fails it, and saving one checks the skeleton again.
- A region attachment's `width` and `height` are its region's size when left out. A bezier's `y1` and `y2` may leave 0 to 1 (an overshoot); `x1` and `x2` may not.
- A mix pair is one way: `("idle", "run", 0.2)` leaves run to idle at the default.
- **A mistake fails the load, naming it:** a RON error at its line and column, or the item and what is wrong with it (`bone "arm": its parent "body" is not a bone`). Unknown fields are refused, so a misspelt one fails rather than doing nothing, and so is a name written twice in one map.
- **What a file must keep to:**
  - names unique within their kind (bones, slots, regions, events, skins, animations, an attachment within a skin's slot, and a mix pair);
  - every name it uses declared (a bone's parent, a slot's bone, a region's image, an attachment's region and slot, an animation's bones, slots and events, a mix's animations);
  - at most 255 bones and 255 slots;
  - every number finite, and durations and mixes not negative;
  - each region's rectangle inside its image, and not empty;
  - a bounding box of at least three vertices;
  - each timeline with at least one key, its times from 0 to the animation's duration and rising (event keys may share a time);
  - one timeline for each property of a bone or slot (`translate` and `translate_x` on one bone clash; `translate_x` with `translate_y` is fine);
  - a draw-order key listing every slot exactly once;
  - no animation named `<empty>`.
- Not checked: a slot's setup attachment and an attachment key are names looked up in the skins at run time, so a name no skin has shows nothing, with no warning.
- **The web** has no asset folder and no hot reload: embed the files with a `MemorySource` (`ryo-assets`, section 5; the skeleton demo's `embedded.rs` is one).

### Placing a part on its bone

A region attachment's `x` and `y` are where the image's centre is from the bone's origin, in the bone's own axes, and `rotation` turns the image. For a limb whose bone runs from one joint to the next:

1. Find the joint in the image, in pixels from its top-left (`p`), and the angle of the line from it to the next joint, clockwise from the image's x axis (`a`).
2. `rotation` is `-a`: it lays that line along the bone.
3. `(x, y)` is the image's centre less `p`, turned by `-a`.

A leg 93 x 164 whose hip is at (31, 18) and ankle at (64, 158): `a` is 76.7, so `rotation: -76.7`; the centre (46.5, 82) less the hip is (15.5, 64), which turned by -76.7 degrees is `x: 65.8, y: -0.4`. `scale_y: -1` flips a part across its bone, and turns its `rotation` and `y` the other way. The skeleton demo's `assets/README.md` records the method and `assets/hero.skeleton.ron` is a whole rig placed this way. The `debug:skeletons` overlay shows each bone and each part's outline while a saved file reloads.

## Building in code

`SkeletonData::builder()` builds the same data, checked by `build`, which names the first thing wrong (`SkeletonError { item, problem }`). Bones, slots and skins are named. Animations refer to bones and slots by id (`BoneId`, `SlotId`) and to events by index, and region attachments refer to regions by index: each is its place in the order added, which `bone_id`, `slot_id`, `region_index` and `event_index` look up.

```rust
fn hero(parts: Handle<Image>) -> Option<SkeletonData> {
  let builder = SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone("torso", Some("root"), BoneSetup { y: -40.0, length: 40.0, ..Default::default() })
    .bone("head", Some("torso"), BoneSetup { y: -40.0, rotation: 5.0_f32.to_radians(), ..Default::default() })
    .slot("torso", "torso", SlotSetup { attachment: Some("torso".into()), ..Default::default() })
    .slot("head", "head", SlotSetup { attachment: Some("head".into()), ..Default::default() })
    .region(SkeletonRegion { name: "head".into(), image: parts.clone(), x: 0, y: 0, width: 64, height: 64 })
    .region(SkeletonRegion { name: "torso".into(), image: parts, x: 64, y: 0, width: 48, height: 80 })
    // Skin, slot, attachment name, attachment: region 1 drawn 48 x 80, its centre 20 up the bone
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "torso",
      "torso",
      SkeletonAttachment::Region(RegionAttachment { y: -20.0, ..RegionAttachment::new(1, 48.0, 80.0) }),
    )
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "head",
      "head",
      SkeletonAttachment::Region(RegionAttachment { y: -24.0, ..RegionAttachment::new(0, 64.0, 64.0) }),
    )
    .event(SkeletonEventData::new("step"))
    .default_mix(0.15);
  let head = builder.bone_id("head")?;
  let step = builder.event_index("step")?;
  let nod = SkeletonClip {
    name: "idle".into(),
    duration: 1.0,
    timelines: vec![
      // Keys are (time, value, curve); angles in radians
      SkeletonTimeline::Rotate {
        bone: head,
        frames: vec![
          (0.0, 0.1, SkeletonCurve::Linear),
          (0.5, -0.1, SkeletonCurve::Bezier { cx1: 0.25, cy1: 0.0, cx2: 0.75, cy2: 1.0 }),
          (1.0, 0.1, SkeletonCurve::Linear),
        ],
      },
      // (time, event index, the values the key overrides)
      SkeletonTimeline::Event { frames: vec![(0.5, step, SkeletonEventValues::default())] },
    ],
  };
  match builder.animation(nod).build() {
    Ok(data) => Some(data),
    Err(error) => {
      warn!("the hero's rig: {error}");
      None
    }
  }
}
```

<!-- given: data: SkeletonData -->
```rust
let handle: Handle<SkeletonData> = ctx.assets().add(data);
```

Other builder calls: `skin(name)` declares a skin, `mix(from, to, seconds)` sets a pair's mix. The timelines are `Rotate`, `Translate`, `Scale`, `Shear` (`Vec2` keys), `TranslateX`, `TranslateY`, `ScaleX`, `ScaleY`, `ShearX`, `ShearY` (`f32` keys), `Attachment { slot, frames: Vec<(f32, Option<Arc<str>>)> }`, `Color { slot, frames }`, `DrawOrder { frames: Vec<(f32, Option<Vec<SlotId>>)> }` and `Event`. Built data is immutable: `bones()`, `slots()`, `regions()`, `skins()`, `skin(name)`, `animations()`, `animation(name)`, `events()`, `bone(name)`, `slot(name)`, `mix(from, to)` and `default_mix()` read it.

## Playing animations

`SkeletonAnimation` has **tracks**, numbered 0 to 63 (`SkeletonAnimation::MAX_TRACKS` is 64). A track plays one **entry** (an animation and its settings), may be mixing from the entries that one replaced, and has a queue of entries to play after it. Each step starts from the setup pose and applies the tracks from the lowest up, so a higher track plays over the lower ones.

<!-- given: hero: Entity -->
```rust
if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
  // Play at once on track 0, looping: what played there mixes out as this mixes in
  let run = state.set_animation(0, "run", true);
  // An entry's settings, through its handle
  if let Some(entry) = run.and_then(|id| state.entry_mut(id)) {
    entry.mix_duration = 0.1; // this crossfade, in place of the data's mix for the pair
    entry.speed = 1.2;
  }
  // A one-shot with the idle queued behind it, starting as the jump ends
  let jump = state.set_animation(0, "jump", false);
  state.add_animation(0, "idle", true, 0.0);
  // The whole skeleton's speed
  state.time_scale = 0.5;
}
```

| Call | What |
|------|------|
| `set_animation(track, name, looping) -> Option<SkeletonTrackEntryId>` | Plays `name` at once. The entry it replaces mixes out over the new entry's `mix_duration`; the track's queue is dropped. On an empty track it shows at once, with nothing to mix from |
| `add_animation(track, name, looping, delay) -> Option<SkeletonTrackEntryId>` | Queues `name` after what plays or is queued. It starts `delay` seconds into the entry before it; with `delay <= 0`, at that entry's end plus `delay` (a looping entry's end is the end of the loop it is in). On an empty track it starts after `delay` seconds. A track queues at most 64 entries: one more is refused (`None`) |
| `set_empty(track, mix)` | Fades the track out over `mix` seconds, to the setup pose on track 0 or to the tracks below, then clears it. Called again while the track is fading out, it does nothing |
| `clear(track)`, `clear_all()` | Stops the track, or every track, at once, with no fade, and drops what is queued |
| `entry(id)`, `entry_mut(id)` | The entry a handle names: playing, mixing out, or queued |
| `current(track)`, `animation(track)`, `mix_progress(track)` | What plays (below) |
| `time_scale` (field, 1) | Multiplies every track's time. A negative or non-finite scale holds the skeleton still |

Each entry has public fields, changed through `entry_mut`:

| Field | Default | What |
|-------|---------|------|
| `speed` | 1 | Multiplies the entry's time (its mix is not sped up). A negative or non-finite speed holds it still: play backward with `reverse` |
| `alpha` | 1 | How much it applies over what is under it, 0 to 1: the setup pose on track 0, the tracks below on a higher one |
| `mix_duration` | the data's mix for the pair, else its default mix | Seconds it takes to mix in from the entry it replaces. It can be changed until the mix ends |
| `mix_blend` | `SkeletonMixBlend::Replace` | `Replace` moves the pose toward the animation; `Add` adds the animation's change from the setup pose |
| `looping` | as given | Whether it repeats; otherwise it holds its last frame |
| `reverse` | false | Plays backward, from its end, firing its event keys backward |
| `start` | 0 | Seconds into the animation at which it begins (Starting part-way, below) |
| `delay` | as given | For a queued entry: when it starts, as `add_animation` worked it out |
| `event_threshold` | 0 | While it mixes out, its event keys fire until the mix replacing it reaches this fraction. At 0 none fires once it is replaced |
| `hold_previous` | false | While this entry mixes in, the entry it replaced holds its time instead of playing on |

An entry reads back with `animation()` (its name), `track_time()` (seconds played, counting every loop, from its `start`) and `is_complete()` (it has reached its animation's end at least once).

### The rules

- **A call shows from the skeleton's next step.** A call made while the game clock is paused shows when the next step runs.
- **A crossfade is a straight blend.** While an entry mixes in, the pose moves evenly from the outgoing animation to the incoming one wherever both key a property, with no dip toward the setup pose or the tracks below. A property only the outgoing animation keys fades to what is under the track.
- **The outgoing entry plays on** while it mixes out, unless the incoming entry sets `hold_previous`.
- **Rotations blend the short way round**, and a bone keeps turning one way through a mix.
- **Attachment and draw-order keys cannot be half applied.** They belong to the newest entry of a track that has any weight, and take no account of `alpha`: a layer at alpha 0.5 raises an arm half-way and switches a mouth attachment fully. An entry at alpha 0 applies none.
- **Nothing lingers.** A property no playing entry keys is at its setup value, so when an animation that showed an attachment is replaced by one that does not key the slot, the slot goes back to what the tracks below key there, else its setup attachment.
- **A queued entry mixes in from the moment it starts.** Queued with a delay of 0 it starts at the end of the entry before it, so a one-shot holds its last frame while the next mixes in. For a mix that ends as the one-shot does, queue with a delay of minus the mix (`add_animation(0, "idle", true, -0.2)`).
- **Two calls in one frame:** an entry replaced before it was ever applied is dropped, not mixed from; the new entry mixes from the one before.
- **Queue once, not every step.** A track's queue holds at most 64 entries: `add_animation` past that does nothing and returns `None`, with one warning per skeleton. Ask before queueing (`state.animation(track)`, or keep the handle).
- **`set_empty` may be called every step.** While the empty animation is the track's current entry with nothing queued after it, another `set_empty` changes nothing, whatever its `mix`: the fade keeps the timing it started with. To cut a fade short, call `clear`. Once the fade is over the track is clear, and a `set_empty` then starts a new fade of nothing, with its own `Start`, `End` and `Dispose`.
- **A name the data lacks** warns once per skeleton per name, does nothing and returns `None`. Before the data has loaded no name can be checked: the call is kept and returns a handle, and a name the data turns out to lack warns when it lands.
- **Time** is the fixed timestep times `time_scale` times the entry's `speed`. A mix runs on the timestep times `time_scale` alone.
- **A track holds at most 16 entries at once:** the one playing and the 15 it is mixing from. One more ends the oldest, whatever weight it has left. It takes a new animation every step for a quarter of a second to get there: the entry ended then weighs under 1% for mixes up to half a second, and about 11% under a 1 s mix.

### Layers

<!-- given: hero: Entity -->
```rust
if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
  state.set_animation(0, "walk", true);
  // Track 1 over track 0: the wave keys an arm and the head, so the legs keep walking
  if let Some(wave) = state.set_animation(1, "wave", true)
    && let Some(entry) = state.entry_mut(wave)
  {
    entry.alpha = 0.6; // blended with the walk's arm
  }
  // A lean added to whatever the lower tracks do
  if let Some(lean) = state.set_animation(2, "lean", true)
    && let Some(entry) = state.entry_mut(lean)
  {
    entry.mix_blend = SkeletonMixBlend::Add;
  }

  // Fade the layer out: the track clears when the fade is over
  state.set_empty(1, 0.3);

  // Fade a layer in: an entry queued after an empty one mixes in from nothing
  state.set_empty(1, 0.0);
  if let Some(wave) = state.add_animation(1, "wave", true, 0.0)
    && let Some(entry) = state.entry_mut(wave)
  {
    entry.mix_duration = 0.3;
  }
}
```

- An animation for a layer keys only the bones and slots it moves; what it leaves alone shows the tracks below.
- **`set_animation` on an empty track shows at once.** To fade a layer in, call `set_empty(track, 0.0)` and queue the animation after it with `add_animation`, then set the queued entry's `mix_duration`.
- Keep the layer's handle to change its `alpha` each step: a handle finds its entry while it is queued, playing or fading out. `current(track)` never gives the empty animation's entry.

### Starting part-way

An entry's `start` is the seconds into its animation at which it begins, as if it had already played from 0 to there. Set it before the entry's first step: as it is set or queued, or while it waits in the queue. After that step the field is not read.

<!-- given: hero: Entity, saved: f32 -->
```rust
if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero)
  && let Some(walk) = state.set_animation(0, "walk", true)
  && let Some(entry) = state.entry_mut(walk)
{
  entry.start = saved; // a crowd out of step, or a saved game's track_time()
}
```

- The entry's first pose is the animation at its start, and its time moves on from there.
- **Event keys at or before the start do not fire**: they have passed. Those after it fire once, and a looping entry fires every key in each later loop. At a start of 0 a key at 0 fires as the entry starts.
- Past the animation's end, a looping entry's start wraps round. Any other entry holds its last frame, `is_complete()` from its first step, and **sends no `Complete`**: its end has passed too.
- **A start of whole loops** (3.0 on a 1 s loop) shows the animation at 0 with the key at 0 passed: it fires a loop later.
- A `reverse` entry's start is measured back from the animation's end, so `start = track_time()` restores an entry whichever way it plays.
- `track_time()` reads the start before the entry's first step.
- A negative or non-finite start counts as 0, with one warning per skeleton.
- **To save and restore** what plays, keep each track's `animation(track)`, its entry's `track_time()` and `looping`, and give the time back as `start`. A restored entry never fires a key it had fired, with one exception (Limits). A game that was waiting for a `Complete` when it saved reads `is_complete()` after the restored entry's first step.

### What is playing

<!-- given: hero: Entity -->
```rust
if let Some(state) = ctx.world.get::<SkeletonAnimation>(hero) {
  let name: Option<&str> = state.animation(0);       // "run"; None for an empty track
  let mixing: Option<f32> = state.mix_progress(0);   // 0 up to 1 while a crossfade runs
  let entry = state.current(0).and_then(|id| state.entry(id));
  let seconds = entry.map(|entry| entry.track_time());
}
```

- `animation(track)` is the name of the entry playing (the one mixing in, during a mix). It is `None` for an empty track and for one `set_empty` is fading out. An animation set reads back at once, and a queued one from the step that starts it, so a state machine asks before it sets (`if state.animation(0) != Some("run")`), also while the data loads.
- `mix_progress(track)` is how far the playing entry has mixed in, and `None` when the track is not mixing: a first entry on an empty track, a mix of no length, and before the data has loaded. A fade-out by `set_empty` reads here while `animation` gives `None`.

### From a pool

**A pooled skeleton keeps its tracks.** A released entity is `Disabled`, not stepped, so acquired again it still plays what its last life played, and a `set_animation` crossfades from that life's pose. As it is acquired, stop every track, or put a new state on the entity:

<!-- given: hero: Entity -->
```rust
if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
  state.clear_all(); // every track stops at once, with no fade
  state.set_animation(0, "idle", true); // nothing to mix from: it shows at once
}
// Or a new state over the old, which says nothing of the old entries
ctx.world.entity_mut(hero).insert(SkeletonAnimation::new());
```

- `clear_all()` ends each entry as `clear` does: their `End` and `Dispose` are sent with the skeleton's next step. A state inserted over the old one sends nothing for the old entries.
- The skins, attachments and bone overrides set on the `Skeleton` are kept too: set those the new life relies on, as for any pooled component.

## Events

An event key reaches the game as a `SkeletonEvent` message when its animation's time crosses it.

```rust
for event in ctx.messages::<SkeletonEvent>() {
  // event.entity: the skeleton; event.track, event.animation: where it played
  // event.name: the event; event.time: the key's time in its animation
  // event.int, event.float, event.string: the key's values, else the event's defaults
  // event.audio: Option<SkeletonEventAudio { path, volume, balance }>
  if &*event.name == "footstep" {
    // dust at the foot, a camera nudge, damage on a hit frame
  }
}
```

- **Each key fires exactly once each time it is crossed**, whatever the step size or `time_scale`: a looping animation's keys once a loop, and several keys crossed in one step all in that step. A key at 0 fires as its entry starts and at the start of each loop.
- A step's events come in the order they happened, across tracks. Several skeletons' events come in entity order.
- **An entry that is mixing out fires nothing** unless its `event_threshold` is above 0. One exception: an entry that a frame showed while no step ran (the clock paused, a display faster than the step) and that was replaced before its first step still takes that step, mixing out, and its key at 0 fires then.
- Nothing fires while no fixed step runs (the game clock paused, hitstop): no key is skipped, and each fires when the clock runs again.
- With `time_scale` at 0 an entry set then still starts: its key at 0 fires once.
- **The messages are written in the skeletons' step, after `Scene::update`**, so a scene reads an event in the next step's `update` (One step behind, below).
- **A scene's `update` does not run during a scene transition**, while skeletons keep animating: what one sends meanwhile is not seen by `update` (Scene transitions, below).

### Playing an event's sound

**The engine plays no sound for an event.** An event's audio is data for the game: a path, a linear volume and a balance. Load the sounds the skeleton names once, then play the one each event carries:

<!-- given: data: Handle<SkeletonData> -->
<!-- self: hero: Entity, sounds: Vec<(std::sync::Arc<str>, Handle<Sound>)> -->
```rust
// As the scene enters: a handle for every sound the skeleton's events name,
// so a footstep never waits for a file
let paths: Vec<std::sync::Arc<str>> = ctx
  .assets()
  .get(&data)
  .map(|data| {
    data
      .events()
      .iter()
      .filter_map(|event| event.audio.as_ref().map(|audio| audio.path.clone()))
      .collect()
  })
  .unwrap_or_default();
for path in paths {
  if let Ok(sound) = ctx.assets().load_now::<Sound>(&path) {
    self.sounds.push((path, sound));
  }
}
```

<!-- self: hero: Entity, sounds: Vec<(std::sync::Arc<str>, Handle<Sound>)> -->
```rust
// Each step
for event in ctx.messages::<SkeletonEvent>() {
  if event.entity == self.hero
    && let Some(audio) = &event.audio
    && let Some((_, sound)) = self.sounds.iter().find(|(path, _)| *path == audio.path)
  {
    ctx.audio().play_with(
      sound,
      PlayOptions {
        // The event's volume is linear; a play takes decibels
        volume_db: 20.0 * audio.volume.max(1e-3).log10(),
        ..Default::default()
      },
    );
  }
}
```

- The path is an asset path: a `.sound.ron` definition (`ryo-audio`) gives a footstep its variations.
- Give `PlayOptions::position` a place (a sole's point attachment, below) for a sound heard from where it happened.
- **`balance` has no counterpart in `PlayOptions`:** a play has no pan of its own. Use a position and the sound's spatial settings, or ignore it.

### Track events

`SkeletonTrackEvent { entity, track, entry, animation, kind }` says what happened to an entry, by its handle:

| `SkeletonTrackEventKind` | When |
|--------------------------|------|
| `Start` | It became its track's playing entry |
| `Interrupt` | Another entry replaced it; it may still be mixing out |
| `Complete` | It passed its animation's end while playing: each loop when looping, once otherwise |
| `End` | It will never be applied again: cleared, or its mix-out is over |
| `Dispose` | It is gone and its handle finds nothing. After its `End`, or alone for an entry that never started (queued and dropped) |

<!-- self: hero: Entity, jump: Option<SkeletonTrackEntryId> -->
```rust
for event in ctx.messages::<SkeletonTrackEvent>() {
  if event.entity == self.hero
    && Some(event.entry) == self.jump
    && event.kind == SkeletonTrackEventKind::Complete
  {
    self.jump = None; // the jump has played to its end
  }
}
```

- What a call causes (`set_animation`'s `Interrupt` and `Start`) is sent with the skeleton's next step.
- **A handle is unique within its own skeleton only.** Every `SkeletonAnimation` numbers its entries from the same start, so two skeletons' handles can be equal: match an event's `entity` as well as its `entry`, as above.
- **An entry that is mixing out completes no more**, and a non-looping entry replaced before its end never sends `Complete`: end a state on `Complete`, `Interrupt` or `End`, or poll `state.entry(id).is_none_or(|entry| entry.is_complete())` (`is_complete` reads the entry's time alone, so it also turns true for an entry that passes its end while mixing out).
- **The empty animation's entries** (`set_empty`) carry the name `SkeletonAnimation::EMPTY` (`<empty>`) and send `Start`, `Interrupt`, `End` and `Dispose`, never `Complete`: "the fade is over" is its `End`. The constant is for reading events: `set_animation(track, SkeletonAnimation::EMPTY, ..)` is an unknown name.
- A track event sent during a scene transition is not seen by `update` either (next).

### Scene transitions

**A skeleton keeps animating through a scene transition** (`SwitchWith`, `PushWith`, `PopWith` and the rest), as particles and physics do, so a character does not stand frozen under a fade. **A scene's `update` does not run while a transition does**, and a message lasts two fixed steps. So what a skeleton sends during a transition (an event key crossed, an entry's `Start` or `Complete`) is never seen by `ctx.messages` in `update`: by the time `update` runs again it has gone.

A scene coming back from under a pause menu meets this too. Its skeletons are held under the menu, with nothing lost. If the menu pops with a fade, they are released at the fade's midpoint: what waited is sent in their next step, while the scene is still fading in, and `update` sees none of what is sent from then until the fade ends.

Three ways to write a game that does not depend on hearing a message in `update`:

**Read them in a system of the scene's own.** A scene's systems (`ctx.add_systems`) do not wait for a transition: they run in every fixed step while the scene is not stopped under another, as its skeletons do.

<!-- with: #[derive(Resource, Default)] struct Dust(Vec<Entity>); -->
```rust
fn footsteps(mut events: MessageReader<SkeletonEvent>, mut dust: ResMut<Dust>) {
  for event in events.read() {
    if &*event.name == "footstep" {
      dust.0.push(event.entity); // heard in every step, a fade's too
    }
  }
}

// In on_enter
ctx.world.init_resource::<Dust>();
ctx.add_systems(Phase::FixedUpdate, footsteps).expect("FixedUpdate takes scene systems");
```

**Queue what follows.** An animation queued behind another starts by itself, so nothing waits to hear a `Complete`:

<!-- given: hero: Entity -->
```rust
if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
  state.set_animation(0, "attack", false);
  state.add_animation(0, "idle", true, 0.0); // as the attack ends
}
```

**Ask the state.** It knows what has happened whatever was heard: an entry's `is_complete()`, and the animation a track plays.

<!-- self: hero: Entity, attack: Option<SkeletonTrackEntryId> -->
```rust
// Each step, in update
if let Some(state) = ctx.world.get::<SkeletonAnimation>(self.hero) {
  // Over when the entry has reached its end, or has gone (replaced and mixed out)
  if let Some(attack) = self.attack
    && state.entry(attack).is_none_or(|entry| entry.is_complete())
  {
    self.attack = None;
  }
  // Or by what the track plays now
  let idling = state.animation(0) == Some("idle");
}
```

## Sockets

A `BoneSocket` puts a child entity of the skeleton's entity on a bone: a weapon in a hand, a light on a head, an emitter at a muzzle. The engine places it, and everything below it, in the same step it poses the bone, so nothing on a socket trails its bone.

<!-- given: hero: Entity, sword: Handle<Image> -->
```rust
// The socket takes the bone's place, direction and scale
let hand = ctx.spawn_child(hero, BoneSocket::new("weapon"));
// Its children follow as any child follows its parent
ctx.spawn_child(hand, (Transform::from_xy(118.0, 0.0), Renderable::image(sword).with_z_order(1)));
// A light on the head, 20 up the bone, at its own size however the bone is scaled
ctx.spawn_child(
  hero,
  (
    BoneSocket::new("head").with_offset(Transform::from_xy(0.0, -20.0)).without_scale(),
    Light2D::torch(160.0),
  ),
);
```

- **The socket entity's `Transform` is the engine's** while the socket is on it: place the entity with the socket's `offset` (its place, turn and scale in the bone's space), or put what you place on a child of the socket.
- `bone` is a public `String`: rename it at any time to move the entity to another bone. It lands there at once, not sliding.
- A socket is drawn between steps with its bone, as the skeleton's entity is (at the step rate if `Interpolated` was taken off that entity). A new socket, and a skeleton that `ctx.skip_interpolation` teleports, are drawn where they land; a particle emitter on a socket leaves no streak along a jump.
- A flipped or mirrored skeleton mirrors its sockets, and a sprite on one mirrors with it.
- A socket added while no step runs (the clock paused) is on its bone in that frame.
- A skeleton can ride another's socket (a rider on a mount).
- **A bone the skeleton lacks** warns once for that name and leaves the entity where it is. A socket whose parent is not a skeleton does nothing.
- **A sprite on a socket is an entity of its own, so it draws wholly in front of the skeleton or wholly behind it**, as its `z_order` says. An item the hand must wrap (fingers over a grip) is a region attachment in a slot of its own between the hand's slots, shown with `set_attachment` or an attachment key, not a socket's sprite.
- **A collider on a socket follows its bone one physics step late**: physics runs before the skeletons' step. For a hit test in the same step, read a bounding-box attachment (below).
- A `Transform` has no shear, so a socket on a skewed bone (a sheared bone, or a turned bone under an unevenly scaled one) keeps the bone's place and direction and drops the skew.

## Bone overrides

An override sets a bone after animation, every step until it is cleared: to aim an arm, turn a head to look, or lean a body.

<!-- given: hero: Entity, target: Vec2 -->
```rust
if let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero)
  && let Some(shoulder) = skeleton.bone_world("arm")
{
  skeleton.override_bone(
    "arm",
    BoneOverride {
      // The way the bone's x axis points in the world, in radians
      rotation: Some((target - shoulder.translation).to_angle()),
      space: BoneSpace::World,
      mix: 0.8, // 0: the animated pose, 1: the override
      ..Default::default()
    },
  );
}
```

`BoneOverride { rotation: Option<f32>, translation: Option<Vec2>, scale: Option<Vec2>, space: BoneSpace, mix: f32 }`; its default sets nothing, in `BoneSpace::Local`, at a mix of 1.

- Each value given replaces the animated one by `mix` (clamped to 0 to 1), a rotation the short way round; the bone's children follow.
- `BoneSpace::Local`: the values are the bone's own, relative to its parent, as its setup pose and animations set them. `BoneSpace::World`: a rotation is the way the bone points in the world and a translation is where its origin is, each worked out against the bone's parent every step, so it holds while the entity and the bones above move. `scale` is always the bone's own.
- `override_bone` on a bone again replaces its override; `clear_override(bone)` hands the bone back to animation from the next step. Within one override the translation is applied first, then the scale, then the rotation; a parent's override is applied before its children's.
- A world rotation is a direction in the world whichever way the skeleton is flipped: the head of a hero facing left looks along `PI`, not 0.
- A value that is not finite is ignored (a mix that is not finite counts as 0), with one warning per skeleton. A bone the data lacks warns once per skeleton per name.
- A world override that cannot be worked out, because a bone above is scaled to zero, is left out for that step.

## Skins and attachments

<!-- given: hero: Entity -->
```rust
if let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero) {
  skeleton.set_skin("ranger");                      // one skin over the default
  skeleton.set_skins(&["dark", "ranger", "hat"]);   // several as one look
  skeleton.set_skins(&[]);                          // the default skin alone

  skeleton.set_attachment("weapon", Some("sword")); // shown whatever animations key there
  skeleton.set_attachment("weapon", None);          // the slot hidden
  skeleton.clear_attachment("weapon");              // back to the animations' keys, else the setup attachment

  let worn: Vec<&str> = skeleton.skins().collect();
  let shown: Option<&str> = skeleton.attachment("weapon");
}
```

- An attachment name is looked up in the last skin worn that has it in the slot, then in the default skin. So skins worn together make a look (a body, an outfit, a hat), the later winning a slot two of them fill.
- **A skin adds or replaces attachments; it cannot hide one.** A hat's skin cannot take away the hair the default skin shows: draw the hat to cover it, keep the hair out of the default skin, or hide the slot with `set_attachment(slot, None)`.
- A skin name the data lacks warns once per skeleton per name and is left out; if the data has none of the names given, the call changes nothing.
- `set_attachment` beats animation for as long as it is set. Its name is looked up in the skins like a key's, so one no skin has in the slot shows nothing, with no warning. A slot the data lacks warns once per skeleton per name.
- Skins, attachments and overrides are the entity's own, not the data's: skeletons sharing data share none of them.
- **A skin shows at once; an attachment or an override from the skeleton's next step.** The skins worn are looked up whenever something asks: the next frame draws the new skin, with the game clock paused too, and `point`, `bounding_box` and `attachment` answer by it at once. Which name each slot shows, and where the bones are, stay as the last step left them: `set_attachment` and `override_bone` are written in the skeleton's step, so they wait for one.
- `skins()` gives the skins worn, in order, without the default; `set_skin` and `set_skins` read back at once.
- `attachment(slot)` is the name the slot shows as the last step left it (the game's, else the animations' key, else the setup attachment), so a `set_attachment` reads back after the next step, while a `set_skins` changes its answer at once. **It is `None` for a name no skin worn has in the slot**, since that shows nothing, and for a hidden slot, a slot the data lacks, and before the data has loaded.

## Reading bones, points and bounding boxes

<!-- given: hero: Entity -->
<!-- self: corners: Vec<Vec2> -->
```rust
if let Some(skeleton) = ctx.world.get::<Skeleton>(hero) {
  // A bone in the world: its origin, and its axes with their scale
  if let Some(hand) = skeleton.bone_world("hand") {
    let at: Vec2 = hand.translation;
    let facing: f32 = hand.matrix2.x_axis.to_angle();
  }
  // A point attachment: where it is and the way it points, in radians
  if let Some((muzzle, direction)) = skeleton.point("muzzle") {
    // spawn the shot here
  }
  // A bounding box's corners, into a Vec the scene keeps
  if skeleton.bounding_box("hit", &mut self.corners) {
    // the box is showing: test the corners against the enemies
  }
}
```

- `point` and `bounding_box` find an attachment of that name and kind that a slot shows. One an animation or the game has hidden is not found, so **an animation turns a hit box on and off by keying its slot's attachment**, and `bounding_box` returns false, leaving the `Vec` empty, while it is off.
- All three are `None` or false for a name the skeleton lacks, and until its data has loaded.
- A flipped skeleton's bones, points and boxes are mirrored with it.

### One step behind

**A scene's `update` runs before the skeletons' step.** So what `update` reads from a skeleton (`bone_world`, `point`, `bounding_box`, `attachment`) is as the last step left it, one step behind what that step will draw, and a `SkeletonEvent` or `SkeletonTrackEvent` is read one step after it happened. At 60 steps a second that is 17 ms: fine for dust, sounds and aiming, and worth removing for a hit test on a fast blade. Two ways to be in step:

- **A `BoneSocket`** is placed in the skeletons' own step: use one for anything that must sit on a bone.
- **A system ordered after `FixedSet::Skeletons`**, added with `EngineBuilder::add_systems`, reads this step's bones and messages. A scene's own `ctx.add_systems` cannot do this: scene systems run in `FixedSet::Game`, before the skeletons' step, and a call that names an engine set is refused.

<!-- with: #[derive(Component)] struct Enemy; -->
```rust
fn sword_hits(
  skeletons: Query<&Skeleton>,
  enemies: Query<(Entity, &GlobalTransform), With<Enemy>>,
  mut corners: Local<Vec<Vec2>>,
) {
  for skeleton in &skeletons {
    if skeleton.bounding_box("hit", &mut corners) {
      // this step's box: test it against the enemies
    }
  }
}

EngineBuilder::new().add_systems(Phase::FixedUpdate, sword_hits.after(FixedSet::Skeletons));
```

## Drawing

An entity with a `Skeleton` and `Renderable::skeleton()` draws the regions its slots show, back to front in its draw order, where its bones put them. **It takes its place among the entities exactly as a sprite does**:

| On the entity | Effect |
|---------------|--------|
| `Renderable`'s `z_order`, `visible`, `lighting`, `filter` | The whole skeleton's, as for a sprite |
| `alpha`, `tint` | Each region's colour is its slot's times its attachment's times the tint; its alpha those three's times `alpha` |
| `blend` | For slots that blend `Normal`; a slot with a blend of its own keeps it |
| `emission` | Every region's |
| `flip_x`, `flip_y` | Mirror the skeleton about its entity's origin, its bones, points, boxes and sockets with it. It lands the other way round at once |
| `shader` (or the entity's `Material`) | Applied to each region on its own (below) |
| `YSort` | Sorts it among y-sorted sprites and tile layers by where it stands (below) |
| `Floor` | Draws it lifted on its storey, as a sprite is (`ryo-tilemap`) |
| `RenderLayers` | Which cameras draw it |
| `Interpolated` (added by `Skeleton`) | Bones, sockets and the entity drawn between fixed steps, together; taken off, all three move at the step rate |

`Renderable`'s `image`, `anchor` and `target` are not used: a skeleton's origin sits on its entity's position.

- **A skeleton's entity is interpolated.** `Skeleton` adds `Interpolated` to its entity, so the entity, its bones and its sockets are drawn between fixed steps and move as one. Remove `Interpolated` for a skeleton drawn at the step rate (below): bones are drawn between steps only when the entity is. `ctx.skip_interpolation(entity)` after a teleport lands the bones and sockets with the entity; without it a skeleton moved a long way in one step is drawn sliding there for that step, as any interpolated entity is.
- **Y-sorting.** A y-sorted skeleton's sort key is its entity's own y plus `YSort::sort_offset`: where it stands. Put the rig's origin at its feet. `YSort::foot_offset` measures down a sprite's image and plays no part. The key does not move with the pose or a flip.
- **Shadows.** A blob or contact shadow is a sprite's, under where the entity stands, and suits a character. **A drop shadow is cast part by part, so where parts overlap it is darker than a sprite's even one**: a half-black shadow on a white floor leaves 187 of 255 under one part and 136 where two overlap. A drop shadow's `scale` shrinks the parts' shadows toward where the skeleton stands. A part faded by its slot casts as much less.
- **A sprite shader applies part by part.** A built-in effect or a game's own shader is run for each region: a flash whitens every part, a dissolve dissolves each, and **an outline or a glow outlines each part, not the figure**. A game shader's `in.uv` runs over the part's own rectangle of its image.
- **A negative scale mirrors**, on a skeleton as on a sprite: a `Transform` scaled by -1 on one axis (on the entity or above it) draws what is under it mirrored, and both axes negative is a half turn. A scale that changes sign in a step is drawn through between steps (it closes to nothing and opens the other way); `flip_x` lands at once, which is what a character turning round wants.
- **Use one atlas.** A skeleton whose regions share an image and blend is one draw item, whatever its slot count; each change of image or blend along the draw order starts another. A game shader on the entity makes each region an item of its own.
- A skeleton's parts are culled together, for each camera, by the box round them. Off screen it keeps animating, so nothing pops when it comes back into view.
- A skeleton moved while no step runs (a paused clock) is drawn where its entity is drawn in that frame, as a sprite is: its bones follow the entity with no step to pose them. After a teleport, `ctx.skip_interpolation(entity)` draws it at its new place at once, as for any interpolated entity.

<!-- given: hero: Entity -->
```rust
// Step-rate drawing: take off what `Skeleton` added. A spawn's bundle cannot
// leave it out, and inserting a `Skeleton` on the entity again adds it again
ctx.world.entity_mut(hero).remove::<Interpolated>();
```

## What the engine does

- **The step.** In each fixed step, after `Scene::update`, physics and transform propagation and before particles (`FixedSet::Skeletons`), every skeleton whose data has loaded, in entity order: advances its animation and applies it over the setup pose, writes the game's attachments and bone overrides over that, places its bones under its entity's transform, places its sockets and what is below them, and sends its `SkeletonEvent`s and `SkeletonTrackEvent`s.
- **A pose in the frame it appears.** A skeleton added, or whose data landed or reloaded, in a frame that runs no fixed step (the clock paused, hitstop, slow motion, a display faster than the step) still has its pose and its sockets their places by the end of that frame. The pose is its animation's first moment, not the setup pose, when an animation is set. Nothing advances and no message is sent until a step runs, and none is lost.
- **Game time.** Skeletons advance in fixed steps: a paused clock and hitstop freeze them, slow motion slows them, and the animation is the same at any frame rate.
- **They freeze with their scene.** A skeleton belongs to the scene whose code or systems added it, and animates while that scene updates: under a scene that stops it (a pause menu pushed over the game) it neither advances nor sends anything, and carries on where it was when its scene updates again. Nothing is lost to the hold: what its calls caused meanwhile is sent in its first step after. A skeleton added outside every scene is held while any scene is.
- **A scene transition does not stop them.** Skeletons keep animating through a fade or a slide, as particles and physics do, while every scene's `update` waits for the transition to end. What a skeleton sends meanwhile is not seen by `update`, and that includes a scene released from under a menu that pops with a fade, from the fade's midpoint to its end (Events, Scene transitions).
- **Disabled.** A `Disabled` skeleton is not stepped or drawn. Enabled again it carries on where it stopped, which is what a pool hands out (Playing animations, From a pool).
- **Hot reload.** A saved file reloads into every skeleton showing it at their next step. Entries are kept by their animation's name, with their times; skins, attachments, overrides and sockets are found again by name. An animation the save removed clears the track it was playing on, with one warning; a queued or fading entry of it is dropped and the track plays on. A name the new data lacks warns once and stays kept, so a later save that has it brings it back. A broken save keeps the last good skeleton, with one warning. Data changed in code (`ctx.assets().get_mut`) is followed the same way.
- **Cost.** A CPU pose on the main thread, drawn through the sprite batch. On an Apple M5 Pro, 200 skeletons of 30 bones and 25 parts each, two tracks playing with a crossfade always in progress, cost about 0.5 ms of a fixed step and 0.16 ms of a frame's drawing from one atlas (0.28 ms with an image for each part); a steady step and frame allocate nothing. A crowd wholly off screen still costs its step and nearly all of its drawing work.

## debug:skeletons

In a development build, the console command `debug:skeletons` (`ryo-console`) draws over every posed skeleton: each bone as an amber line its length along it, with a white ring at its origin; the blue outline of each region drawn; each bounding box its slots show, in green; and each point as a magenta cross with a line the way it faces. It is drawn where the frame draws the skeleton (between steps, lifted, flipped), so the lines lie on the art. Use it to place parts and to see a hit box switch on. A bone with a `length` of 0 shows as its ring alone. A game built with `ryo2d build` has no console and no overlay.

## Limits

- **Not built:** mesh attachments and deform keys; IK, transform and path constraints; clipping attachments; reading another tool's files (Spine, DragonBones). A rig is written as `.skeleton.ron` or built in code.
- **Size:** at most 255 bones and 255 slots a skeleton, and tracks 0 to 63 (a higher track warns once per skeleton and does nothing).
- **Game time only:** there is no unscaled mode. A menu's skeleton stands still while the game clock is paused.
- **One step behind** in a scene's `update` (above), and a collider on a socket one physics step late.
- **Scene transitions:** a scene's `update` does not run during one, and does not see what a skeleton sends meanwhile (Events, Scene transitions).
- **A queue of 64:** `add_animation` on a track with 64 entries queued is refused, with one warning per skeleton.
- **Sockets:** a skew is dropped; a socket's sprite is wholly in front of or behind the skeleton; an ordinary child of the skeleton's entity (not a socket, nor below one) does not jump with `skip_interpolation`.
- **Mixing:** a crossfade dips a little where the incoming animation keys a property in another form (a `translate_x` over a combined `translate`), and, in a chain of three or more entries, past an entry that does not key the property. With `time_scale` at 0 no mix advances, so a track given 17 animations meanwhile jumps as its oldest entry ends.
- **Drawing:** a drop shadow is darker where parts overlap; blob and contact shadows stand under the entity and do not follow the pose (while an animation jumps, set a blob shadow's height from a bone, as the demo's `lift_shadow` does); sprite shaders, outlines and glows apply part by part; `YSort::foot_offset` is not used; there is no culling of single parts, and a skeleton off screen still builds its quads.
- **Skins** cannot hide an attachment.
- **Event audio** is the game's to play, and `balance` has nowhere to go in `PlayOptions`.
- **Before the data has loaded:** at most 256 animation calls are kept (one more is ignored and returns `None`, with one warning), and at most 255 attachments and 255 overrides; a `mix_duration` set to 0 on a kept entry counts as unset and takes the data's mix.
- **Warnings are bounded:** a skeleton remembers 64 names it has warned about, and its animation state 64 more; past that a missing name is ignored in silence.
- **Track events from calls** wait for the next step in a list of 1,024; a skeleton that is not stepped while a game keeps calling it drops the oldest, with one warning.
- **A despawned skeleton** sends nothing more: what its last calls caused goes untold.
- **Long runs:** an entry's time is an `f32` that counts every loop. A looping entry left playing moves in steps of 2 ms after 4.6 hours and 8 ms after 18 hours, runs near twice as fast after 3 days and stops after 6. A layer at partial alpha whose bone keeps turning remembers its turns: after 4.5 hours of a turn a second its rotation moves in steps of 0.45 degrees times its alpha, and where it rests depends on how far it has wound. Setting the animation again starts both afresh. One step fires the event keys of at most the last 1,000 loops it crossed, and sends at most 1,000 `Complete`s.
- **Restoring on a loop boundary:** an entry started at a whole number of loops reads 0.0 from `track_time()` until its first step. A game that saves again before that step (loaded, then saved while paused) and restores from it fires the key at 0 a second time.
- **Not covered by the pose in a frame:** a skeleton enabled again (`Disabled` taken off) while no step runs keeps the pose it had until its next step, and its flip too if its renderable was flipped while it was disabled; one whose data changed meanwhile draws nothing until then. A socket given to a skeleton by `ChildOf` alone (its `BoneSocket` already on it) while no step runs is placed in the next step.
- **Inherit modes act inside the skeleton:** the entity's turn, scale and flip apply to every bone, an `OnlyTranslation` or `NoRotationOrReflection` one too.
- **Where it differs from Spine's runtimes**, for a reader who knows them:
  - a crossfade on a higher track does not dip toward the tracks below, and `hold_previous` holds the previous entry's time;
  - a queued entry's `delay <= 0` does not take the mix off, so the mix starts at the previous entry's end;
  - an entry queued on an empty track waits out its delay before it starts;
  - `Complete` comes only from the playing entry, never from one mixing out;
  - `clear` returns the track's properties to what is under it, since every step starts from the setup pose;
  - an entry mixing out applies its attachment and draw-order keys only until the entry replacing it has weight, and an entry with no weight applies none;
  - not offered: a track end time, a part of an animation to loop, queued empty animations, the attachment and draw-order thresholds, and the rotation-direction calls.

## Testing

`ryo2d::testing::HeadlessEngine` steps a skeleton without a window, one fixed step a `step`. A rig built in code needs no files, and one with no regions needs no image:

```rust
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

struct Stage;
impl Scene for Stage {}

/// An arm that turns a quarter turn in a second, with an event half-way
fn rig() -> SkeletonData {
  SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone("arm", Some("root"), BoneSetup { x: 10.0, length: 20.0, ..Default::default() })
    .event(SkeletonEventData::new("swish"))
    .animation(SkeletonClip {
      name: "swing".into(),
      duration: 1.0,
      timelines: vec![
        SkeletonTimeline::Rotate {
          bone: BoneId(1),
          frames: vec![
            (0.0, 0.0, SkeletonCurve::Linear),
            (1.0, std::f32::consts::FRAC_PI_2, SkeletonCurve::Linear),
          ],
        },
        SkeletonTimeline::Event { frames: vec![(0.5, 0, SkeletonEventValues::default())] },
      ],
    })
    .build()
    .expect("a valid rig")
}

#[test]
fn the_arm_swings_and_says_so() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new().resolution(64, 64).start_scene(Stage),
  ) else {
    return; // no GPU
  };
  let data = engine.assets().add(rig());
  let mut state = SkeletonAnimation::new();
  state.set_animation(0, "swing", false);
  let hero = engine
    .world_mut()
    .spawn((Transform::from_xy(32.0, 32.0), Skeleton::new(data), state))
    .id();

  // Messages last two fixed steps: read them every step
  let mut cursor = engine.world().resource::<Messages<SkeletonEvent>>().get_cursor();
  let mut heard = Vec::new();
  for _ in 0..30 {
    engine.step().unwrap(); // half a second at 60 steps a second
    let events = engine.world().resource::<Messages<SkeletonEvent>>();
    heard.extend(cursor.read(events).map(|event| event.name.to_string()));
  }

  let skeleton = engine.world().get::<Skeleton>(hero).unwrap();
  let arm = skeleton.bone_world("arm").unwrap();
  assert!((arm.matrix2.x_axis.to_angle() - std::f32::consts::FRAC_PI_4).abs() < 0.01);
  assert_eq!(heard, ["swish"]);
}
```

- Between steps, `engine.world()` and `world_mut()` reach the skeleton as a scene does: set animations, read `bone_world`, `point`, `bounding_box`, `skins` and `attachment`, and `SkeletonAnimation::animation`, `mix_progress` and an entry's `track_time`.
- To test a file, give the engine a `MemorySource` holding it and its images (`EngineBuilder::asset_source`) and load it with `engine.assets().load_now`, which returns the load's error for a bad file.
- To test drawing, compare `engine.frame()` pixels (`ryo-game-dev`, Visual Tests), with regions over `Image::solid_rect` images.
- Assert on a warning (an unknown animation, a missing bone) with `ryo2d::testing::capture_log()`, opened before the engine is built.
- A headless run is the same every time: skeletons have no randomness.

## Tips

- Start from the skeleton demo (`examples/demos/skeleton`): a page each for a character's crossfades, layers, events with sounds and dust, a sword on a socket with a hit box, skins, aiming and a crowd, over one hand-written rig.
- Give every pair of animations that follow each other a mix, or a `default` of 0.1 to 0.2 s: a mix of 0 cuts.
- Name bones, slots and attachments for what they are (`arm_near`, `sole_far`, `hit`): the game refers to all of them by name.
- Put points where things happen (soles, a muzzle, a hand) and read them with `point`, rather than working places out from bones.
- A jump keyed in the animation leaves the entity on the ground: move the entity for travel, and keep in-place motion in the rig.
- For a crowd, give each entry its own `start` and a `speed` a little off 1, and one atlas for the rig.
