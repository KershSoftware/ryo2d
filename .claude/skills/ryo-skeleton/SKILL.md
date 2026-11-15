---
name: ryo-skeleton
description: Skeletal animation - SkeletonData assets read from hot-reloading .skeleton.ron files or built in code (bones, slots, region, mesh, point and bounding-box attachments, skins, animations, events), meshes that bend (weighted over bones, moved by deform keys), Skeleton entities the engine poses and draws as it draws sprites, SkeletonAnimation tracks (crossfades, layers, additive, queues, start offsets), SkeletonEvent and SkeletonTrackEvent messages and the event audio recipe, BoneSocket children on bones, bone overrides for aiming, constraints (IK on one bone, two bones or a chain reaching a target bone, transform constraints copying a bone, keyed and overridden), skins, the debug:skeletons overlay, limits and testing.
metadata:
  version: 1.2.0
---

# Ryo2D Skeletons

You help users animate characters and creatures made of parts on bones (cutout animation), with meshes for parts that bend: a rig in a `.skeleton.ron` file or built in code, played, crossfaded and layered on an entity, with events, items on bones, aiming, constraints (feet planted by IK, a part that copies another) and skins.

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

- **Bones** make the tree the parts hang on. A bone has a name, a parent (bones are listed parents first), a setup pose relative to its parent (`x`, `y`, `rotation`, `scale_x`, `scale_y`, `shear_x`, `shear_y`), a `length` along its x axis (where its tip is: the debug overlay draws it, and an IK constraint reaches with the last bone's), and an inherit mode (`BoneInherit`): `Normal` (all of its parent's transform), `OnlyTranslation`, `NoRotationOrReflection`, `NoScale` or `NoScaleOrReflection`.
- **Slots** are places on bones where one attachment at a time shows. They are listed in draw order, back to front. A slot has its bone, a colour (multiplied into its attachment's), the name of its setup attachment and a blend (`SlotBlend`: `Normal`, `Additive`, `Multiply`, `Screen`).
- **Attachments** are what a slot can show, each under a name:
  - a **region** (`RegionAttachment`): a quad of an image, centred on its `x`, `y` from the bone, with a rotation, scale, size and colour;
  - a **mesh** (`MeshAttachment`): triangles between vertices, textured from a region, for a part that bends, by the bones that hold its vertices or by an animation's deform keys (Meshes, below);
  - a **point** (`PointAttachment`): a place and a direction on the bone, drawn as nothing (a muzzle, a sole);
  - a **bounding box** (`BoundingBoxAttachment`): a polygon on the bone, drawn as nothing, for hit tests.
- **Regions** (`SkeletonRegion`) are named pixel rectangles of images: the parts of an atlas.
- **Skins** (`SkeletonSkin`) say which attachment each name means in each slot. Every skeleton has the skin `"default"` (`SkeletonData::DEFAULT_SKIN`); other skins add or replace attachments over it.
- **Animations** (`SkeletonClip`) have a duration and timelines of keys (`SkeletonTimeline`): a bone's rotation, translation, scale and shear (both axes together, or one axis at a time), a slot's attachment and colour, a mesh's shape (deform keys: Meshes, Deform keys, below), the draw order, and events. Each value key eases to the next by its curve (`SkeletonCurve`): `Linear`, `Stepped` or `Bezier`.
- **Events** (`SkeletonEventData`) are named moments an animation keys, with an `int`, a `float`, a `string` and an optional sound for the game to play.
- **Mixes** are the seconds a crossfade takes: a default, and one per pair of animations, each way on its own.
- **Constraints** (`SkeletonConstraint`) move bones after animation by a rule: an IK constraint turns bones so a tip reaches a target bone, a transform constraint makes bones copy a target bone (Constraints, below).

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
    (name: "arm", parent: "torso", length: 30),
    (name: "aim", parent: "root"),                          // a target for the constraints
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
        // Hull is how many of the first vertices outline the mesh (0 for none).
        // UVs run 0 to 1 across the region; triangles index the vertices
        "cape": Mesh(
          region: "torso",
          uvs: [(0, 0), (1, 0), (1, 1), (0, 1)],
          triangles: [0, 1, 2, 0, 2, 3],
          hull: 4,                                          // 0
          // A vertex is weighted over bones, each (bone, x, y, weight): x and y are
          // where it sits in that bone's space, and a vertex's weights are scaled to
          // sum to 1. Or unweighted: positions in the slot's bone
          vertices: Weighted([
            [("torso", -20, 0, 1)],
            [("torso", 20, 0, 1)],
            [("arm", 20, 0, 0.75), ("torso", 20, 60, 0.25)],
            [("arm", -20, 0, 1)],
          ]),
          color: "#ffffff"),                                // white
        "flag": Mesh(region: "torso", uvs: [(0, 0), (1, 0), (1, 1)], triangles: [0, 1, 2],
                     vertices: Unweighted([(0, 0), (64, 0), (64, 40)])),
      },
    },
    "red": { "gear": { "gear": Region(region: "gear") } },
  },
  // Constraints, in the order they apply (Constraints, below). Each names its
  // bones (an IK chain runs from parent to child), a target bone that is not
  // one of them nor below one, and settings, each with its default
  constraints: [
    Ik(name: "reach", bones: ["torso", "arm"], target: "aim",
       mix: 1,                                             // 0 (animated) to 1 (solved)
       softness: 0,                                        // two bones, a distance
       bend_positive: true,                                // two bones only
       compress: false, stretch: false, uniform: false),   // compress: one bone; the
                                                           // others: not a chain
    Transform(name: "level", bones: ["arm"], target: "aim",
              mix: (rotate: 1, x: 0, y: 0, scale_x: 0, scale_y: 0, shear_y: 0),   // all 0
              offset: (rotation: 0, x: 0, y: 0, scale_x: 0, scale_y: 0, shear_y: 0), // all 0
              local: false, relative: false),              // false, false
  ],
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
        // The shape of a mesh the slot shows, by its attachment's name. A key
        // holds an offset from the setup shape, x then y, for each position
        // of the mesh: a vertex each when unweighted (in the slot's bone), an
        // influence each when weighted (in that influence's bone). [] is no
        // offset, the setup shape
        deform: { "flag": [(0.0, []), (0.5, [0, -6, 0, -6, 0, 6], Bezier(0.25, 0, 0.75, 1)), (1.0, [])] },
      ) },
      // Constraints by name: an IK constraint's `ik` keys (mix, softness, bend_positive,
      // compress, stretch) or a transform constraint's `transform` keys (its six mixes).
      // A value a key leaves out is the constraint's setup value. The numbers ease to
      // the next key by its curve; the switches change at their key
      constraints: {
        "reach": (ik: [(0.0, (mix: 1)), (0.5, (mix: 0.5, softness: 4, bend_positive: false), Stepped)]),
        "level": (transform: [(0.0, (rotate: 1)), (1.0, (rotate: 0.5, x: 0.25))]),
      },
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
  - names unique within their kind (bones, slots, regions, events, skins, animations, constraints of both kinds together, an attachment within a skin's slot, and a mix pair);
  - every name it uses declared (a bone's parent, a slot's bone, a region's image, an attachment's region and slot, an animation's bones, slots, constraints and events, a mix's animations, a constraint's bones and target);
  - at most 255 bones, 255 slots and 255 constraints;
  - every number finite, and durations and mixes not negative;
  - each region's rectangle inside its image, and not empty;
  - a bounding box of at least three vertices;
  - a mesh that holds together (Meshes, below), and a deform that fits the data (Deform keys, below);
  - each timeline with at least one key, its times from 0 to the animation's duration and rising (event keys may share a time);
  - one timeline for each property of a bone or slot (`translate` and `translate_x` on one bone clash; `translate_x` with `translate_y` is fine);
  - a draw-order key listing every slot exactly once;
  - no animation named `<empty>`;
  - constraints that can work (Constraints, What fails to load).
- Not checked: a slot's setup attachment and an attachment key are names looked up in the skins at run time, so a name no skin has shows nothing, with no warning. Nor is it checked that a deform's attachment is a mesh, or that its keys are as long as the mesh's positions: that is found as the deform plays (Deform keys, below).
- **The web** has no asset folder and no hot reload: embed the files with a `MemorySource` (`ryo-assets`, section 5; the skeleton demo's `embedded.rs` is one).

### Placing a part on its bone

A region attachment's `x` and `y` are where the image's centre is from the bone's origin, in the bone's own axes, and `rotation` turns the image. For a limb whose bone runs from one joint to the next:

1. Find the joint in the image, in pixels from its top-left (`p`), and the angle of the line from it to the next joint, clockwise from the image's x axis (`a`).
2. `rotation` is `-a`: it lays that line along the bone.
3. `(x, y)` is the image's centre less `p`, turned by `-a`.

A leg 93 x 164 whose hip is at (31, 18) and ankle at (64, 158): `a` is 76.7, so `rotation: -76.7`; the centre (46.5, 82) less the hip is (15.5, 64), which turned by -76.7 degrees is `x: 65.8, y: -0.4`. `scale_y: -1` flips a part across its bone, and turns its `rotation` and `y` the other way. The skeleton demo's `assets/README.md` records the method and `assets/hero.skeleton.ron` is a whole rig placed this way. The `debug:skeletons` overlay shows each bone and each part's outline while a saved file reloads.

## Meshes

A region is one rectangle on one bone: it can turn and scale, and nothing more. A **mesh** is a sheet cut into triangles. Its corners are **vertices**: each has a place in the image (its **UV**) and a place on the rig, and the engine draws the part of the image between each triangle's three vertices. Move a vertex and the picture stretches with it. A cape bends along its length, a flag ripples, a cheek puffs, where a region could only swing as a whole.

**Use a mesh for a part that must bend or change shape; keep a region for a part that stays a rectangle** (a limb that turns on a joint, a head, a weapon). A mesh bends only where it has vertices, so give it a few rows across the bend. **Each mesh is a draw of its own** (two with a drop shadow), where all the regions of one image in a row are one draw: a character is made of a few meshes, not thirty (Limits, below).

A mesh is held in one of two ways, and a deform key (an animation's key that moves its vertices: Deform keys, below) can move either.

- **Unweighted:** each vertex is a place in the space of the slot's bone (`x` and `y`, in pixels along the bone's axes). The whole mesh rides that one bone, and its shape changes only by deform keys.
- **Weighted:** each vertex is held by several bones at once. Each pull on it is an **influence**: a bone, the place the vertex sits in *that bone's* space, and a **weight**, how much that bone counts. The vertex is drawn at the sum of where each bone puts it, times its weight. Bones that swing apart stretch the vertices between them, so a cape follows its bones with no deform key at all. The slot's own bone plays no part.

### An unweighted mesh in a file

A mesh is an attachment like a region, written in a skin. Every vertex is written twice, in the same order: in `uvs`, where it is in the image, and in `vertices`, where it is on the rig.

```ron
(
  images: { "banner": "rigs/banner.png" },
  regions: { "flag": (image: "banner", rect: (0, 0, 96, 64)) },
  bones: [
    (name: "root"),
    (name: "pole", parent: "root", length: 80),
  ],
  slots: [(name: "flag", bone: "pole", attachment: "flag")],
  skins: {
    "default": {
      "flag": {
        "flag": Mesh(
          region: "flag",
          // The outline first, round from the top-left corner, then any vertices
          // inside it (none here). 0, 0 is the region's top-left, 1, 1 its bottom-right
          uvs: [(0, 0), (0.5, 0), (1, 0), (1, 1), (0.5, 1), (0, 1)],
          // Indices into the vertices, three to a triangle, wound either way
          triangles: [0, 1, 4, 0, 4, 5, 1, 2, 3, 1, 3, 4],
          // The first six vertices go round the outline: for the overlay
          hull: 6,
          // Places in the pole's space, in pixels, in the order of the UVs
          vertices: Unweighted([(0, 0), (48, 0), (96, 0), (96, 64), (48, 64), (0, 64)]),
        ),
      },
    },
  },
)
```

| Field | What it is |
|-------|-----------|
| `region` | The region whose image the mesh draws (required) |
| `uvs` | One `(u, v)` for each vertex: 0 to 1 across the region. A UV outside 0 to 1 draws nothing there; it does not read a neighbouring region of the atlas |
| `triangles` | Vertex numbers, three to a triangle. Winding does not matter: a mirrored skeleton draws the same triangles |
| `hull` | How many of the first vertices go round the mesh's outline, in order (3 or more), or 0 for none (the default). Only the overlay uses it |
| `vertices` | `Unweighted([(x, y), ..])` or `Weighted([..])`, one entry for each vertex (required) |
| `color` | Multiplied into the slot's colour (`"#rrggbb"`, as elsewhere; white when left out) |

Positions are in pixels, and no angle occurs, so nothing is converted. A mesh shows like any attachment: a slot's `attachment` or an attachment key names it, and a skin may hold it.

### A weighted mesh

A weighted vertex is a list of influences, each `(bone, x, y, weight)`, and `x` and `y` are where the vertex is *in that bone's space in the setup pose*. A vertex held by two bones is written twice, once in each bone's space, because the two spaces differ. Here the cape's top is held by the torso, its bottom by a `cape` bone that hangs 30 below the torso's origin, and its middle row half by each. The cape is in a skin of its own, so a hero without it simply wears the other skin.

```ron
(
  images: { "parts": "rigs/hero.png" },
  regions: { "cape": (image: "parts", rect: (0, 0, 48, 96)) },
  bones: [
    (name: "root"),
    (name: "torso", parent: "root", y: -60, length: 30),
    // 30 below the torso's origin, in the torso's space
    (name: "cape", parent: "torso", y: 30, length: 30),
  ],
  slots: [(name: "cape", bone: "torso", attachment: "cape")],
  skins: {
    "default": {},
    "cape": {
      "cape": {
        "cape": Mesh(
          region: "cape",
          // Round the outline: top-left, top-right, middle-right, bottom-right,
          // bottom-left, middle-left
          uvs: [(0, 0), (1, 0), (1, 0.5), (1, 1), (0, 1), (0, 0.5)],
          triangles: [0, 1, 2, 0, 2, 5, 5, 2, 3, 5, 3, 4],
          hull: 6,
          // For each vertex, its influences: (bone, x, y, weight)
          vertices: Weighted([
            [("torso", -24, 0, 1)],
            [("torso", 24, 0, 1)],
            // The same place twice: 30 down in the torso's space, level with the cape bone's origin
            [("torso", 24, 30, 0.5), ("cape", 24, 0, 0.5)],
            [("cape", 24, 30, 1)],
            [("cape", -24, 30, 1)],
            [("torso", -24, 30, 0.5), ("cape", -24, 0, 0.5)],
          ]),
        ),
      },
    },
  },
  animations: {
    "sway": (
      duration: 1.0,
      bones: { "cape": (rotate: [(0.0, -10.0), (0.5, 10.0), (1.0, -10.0)]) },
    ),
  },
)
```

- **A vertex's weights are scaled to sum to 1** as the file loads, so `("torso", .., 1), ("cape", .., 1)` is the same as 0.5 and 0.5, and 3 and 1 mean 0.75 and 0.25. A weight is not negative, and a vertex needs at least one influence with weight above 0.
- Tune weights where a part bends: a row held by one bone follows that bone rigidly; a row shared by two stretches between them. Turn the second bone far enough and the rows between can fold over.
- `skeleton.set_skin("cape")` puts the cape on (Skins and attachments). The skin has the mesh and the default skin does not, so until it is worn the slot shows nothing, and nothing is said: a skin cannot hide an attachment, but a character without a cape can leave the cape's skin off.

### Building a mesh in code

`MeshAttachment::new(region, vertices, uvs, triangles)` has no hull and a white colour; set `hull` and `color` as fields. `region` is an index into the regions added. A mesh goes into a skin with `attachment`, as a region does (`SkeletonAttachment::Mesh`).

```rust
fn flag(image: Handle<Image>) -> Option<SkeletonData> {
  let builder = SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone("pole", Some("root"), BoneSetup { length: 80.0, ..Default::default() })
    .slot("flag", "pole", SlotSetup { attachment: Some("flag".into()), ..Default::default() })
    .region(SkeletonRegion { name: "flag".into(), image, x: 0, y: 0, width: 96, height: 64 });
  let mesh = MeshAttachment {
    hull: 6,
    ..MeshAttachment::new(
      0, // the region, by its place in the order added
      MeshVertices::Unweighted(
        [(0.0, 0.0), (48.0, 0.0), (96.0, 0.0), (96.0, 64.0), (48.0, 64.0), (0.0, 64.0)]
          .map(Vec2::from)
          .to_vec(),
      ),
      [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0)]
        .map(Vec2::from)
        .to_vec(),
      vec![0, 1, 4, 0, 4, 5, 1, 2, 3, 1, 3, 4],
    )
  };
  let flag = builder.slot_id("flag")?;
  // A deform key: (time, offsets, curve). Twelve offsets, an x and a y for each of the six vertices
  let wave = SkeletonClip {
    name: "wave".into(),
    duration: 1.0,
    timelines: vec![SkeletonTimeline::Deform {
      slot: flag,
      attachment: "flag".into(),
      frames: vec![
        (0.0, vec![], SkeletonCurve::Linear), // empty: the setup shape
        (0.5, vec![0.0, 0.0, 0.0, -6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 6.0, 0.0, 0.0], SkeletonCurve::Linear),
        (1.0, vec![], SkeletonCurve::Linear),
      ],
    }],
  };
  match builder
    .attachment(SkeletonData::DEFAULT_SKIN, "flag", "flag", SkeletonAttachment::Mesh(mesh))
    .animation(wave)
    .build()
  {
    Ok(data) => Some(data),
    Err(error) => {
      warn!("the flag: {error}");
      None
    }
  }
}
```

A weighted mesh holds `MeshVertices::Weighted(WeightedVertices::new(vertices))`, one `Vec` of `SkeletonInfluence::new(bone, x, y, weight)` for each vertex, and scales the weights as the file does:

```rust
fn cape_mesh(torso: BoneId, cape: BoneId) -> MeshAttachment {
  let pull = SkeletonInfluence::new;
  let vertices = WeightedVertices::new([
    vec![pull(torso, -24.0, 0.0, 1.0)],
    vec![pull(torso, 24.0, 0.0, 1.0)],
    vec![pull(torso, 24.0, 30.0, 0.5), pull(cape, 24.0, 0.0, 0.5)],
    vec![pull(cape, 24.0, 30.0, 1.0)],
    vec![pull(cape, -24.0, 30.0, 1.0)],
    vec![pull(torso, -24.0, 30.0, 0.5), pull(cape, -24.0, 0.0, 0.5)],
  ]);
  MeshAttachment {
    hull: 6,
    ..MeshAttachment::new(
      0,
      MeshVertices::Weighted(vertices),
      [(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0), (0.0, 0.5)]
        .map(Vec2::from)
        .to_vec(),
      vec![0, 1, 2, 0, 2, 5, 5, 2, 3, 5, 3, 4],
    )
  }
}
```

Reading built data: `MeshAttachment::vertex_count()` and `position_count()` (below), `MeshVertices::Unweighted(Vec<Vec2>)` and `Weighted(WeightedVertices)`; `WeightedVertices` has `vertex_count()`, `influences(vertex)` and `position_count()`. `skin.attachment(slot, name)` returns a `SkeletonAttachment::Mesh` to match.

### Deform keys

A **deform key** moves a mesh's vertices in an animation. A key is a list of numbers: an offset from the mesh's setup shape, an x then a y, for each of the mesh's **positions**. A position is a vertex when the mesh is unweighted, and an *influence* when it is weighted (the influences of vertex 0 first, then vertex 1's, and so on, in the order of the file). `MeshAttachment::position_count()` counts them: the six-vertex flag above has six positions and its keys hold twelve numbers; the cape has eight influences, so its keys hold sixteen.

Add this to the flag file above (before its final `)`) and the flag swells and settles: the pole's edge and the fly stay, and the key moves the top-middle vertex up and the bottom-middle one down.

```ron
  animations: {
    "wave": (
      duration: 1.0,
      // A slot's block holds its deforms, by the name of the attachment they move
      slots: { "flag": (deform: { "flag": [
        (0.0, []),
        // (time, offsets) or (time, offsets, curve): an x and a y for each of the six vertices
        (0.5, [0, 0, 0, -6, 0, 0, 0, 0, 0, 6, 0, 0], Bezier(0.25, 0, 0.75, 1)),
        (1.0, []),
      ] }) },
    ),
  },
```

And this in place of the cape file's `animations`: the bones swing and the hem flares as well. The key's eight x, y pairs, in order, are for the influences of the torso (top left), the torso (top right), the torso and the cape (middle right), the cape (bottom right), the cape (bottom left), and the torso and the cape (middle left).

```ron
  animations: {
    "sway": (
      duration: 1.0,
      bones: { "cape": (rotate: [(0.0, -10.0), (0.5, 10.0), (1.0, -10.0)]) },
      slots: { "cape": (deform: { "cape": [
        (0.0, []),
        (0.5, [0, 0, 0, 0, 0, 0, 0, 0, 6, 0, -6, 0, 0, 0, 0, 0]),
        (1.0, []),
      ] }) },
    ),
  },
```

- **An offset is in the space its position is written in**: the slot's bone for an unweighted vertex, the influence's own bone for a weighted one. So a keyed shape turns and scales with its bones, and a weighted vertex moves by each influence's offset times that influence's weight.
- **`[]` is the setup shape**, not a missing key. A key is whole (an x and a y for every position) or empty, never part of a mesh, and every key of a timeline is as long as the others or empty. Between two keys the offsets ease by the first key's curve; before the first key the deform does nothing, and after the last its offsets hold. Key a loop's last frame as its first so the shape does not jump.
- **A deform is keyed by the slot and the attachment's name**, not by the mesh. It moves whichever mesh of that name the skins worn show in the slot, while the slot shows that name. An animation may deform two names of one slot; the slot shows one at a time.
- **Moving the shape does not move the bones.** Animate bones and deforms in one animation as the cape does, or either alone. Both are drawn between fixed steps, as bones are, so a deform is smooth at any frame rate.
- A skeleton's meshes are its own: skeletons that share data share no shape, each playing its own deform.

**How deforms mix.** A deform key blends as a bone key does, in the order the tracks apply:

- A higher track's entry **replaces** what the tracks below left by its `alpha` (`entry.alpha` 0.5 moves the shape half way to its key), or, with `mix_blend` set to `SkeletonMixBlend::Add`, **adds** its key times its alpha.
- A **crossfade** between two animations that both deform the same slot and name is a plain blend of their shapes, with no dip toward the setup shape in the middle. A crossfade to an animation that deforms nothing there, or to the empty animation, returns the mesh to its setup shape in step with the mix, and the other way the deform grows in from it. Neither jumps when the mix ends.
- Keys of another length cover nothing: if the incoming animation's deform holds a different count of positions than the outgoing one's, the outgoing deform fades with the mix as it would under an animation with no deform.
- When the slot's attachment changes (an attachment key, `set_attachment`, a skin), the mesh now shown takes its own shape at once, and is not drawn from the offsets of the one before.

**What skips a deform, and what is said.** A deform changes a mesh only while the engine finds the mesh it was keyed for. Case by case:

| What happens | Said? |
|--------------|-------|
| The slot shows another attachment than the deform's name (a higher track's attachment key, `set_attachment`, a crossfade to an animation that shows another one), or shows nothing, or a name no worn skin has | Silent. It is ordinary play: a character without its cape hides the slot, and its animations go on keying the cape |
| The deform names an attachment some skin has in the slot but the slot never shows | The file loads, and nothing is said |
| The slot shows the deform's name, but the attachment found under it is not a mesh (a skin has a region there), or is a mesh holding another count of positions than the keys | One warning in the log for each skeleton and animation, naming the animation, the attachment, the slot and the reason; the deform is skipped and the mesh keeps its setup shape. Not repeated each step, loop or skin change. When the skeleton's file is saved and reloads, it is told of afresh: a deform that still does not fit the new data, or no longer does, warns once more, and a save that fits says nothing. A skeleton remembers up to 64 animations it has warned of in the data it has |
| A deform whose keys are all empty | Fits any mesh |

**What fails to load**, with the animation and timeline named (`animation "wave" timeline 0 (deform)`):

- a deform naming an attachment that **no skin has in its slot** (the default skin or any other): such a deform could never apply, and a misspelt name would otherwise do nothing and say nothing;
- a key with an odd count of values (a number left out of an x, y pair);
- keys of different lengths in one timeline (a key may also be empty);
- two deforms of one slot and name in one animation (two names of one slot are fine);
- a number that is not finite, a key time outside 0 to the duration or not rising, or a bad curve, as for any timeline.

A file saved with a deform of a mesh it no longer has in any skin fails the same way: the reload is refused, naming the deform, and the skeletons keep playing the data they had. A reload that changes a mesh's count of positions while its deform plays draws the setup shape, reads nothing out of range, and says so once.

### Errors you will meet

A mistake in a mesh fails the load naming `skin "default" slot "flag" attachment "flag"`; a mistake in a deform names `animation "wave" timeline 0 (deform)`. A mistake in the text itself (a misspelt field, a name written twice in a map) is a RON error at its line and column. The first problem found is reported.

| The problem says | What it means |
|------------------|---------------|
| `it has 5 UVs for 6 vertices` | `uvs` and `vertices` are the same length: a UV for every vertex |
| `it has no triangles; it needs at least one` | `triangles` is empty |
| `it has 11 triangle indices; they come in threes` | `triangles` is a list of vertex numbers, three to a triangle |
| `triangle index 9 is out of range; there are 6 vertices` | a triangle names a vertex the mesh does not have (they are counted from 0) |
| `it has a hull of 2 vertices; it is 0 or 3 to 6` | `hull` is 0 (none), or at least 3 and no more than the vertices |
| `it has 65536 vertices; at most 65535` | a mesh holds at most 65,535 vertices |
| ``region "flg" is not in `regions` `` | the mesh's `region` is not one the file declares |
| ``bone "cap" is not in `bones` `` | an influence names a bone the file does not have |
| `vertex 3 has an influence with weight -1 that is negative` | weights are not negative |
| `vertex 3's weights sum to 0; they need to sum above 0` | a vertex has no influence, or only weight 0: it would have nowhere to be |
| ``Unexpected field named `hul` in `Mesh` ..`` | a misspelt field fails, naming the ones it knows |
| `no skin has an attachment "flg" in slot "flag"; a deform moves the attachment its slot shows under that name` | the deform's name is in no skin under that slot (check the spelling, and the slot) |
| `key 1 holds 11 values; a key holds an x and a y for each position` | a key has an odd count: a number is missing from an x, y pair |
| `key 2 holds 2 values where key 1 holds 12; every key is empty or as long as the others` | the keys of one deform disagree on the mesh's size |
| `another timeline already keys slot "flag"'s "flag" deform` | an animation has two deforms of one slot and name |

In code, a mesh whose `region` is past the regions added fails `build` with `its region 3 is out of range; there are 1 regions` and an influence of a bone that is not there with `vertex 2 has an influence of bone 9, which is not a bone; there are 2`.

### Drawing a mesh

A mesh draws in its slot's place in the draw order, as a region of the same image would: among the regions before and after it, under a draw-order key too. **The same pixels as a region.** A mesh of two triangles that cover a region's rectangle draws what the region does:

- colour: the slot's times the mesh's times the renderable's `tint`, and `alpha`; the slot's blend, else the renderable's;
- `lighting`, `filter`, `emission`, `flip_x` and `flip_y`, a mirrored entity or bone;
- `z_order`, `YSort`, a `Floor` and its lift, `RenderLayers`, and interpolation between steps (a weighted vertex follows the bones that hold it);
- the **drop shadow**, in the mesh's own shape and not its box (it is darker where parts overlap, as for regions);
- **the entity's built-in effects**: a hit flash (`SpriteEffect::flash`), a colour shift, silhouette, grayscale, invert, pixelate, wave and dissolve change the regions and the meshes beside them alike. **A game's own sprite shader** (on the renderable or in a `Material`) reaches meshes too, with the same `in.uv` over the mesh's region.

A mesh is drawn only if a camera sees its box, worked out from where the skinning put its vertices, so a mesh carried into view by a bone shows even when its entity is off screen. A mesh whose image is not loaded draws what a region of it does (the checker in a debug build, nothing in a release build). A vertex skinned to a place that is not a finite number draws no mesh, and nothing panics.

**Limits of drawing a mesh.**

- **An effect that grows the sprite's quad leaves the meshes plain.** An outline, a glow or a directional outline draws round the part's rectangle, and a mesh has its own shape: the regions take the effect, the meshes draw without it, and the log says so once in a run, naming the first such entity.
- **Pixel-art snapping does not apply to a mesh's vertices.** On a pixel-art target a region's rectangle is snapped to whole pixels and a mesh's triangles are drawn where the bones put them, so the two agree only where both stand on whole pixels.
- **Each mesh is a draw of its own**, and a drop shadow makes it two. A mesh cannot join the draw before or after it, so a skeleton of one run of regions with one mesh among them is three draws. Keep a character's meshes to those that must bend, and its other parts regions on one atlas.
- **The first frames of a game's sprite shader** may draw a mesh plain: the shader is checked for meshes separately from quads, so the meshes can come a few frames after the regions have turned.
- A mesh has at most 65,535 vertices (its triangle indices are 16 bits).
- Not built: linked meshes (a mesh that borrows another's vertices) and clipping attachments.

**What a mesh costs.** The engine skins a mesh on the CPU in the frame, from the bones as that frame draws them, and keeps its scratch between frames: a steady frame allocates nothing of the engine's own with meshes drawn and deforms playing. Measured on an Apple M5 Pro that was busy with other work, so rounded: a weighted mesh of 60 vertices between two runs of regions adds about 1.6 microseconds to a frame's render step, and a deform playing on it about 0.55 microseconds to a skeleton's fixed step. Those are a mesh's own. For scale, a whole crowd of 200 skeletons of 25 regions each took about 490 microseconds a fixed step and about 165 microseconds of a frame's drawing with regions alone, and about 600 and about 285 with that mesh and a deform on every one. The wgpu graphics layer on Metal allocates about four times a mesh a frame for the extra pipeline changes; that is wgpu's, and costs nothing that shows.

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

Other builder calls: `skin(name)` declares a skin, `mix(from, to, seconds)` sets a pair's mix, and `ik(name, bones, target, IkSetup)` and `transform_constraint(name, bones, target, TransformSetup)` add constraints in the order they apply (Constraints, below). The timelines are `Rotate`, `Translate`, `Scale`, `Shear` (`Vec2` keys), `TranslateX`, `TranslateY`, `ScaleX`, `ScaleY`, `ShearX`, `ShearY` (`f32` keys), `Attachment { slot, frames: Vec<(f32, Option<Arc<str>>)> }`, `Color { slot, frames }`, `Deform { slot, attachment: Arc<str>, frames: Vec<(f32, Vec<f32>, SkeletonCurve)> }` (Meshes, above), `DrawOrder { frames: Vec<(f32, Option<Vec<SlotId>>)> }`, `Event`, and a constraint's `Ik { constraint, frames }` and `TransformMix { constraint, frames }` (Constraints, Keys). Meshes are `SkeletonAttachment::Mesh` (Meshes, above). Built data is immutable: `bones()`, `slots()`, `regions()`, `skins()`, `skin(name)`, `animations()`, `animation(name)`, `events()`, `bone(name)`, `slot(name)`, `constraints()`, `constraint(name)`, `mix(from, to)` and `default_mix()` read it.

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
- The skins, attachments, bone overrides and constraint overrides set on the `Skeleton` are kept too: set those the new life relies on, as for any pooled component.

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
- To make a limb reach a point (a hand on a ledge, feet on the ground), give the rig an IK constraint and move its target bone with a world override (Constraints, next): an override sets one bone, a constraint works out the joints between.

## Constraints

A **constraint** moves bones after animation by a rule instead of by keys. Animation poses the skeleton as usual; then each constraint looks at a **target bone** and changes its own bones to suit it, in every step. There are two kinds:

- an **IK constraint** (inverse kinematics: working out the joints from where the tip must go) turns one bone, two bones or a chain of bones so the last one's tip reaches the target, or, for one bone, points at it;
- a **transform constraint** makes bones copy the target's rotation, place, scale or shear, each by its own amount.

| You want | Use |
|----------|-----|
| A head, an eye, a turret or a gun arm that looks at something | IK, one bone: it turns to point at the target. Pointing needs no `length` |
| A leg or an arm with one joint (knee, elbow): feet on uneven ground, a hand on a ledge, a hand reaching for the pointer | IK, two bones: the joint bends to the side you choose and the tip lands on the target |
| A rope, a cable, or a tail or tentacle held in a one-sided curve, reaching for something | IK, a chain of 3 to 16 bones (read its limits first: Chains, below) |
| A part that follows another part: a pelvis that turns with the trunk, gears, a tray kept level, a second hand on a two-handed grip | A transform constraint |
| One bone set to a value the game works out | A bone override (Bone overrides, above) is simpler |

A constraint is data: it lives in the rig, so every animation gets it, and an animation does not need keys for what the constraint does.

### Target bones

**A target is a bone.** Give a constraint a bone of its own as its target, usually a child of the root with no image, and move that bone: an animation can key it, and a game moves it with a world-space bone override. The constraint reads the target where it is after animation, the game's overrides and any constraint listed before it, in the same step, so the bones follow it with no lag. An IK constraint reaches for the target bone's origin; a transform constraint copies the target's whole transform.

<!-- given: hero: Entity, ground: Vec2 -->
```rust
// Put the foot's target where the foot should stand, in the world
if let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero) {
  skeleton.override_bone(
    "foot_target",
    BoneOverride {
      translation: Some(ground),
      space: BoneSpace::World,
      ..Default::default()
    },
  );
}
```

- A world-space override holds its point while the entity and the bones above the target move, and it is worked out through the flip, so a target put at a point in the world is reached whichever way the skeleton faces.
- The target cannot be one of the constraint's bones, nor below one of them: a bone cannot follow what it carries. A target above them is allowed.
- A target moved from input read each frame (the pointer) is read in the fixed step, so the bones follow it at the step's rate and are drawn blended between steps, as every bone is.
- **A game cannot read where animation puts a bone in the step under way**, only where the last step left it (One step behind, below). To put a keyed foot's target on the ground, work out the foot's place yourself, as the demo's walker does, or accept a step's lag.

### IK settings

| Setting | Default | Applies to | What it does |
|---------|---------|------------|--------------|
| `mix` | 1 | all | How far from the animated pose to the solved one, 0 to 1. At 0 the constraint is off; between, each bone's rotation is blended the short way round |
| `bend_positive` | true | two bones | Which way the joint bends. Positive is clockwise on screen, judged in the space of the first bone's parent: for a leg hanging down from an unturned hip, a knee pointing toward +x (a character facing right). A flipped or mirrored skeleton draws the limb mirrored, so a character turned round keeps its knees. The bones' own scales do not change the side |
| `softness` | 0 | two bones | A distance short of full reach. While the target is within it of full reach, the tip slows as the limb straightens and never snaps straight; a target past full reach is followed by a tip that nears full reach and stays a little short of it, the joint still bent. 0: the tip follows the target all the way |
| `stretch` | false | one or two bones | When the target is beyond reach, scale the first bone along its length so the tip reaches it. On two bones, only with `softness` 0 and a first bone whose own scale is even and unsheared |
| `compress` | false | one bone | When the target is nearer than the bone's length, scale the bone down so its tip is on it |
| `uniform` | false | one or two bones | With stretch or compress, scale the bone across its length by the same factor, so it grows or shrinks evenly. It is not keyed or overridden: it takes effect whenever stretch or compress is on, by the setup, a key or the game's override, so a setup may set it with neither |

- **Every bone an IK constraint turns inherits normally** (`BoneInherit::Normal`), and each bone after the first is an immediate child of the one before.
- **A bone's `length` is where its tip is.** Two bones and a chain reach with the last bone's tip, so the last bone needs a length above 0; so does a lone bone that stretches or compresses. A bone that only points needs none.
- **IK writes only rotations and scales:** each bone's rotation, and under stretch or compress the first bone's x scale (and its y scale with `uniform`). The bones' places and shears are never changed.
- A target out of reach straightens the limb toward it (or stretches it); a target too near folds the limb as far as it goes. A target exactly on the first bone's origin gives no way to turn, and the bones keep the pose they had.

### A worked leg

A leg of two bones, 40 and 30 long, hanging from a hip, with a target bone for its foot:

```ron
(
  bones: [
    (name: "root"),
    (name: "hip", parent: "root", y: -70),
    // The thigh points straight down (90 degrees is clockwise from +x, and y is down)
    (name: "thigh", parent: "hip", rotation: 90, length: 40),
    (name: "shin", parent: "thigh", x: 40, length: 30),
    // The foot's target: a bone of its own, a child of the root, 50 below the hip
    (name: "foot_target", parent: "root", y: -20),
  ],
  constraints: [
    Ik(name: "leg", bones: ["thigh", "shin"], target: "foot_target"),
  ],
)
```

The target is 50 from the hip and the leg is 70 long, so the knee bends. The thigh, the shin and the line from the hip to the target make a triangle of sides 40, 30 and 50, and the knee is where the thigh and shin meet: 32 down the line to the target and 24 to its side, at (24, -38) in the skeleton's space. With `bend_positive` (the default) that side is +x, the way a character facing right looks; `bend_positive: false` puts the knee at (-24, -38) instead. The shin's tip is on the target.

- Move `foot_target` farther than 70 from the hip and the leg lies straight toward it, the tip short of it; add `stretch: true` and the thigh grows along its length until the tip is there.
- Bring the target nearer than 10 (40 less 30) and the leg folds shut, its tip as near as it gets.
- Animate the leg as usual: the constraint starts from the animated pose and replaces the turns, so a walk keyed on the thigh and shin is overruled while `mix` is 1, and shows through at a lower mix.
- **Let a foot go in the air** by turning its constraint off with a mix of 0: set in `update`, it applies in the skeletons' step that follows, which leaves the leg to animation (and to any bone override the game puts on it).

<!-- given: hero: Entity, airborne: bool -->
```rust
if let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero) {
  let mix = if airborne { 0.0 } else { 1.0 };
  skeleton.override_constraint(
    "leg",
    SkeletonConstraintOverride::Ik(IkOverride { mix: Some(mix), ..Default::default() }),
  );
}
```

The same leg in code:

```rust
fn leg() -> Option<SkeletonData> {
  let down = std::f32::consts::FRAC_PI_2; // radians in code
  let built = SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone("hip", Some("root"), BoneSetup { y: -70.0, ..Default::default() })
    .bone("thigh", Some("hip"), BoneSetup { rotation: down, length: 40.0, ..Default::default() })
    .bone("shin", Some("thigh"), BoneSetup { x: 40.0, length: 30.0, ..Default::default() })
    .bone("foot_target", Some("root"), BoneSetup { y: -20.0, ..Default::default() })
    .ik("leg", &["thigh", "shin"], "foot_target", IkSetup::default())
    .build();
  match built {
    Ok(data) => Some(data),
    Err(error) => {
      warn!("the leg: {error}");
      None
    }
  }
}
```

`IkSetup { mix, softness, bend_positive, compress, stretch, uniform }` has the defaults of the table above.

### Two bones: exact, with one limit

Two bones are solved exactly as a triangle, under any parent: a parent that is turned, squashed, sheared or mirrored is handled, because the solve works in the first bone's parent's space. The second bone need not lie on the first bone's axis, and the second bone's own scale and shear are allowed.

**The limit is the first bone's own scale.** When the first bone is scaled unevenly (`scale_x` and `scale_y` differ) or sheared, the length of the second part changes as it turns, and the tip misses its target, along the shin, by up to the difference: for a second part `L` long under a first bone scaled `(a, 1)`, at most `L × (a - 1)`. On a limb of parts 40 and 30 the worst miss is 7.1% of the limb's straight reach at a scale of `(1.2, 1)`, 14% at `(1.5, 1)` and 21.6% at `(2, 1)`; the median miss over targets at random is 2% at `(1.2, 1)` and 7% at `(2, 1)`. Where such a limb passes from bent to straight or folded its tip follows the target steeply, though without a jump. **Keep the thigh's own scale even**: scale a parent bone, or the whole entity, instead.

Stretch on two bones lays both along the line to the target; with the second bone off the first bone's axis and without `uniform`, the tip falls a little short (3.23 short of a target 120 away, for a second bone at (40, 12) and 30 long).

### Chains

A chain is three to sixteen bones, each an immediate child of the one before, whose last tip reaches for the target. It has `mix` only: `bend_positive: false`, `softness`, `stretch`, `compress` and `uniform` fail to load on a chain. A chain is the engine's own; Spine has none.

```ron
  // In a file's bones: four tail bones, each 20 long, curled the positive way
    (name: "tail_1", parent: "hip", x: -20, rotation: 160, length: 20),
    (name: "tail_2", parent: "tail_1", x: 20, rotation: 20, length: 20),
    (name: "tail_3", parent: "tail_2", x: 20, rotation: 20, length: 20),
    (name: "tail_4", parent: "tail_3", x: 20, rotation: 20, length: 20),
    (name: "tail_target", parent: "root"),
  // And in its constraints
    Ik(name: "tail", bones: ["tail_1", "tail_2", "tail_3", "tail_4"], target: "tail_target"),
```

**How it moves.** The chain keeps the shape its animation gives it and bends from there. A target farther than the animated tip opens the curl toward straight, every bend scaled down by one factor; a nearer target curls it further, the same turn added at every joint, as an arc. The shape is chosen by the target's distance from the first joint alone, and the whole chain is then turned about that joint to point the tip at the target. So the **animation decides which side the chain curls to**, a chain bent one way stays bent that way, and a target that moves does not make it pop. A chain animated perfectly straight has no side of its own and curls the positive way (clockwise on screen). The same pose and target always give the same result.

**What a chain is fit for:** a rope, chain or cable hanging straight or curved one way, or a tail or tentacle held in a one-sided curve, reaching for a moving target from about its nearest shape (typically 0.1 to 0.3 of its length from its root) out to beyond its reach, where it lies straight toward the target.

**What it is not fit for:**

- **A side-to-side or S-wave sway while its target is nearer than its tip.** The chain curls the way that brings its tip nearer, and for an S-shaped pose that way flips as the wave travels: it flips 0.2 to 0.7 of its length to the other side, twice a cycle. Swinging through straight flips it too. A chain that is to reach inward is kept bent one way throughout its animation, as a limb is given a bend direction. A target farther than the tip only opens the pose, and does not flip it.
- **Touching points near its own root.** It curls and does not fold: it reaches inward only as far as an arc of its bones curls. Equal bones bent evenly curl onto their root; others stop short, about a tenth of the chain's length away at the median and a third or more for one pose in ten, and a nearer target is not reached (the chain takes its nearest shape, pointed at the target). Of targets drawn at random from 5% to 30% of the length (among those its bones could reach at all), 61% to 91% are met, by pose and by count of bones; from half the length out, all but one in a thousand.
- **A pose coiled past half a turn reaching outward.** A chain whose bones point more than half a turn apart can jump as its target moves outward: a quarter to a third of its length or more (half its length for sixteen bones wound past a full turn).
- **A target passing over its root.** On its first joint the target gives no way to turn; either side of it the chain points opposite ways, so it turns half round as the target passes over. Missing the root by a thousandth of the length turns it round quickly, without a jump.

A chain also settles steeply into its nearest shape (its joints move fast for a little movement of the target as that shape is reached), and it assumes every bone but the last scales evenly and is unsheared: otherwise the tip may miss.

### Transform constraints

A transform constraint names its bones (one or more), its target, and:

- **six mixes** (`rotate`, `x`, `y`, `scale_x`, `scale_y`, `shear_y`), each how far that property goes: 0 leaves it, 1 copies it fully. A mix may be below 0 or above 1: a gear that turns the other way at twice the speed is a rotate mix of -2;
- **six offsets** (`rotation`, `x`, `y`, `scale_x`, `scale_y`, `shear_y`), added to the target's values before they are copied: a bone held a quarter turn off its target, or 10 to its side. Angles are degrees in files and radians in code;
- **two switches**, `local` and `relative`, that make four modes:

| `local` | `relative` | Mode | What each property does |
|---------|------------|------|------------------------|
| false | false | World (the default) | The bone's transform in the skeleton's space moves toward the target's: its place toward the point `(offset x, offset y)` in the target's own axes, its turn the short way toward the target's plus `offset.rotation`, each axis's length toward the target's plus the offset, its shear toward the target's plus the offset |
| false | true | World, relative | The target's values are added to the bone's, times the mix: its place moves by the target's point, it is turned by the target's turn plus the offset, each axis's length is multiplied by `1 + (target's - 1 + offset) × mix`, and its shear has the target's plus the offset added |
| true | false | Local | The bone's own values (what its setup and keys set, relative to its parent) move toward the target's own values plus the offsets: `value + (target's + offset - value) × mix` for rotation, x, y, both scales and the y shear |
| true | true | Local, relative | The target's own values plus the offsets are added to the bone's, times the mix; each scale is multiplied by `1 + (target's - 1 + offset) × mix` |

- **World or local.** A world mode places the bone where the target is in the skeleton (a hand held at a post, a tray kept level whatever the arm does), whatever the bone's inherit mode, and leaves its own values alone. A local mode works on the numbers animation sets, so it copies motion rather than place (a pelvis turning with the trunk, a gear following a gear).
- **A local rotation is the plain number**, never taken the short way: a gear turning through its target's full turns follows smoothly. A world mode reads the way an axis points, which is only known within a turn.
- **A local mode reads the target's own values**, as animation, the game's overrides and earlier constraints left them. A world mode's result is not in a bone's own values, so a target that an earlier world-mode constraint holds is copied as its own values stand, not where it is held.
- **Mirroring stays the bone's own.** A world mode never mirrors a bone or undoes its mirror. A mirrored target (its axes reflected, as a negative scale on one axis does) mirrors the rotation and shear offsets; a target flipped by a negative `scale_x` reads as turned half a turn, so a bone copying its rotation points the way the target's x axis now points.
- A constraint's bones are taken in bone order, so a bone and its own descendant can both be in one constraint: the descendant is read where the bone above it now puts it.
- A local mode never touches the x shear.

**The hero's pelvis** in the skeleton demo carries the band of the hips. The legs hang from the hip, and the trunk turns on it, so a band that stayed level would show a gap when the trunk leans. One constraint turns the pelvis six tenths of the way with the trunk, in every animation:

```ron
  constraints: [
    Transform(name: "pelvis", bones: ["pelvis"], target: "torso", mix: (rotate: 0.6), offset: (rotation: 90), local: true),
  ],
```

The torso's setup rotation is -90 (its bone runs up the spine) and the pelvis's 0, so the offset of 90 makes the two agree at the setup pose. Local: the pelvis's own rotation moves 0.6 of the way to the torso's own plus 90, so for a lean of 20 degrees it turns 12.

**Gears.** A small gear that meshes with a big one with twice its teeth turns twice as fast the other way: a local relative rotate mix of -2 adds the big gear's own rotation times -2 to the small one's.

```ron
(
  bones: [
    (name: "root"),
    (name: "big", parent: "root", length: 40),
    (name: "small", parent: "root", x: 60, length: 20),
  ],
  constraints: [
    Transform(name: "gears", bones: ["small"], target: "big", mix: (rotate: -2), local: true, relative: true),
  ],
  animations: {
    // Only the big gear is keyed: one turn in two seconds
    "spin": (duration: 2.0, bones: { "big": (rotate: [(0.0, 0.0), (2.0, 360.0)]) }),
  },
)
```

Relative adds the target's whole own rotation, its setup rotation with it: give the driving gear a setup rotation of 0, or take it back off with the offset.

**A part held level.** The demo's reaching arm keeps its tray level with a world rotate mix of 1 toward a `horizon` bone, an unturned child of the root, listed after the arm's IK so it levels the hand where the reach has put it:

```ron
  constraints: [
    Ik(name: "arm", bones: ["upper", "fore"], target: "target", bend_positive: false),
    Transform(name: "level", bones: ["hand"], target: "horizon", mix: (rotate: 1)),
  ],
```

In code, `TransformSetup { mix: TransformMix, offset: TransformOffset, local: bool, relative: bool }` (`Default`: every mix and offset 0, the world mode), with `TransformMix::ALL` for every mix at 1:

```rust
fn pelvis_follows_torso(builder: SkeletonDataBuilder) -> SkeletonDataBuilder {
  let follow = TransformSetup {
    mix: TransformMix { rotate: 0.6, ..Default::default() },
    offset: TransformOffset { rotation: 90.0_f32.to_radians(), ..Default::default() },
    local: true,
    ..Default::default()
  };
  builder.transform_constraint("pelvis", &["pelvis"], "torso", follow)
}
```

### The order

**Constraints apply in the order the data lists them**, after animation and the game's bone overrides, and after each one the bones it moved, and everything below them, are placed again before the next is read. So:

- **Of two constraints on one bone, the later one's result stands.**
- **A later constraint sees a target an earlier one moved** where it now is; listed the other way round, it sees it where animation left it. List the reach before the levelling, the legs before what follows the feet.
- **A world-mode result is lost when a later constraint moves a bone above it:** that bone is placed again from its own values, which a world mode did not change. IK and local-mode results are kept, since they are in the bones' own values.

### Constraints and bone overrides

- **Overrides come first.** A bone override is applied before any constraint, so an override on a target bone steers its constraint in the same step.
- **A world override below an IK bone still holds.** When a constraint moves a bone above an overridden bone, the override is applied afresh where the bone now hangs: a foot held level in the world under a shin that IK turns stays level.
- **A constraint has the last word on its own bones.** An override on a bone the constraint turns is where the solve starts (an override and a mix of a half end half way from the override to the solved turn), and the constraint's result is kept.
- **A mix of 0 turns a constraint off:** an IK constraint at a mix of 0, or a transform constraint with every mix 0, is passed over, and its bones are left to animation and the game's bone overrides, exactly as if the rig had no constraint. It does not matter whether the 0 is the setup's, a key's or an override's.

<!-- given: hero: Entity, mouse: Vec2 -->
```rust
if let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero) {
  // The arm reaches for the pointer, the elbow down, the tip slowing near full reach
  skeleton.override_bone(
    "target",
    BoneOverride { translation: Some(mouse), space: BoneSpace::World, ..Default::default() },
  );
  skeleton.override_constraint(
    "arm",
    SkeletonConstraintOverride::Ik(IkOverride {
      bend_positive: Some(false),
      softness: Some(20.0),
      ..Default::default()
    }),
  );
  // The tray half way to level; the other five mixes stay as animated
  skeleton.override_constraint(
    "level",
    SkeletonConstraintOverride::Transform(TransformMixOverride {
      rotate: Some(0.5),
      ..Default::default()
    }),
  );
}
```

`override_constraint(name, SkeletonConstraintOverride)` sets the constraint every step until `clear_constraint_override(name)` gives it back to the animation, from the next step:

- `SkeletonConstraintOverride::Ik(IkOverride { mix, softness, bend_positive, compress, stretch })` and `SkeletonConstraintOverride::Transform(TransformMixOverride { rotate, x, y, scale_x, scale_y, shear_y })`, every field an `Option` (`Default`: nothing set). Each value given replaces what the animations key, or the setup's where none does, a switch included; each left `None` is the animated one. Its kind must be the constraint's.
- It shows from the skeleton's next step, as a bone override does. Setting a constraint again replaces its override.
- An IK `mix` is taken within 0 to 1 and a `softness` below 0 as 0; a transform mix outside 0 to 1 is kept. A value that is not finite is ignored, with one warning per skeleton.
- A name the data lacks warns once per skeleton per name (`skeleton constraint "leg" is not in the skeleton's data; its override is ignored`), and so does a constraint of the other kind (`skeleton constraint "leg" is not of its override's kind (IK or transform); the override is ignored`); either does nothing. Before the data has loaded the override is kept by name, and it is kept by name through a reload.
- **Limit: an override on an IK bone changes hands as the mix leaves 0**, where a later constraint moves a bone above that bone. At a mix of 0 the override is the bone's, applied afresh; above 0 the constraint has the last word and the bone is carried round by the bone above it (an eye held at 0.3 rad in the world under a hip a later constraint turns a quarter turn points at 0.3 at a mix of 0, and about 1.6 rad further round at 0.01).

### Keys

An animation can key a constraint's values, and they blend through layers and crossfades as a bone's values do:

```ron
  animations: {
    "jump": (
      duration: 0.8,
      constraints: {
        // The feet let go as the jump leaves the ground and land at the end
        "leg": (ik: [(0.0, (mix: 1)), (0.1, (mix: 0)), (0.7, (mix: 0)), (0.8, (mix: 1))]),
        // The small gear lets go of the big one half way through, back to its own keys
        "gears": (transform: [(0.0, (rotate: -2), Stepped), (0.4, (rotate: 0))]),
      },
    ),
  },
```

- **An IK key holds the five values** a game can override: `mix`, `softness`, `bend_positive`, `compress` and `stretch`. **A transform key holds the six mixes** (`rotate`, `x`, `y`, `scale_x`, `scale_y`, `shear_y`). Offsets, `local`, `relative` and `uniform` are the setup's alone.
- **A value a key leaves out is the constraint's setup value**, not the previous key's: a key is always whole. One animation keys one constraint at most once, as an `ik` or a `transform` list of its kind.
- **The numbers** (`mix`, `softness`, the six mixes) ease to the next key by the key's curve, and blend between animations as bone values do: a higher track replaces by its alpha or adds; a crossfade between two animations that key the constraint moves each number in a straight line from one to the other, with no dip; a crossfade to an animation that keys none fades the values back to the setup's (or the tracks below's), so a limb leaves its target without a pop.
- **The switches** (`bend_positive`, `compress`, `stretch`) are stepped: they change at their key, whatever the curve, and follow the rule for attachment keys. In a crossfade the newest animation with any weight decides; if it keys none for the constraint, the setup's (or the tracks below's) stand from its first step with weight, though the outgoing animation still has most of the weight.
- **An IK mix is held within 0 to 1 and its softness at 0 or more once every track has applied**, not after each, so an additive layer or a bezier's overshoot cannot carry them out of range. A transform constraint's mixes are not clamped.
- The game's `override_constraint` is written over whatever the keys leave.

In code, `SkeletonTimeline::Ik { constraint: SkeletonConstraintId, frames: Vec<(f32, IkValues, SkeletonCurve)> }` and `SkeletonTimeline::TransformMix { constraint, frames: Vec<(f32, TransformMix, SkeletonCurve)> }`. A key gives every value: `IkValues { mix, softness, bend_positive, compress, stretch }`, and `IkValues::of(&setup)` is a setup's. A constraint's id is its place in the order added, from 0:

<!-- given: setup: IkSetup -->
```rust
let lift = SkeletonTimeline::Ik {
  constraint: SkeletonConstraintId(0),
  frames: vec![
    (0.0, IkValues::of(&setup), SkeletonCurve::Linear),
    (0.1, IkValues { mix: 0.0, ..IkValues::of(&setup) }, SkeletonCurve::Linear),
  ],
};
```

### What fails to load

A constraint that cannot work fails the load, naming the constraint (`constraint "leg"`) or the animation's timeline (`animation "jump" timeline 2 (ik)`, or `(transform mix)`), with the reason. The first problem found is reported.

| The problem says | What it means |
|------------------|---------------|
| `another constraint has the same name` | names are unique across IK and transform constraints together |
| `a skeleton has at most 255 constraints` | a skeleton has 255 constraints at most |
| `its bone "thigh" is not a bone`, `its target "foot" is not a bone` | a name the file's `bones` lacks |
| `it lists bone "thigh" twice` | each bone once |
| `it has no bones; it needs at least one` | every constraint moves one bone or more |
| `its target "shin" is one of its bones` | the target is a bone of its own |
| `its target "toe" is below its bone "thigh"; a bone cannot follow what it carries` | the target is not below any of the constraint's bones |
| `it has 17 bones; at most 16` | an IK constraint turns 1 to 16 bones |
| `bone "shin" is not a child of "hip"; each bone after the first must be an immediate child of the one before` | an IK constraint's bones run from parent to child, with none skipped |
| `bone "shin" does not inherit normally; an IK constraint turns only bones that do` | every IK bone has `inherit: Normal` |
| `mix is 1.5; it must be within 0 to 1` | an IK mix is 0 to 1 (a transform mix may be anything finite) |
| `softness is -1; it must not be negative` | softness is 0 or more |
| `bend_positive false applies only to two bones`, `softness applies only to two bones` | they bend a joint, which only two bones have |
| `compress applies only to one bone` | compress is a lone bone's |
| `stretch applies only to one or two bones`, `uniform applies only to one or two bones` | a chain does not stretch |
| `its last bone "shin" has a length of 0; its tip is what reaches the target, so the length must be above 0` | give the last bone of two bones or a chain a `length` |
| `its bone "head" has a length of 0; stretch and compress need a length above 0` | a lone bone that stretches or compresses needs a `length` |
| `mix rotate is inf, not a finite number`, `offset rotation is NaN, not a finite number` | every value finite (an IK setting's name is `mix` or `softness`) |
| `constraint "leg" is an IK constraint; a transform mix timeline keys a transform constraint` | a `transform` list for an IK constraint (or the other way round: `constraint "gears" is a transform constraint; an ik timeline keys an IK constraint`) |
| `another timeline already keys constraint "leg"'s values` | an animation keys a constraint once: an `ik` and a `transform` list for one name clash |
| `key 1's mix is 2; it must be within 0 to 1`, `key 0's compress applies only to one bone` | a key's values are checked as a setup's, each reason beginning `key N's` |
| `key 0's stretch or compress is on, and its bone "head" has a length of 0; stretch and compress need a length above 0` | a key may not switch on what its constraint cannot do |

- An animation's `constraints` naming a constraint the file does not have fails naming the animation: `animation "jump": constraints: "lgs" is not a constraint (or is past the 255th)`.
- A misspelt setting or key value (`strech: true`) is a RON error at its line and column.
- Allowed: a target above the constraint's bones, a transform constraint naming a bone and its own descendant, a transform mix outside 0 to 1, and `uniform` set without stretch or compress.
- In code, a timeline's constraint past those added fails `build` with `constraint 3 is out of range; there are 1 constraints`.

### Limits of constraints

- **Two bones under a first bone scaled unevenly or sheared** miss their target by up to the spread above (7.1% of the reach at `(1.2, 1)`, 21.6% at `(2, 1)`); an uneven ancestor is exact. Stretch on two bones falls a little short with the second bone off the first's axis and no `uniform`.
- **A chain** is for a rope, cable, or a tail or tentacle in a one-sided curve: it flips during an S-wave sway while its target is nearer than its tip, curls rather than folds onto a point near its root, may jump when coiled past half a turn and reaching outward, turns half round as a target passes over its root, and assumes evenly scaled bones (Chains, above).
- **A mix strictly between 0 and 1 can jump.** An IK constraint at such a mix turns each bone part of the short way from its animated rotation, and when the solved rotation passes half a turn from the animated one the short way changes sides: the bone jumps by a full turn times the mix. A world-mode transform constraint at a mix that is not a whole number jumps the same way as its target passes half a turn. At a mix of 1 nothing jumps, and a local mode never does.
- **An override on an IK bone changes hands as the mix leaves 0** (Constraints and bone overrides, above).
- **One step behind:** a scene reads where a constraint put a bone as the last step left it, and cannot read where animation puts a target in the step under way.
- **Not built:** path constraints, physics constraints (springs), and constraints that apply only while a skin is worn: every constraint is always on.

### Cost

**Skeletons without constraints do no new work**, and a steady step allocates nothing with constraints running. Measured on an Apple M5 Pro that was busy with other work (loads of 5 to 7), so the figures are rounded: 200 skeletons, each with two legs of two bones, a bone pointed by one-bone IK and a transform constraint, their three targets moved by world-space overrides every step, took about 550 µs a fixed step against about 500 without the constraints: about a quarter of a microsecond a skeleton. A chain of sixteen bones takes about 2 µs to solve and one of three under 1 µs.

### The demo

The skeleton demo's ninth page, Constraints (`examples/demos/skeleton`, `pages/constraints.rs`): an arm on a stand reaches for the pointer by two-bone IK (with Bend, Softness, Stretch and Mix settings set through `override_constraint`), a transform constraint keeps the tray in its hand level (its rotate mix the Level hand setting), and a walker crosses uneven ground on two-bone legs whose feet its page plants and steps ahead by world-space overrides on their target bones. Its rigs are `assets/reacher.skeleton.ron` and `assets/walker.skeleton.ron`; the hero's pelvis (`assets/hero.skeleton.ron`) is the transform constraint above.

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

An entity with a `Skeleton` and `Renderable::skeleton()` draws the regions and meshes its slots show, back to front in its draw order, where its bones put them (a mesh is drawn as a region is; Meshes, above). **It takes its place among the entities exactly as a sprite does**:

| On the entity | Effect |
|---------------|--------|
| `Renderable`'s `z_order`, `visible`, `lighting`, `filter` | The whole skeleton's, as for a sprite |
| `alpha`, `tint` | Each part's colour is its slot's times its attachment's times the tint; its alpha those three's times `alpha` |
| `blend` | For slots that blend `Normal`; a slot with a blend of its own keeps it |
| `emission` | Every part's, regions and meshes alike |
| `flip_x`, `flip_y` | Mirror the skeleton about its entity's origin, its bones, points, boxes and sockets with it. It lands the other way round at once |
| `shader` (or the entity's `Material`) | Applied to each region and mesh on its own (below) |
| `YSort` | Sorts it among y-sorted sprites and tile layers by where it stands (below) |
| `Floor` | Draws it lifted on its storey, as a sprite is (`ryo-tilemap`) |
| `RenderLayers` | Which cameras draw it |
| `Interpolated` (added by `Skeleton`) | Bones, sockets and the entity drawn between fixed steps, together; taken off, all three move at the step rate |

`Renderable`'s `image`, `anchor` and `target` are not used: a skeleton's origin sits on its entity's position.

- **A skeleton's entity is interpolated.** `Skeleton` adds `Interpolated` to its entity, so the entity, its bones and its sockets are drawn between fixed steps and move as one. Remove `Interpolated` for a skeleton drawn at the step rate (below): bones are drawn between steps only when the entity is. `ctx.skip_interpolation(entity)` after a teleport lands the bones and sockets with the entity; without it a skeleton moved a long way in one step is drawn sliding there for that step, as any interpolated entity is.
- **Y-sorting.** A y-sorted skeleton's sort key is its entity's own y plus `YSort::sort_offset`: where it stands. Put the rig's origin at its feet. `YSort::foot_offset` measures down a sprite's image and plays no part. The key does not move with the pose or a flip.
- **Shadows.** A blob or contact shadow is a sprite's, under where the entity stands, and suits a character. **A drop shadow is cast part by part, so where parts overlap it is darker than a sprite's even one**: a half-black shadow on a white floor leaves 187 of 255 under one part and 136 where two overlap. A drop shadow's `scale` shrinks the parts' shadows toward where the skeleton stands; a mesh casts its own shape. A part faded by its slot casts as much less.
- **A sprite shader applies part by part.** A built-in effect or a game's own shader is run for each region and mesh: a flash whitens every part, a dissolve dissolves each, and **an outline or a glow outlines each region, not the figure, and leaves the meshes plain** (Meshes, Limits of drawing a mesh). A game shader's `in.uv` runs over the part's own rectangle of its image.
- **A negative scale mirrors**, on a skeleton as on a sprite: a `Transform` scaled by -1 on one axis (on the entity or above it) draws what is under it mirrored, and both axes negative is a half turn. A scale that changes sign in a step is drawn through between steps (it closes to nothing and opens the other way); `flip_x` lands at once, which is what a character turning round wants.
- **Use one atlas.** A skeleton whose regions share an image and blend is one draw item, whatever its slot count; each change of image or blend along the draw order starts another, and so does each mesh, which is an item of its own. A game shader on the entity makes each region an item of its own.
- A skeleton's parts are culled together, for each camera, by the box round them. Off screen it keeps animating, so nothing pops when it comes back into view.
- A skeleton moved while no step runs (a paused clock) is drawn where its entity is drawn in that frame, as a sprite is: its bones follow the entity with no step to pose them. After a teleport, `ctx.skip_interpolation(entity)` draws it at its new place at once, as for any interpolated entity.

<!-- given: hero: Entity -->
```rust
// Step-rate drawing: take off what `Skeleton` added. A spawn's bundle cannot
// leave it out, and inserting a `Skeleton` on the entity again adds it again
ctx.world.entity_mut(hero).remove::<Interpolated>();
```

## What the engine does

- **The step.** In each fixed step, after `Scene::update`, physics and transform propagation and before particles (`FixedSet::Skeletons`), every skeleton whose data has loaded, in entity order: advances its animation and applies it over the setup pose, writes the game's attachments, constraint overrides and bone overrides over that, applies its constraints in order, places its bones under its entity's transform, places its sockets and what is below them, and sends its `SkeletonEvent`s and `SkeletonTrackEvent`s.
- **A pose in the frame it appears.** A skeleton added, or whose data landed or reloaded, in a frame that runs no fixed step (the clock paused, hitstop, slow motion, a display faster than the step) still has its pose and its sockets their places by the end of that frame. The pose is its animation's first moment, not the setup pose, when an animation is set. Nothing advances and no message is sent until a step runs, and none is lost.
- **Game time.** Skeletons advance in fixed steps: a paused clock and hitstop freeze them, slow motion slows them, and the animation is the same at any frame rate.
- **They freeze with their scene.** A skeleton belongs to the scene whose code or systems added it, and animates while that scene updates: under a scene that stops it (a pause menu pushed over the game) it neither advances nor sends anything, and carries on where it was when its scene updates again. Nothing is lost to the hold: what its calls caused meanwhile is sent in its first step after. A skeleton added outside every scene is held while any scene is.
- **A scene transition does not stop them.** Skeletons keep animating through a fade or a slide, as particles and physics do, while every scene's `update` waits for the transition to end. What a skeleton sends meanwhile is not seen by `update`, and that includes a scene released from under a menu that pops with a fade, from the fade's midpoint to its end (Events, Scene transitions).
- **Disabled.** A `Disabled` skeleton is not stepped or drawn. Enabled again it carries on where it stopped, which is what a pool hands out (Playing animations, From a pool).
- **Hot reload.** A saved file reloads into every skeleton showing it at their next step. Entries are kept by their animation's name, with their times; skins, attachments, bone and constraint overrides and sockets are found again by name, so a save that reorders, renames or removes constraints keeps playing. An animation the save removed clears the track it was playing on, with one warning; a queued or fading entry of it is dropped and the track plays on. A name the new data lacks warns once and stays kept, so a later save that has it brings it back. A broken save keeps the last good skeleton, with one warning. Data changed in code (`ctx.assets().get_mut`) is followed the same way.
- **Cost.** A CPU pose on the main thread, drawn through the sprite batch. On an Apple M5 Pro, 200 skeletons of 30 bones and 25 parts each, two tracks playing with a crossfade always in progress, cost about 0.5 ms of a fixed step and 0.16 ms of a frame's drawing from one atlas (0.28 ms with an image for each part); a steady step and frame allocate nothing. A crowd wholly off screen still costs its step and nearly all of its drawing work. Constraints cost about a quarter of a microsecond a skeleton for four of them (Constraints, Cost).

## debug:skeletons

In a development build, the console command `debug:skeletons` (`ryo-console`) draws over every posed skeleton: each bone as an amber line its length along it, with a white ring at its origin; the blue outline of each region drawn; each mesh as its triangles (a darker blue) with its hull over them in the regions' blue, where the skinning and the deform have put them; each bounding box its slots show, in green; each point as a magenta cross with a line the way it faces; and each constraint in orange: an IK constraint as a cross at its target with a line from its last bone's tip (its `length` along it) to the target, so a gap shows how far the tip falls short, and a transform constraint as a line from each of its bones' origins to its target. It is drawn where the frame draws the skeleton (between steps, lifted, flipped), so the lines lie on the art. Use it to place parts, to see how a mesh's vertices sit and bend, to see a hit box switch on, and to see where a constraint's target is. A mesh with no `hull` shows its triangles alone. A bone with a `length` of 0 shows as its ring alone. A game built with `ryo2d build` has no console and no overlay, and there is no switch for it in a scene.

## Limits

- **Not built:** path and physics constraints, and constraints bound to a skin; clipping attachments; linked meshes; reading another tool's files (Spine, DragonBones). A rig is written as `.skeleton.ron` or built in code.
- **Constraints:** two bones under an unevenly scaled or sheared first bone miss their target; a chain is fit only for a rope, cable, or a tail or tentacle in a one-sided curve; a mix strictly between 0 and 1 can jump; an override on an IK bone changes hands as the mix leaves 0 (Constraints, Limits of constraints).
- **Size:** at most 255 bones, 255 slots and 255 constraints a skeleton, 16 bones an IK constraint, and tracks 0 to 63 (a higher track warns once per skeleton and does nothing).
- **Game time only:** there is no unscaled mode. A menu's skeleton stands still while the game clock is paused.
- **One step behind** in a scene's `update` (above), and a collider on a socket one physics step late.
- **Scene transitions:** a scene's `update` does not run during one, and does not see what a skeleton sends meanwhile (Events, Scene transitions).
- **A queue of 64:** `add_animation` on a track with 64 entries queued is refused, with one warning per skeleton.
- **Sockets:** a skew is dropped; a socket's sprite is wholly in front of or behind the skeleton; an ordinary child of the skeleton's entity (not a socket, nor below one) does not jump with `skip_interpolation`.
- **Mixing:** a crossfade dips a little where the incoming animation keys a property in another form (a `translate_x` over a combined `translate`), and, in a chain of three or more entries, past an entry that does not key the property. With `time_scale` at 0 no mix advances, so a track given 17 animations meanwhile jumps as its oldest entry ends.
- **Drawing:** a drop shadow is darker where parts overlap; blob and contact shadows stand under the entity and do not follow the pose (while an animation jumps, set a blob shadow's height from a bone, as the demo's `lift_shadow` does); sprite shaders apply part by part, and outlines and glows outline each region and leave meshes plain; `YSort::foot_offset` is not used; no single region is culled (a run of regions is, whole, by the box round it, and a mesh by the box of its own vertices), and a skeleton off screen still builds its quads.
- **Meshes:** an effect that grows the quad (outline, glow, directional outline) leaves them plain, with one warning a run; pixel-art snapping does not apply to their vertices; each is a draw of its own (two with a drop shadow); a game's sprite shader may draw them plain for its first few frames; at most 65,535 vertices a mesh (Meshes, Limits of drawing a mesh).
- **Skins** cannot hide an attachment.
- **Event audio** is the game's to play, and `balance` has nowhere to go in `PlayOptions`.
- **Before the data has loaded:** at most 256 animation calls are kept (one more is ignored and returns `None`, with one warning), and at most 255 attachments, 255 bone overrides and 255 constraint overrides; a `mix_duration` set to 0 on a kept entry counts as unset and takes the data's mix.
- **Warnings are bounded:** a skeleton remembers 64 names it has warned about, and its animation state 64 more; past that a missing name is ignored in silence.
- **Track events from calls** wait for the next step in a list of 1,024; a skeleton that is not stepped while a game keeps calling it drops the oldest, with one warning.
- **A despawned skeleton** sends nothing more: what its last calls caused goes untold.
- **Long runs:** an entry's time is an `f32` that counts every loop. A looping entry left playing moves in steps of 2 ms after 4.6 hours and 8 ms after 18 hours, runs near twice as fast after 3 days and stops after 6. A layer at partial alpha whose bone keeps turning remembers its turns: after 4.5 hours of a turn a second its rotation moves in steps of 0.45 degrees times its alpha, and where it rests depends on how far it has wound. Setting the animation again starts both afresh. One step fires the event keys of at most the last 1,000 loops it crossed, and sends at most 1,000 `Complete`s.
- **Restoring on a loop boundary:** an entry started at a whole number of loops reads 0.0 from `track_time()` until its first step. A game that saves again before that step (loaded, then saved while paused) and restores from it fires the key at 0 a second time.
- **Not covered by the pose in a frame:** a skeleton enabled again (`Disabled` taken off) while no step runs keeps the pose it had until its next step, and its flip too if its renderable was flipped while it was disabled; one whose data changed meanwhile draws nothing until then. A socket given to a skeleton by `ChildOf` alone (its `BoneSocket` already on it) while no step runs is placed in the next step.
- **Inherit modes act inside the skeleton:** the entity's turn, scale and flip apply to every bone, an `OnlyTranslation` or `NoRotationOrReflection` one too.
- **Where it differs from Spine's runtimes**, for a reader who knows them:
  - a crossfade on a higher track does not dip toward the tracks below, and `hold_previous` holds the previous entry's time;
  - a deform is keyed by its slot and the attachment's name, not by a skin, and a key holds an offset for every position of the mesh or none;
  - a queued entry's `delay <= 0` does not take the mix off, so the mix starts at the previous entry's end;
  - an entry queued on an empty track waits out its delay before it starts;
  - `Complete` comes only from the playing entry, never from one mixing out;
  - `clear` returns the track's properties to what is under it, since every step starts from the setup pose;
  - an entry mixing out applies its attachment and draw-order keys only until the entry replacing it has weight, and an entry with no weight applies none;
  - two-bone IK leaves the second bone where it is and the first bone's shear as it is (an uneven or sheared first bone misses instead), an IK chain is the engine's own, and no constraint is bound to a skin;
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

- Start from the skeleton demo (`examples/demos/skeleton`): a page each for a character's crossfades, layers, events with sounds and dust, a sword on a socket with a hit box, skins, aiming and a crowd, over one hand-written rig, and a Meshes page: the hero's cape (a weighted mesh in a `cape` skin, its bones keyed in idle, walk and run) and a banner (an unweighted mesh rippled by a looping deform), with Gait, Cape and Ripple speed settings and a hit flash on Space, and a Constraints page: an arm reaching for the pointer by IK with a tray kept level, and a walker planting its feet on uneven ground (Constraints, The demo). Its `assets/README.md` lays out the cape's and banner's data and the constraint pages' rigs.
- Give every pair of animations that follow each other a mix, or a `default` of 0.1 to 0.2 s: a mix of 0 cuts.
- Name bones, slots and attachments for what they are (`arm_near`, `sole_far`, `hit`): the game refers to all of them by name.
- Put points where things happen (soles, a muzzle, a hand) and read them with `point`, rather than working places out from bones.
- A jump keyed in the animation leaves the entity on the ground: move the entity for travel, and keep in-place motion in the rig.
- For a crowd, give each entry its own `start` and a `speed` a little off 1, and one atlas for the rig.
