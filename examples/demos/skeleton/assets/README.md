# Skeleton demo assets

| File | What |
|---|---|
| `hero.skeleton.ron` | The hero: 17 bones, 28 slots, seven skins, seven animations, a footstep event, a transform constraint that turns its pelvis with its trunk and two springs that swing its cape. Written by hand; its comments say how it is put together |
| `hero.png` | The hero's parts in one 1024 x 744 atlas: three bodies, three outfits, the faces' parts and a cap. The skeleton file names each part's rectangle |
| `cape.png` | The cape the Meshes page puts on the hero, 280 x 292: the image of the mesh in the hero's `cape` skin |
| `banner.skeleton.ron` | The Meshes page's banner: three bones, a pole and a flag. The flag is a mesh of 27 vertices, and the `ripple` animation's deform keys move them. Written by hand, but for the ripple's keys (below) |
| `banner.png` | The banner's flag (296 x 176) and its pole (640 x 20, lying down) in one 656 x 220 image |
| `sword.png` | The sword the Sockets page puts on the hero's weapon bone, 300 x 68, its grip on the left |
| `reacher.skeleton.ron` | The Constraints page's arm on its stand: six bones, an IK constraint that reaches its wrist for a target bone and a transform constraint that keeps its hand level. Written by hand |
| `reacher.png` | The arm's parts in one 262 x 296 image: its upper arm, forearm, hand with a tray, shoulder pivot, elbow cap and stand |
| `walker.skeleton.ron` | The Constraints page's walker: a body on two legs of two parts each, an IK constraint on each leg that puts its foot on a target bone. Written by hand |
| `walker.png` | The walker's parts in one 214 x 102 image: its body, a thigh, a shin with its foot, and a joint cap |
| `tail.skeleton.ron` | The Constraints page's tail: six segments and a sting on a stone mound, a chain IK kept to one side that reaches the sting for a target bone, and a `sway` animation that bends it through straight, into an S and the other way. Written by hand |
| `tail.png` | The tail's parts in one 172 x 104 image: a segment, the sting and the mound |
| `audio/footstep.sound.ron` | The sound the skeleton's footstep event names, which the engine plays for every hero (its `SkeletonSounds`): one of four takes, heard from where the hero is |
| `audio/step-1.wav` to `step-4.wav` | The four takes |

The web build embeds every file here except this one (`src/embedded.rs`); a test checks the list matches the folder.

## The art

The hero's parts are from **Modular Characters** by Kenney (<https://kenney.nl/assets/modular-characters>), licensed **CC0 1.0**. The pack's `license.txt`:

```text
	Character pack by Kenney Vleugels (www.kenney.nl)

			------------------------------

			        License (CC0)
	       http://creativecommons.org/publicdomain/zero/1.0/

	You may use these graphics in personal and commercial projects.
	Credit (Kenney or www.kenney.nl) would be nice but is not mandatory.
```

The parts are assembled as the pack's `instruction.png` shows: hair and a face on a head, a neck, a shirt with a sleeve and a hand each side, the trousers' top under the shirt, a leg each side and a shoe under each. An arm and a leg are one piece each in the pack, so the rig has one bone for each, and one for each hand and shoe. The hero is turned a little to its right, so its two legs are the same piece and its two arms the same piece, the far ones drawn behind and a shade darker.

The cap, the sword, the cape, the banner (its flag and its pole), the reaching arm, the walker and the tail are not from the pack: it has none of them. They were drawn for this demo with ImageMagick, as flat shapes with a little shading in the hero's colours (the cap's yellows, the red of the shirt, the brown of the shoes, the sword's steel and the pole's wood), and are under the repository's licence.

## Making the images

ImageMagick 7 (`magick`). Each command writes the same pixels every time.

