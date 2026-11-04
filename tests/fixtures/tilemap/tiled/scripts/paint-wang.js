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
  ["maps/wang-dirt.tmj", "maps/wang-grass-edge.tmj", "ground", 4, 2, "grass"],
  ["maps/wang-dirt.tmj", "maps/wang-grass-far-corner.tmj", "ground", 4, 4, "grass"],
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
// The same sets painted on isometric and staggered maps (32 x 16 cells): a
// corner and an edge set in the middle of a map nine cells square, the corner
// set at its first and last cell, and a mixed set on a map of thirteen at
// four neighbouring cells, which are of every parity and make its correction
// spread as far as it may
var grids = ["iso", "stag-y-odd", "stag-y-even", "stag-x-odd", "stag-x-even"];
for (var g = 0; g < grids.length; g++) {
  var m = "maps/wang-" + grids[g];
  jobs.push([m + "-dirt.tmj", m + "-grass.tmj", "ground", 4, 4, "grass"]);
  jobs.push([m + "-walls.tmj", m + "-plank.tmj", "walls", 4, 4, "plank"]);
  jobs.push([m + "-dirt.tmj", m + "-grass-first.tmj", "ground", 0, 0, "grass"]);
  jobs.push([m + "-dirt.tmj", m + "-grass-last.tmj", "ground", 8, 8, "grass"]);
  jobs.push([m + "-mix.tmj", m + "-b-6-6.tmj", "mix", 6, 6, "b"]);
  jobs.push([m + "-mix.tmj", m + "-b-7-6.tmj", "mix", 7, 6, "b"]);
  jobs.push([m + "-mix.tmj", m + "-b-6-7.tmj", "mix", 6, 7, "b"]);
  jobs.push([m + "-mix.tmj", m + "-b-7-7.tmj", "mix", 7, 7, "b"]);
  jobs.push([m + "-long.tmj", m + "-long-b.tmj", "long", 12, 12, "b"]);
}

// A staggered map's cell as its isometric lattice cell, and back
function toLattice(map, x, y) {
  var odd = map.staggerIndex == TileMap.StaggerOdd;
  var down = function(n) { return Math.floor(n / 2); };
  var up = function(n) { return Math.floor((n + 1) / 2); };
  if (map.staggerAxis == TileMap.StaggerY) {
    return odd ? [x + up(y), down(y) - x] : [x + down(y), up(y) - x];
  }
  return odd ? [y + up(x), y - down(x)] : [y + down(x), y - up(x)];
}
function fromLattice(map, x, y) {
  var odd = map.staggerIndex == TileMap.StaggerOdd;
  var half = function(n) { return odd ? Math.floor((n + 1) / 2) : Math.floor(n / 2); };
  if (map.staggerAxis == TileMap.StaggerY) return [x - half(x + y), x + y];
  return [x - y, x - half(x - y)];
}
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
  // On a staggered map the brush paints in the isometric lattice its
  // diamonds make: a cell's corners are its lattice cell's, and a corner
  // (x, y) is the top vertex of grid cell (x, y)'s lattice cell. Measured:
  // one corner fills the four cells around a lattice vertex, one edge the
  // two lattice cells either side of it. Every other map paints in its
  // index space
  var stag = map.orientation == TileMap.Staggered;
  var lat = stag ? toLattice(map, x, y) : [x, y];
  var corner = function(dx, dy) {
    var g = stag ? fromLattice(map, lat[0] + dx, lat[1] + dy) : [lat[0] + dx, lat[1] + dy];
    edit.setCorner(g[0], g[1], color);
  };
  if (set.type == WangSet.Corner || set.type == WangSet.Mixed) {
    corner(0, 0);
    corner(1, 0);
    corner(0, 1);
    corner(1, 1);
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
