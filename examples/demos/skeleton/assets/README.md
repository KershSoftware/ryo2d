# Skeleton demo assets

| File | What |
|---|---|
| `hero.skeleton.ron` | The hero: 14 bones, 27 slots, six skins, seven animations and a footstep event. Written by hand; its comments say how it is put together |
| `hero.png` | The hero's parts in one 1024 x 744 atlas: three bodies, three outfits, the faces' parts and a cap. The skeleton file names each part's rectangle |
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

The cap and the sword are not from the pack. They were drawn for this demo with ImageMagick, in the pack's flat style, and are under the repository's licence.

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

## The sounds

`step-1.wav` to `step-4.wav` are the footsteps of the audio demo, copied from `examples/demos/audio/assets/sounds/`. That demo's `src/bin/make-sounds.rs` writes them from code (a thump falling in pitch and a burst of seeded, filtered noise; mono, 16-bit, 44.1 kHz), so none carries a licence restriction. To change one, edit that program, run it, and copy the file again:

```sh
cargo run -p audio-demo --bin make-sounds
cp examples/demos/audio/assets/sounds/step-[1-4].wav examples/demos/skeleton/assets/audio/
```