```sh
# hat.png: a cap, drawn at twice its size and halved for smooth edges
magick -size 520x168 xc:none \
  -fill '#f2b632' -draw 'ellipse 180,144 176,128 180,360' \
  -fill '#e09f1f' -draw 'path "M 180,16 C 250,24 300,70 318,144 L 356,144 C 350,70 290,20 180,16 Z"' \
  -fill '#c9861a' -draw 'path "M 150,124 L 356,124 C 420,126 480,134 512,148 C 500,162 470,164 440,162 L 150,162 Z"' \
  -fill '#f7d070' -draw 'circle 180,20 180,31' \
  -filter Lanczos -resize 50% PNG32:hat.png

# sword.png: a blade, a guard, a grip and a pommel, the same way
magick -size 600x136 xc:none \
  -fill '#e6ebf2' -draw 'polygon 118,44 520,44 588,68 118,68' \
  -fill '#b8c3d1' -draw 'polygon 118,68 588,68 520,92 118,92' \
  -fill '#8f9bab' -draw 'rectangle 118,66 560,70' \
  -fill '#7a4f2a' -draw 'roundrectangle 22,54 104,82 6,6' \
  -fill '#5f3c1f' -draw 'rectangle 40,54 46,82' -draw 'rectangle 60,54 66,82' -draw 'rectangle 80,54 86,82' \
  -fill '#e8a923' -draw 'roundrectangle 98,10 122,126 8,8' \
  -fill '#c98a14' -draw 'rectangle 110,14 118,122' \
  -fill '#e8a923' -draw 'circle 20,68 20,86' \
  -filter Lanczos -resize 50% PNG32:sword.png

# cape.png: the cloth, then its folds, hem and collar drawn only where the
# cloth is (SrcAtop). The hem is seven scallops
magick -size 560x584 xc:none \
  -fill '#f2b632' -draw 'path "M 156,16 L 404,16 Q 412,16 414,24 L 536,540 Q 499.4,530 462.9,548 Q 426.3,568 389.7,550 Q 353.1,530 316.6,548 Q 280,568 243.4,550 Q 206.9,530 170.3,548 Q 133.7,568 97.1,550 Q 60.6,530 24,540 L 146,24 Q 148,16 156,16 Z"' \
  \( -size 560x584 xc:none \
     -fill '#e09f1f' -draw 'polygon 190,16 204,16 130,584 86,584' \
                     -draw 'polygon 246,16 260,16 238,584 196,584' \
                     -draw 'polygon 300,16 314,16 364,584 322,584' \
                     -draw 'polygon 356,16 370,16 474,584 430,584' \
     -fill '#c9861a' -draw 'path "M 0,500 L 24,500 Q 60.6,490 97.1,510 Q 133.7,528 170.3,508 Q 206.9,490 243.4,510 Q 280,528 316.6,508 Q 353.1,490 389.7,510 Q 426.3,528 462.9,508 Q 499.4,490 536,500 L 560,500 L 560,584 L 0,584 Z"' \
                     -draw 'rectangle 0,0 560,40' \) \
  -compose SrcAtop -composite \
  -filter Lanczos -resize 50% PNG32:cape.png

# flag.png: a red field with a darker hoist, two stripes and a disc
magick -size 592x352 xc:none \
  -fill '#e05848' -draw 'roundrectangle 8,8 583,343 10,10' \
  \( -size 592x352 xc:none \
     -fill '#bf493b' -draw 'rectangle 0,0 40,352' \
     -fill '#f2b632' -draw 'rectangle 40,34 592,58' -draw 'rectangle 40,294 592,318' \
                     -draw 'circle 312,176 312,244' \
     -fill '#e09f1f' -draw 'circle 312,176 312,218' \) \
  -compose SrcAtop -composite \
  -filter Lanczos -resize 50% PNG32:flag.png

# pole.png: a pole lying down, its foot on the left and a ball on its top
magick -size 1280x40 xc:none \
  -fill '#af9360' -draw 'rectangle 0,8 1240,31' \
  -fill '#8f774c' -draw 'rectangle 0,22 1240,31' -draw 'rectangle 1208,8 1240,31' \
  -fill '#f2b632' -draw 'circle 1258,20 1258,2' \
  -fill '#e09f1f' -draw 'path "M 1242,26 A 18,18 0 0 0 1274,26 A 22,22 0 0 1 1242,26 Z"' \
  -filter Lanczos -resize 50% PNG32:pole.png

# banner.png: the flag and the pole, 8 pixels apart
magick -size 656x220 xc:none \
  flag.png -geometry +8+8 -composite \
  pole.png -geometry +8+192 -composite \
  PNG32:banner.png
```

