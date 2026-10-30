// Paints terrain into the Wang test maps through Tiled's own Wang brush and
// saves what the tests compare against.
//
// Regenerate (Tiled 1.12.2, from the repository root):
//   /Applications/Tiled.app/Contents/MacOS/Tiled -e tests/fixtures/tilemap/tiled/scripts/paint-wang.js
//
// Each job reads a map, paints the colour into one cell the way one click of
// the terrain brush does (the cell's corners for a corner set, its edges for
// an edge set, both for a mixed one; a null colour erases), with corrections
// on as the brush has them, and writes the result. Tiled picks at random
// among tiles that fit equally, so every job here is built to have one fit.
// The inputs are plain files: nothing here creates them.
var dir = "tests/fixtures/tilemap/tiled/";
var jobs = [
  // [from, to, wang set, x, y, colour]
  ["maps/wang-dirt.tmj", "maps/wang-grass.tmj", "ground", 2, 2, "grass"],
  ["xml/maps/wang-dirt.tmx", "xml/maps/wang-grass.tmx", "ground", 2, 2, "grass"],
  ["maps/wang-grass.tmj", "maps/wang-erased.tmj", "ground", 2, 2, null],
  ["maps/wang-solo-empty.tmj", "maps/wang-solo-painted.tmj", "solo", 2, 2, "ground"],
  ["maps/wang-solo-full.tmj", "maps/wang-solo-erased.tmj", "solo", 2, 2, null],
  ["maps/wang-mix-a.tmj", "maps/wang-mix-b.tmj", "mix", 2, 2, "b"],
  ["maps/wang-mix-a.tmj", "maps/wang-mix-corner.tmj", "mix", 0, 0, "b"],
  ["maps/wang-tri-grass.tmj", "maps/wang-tri-water.tmj", "shore", 3, 3, "water"],
  ["maps/wang-flipped.tmj", "maps/wang-flipped-painted.tmj", "ground", 2, 2, "grass"],
  ["maps/wang-turn-dirt.tmj", "maps/wang-turn-painted.tmj", "turn", 2, 2, "grass"],
  ["maps/wang-chain-grass.tmj", "maps/wang-chain-deep.tmj", "depth", 5, 5, "deep"],
  ["maps/wang-rot-dirt.tmj", "maps/wang-rot-painted.tmj", "ground", 2, 2, "grass"],
];
for (var i = 0; i < jobs.length; i++) {
  var job = jobs[i];
  // -e runs without an editor, so the maps are read, not opened
  var from = dir + job[0], to = dir + job[1];
  var map = tiled.mapFormatForFile(from).read(from);
  var layer = map.layerAt(0);
  var set = null;
  for (var t = 0; t < map.tilesets.length; t++) {
    for (var w = 0; w < map.tilesets[t].wangSets.length; w++) {
      if (map.tilesets[t].wangSets[w].name == job[2]) set = map.tilesets[t].wangSets[w];
    }
  }
  var color = 0;
  if (job[5] !== null) {
    for (var c = 1; c <= set.colorCount; c++) if (set.colorName(c) == job[5]) color = c;
  }
  var x = job[3], y = job[4];
  var edit = layer.wangEdit(set);
  edit.correctionsEnabled = true;
  if (set.type == WangSet.Corner || set.type == WangSet.Mixed) {
    edit.setCorner(x, y, color);
    edit.setCorner(x + 1, y, color);
    edit.setCorner(x, y + 1, color);
    edit.setCorner(x + 1, y + 1, color);
  }
  if (set.type == WangSet.Edge || set.type == WangSet.Mixed) {
    edit.setEdge(x, y, WangIndex.Top, color);
    edit.setEdge(x, y, WangIndex.Right, color);
    edit.setEdge(x, y, WangIndex.Bottom, color);
    edit.setEdge(x, y, WangIndex.Left, color);
  }
  edit.apply();
  var error = tiled.mapFormatForFile(to).write(map, to);
  if (error) tiled.log("error: " + error);
}
