# Effect files

The effect files the Files and Collisions pages load, and the two images they draw.

| File | What |
|---|---|
| `sparks.particles.ron` | Sparks in a burst a second, bouncing |
| `splash.particles.ron` | Drops thrown up where rain lands (a sub-effect) |
| `embers.particles.ron` | Embers that stick where they land |
| `rain.particles.ron` | Rain that dies on a collider and plays the splash |
| `spark.png` | A 16 x 16 soft white dot, tinted by each effect |
| `smoke_sheet.png` | A 4 x 2 sheet of 64 x 64 smoke puffs, thinning frame by frame |

The pages play them as they are, and the engine reloads one saved while the
demo runs.

## The images

Made with ImageMagick 7 (`magick`); the sheet's noise is seeded, so it comes out the same each time.

```sh
# spark.png: white, its opacity a radial falloff
magick -size 16x16 xc:white \( -size 16x16 radial-gradient:white-black \) \
  -alpha off -compose CopyOpacity -composite PNG32:spark.png

# smoke_sheet.png: eight puffs of seeded noise under a radial falloff, each
# fainter than the last, joined four across and two down
mkdir smoke
for i in 0 1 2 3 4 5 6 7; do
  A=$(echo "1 - $i*0.1" | bc -l)
  magick -seed $((20+i)) -size 64x64 plasma:fractal -colorspace gray -blur 0x1 -auto-level \
    \( -size 64x64 radial-gradient:white-black \) -compose multiply -composite \
    -auto-level -evaluate multiply $A \
    \( -size 64x64 xc:white \) +swap -compose CopyOpacity -composite smoke/$i.png
done
magick \( smoke/0.png smoke/1.png smoke/2.png smoke/3.png +append \) \
  \( smoke/4.png smoke/5.png smoke/6.png smoke/7.png +append \) \
  -background none -append +repage PNG32:smoke_sheet.png
rm -r smoke
```