The Constraints page's three figures are drawn the same way, each part at twice its size and halved, then packed with 8 pixels between parts. A limb's part is a bar with rounded ends whose joints are as far apart as its bone is long:

```sh
# upper.png: the upper arm, joint to joint 130 apart, bolts at each joint
magick -size 316x56 xc:none \
  -fill '#f2b632' -draw 'roundrectangle 0,0 315,55 28,28' \
  \( -size 316x56 xc:none -fill '#e09f1f' -draw 'rectangle 0,36 316,56' \) \
  -compose SrcAtop -composite -compose Over \
  -fill '#c9861a' -draw 'circle 28,28 28,37' -draw 'circle 288,28 288,37' \
  -filter Lanczos -resize 50% PNG32:upper.png

# fore.png: the forearm, joint to wrist 120 apart, a bolt at the wrist
magick -size 288x48 xc:none \
  -fill '#f2b632' -draw 'roundrectangle 0,0 287,47 24,24' \
  \( -size 288x48 xc:none -fill '#e09f1f' -draw 'rectangle 0,31 288,48' \) \
  -compose SrcAtop -composite -compose Over \
  -fill '#c9861a' -draw 'circle 264,24 264,32' \
  -filter Lanczos -resize 50% PNG32:fore.png

# elbow.png: the cap over the elbow
magick -size 60x60 xc:none \
  -fill '#c9861a' -draw 'circle 30,30 30,1' \
  -fill '#f7d070' -draw 'circle 30,30 30,16' \
  -filter Lanczos -resize 50% PNG32:elbow.png

# hub.png: the shoulder's pivot on the top of the stand
magick -size 96x96 xc:none \
  -fill '#8f9bab' -draw 'circle 48,48 48,2' \
  -fill '#b8c3d1' -draw 'circle 48,48 48,20' \
  -fill '#8f9bab' -draw 'circle 48,48 48,38' \
  -filter Lanczos -resize 50% PNG32:hub.png

# hand.png: a red hand under a tray with a cup on it, the palm's middle
# (the wrist) at (42, 42)
magick -size 168x112 xc:none \
  -fill '#e6ebf2' -draw 'roundrectangle 60,2 106,38 6,6' \
  -fill '#b8c3d1' -draw 'rectangle 92,6 104,36' \
  -fill none -stroke '#e6ebf2' -strokewidth 6 -draw 'circle 114,18 114,28' -stroke none \
  -fill '#af9360' -draw 'roundrectangle 6,36 162,52 6,6' \
  -fill '#8f774c' -draw 'roundrectangle 6,46 162,52 3,3' \
  -fill '#e05848' -draw 'circle 84,84 84,57' \
  -fill '#bf493b' -draw 'path "M 58,92 A 27,27 0 0 0 110,92 Z"' \
  -fill '#e05848' -draw 'roundrectangle 62,40 106,62 10,10' \
  -filter Lanczos -resize 50% PNG32:hand.png

# post.png: the stand, a steel pole on a wooden foot, 280 tall
magick -size 120x560 xc:none \
  -fill '#8f9bab' -draw 'rectangle 46,24 74,540' \
  -fill '#b8c3d1' -draw 'rectangle 50,24 58,540' \
  -fill '#af9360' -draw 'roundrectangle 4,524 116,559 10,10' \
  -fill '#8f774c' -draw 'roundrectangle 4,546 116,559 6,6' \
  -filter Lanczos -resize 50% PNG32:post.png

# reacher.png: the arm's parts
magick -size 262x296 xc:none \
  upper.png -geometry +8+8 -composite \
  fore.png -geometry +8+44 -composite \
  hand.png -geometry +8+76 -composite \
  hub.png -geometry +100+76 -composite \
  elbow.png -geometry +156+76 -composite \
  post.png -geometry +194+8 -composite \
  PNG32:reacher.png

# body.png: the walker's body, its hip at (48, 62) on its lower edge
magick -size 192x144 xc:none \
  -fill '#e05848' -draw 'roundrectangle 4,4 188,124 36,36' \
  \( -size 192x144 xc:none -fill '#bf493b' -draw 'rectangle 0,94 192,144' \) \
  -compose SrcAtop -composite -compose Over \
  -fill '#f2b632' -draw 'roundrectangle 104,28 176,64 16,16' \
  -fill '#f7d070' -draw 'roundrectangle 114,34 150,44 5,5' \
  -fill '#8f9bab' -draw 'circle 96,124 96,104' \
  -fill '#b8c3d1' -draw 'circle 96,124 96,114' \
  -filter Lanczos -resize 50% PNG32:body.png

# thigh.png: hip to knee 70 apart
magick -size 188x48 xc:none \
  -fill '#f2b632' -draw 'roundrectangle 0,0 187,47 24,24' \
  \( -size 188x48 xc:none -fill '#e09f1f' -draw 'rectangle 0,31 188,48' \) \
  -compose SrcAtop -composite \
  -filter Lanczos -resize 50% PNG32:thigh.png

# shin.png: the knee's end at (10, 12) and a round foot whose edge is 70
# along from it
magick -size 160x48 xc:none \
  \( -size 160x48 xc:none -fill '#f2b632' -draw 'roundrectangle 0,4 140,43 20,20' \
     \( -size 160x48 xc:none -fill '#e09f1f' -draw 'rectangle 0,32 160,48' \) \
     -compose SrcAtop -composite \) -compose Over -composite \
  -fill '#7a4f2a' -draw 'circle 136,24 136,0' \
  -fill '#9a6a3e' -draw 'circle 130,16 130,8' \
  -filter Lanczos -resize 50% PNG32:shin.png

# joint.png: the cap over a hip or knee
magick -size 44x44 xc:none \
  -fill '#8f9bab' -draw 'circle 22,22 22,1' \
  -fill '#b8c3d1' -draw 'circle 22,22 22,12' \
  -filter Lanczos -resize 50% PNG32:joint.png

# walker.png: the walker's parts
magick -size 214x102 xc:none \
  body.png -geometry +8+8 -composite \
  thigh.png -geometry +112+8 -composite \
  shin.png -geometry +112+40 -composite \
  joint.png -geometry +112+72 -composite \
  PNG32:walker.png

# segment.png: a segment of the tail, red with a yellow band and a bolt
# at its root joint (20, 20); its other joint, the middle of its far end,
# is at (70, 20), 50 along
magick -size 168x80 xc:none \
  -fill '#e05848' -draw 'roundrectangle 0,0 167,79 40,40' \
  \( -size 168x80 xc:none -fill '#bf493b' -draw 'rectangle 0,54 168,80' \
     -fill '#f2b632' -draw 'rectangle 78,0 102,80' \
     -fill '#e09f1f' -draw 'rectangle 78,54 102,80' \) \
  -compose SrcAtop -composite -compose Over \
  -fill '#8f9bab' -draw 'circle 40,40 40,26' \
  -fill '#b8c3d1' -draw 'circle 40,40 40,33' \
  -filter Lanczos -resize 50% PNG32:segment.png

# sting.png: the tail's sting, its bolt at (16, 18) and its point at (63, 18)
magick -size 128x72 xc:none \
  -fill '#f2b632' -draw 'polygon 30,12 126,36 30,60' \
  -fill '#e09f1f' -draw 'polygon 30,36 126,36 30,60' \
  -fill '#c9861a' -draw 'circle 32,36 32,10' \
  -fill '#8f9bab' -draw 'circle 32,36 32,22' \
  -fill '#b8c3d1' -draw 'circle 32,36 32,29' \
  -filter Lanczos -resize 50% PNG32:sting.png

# mound.png: the stone mound the tail rises from, its foot's middle at
# (60, 40) and the hole in its top 27 above it
magick -size 240x80 xc:none \
  -fill '#8f9bab' -draw 'ellipse 120,80 116,70 180,360' \
  -fill '#a3aebd' -draw 'ellipse 120,80 92,52 180,360' \
  -fill '#5c6573' -draw 'ellipse 120,26 34,12 0,360' \
  -filter Lanczos -resize 50% PNG32:mound.png

# tail.png: the tail's parts
magick -size 172x104 xc:none \
  segment.png -geometry +8+8 -composite \
  sting.png -geometry +100+8 -composite \
  mound.png -geometry +8+56 -composite \
  PNG32:tail.png
```

