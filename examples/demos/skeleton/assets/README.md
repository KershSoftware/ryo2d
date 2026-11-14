# Skeleton demo assets

| File | What |
|---|---|
| `hero.skeleton.ron` | The hero: 16 bones, 28 slots, seven skins, seven animations and a footstep event. Written by hand; its comments say how it is put together |
| `hero.png` | The hero's parts in one 1024 x 744 atlas: three bodies, three outfits, the faces' parts and a cap. The skeleton file names each part's rectangle |
| `cape.png` | The cape the Meshes page puts on the hero, 280 x 292: the image of the mesh in the hero's `cape` skin |
| `banner.skeleton.ron` | The Meshes page's banner: three bones, a pole and a flag. The flag is a mesh of 27 vertices, and the `ripple` animation's deform keys move them. Written by hand, but for the ripple's keys (below) |
| `banner.png` | The banner's flag (296 x 176) and its pole (640 x 20, lying down) in one 656 x 220 image |
| `sword.png` | The sword the Sockets page puts on the hero's weapon bone, 300 x 68, its grip on the left |
| `audio/footstep.sound.ron` | The sound the skeleton's footstep event names: one of four takes, heard from where it is played |
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

The cap, the sword, the cape and the banner (its flag and its pole) are not from the pack: it has none of them. They were drawn for this demo with ImageMagick, as flat shapes with a little shading in the hero's colours (the cap's yellows, the red of the shirt, the brown of the shoes), and are under the repository's licence.

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

A region attachment's `x` and `y` are where the image's centre is from its bone's origin, in the bone's own axes, and `rotation` turns the image. For a limb, whose bone runs from one joint to the next:

- `rotation` lays the image's line between the two joints along the bone. The leg's hip is at (31, 18) in its image and its ankle at (64, 158), a line 76.7 degrees round from the image's x axis, so its `rotation` is -76.7.
- `x`, `y` are the image's centre less the joint, turned by `rotation`: the leg's centre (46.5, 82) less its hip is (15.5, 64), which turned by -76.7 degrees is (65.8, -0.4).
- An arm is also flipped across its bone (`scale_y: -1`), which turns its `rotation` and its `y` the other way, so that the arm's curve bends its elbow back.

A sleeve or a trouser leg is placed by the same joint as the arm or leg under it.

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

## The sounds

`step-1.wav` to `step-4.wav` are the footsteps of the audio demo, copied from `examples/demos/audio/assets/sounds/`. That demo's `src/bin/make-sounds.rs` writes them from code (a thump falling in pitch and a burst of seeded, filtered noise; mono, 16-bit, 44.1 kHz), so none carries a licence restriction. To change one, edit that program, run it, and copy the file again:

```sh
cargo run -p audio-demo --bin make-sounds
cp examples/demos/audio/assets/sounds/step-[1-4].wav examples/demos/skeleton/assets/audio/
```
