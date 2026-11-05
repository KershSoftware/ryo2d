#!/usr/bin/env bash
# Derives the tile grids demo's Kenney art from the packs' zips, as
# assets/README.md records (ImageMagick 7):
#
#   scripts/derive-art.sh <Sketch Town's Tiles folder> <Hexagon Pack's Spritesheets folder>
#
# run from examples/demos/tile-grids, with each zip unzipped in a folder of
# its own. It writes, at the packs' own size:
#
# - assets/kenney-sketch-town/<name>.png: each picture the pages use (256 x
#   352) cut to its bottom 256 rows (the 96 above are empty in every one of
#   them), so a picture is 256 x 256;
# - assets/kenney-hexagon-pack/hexagon-board.png: the hexagons the board
#   uses, cut from the terrain sheet (120 x 140 each, 122 px apart across
#   and 142 down) and laid out 6 a row in BOARD's order, which the board's
#   tileset and `scripts/make-maps.js` (HEX_SHEET) number from 0.

set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <Sketch Town Tiles folder> <Hexagon Pack Spritesheets folder>" >&2
  exit 2
fi
tiles=$1
sheets=$2
out=assets

sketch=(
  grass_center_N water_center_N
  grass_water_N grass_water_E grass_water_S grass_water_W
  grass_waterConcave_N grass_waterConcave_E grass_waterConcave_S grass_waterConcave_W
  grass_waterConvex_N grass_waterConvex_E grass_waterConvex_S grass_waterConvex_W
  castle_wall_N castle_wall_E castle_tower_N castle_gateOpen_N castle_gateOpen_E
  tree_single_N tree_single_E tree_multiple_N tree_multiple_E rocks_grass_N rocks_grass_E
)
mkdir -p "$out/kenney-sketch-town"
for name in "${sketch[@]}"; do
  magick "$tiles/$name.png" -crop 256x256+0+96 +repage \
    -strip -define png:exclude-chunks=date,time "$out/kenney-sketch-town/$name.png"
done

# The board's hexagons in the new sheet's order, each cut where the terrain
# sheet's XML says it is
board=(
  grass_05 grass_10 grass_12 grass_13 grass_11 grass_14
  grass_15 grass_16 dirt_06 dirt_11 dirt_12 dirt_14
  dirt_13 dirt_16 dirt_18 dirt_17 stone_07 sand_07
  sand_14 sand_13 sand_12 sand_01 stone_01 mars_01
)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pieces=()
for i in "${!board[@]}"; do
  name=${board[$i]}
  where=$(sed -n "s/.*name=\"$name.png\" x=\"\([0-9]*\)\" y=\"\([0-9]*\)\" width=\"120\" height=\"140\".*/\1 \2/p" \
    "$sheets/hexagonTerrain_sheet.xml")
  if [ -z "$where" ]; then
    echo "derive-art.sh: no 120 x 140 $name in the terrain sheet" >&2
    exit 1
  fi
  read -r x y <<<"$where"
  piece=$(printf "%s/%02d-%s.png" "$work" "$i" "$name")
  magick "$sheets/hexagonTerrain_sheet.png" -crop "120x140+$x+$y" +repage "$piece"
  pieces+=("$piece")
done
mkdir -p "$out/kenney-hexagon-pack"
magick montage "${pieces[@]}" -tile 6x -geometry 120x140+0+0 -background none \
  -strip -define png:exclude-chunks=date,time "$out/kenney-hexagon-pack/hexagon-board.png"
echo "derive-art.sh: wrote ${#sketch[@]} Sketch Town pictures and the board's sheet of ${#board[@]} hexagons"