Each part is placed on its bone by its joint pixels (Placing a part on its bone, below). The upper arm's bolts are at (14, 14) and (144, 14), 130 apart as its bone is long, and the forearm's elbow and wrist at (12, 12) and (132, 12), 120 apart. The thigh's joints are 12 in from its ends, (12, 12) and (82, 12); the shin's knee is 10 in from its end, (10, 12), and the bottom of its foot 70 along from it, (80, 12), at the tip of its bone, so the IK puts the foot's edge on its target. The hand's wrist is the palm's middle, (42, 42); the stand's foot is the middle of its bottom edge, (30, 280); the body's hip is on its lower edge, (48, 62); and a cap's pivot is its middle.

The tail's segments are one image drawn at six sizes: each at its bone's length over 50 (`width` and `height` in its region attachment), so its two joints fall on its bone's two ends at any size, and its parts taper from 50 long at the root to 28. The sting is drawn at 30 over 47, its point on the tip of the last bone, where the chain IK puts the target.

`hero.png` is the pack's own PNGs, unscaled, each laid at its place on a transparent sheet with 8 pixels between neighbours, and the cap. `PACK` is the pack's `PNG` folder:

```sh
PACK="/path/to/kenney_modular-characters/PNG"
magick -size 1024x744 xc:none \
  "$PACK/Skin/Tint 1/tint1_head.png" -geometry +491+8 -composite \
  "$PACK/Skin/Tint 1/tint1_neck.png" -geometry +797+639 -composite \
  "$PACK/Skin/Tint 1/tint1_arm.png" -geometry +175+366 -composite \
  "$PACK/Skin/Tint 1/tint1_hand.png" -geometry +758+512 -composite \
  "$PACK/Skin/Tint 1/tint1_leg.png" -geometry +427+190 -composite \
  "$PACK/Face/Nose/Tint 1/tint1Nose1.png" -geometry +112+694 -composite \
  "$PACK/Skin/Tint 5/tint5_head.png" -geometry +672+8 -composite \
  "$PACK/Skin/Tint 5/tint5_neck.png" -geometry +901+639 -composite \
  "$PACK/Skin/Tint 5/tint5_arm.png" -geometry +353+366 -composite \
  "$PACK/Skin/Tint 5/tint5_hand.png" -geometry +827+512 -composite \
  "$PACK/Skin/Tint 5/tint5_leg.png" -geometry +528+190 -composite \
  "$PACK/Face/Nose/Tint 5/tint5Nose1.png" -geometry +151+694 -composite \
  "$PACK/Skin/Tint 8/tint8_head.png" -geometry +8+190 -composite \
  "$PACK/Skin/Tint 8/tint8_neck.png" -geometry +8+694 -composite \
  "$PACK/Skin/Tint 8/tint8_arm.png" -geometry +531+366 -composite \
  "$PACK/Skin/Tint 8/tint8_hand.png" -geometry +896+512 -composite \
  "$PACK/Skin/Tint 8/tint8_leg.png" -geometry +629+190 -composite \
  "$PACK/Face/Nose/Tint 8/tint8Nose1.png" -geometry +190+694 -composite \
  "$PACK/Hair/Brown 2/brown2Man5.png" -geometry +709+366 -composite \
  "$PACK/Hair/Blonde/blondeMan2.png" -geometry +8+366 -composite \
  "$PACK/Hair/Black/blackMan3.png" -geometry +8+512 -composite \
  "$PACK/Face/Eyebrows/brown2Brow1.png" -geometry +316+694 -composite \
  "$PACK/Face/Eyebrows/blondeBrow1.png" -geometry +364+694 -composite \
  "$PACK/Face/Eyebrows/blackBrow1.png" -geometry +412+694 -composite \
  "$PACK/Face/Eyes/eyeBrown_large.png" -geometry +229+694 -composite \
  "$PACK/Face/Eyes/eyeBlue_large.png" -geometry +258+694 -composite \
  "$PACK/Face/Eyes/eyeBlack_large.png" -geometry +287+694 -composite \
  "$PACK/Face/Mouth/mouth_glad.png" -geometry +483+694 -composite \
  "$PACK/Face/Mouth/mouth_happy.png" -geometry +525+694 -composite \
  "$PACK/Face/Mouth/mouth_oh.png" -geometry +460+694 -composite \
  "$PACK/Face/Mouth/mouth_straight.png" -geometry +567+694 -composite \
  "$PACK/Shirts/Red/redShirt2.png" -geometry +8+8 -composite \
  "$PACK/Shirts/Red/redArm_long.png" -geometry +730+190 -composite \
  "$PACK/Pants/Light Blue/pantsLightBlue_long.png" -geometry +189+190 -composite \
  "$PACK/Pants/Light Blue/pantsLightBlue3.png" -geometry +8+639 -composite \
  "$PACK/Shoes/Brown 1/brownShoe5.png" -geometry +491+639 -composite \
  "$PACK/Shirts/Green/greenShirt7.png" -geometry +169+8 -composite \
  "$PACK/Shirts/Green/greenArm_short.png" -geometry +289+512 -composite \
  "$PACK/Pants/Tan/pantsTan_short.png" -geometry +174+512 -composite \
  "$PACK/Pants/Tan/pantsTan4.png" -geometry +169+639 -composite \
  "$PACK/Shoes/Black/blackShoe4.png" -geometry +593+639 -composite \
  "$PACK/Shirts/Navy/navyShirt5.png" -geometry +330+8 -composite \
  "$PACK/Shirts/Navy/navyArm_shorter.png" -geometry +415+512 -composite \
  "$PACK/Pants/White/pantsWhite_long.png" -geometry +308+190 -composite \
  "$PACK/Pants/White/pantsWhite2.png" -geometry +330+639 -composite \
  "$PACK/Shoes/Grey/greyShoe2.png" -geometry +695+639 -composite \
  "hat.png" -geometry +490+512 -composite \
  PNG32:hero.png
```

The `regions` of `hero.skeleton.ron` are those places, each with its part's size: the head laid at `+491+8` is `"light_head": (image: "parts", rect: (491, 8, 173, 168))`.

| Regions | From the pack |
|---|---|
| `light_*`, `tan_*`, `dark_*` (head, neck, arm, hand, leg, nose) | `Skin/Tint 1`, `Tint 5` and `Tint 8`, and each tint's `Face/Nose` |
| `hair_brown`, `hair_blond`, `hair_black` | `Hair/Brown 2/brown2Man5`, `Hair/Blonde/blondeMan2`, `Hair/Black/blackMan3` |
| `brow_*`, `eye_*`, `mouth_*` | `Face/Eyebrows`, `Face/Eyes`, `Face/Mouth` |
| `red_shirt`, `red_sleeve`, `blue_pants`, `blue_hips`, `brown_shoe` | The runner, the default skin's outfit: `Shirts/Red`, `Pants/Light Blue`, `Shoes/Brown 1` |
| `green_shirt`, `green_sleeve`, `tan_shorts`, `tan_hips`, `black_shoe` | The `ranger` skin: `Shirts/Green`, `Pants/Tan`, `Shoes/Black` |
| `navy_shirt`, `navy_sleeve`, `white_pants`, `white_hips`, `grey_shoe` | The `sailor` skin: `Shirts/Navy`, `Pants/White`, `Shoes/Grey` |
| `hat` | The cap drawn above |

## Placing a part on its bone

A limb's bone runs from one joint to the next, so a part is placed on it by naming two of its pixels, counted from the image's top-left (x right, y down, as the image is drawn):

- `pivot` is the pixel that sits on the bone's origin: the joint the bone turns about.
- `along` is the pixel of the next joint. The image is turned so the line from `pivot` to `along` lies along the bone; `rotation`, if written, turns it further, about the pivot.

The leg's hip is at (31, 18) in its image and its ankle at (64, 158), so it is `Region(region: "light_leg", pivot: (31, 18), along: (64, 158))`. A part with no next joint (a hand, a body, a cap) takes a `pivot` alone, the pixel on its bone's origin, and keeps the image's own axes. A part drawn larger or smaller (`width`, `height`) keeps its pivot on the same pixel, and a mirroring scale (`scale_y: -1`) flips it across its bone about the pivot, with `along` still along the bone.

Measure the two pixels in the image at the size the atlas holds it, as the commands above write them. The reacher, the walker and the tail are placed this way.

The hero's file is older than pivots: its limbs carry the same placing worked out by hand, the image's centre from the joint (`x`, `y`) and the turn that lays the joints along the bone (`rotation`). The leg's line from hip to ankle is 76.7 degrees round from the image's x axis, so its `rotation` is -76.7, and its centre (46.5, 82) less its hip, (15.5, 64), turned by that, is its `x: 65.8, y: -0.4`. An arm is also flipped across its bone (`scale_y: -1`), which turns its `rotation` and its `y` the other way, so that the arm's curve bends its elbow back. A sleeve or a trouser leg is placed by the same joint as the arm or leg under it.

## The meshes

A mesh attachment is drawn as triangles between vertices, where a region attachment is one rectangle. Each vertex is written twice, in one order: in `uvs`, where it is in the image (0 to 1 across the region), and in `vertices`, where it is on the bones. The first `hull` vertices go round the mesh's outline, for the `debug:skeletons` overlay. Both meshes here are grids, their outlines a few pixels outside the cloth's edge, so the image's own smooth edge is what shows.

### The cape: vertices weighted over bones

The cape is 18 vertices, three across by six down, on the torso and the two cape bones, which hang down the hero's back one after the other. Its rows are at 3, 60, 117, 174, 231 and 278 down the 280 x 292 image (the middle of the last at 288, under the hem's scallops), each row's ends 5 outside the cloth.

A weighted vertex names the bones that hold it, and for each bone where the vertex is in that bone's own space in the setup pose, with a weight. The image's point (x, y) is at:

| Bone | In the bone's space |
|---|---|
| `torso` (its x up the spine, its y toward the hero's front) | (166 - y, x - 148) |
| `cape_upper` (at the collar, its x down the cape) | (y - 8, 140 - x) |
| `cape_lower` (140 further down) | (y - 148, 140 - x) |

The rows' weights, from the collar down: the torso alone; half the torso and half the upper bone; the upper bone alone; half the upper and half the lower; and the lower bone alone for the last two. So the collar stays on the shoulders, and the cloth bends where a row is held by two bones.

### The banner: vertices moved by deform keys

The flag is 27 vertices, nine along by three down, 37 apart along the flag and 88 down it, in the space of the one bone it hangs on. No bone moves: the `ripple` animation's deform keys give every vertex an offset from its place, an x and a y each, in the mesh's order, and the flag is drawn between one key's offsets and the next.

A wave runs from the pole to the fly once a loop of 1.6 seconds. It is 10 columns long and grows from nothing at the pole to 16 at the fly; each row is half a radian behind the row above; and a vertex goes round a flat ellipse, 0.3 as wide as it is tall. There are twelve keys a loop, and a thirteenth that is the first again, so the loop has no jump.

The keys are written by this program, which prints the lines of the `deform` block of `banner.skeleton.ron` (Python 3; the same text every time):

```python
import math

COLUMNS, ROWS, KEYS = 9, 3, 12
DURATION, HEIGHT, LENGTH, LAG, WIDE = 1.6, 16.0, 10.0, 0.5, 0.3

# The mesh's order: round the outline from the top corner on the pole,
# then along the middle row
order = [(c, 0) for c in range(COLUMNS)]
order += [(COLUMNS - 1, r) for r in range(1, ROWS)]
order += [(c, ROWS - 1) for c in range(COLUMNS - 2, -1, -1)]
order += [(0, r) for r in range(ROWS - 2, 0, -1)]
order += [(c, r) for r in range(1, ROWS - 1) for c in range(1, COLUMNS - 1)]

def number(value):
    text = f"{value:.1f}"
    return "0" if text in ("0.0", "-0.0") else text.rstrip("0").rstrip(".")

for key in range(KEYS + 1):
    turn = 2 * math.pi * (key % KEYS) / KEYS
    offsets = []
    for column, row in order:
        height = HEIGHT * column / (COLUMNS - 1)
        phase = turn - 2 * math.pi * column / LENGTH - LAG * row
        offsets += [WIDE * height * math.cos(phase), height * math.sin(phase)]
    time = f"{DURATION * key / KEYS:.4f}".rstrip("0")
    time = time + "0" if time.endswith(".") else time
    print(f"            ({time}, [{', '.join(number(v) for v in offsets)}]),")
```

A deform's keys must hold an x and a y for each of the mesh's vertices: change the flag's grid, and the keys change with it.

## The cape's springs

No animation keys the cape's two bones: a spring on each (in `hero.skeleton.ron`'s `constraints`) swings it as the hero moves, so the cloth trails a walk, streams out behind a run, lifts and swings through a jump's landing and swings on round a turn. The numbers were found by trying them in the contact sheet and on the Meshes page, as the hero's file explains. To see a change, draw the sheet (the cape is a skin, so name it):

```sh
cargo run -p ryo2d --bin ryo2d -- sheet examples/demos/skeleton/assets/hero.skeleton.ron \
  --animations idle,walk,run,jump,slash --once jump,slash --skins cape
```

The sheet's hero stands where it is, so it shows what the body's own motion does to the cape (a jump, a lean); the swing behind a hero that walks across the ground shows on the Meshes page.

## The sounds

Every hero carries `SkeletonSounds`, so the engine plays the sound each footstep event names as it fires, where the hero stands; the pages play no sound themselves.

`step-1.wav` to `step-4.wav` are the footsteps of the audio demo, copied from `examples/demos/audio/assets/sounds/`. That demo's `src/bin/make-sounds.rs` writes them from code (a thump falling in pitch and a burst of seeded, filtered noise; mono, 16-bit, 44.1 kHz), so none carries a licence restriction. To change one, edit that program, run it, and copy the file again:

```sh
cargo run -p audio-demo --bin make-sounds
cp examples/demos/audio/assets/sounds/step-[1-4].wav examples/demos/skeleton/assets/audio/
```
