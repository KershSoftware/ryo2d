// Writes the tile grids demo's Tiled files through Tiled's own scripting
// API: the property types of project.tiled-project, the Pixel Platformer
// tileset and the maps. make-grid-maps runs it from assets/, after writing an
// empty project:
//
//   cargo run -p tile-grids-demo --bin make-grid-maps
//
// which comes down to (Tiled 1.12.2, from examples/demos/tile-grids/assets):
//
//   Tiled --project project.tiled-project --evaluate ../scripts/make-maps.js
//
// Every choice is made here, by rule or by a hash of the cell, so a run
// writes the same files each time. Ground tiles are picked from the four
// cells beside each one, as the paint page picks them (src/shared.rs,
// `ground_tile`): a grass top where the cell above is open, an edge on each
// open side, a bottom edge where the cell below is open.
//
// A page with a map of its own adds it here, in a section of its own.

function fail(message) {
  throw new Error("make-maps.js: " + message);
}

// ---- helpers ---------------------------------------------------------------

// A number in [0, 1) from a cell, the same every run
function hash(x, y, salt) {
  var h = (Math.imul(x, 374761393) + Math.imul(y, 668265263) + Math.imul(salt, 1103515245)) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h = h ^ (h >>> 16);
  return (h >>> 0) / 4294967296;
}

function rect(x, y, width, height, properties) {
  var shape = new MapObject(MapObject.Rectangle, "");
  shape.x = x;
  shape.y = y;
  shape.width = width;
  shape.height = height;
  for (var name in properties || {}) shape.setProperty(name, properties[name]);
  return shape;
}

// Gives each tile in `ids` the collision shapes `make()` returns, numbered
// as Tiled's collision editor numbers them (the group is laid in a scratch
// map, after a tile layer, so it gets layer id 2 and its objects ids from 1)
function shapes(tileset, ids, make) {
  for (var i = 0; i < ids.length; i++) {
    var scratch = new TileMap();
    scratch.setSize(1, 1);
    scratch.setTileSize(tileset.tileWidth, tileset.tileHeight);
    scratch.addLayer(new TileLayer(""));
    var group = new ObjectGroup();
    group.drawOrder = ObjectGroup.IndexOrder;
    var made = make();
    for (var j = 0; j < made.length; j++) group.addObject(made[j]);
    scratch.addLayer(group);
    scratch.removeLayer(group);
    tileset.tile(ids[i]).objectGroup = group;
  }
}

function solid(tileset, ids) {
  var w = tileset.tileWidth, h = tileset.tileHeight;
  shapes(tileset, ids, function () { return [rect(0, 0, w, h)]; });
}

// A corner Wang set from [[tile id, [top-right, bottom-right, bottom-left,
// top-left]], ...], its colours numbered from 1 in `colors`' order
function wangSet(tileset, name, colors, tiles, imageTile) {
  var set = tileset.addWangSet(name, WangSet.Corner);
  set.colorCount = colors.length;
  for (var i = 0; i < colors.length; i++) set.setColorName(i + 1, colors[i]);
  for (var j = 0; j < tiles.length; j++) {
    var c = tiles[j][1];
    set.setWangId(tileset.tile(tiles[j][0]), [0, c[0], 0, c[1], 0, c[2], 0, c[3]]);
  }
  set.imageTile = tileset.tile(imageTile);
  return set;
}

function animate(tileset, id, frames) {
  var out = [];
  for (var i = 0; i < frames.length; i++) out.push({ tileId: frames[i][0], duration: frames[i][1] });
  tileset.tile(id).frames = out;
}

function range(from, to) {
  var out = [];
  for (var i = from; i <= to; i++) out.push(i);
  return out;
}

function writeText(path, text) {
  var out = new TextFile(file(path), TextFile.WriteOnly);
  out.write(text);
  out.commit();
}

function save(asset, format, path) {
  var error = format.write(asset, file(path));
  if (error) fail(path + ": " + error);
}

// ---- the project's classes -------------------------------------------------

var project = tiled.project;
if (!project) fail("run with --project project.tiled-project");
// Paths are the project's folder's (assets/): Tiled keeps a relative path
// as written, and reads it against the file that names it
var ROOT = FileInfo.path(project.fileName);
function file(path) {
  return FileInfo.joinPaths(ROOT, path);
}
var facing = project.addEnumType("Facing");
facing.values = ["left", "right"];
function classType(name, members, usage) {
  var type = project.addClassType(name);
  for (var i = 0; i < members.length; i++) type.setMember(members[i][0], members[i][1]);
  type.usageFlags = usage;
}
classType("player_start", [["facing", tiled.propertyValue("Facing", "right")]], ClassPropertyType.MapObjectClass);
classType("coin", [["value", 1]], ClassPropertyType.MapObjectClass | ClassPropertyType.TileClass);

// ---- the tileset -----------------------------------------------------------

// Pixel Platformer, 18 px: the terrain solid, the rope bridge and the planks
// one-way, the coin (a class tile) and the water animated
var platformer = new Tileset("platformer");
platformer.setTileSize(18, 18);
platformer.imageFileName = file("kenney-pixel-platformer/tilemap_packed.png");
solid(platformer, [].concat(range(0, 5), range(20, 25), [104], range(120, 123), range(140, 143)));
shapes(platformer, [90, 91, 92], function () { return [rect(0, 6, 18, 6, { one_way: true })]; });
shapes(platformer, [146, 147], function () { return [rect(0, 0, 18, 10, { one_way: true })]; });
animate(platformer, 151, [[151, 300], [152, 150]]);
animate(platformer, 33, [[33, 500], [53, 500]]);
platformer.tile(151).className = "coin";
save(platformer, tiled.tilesetFormat("tsx"), "tilesets/platformer.tsx");

// ---- the other grids' tilesets -----------------------------------------------

// Kenney's Sketch Town, one picture a tile, halved and cut to its bottom
// 128 rows (scripts/derive-art.sh): blocks and props drawn for isometric
// cells of 116 x 55 (the pack's 232 x 110 halved; its diamond is 55 tall,
// so the cell is too, and its half-cell steps fall on half pixels), each
// picture standing 6 px left of its cell (Kenney's own tileset's offset,
// halved). A block is a cell high, so it is two tilesets of the same
// pictures' kind: the ground's blocks drawn a cell lower (offset y 58),
// their tops on the cells, and the props standing on the cells (offset y
// 3), so a prop stands on the block of its cell and the cells the
// characters walk, pick and collide on are the blocks' tops. Each
// tileset's grid stays orthogonal: the collision shapes are in the
// pictures' pixels
var SKETCH = "kenney-sketch-town/";
var ISO_W = 116, ISO_H = 55;
var SKETCH_W = 128, SKETCH_H = 128, SKETCH_LEFT = 6;

function collection(name, offsetY, files) {
  var set = new Tileset(name);
  set.setTileSize(SKETCH_W, SKETCH_H);
  set.tileOffset = Qt.point(-SKETCH_LEFT, offsetY);
  for (var i = 0; i < files.length; i++) {
    var tile = set.addTile();
    tile.imageFileName = file(SKETCH + files[i] + ".png");
  }
  return set;
}

// The cell's diamond in a picture drawn with offset y `offsetY`: a shape on
// its four vertices collides as the whole cell
function diamond(offsetY) {
  var top = SKETCH_H - offsetY - ISO_H, left = SKETCH_LEFT;
  var shape = new MapObject(MapObject.Polygon, "");
  shape.x = 0;
  shape.y = 0;
  shape.polygon = [
    Qt.point(left + ISO_W / 2, top),
    Qt.point(left + ISO_W, top + ISO_H / 2),
    Qt.point(left + ISO_W / 2, top + ISO_H),
    Qt.point(left, top + ISO_H / 2),
  ];
  return shape;
}

// The ground: grass and water blocks, and the water's edges as a corner
// Wang set, `terrain`, of grass (1) and water (2). A tile's corners are its
// cell's in the map: top-left the diamond's top vertex, top-right its right
// one, bottom-right its bottom, bottom-left its left. The pack's edges
// cover every corner combination but the two diagonals. Water collides
var GROUND_OFFSET = 58, PROPS_OFFSET = 3;
var GROUND_FILES = [
  ["grass_center_N", [1, 1, 1, 1]],
  ["water_center_N", [2, 2, 2, 2]],
  // [top-right, bottom-right, bottom-left, top-left]: a grass bank along
  // one edge, the water on the other half
  ["grass_water_N", [1, 2, 2, 1]],
  ["grass_water_E", [1, 1, 2, 2]],
  ["grass_water_S", [2, 1, 1, 2]],
  ["grass_water_W", [2, 2, 1, 1]],
  // a pool in one corner
  ["grass_waterConcave_N", [1, 2, 1, 1]],
  ["grass_waterConcave_E", [1, 1, 2, 1]],
  ["grass_waterConcave_S", [1, 1, 1, 2]],
  ["grass_waterConcave_W", [2, 1, 1, 1]],
  // grass in one corner
  ["grass_waterConvex_N", [2, 2, 2, 1]],
  ["grass_waterConvex_E", [1, 2, 2, 2]],
  ["grass_waterConvex_S", [2, 1, 2, 2]],
  ["grass_waterConvex_W", [2, 2, 1, 2]],
];
var GROUND = {};
for (var gi = 0; gi < GROUND_FILES.length; gi++) GROUND[GROUND_FILES[gi][0]] = gi;
var sketchGround = collection("sketch-town-ground", GROUND_OFFSET, GROUND_FILES.map(function (f) { return f[0]; }));
shapes(sketchGround, [GROUND.water_center_N], function () { return [diamond(GROUND_OFFSET)]; });
wangSet(sketchGround, "terrain", ["grass", "water"], GROUND_FILES.map(function (f, i) { return [i, f[1]]; }), GROUND.water_center_N);
save(sketchGround, tiled.tilesetFormat("tsx"), "tilesets/sketch-town-ground.tsx");

// The props: castle walls and towers (colliding, their shapes the cell's
// diamond), an open gate (walked through), trees and rocks. Walls `_N` run
// along the map's x, `_E` along its y
var PROP_FILES = [
  "castle_wall_N", "castle_wall_E", "castle_tower_N", "castle_gateOpen_N", "castle_gateOpen_E",
  "tree_single_N", "tree_single_E", "tree_multiple_N", "tree_multiple_E", "rocks_grass_N", "rocks_grass_E",
];
var PROP = {};
for (var pi = 0; pi < PROP_FILES.length; pi++) PROP[PROP_FILES[pi]] = pi;
var sketchProps = collection("sketch-town-props", PROPS_OFFSET, PROP_FILES);
shapes(sketchProps, [PROP.castle_wall_N, PROP.castle_wall_E, PROP.castle_tower_N], function () {
  return [diamond(PROPS_OFFSET)];
});
save(sketchProps, tiled.tilesetFormat("tsx"), "tilesets/sketch-town-props.tsx");

// Kenney's Hexagon Pack: the hexagons the board uses, cut from its terrain
// sheet and halved (scripts/derive-art.sh), pointy-topped hexagons of 60 x
// 70 side by side, 6 a row, in HEX_SHEET's order. Nothing collides:
// hexagonal layers don't
var HEX_SHEET = [
  "grass_05", "grass_10", "grass_12", "grass_13", "grass_11", "grass_14",
  "grass_15", "grass_16", "dirt_06", "dirt_11", "dirt_12", "dirt_14",
  "dirt_13", "dirt_16", "dirt_18", "dirt_17", "stone_07", "sand_07",
  "sand_14", "sand_13", "sand_12", "sand_01", "stone_01", "mars_01",
];
function hexTile(name) {
  var id = HEX_SHEET.indexOf(name);
  if (id < 0) fail("no hexagon " + name + " in the board's sheet");
  return id;
}
var hexTerrain = new Tileset("hexagon-board");
hexTerrain.setTileSize(60, 70);
hexTerrain.imageFileName = file("kenney-hexagon-pack/hexagon-board.png");
save(hexTerrain, tiled.tilesetFormat("tsx"), "tilesets/hexagon-board.tsx");

// Tiny Dungeon, 16 px, for the oblique dungeon: its grey brick walls solid,
// a rectangle filling the tile (on an oblique map, the cell unskewed)
var WALL = 40, FLOORS = [48, 49];
var dungeonSet = new Tileset("dungeon");
dungeonSet.setTileSize(16, 16);
dungeonSet.imageFileName = file("kenney-tiny-dungeon/tilemap_packed.png");
solid(dungeonSet, [WALL]);
save(dungeonSet, tiled.tilesetFormat("tsx"), "tilesets/dungeon.tsx");

// ---- the tilesets as the maps reference them -------------------------------

// A map written by a script embeds a tileset it built; one read from a file
// keeps its file name, so the maps take their tilesets from scratch maps
// that name the files
function external(path) {
  writeText("make-maps-scratch.tmx",
    '<?xml version="1.0" encoding="UTF-8"?>\n' +
    '<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" ' +
    'width="1" height="1" tilewidth="18" tileheight="18" infinite="0" nextlayerid="1" nextobjectid="1">\n' +
    ' <tileset firstgid="1" source="' + path + '"/>\n' +
    '</map>\n');
  var scratch = tiled.mapFormat("tmx").read(file("make-maps-scratch.tmx"));
  File.remove(file("make-maps-scratch.tmx"));
  return scratch.tilesets[0];
}
var P = external("tilesets/platformer.tsx");

// ---- ground ----------------------------------------------------------------

// A set of cells, keyed "x,y"
function CellSet() {
  this.cells = {};
}
CellSet.prototype.add = function (x, y) { this.cells[x + "," + y] = true; };
CellSet.prototype.has = function (x, y) { return this.cells[x + "," + y] === true; };
CellSet.prototype.each = function (f) {
  var keys = Object.keys(this.cells).sort();
  for (var i = 0; i < keys.length; i++) {
    var xy = keys[i].split(",");
    f(Number(xy[0]), Number(xy[1]));
  }
};

// The Pixel Platformer tile for a ground cell, from which of the four cells
// beside it are ground (`has(x, y)`): rows of the sheet for open above and
// below (a one cell platform), open above (a grass top), closed both ways
// (earth) and open below (a bottom edge); in each row, the tile for open on
// both sides, open on the left, closed both sides, and open on the right
function groundTile(has, x, y) {
  var up = has(x, y - 1), down = has(x, y + 1);
  var left = has(x - 1, y), right = has(x + 1, y);
  var row = !up ? (down ? 20 : 0) : (down ? 120 : 140);
  var column = left ? (right ? 2 : 3) : (right ? 1 : 0);
  return row + column;
}

function tileLayer(name, properties) {
  var layer = new TileLayer(name);
  for (var key in properties || {}) layer.setProperty(key, properties[key]);
  return layer;
}

function tileObject(tileset, id, x, y, name) {
  var object = new MapObject(name);
  object.tile = tileset.tile(id);
  object.x = x;
  object.y = y;
  object.width = tileset.tileWidth;
  object.height = tileset.tileHeight;
  return object;
}

// ---- the endless meadow ----------------------------------------------------

// An infinite map of 18 px cells, saved in Tiled's chunks of 16 x 16 cells:
// a meadow from cell -96 to cell 63 with its grass at row 0, hills, a pond
// under a rope bridge, a dip, planks and coins; and a small island far up
// and left, at cell (-204, -40), alone in its chunk. The meadow is the
// hand-made part of an endless world: the page's generator streams the
// ground on past both its ends, at the grass row, down to the meadow's
// bottom row. So the meadow ends on the edges of the engine's chunks (32 x
// 32 cells: -96 and 63 are the first and last cells of chunks -3 and 1),
// whose cells a stream leaves to the file, and its end columns are laid as
// ground going on, with no edge facing the generated ground. The runner
// starts at `player_start`
var CELL = 18;
var BOTTOM = 5;
var meadow = new TileMap();
meadow.setTileSize(CELL, CELL);
meadow.infinite = true;
meadow.layerDataFormat = TileMap.CSV;
meadow.backgroundColor = "#9fdcf4";
meadow.addTileset(P);

// The grass row of each column: [from, to, row], and the pond and the dip
var SURFACE = [
  [-96, -69, 0], [-68, -67, -1], [-66, -63, -2], [-62, -61, -1], [-60, -46, 0],
  [-36, -27, 0], [-26, -21, -1], [-20, 7, 0], [8, 9, -1], [10, 13, -2], [14, 15, -1],
  [16, 33, 0], [37, 63, 0],
];
var POND = { from: -45, to: -37, floor: 3 }; // bank to bank; the bridge spans it at row 0
var DIP = { from: 34, to: 36, floor: 2 };
var ground = new CellSet();
for (var s = 0; s < SURFACE.length; s++) {
  for (var x = SURFACE[s][0]; x <= SURFACE[s][1]; x++) {
    for (var y = SURFACE[s][2]; y <= BOTTOM; y++) ground.add(x, y);
  }
}
for (var px = POND.from; px <= POND.to; px++) {
  for (var py = POND.floor; py <= BOTTOM; py++) ground.add(px, py);
}
for (var dx = DIP.from; dx <= DIP.to; dx++) {
  for (var dy = DIP.floor; dy <= BOTTOM; dy++) ground.add(dx, dy);
}
// The ground goes on past the meadow's ends, at the grass row of each down
// to the bottom row: the generated world's, which the end columns meet
var LEFT = SURFACE[0], RIGHT = SURFACE[SURFACE.length - 1];
function meadowGround(x, y) {
  if (ground.has(x, y)) return true;
  if (x < LEFT[0]) return y >= LEFT[2] && y <= BOTTOM;
  if (x > RIGHT[1]) return y >= RIGHT[2] && y <= BOTTOM;
  return false;
}
// The island: three rows, nine cells across
var ISLAND = { x: -204, y: -40, width: 9 };
for (var ix = 0; ix < ISLAND.width; ix++) {
  for (var iy = 0; iy < 3; iy++) ground.add(ISLAND.x + ix, ISLAND.y + iy);
}

// The scenery: trees (a canopy of three by three over a trunk), pines,
// sprouts, mushrooms, bushes, signs and clouds, none colliding
var scenery = tileLayer("scenery", { collides: false });
var sceneryEdit = scenery.edit();
var decorated = new CellSet();
function decorate(x, y, id, flags) {
  sceneryEdit.setTile(x, y, P.tile(id), flags || 0);
  decorated.add(x, y);
}
function surfaceAt(x) {
  for (var i = 0; i < SURFACE.length; i++) {
    if (x >= SURFACE[i][0] && x <= SURFACE[i][1]) return SURFACE[i][2];
  }
  return null;
}
function tree(x) {
  var top = surfaceAt(x);
  if (top === null) fail("no ground under the tree at " + x);
  var canopy = [[17, 18, 19], [37, 38, 39], [57, 58, 59]];
  for (var r = 0; r < 3; r++) {
    for (var c = 0; c < 3; c++) decorate(x - 1 + c, top - 6 + r, canopy[r][c]);
  }
  decorate(x, top - 3, 97);
  decorate(x, top - 2, 117);
  decorate(x, top - 1, 137);
}
function standing(x, id, flags) {
  var top = surfaceAt(x);
  if (top === null) fail("no ground under tile " + id + " at " + x);
  decorate(x, top - 1, id, flags);
}
var TREES = [-74, -31, 2, 45];
for (var t = 0; t < TREES.length; t++) tree(TREES[t]);
var PINES = [-84, -64, -24, -18, 19, 31, 47];
for (var p = 0; p < PINES.length; p++) standing(PINES[p], 126);
standing(-86, 85); // a sign pointing right, near the left end
standing(49, 84); // and left, near the right end
standing(-49, 128);
standing(25, 128, Tile.FlippedHorizontally);
standing(-52, 16);
standing(12, 16);
// Sprouts on open grass, by a hash of the column
for (var sx = LEFT[0]; sx <= RIGHT[1]; sx++) {
  var top = surfaceAt(sx);
  if (top === null || decorated.has(sx, top - 1)) continue;
  var roll = hash(sx, top, 1);
  if (roll < 0.22) decorate(sx, top - 1, roll < 0.11 ? 124 : 125);
}
// Clouds: [left cell, row, middle cells]
var CLOUDS = [[-82, -12, 2], [-60, -9, 1], [-38, -13, 3], [-12, -10, 1], [6, -14, 2], [28, -11, 1], [44, -13, 0]];
for (var k = 0; k < CLOUDS.length; k++) {
  var cl = CLOUDS[k];
  if (cl[2] === 0) {
    decorate(cl[0], cl[1], 156);
    continue;
  }
  decorate(cl[0], cl[1], 153);
  for (var m = 1; m <= cl[2]; m++) decorate(cl[0] + m, cl[1], 154);
  decorate(cl[0] + cl[2] + 1, cl[1], 155);
}
// The island's pine and sign
decorate(ISLAND.x + 2, ISLAND.y - 1, 126);
decorate(ISLAND.x + 6, ISLAND.y - 1, 86);
sceneryEdit.apply();
meadow.addLayer(scenery);

// The pond's water, under the bridge
var water = tileLayer("water", { collides: false });
var waterEdit = water.edit();
for (var wx = POND.from; wx <= POND.to; wx++) {
  waterEdit.setTile(wx, 1, P.tile(33));
  waterEdit.setTile(wx, 2, P.tile(73));
}
waterEdit.apply();
meadow.addLayer(water);

// The ground: the terrain picked by its neighbours, the bridge over the
// pond and the planks
var groundLayer = tileLayer("ground");
var groundEdit = groundLayer.edit();
ground.each(function (x, y) {
  groundEdit.setTile(x, y, P.tile(groundTile(meadowGround, x, y)));
});
for (var bx = POND.from; bx <= POND.to; bx++) {
  var id = bx == POND.from ? 90 : (bx == POND.to ? 92 : 91);
  groundEdit.setTile(bx, 0, P.tile(id));
}
// Planks: [left cell, row, cells]
var PLANKS = [[-56, -4, 4], [-12, -4, 4], [20, -5, 4], [39, -4, 3]];
for (var q = 0; q < PLANKS.length; q++) {
  for (var n = 0; n < PLANKS[q][2]; n++) groundEdit.setTile(PLANKS[q][0] + n, PLANKS[q][1], P.tile(146));
}
groundEdit.apply();
meadow.addLayer(groundLayer);

// The objects: where the runner starts, and the coins, tile objects of the
// coin tile taking its class
var objects = new ObjectGroup("objects");
meadow.addLayer(objects);
var start = new MapObject(MapObject.Point, "start");
start.className = "player_start";
start.x = -16 * CELL + CELL / 2;
start.y = 0;
objects.addObject(start);
start.setProperty("facing", tiled.propertyValue("Facing", "left"));
// Coins: [cell x, row]; walked through at row -1, jumped to above it, and
// over the planks
var COINS = [
  [-80, -1], [-78, -1], [-76, -1], [-58, -1], [-57, -1], [-55, -5], [-54, -5],
  [-42, -3], [-40, -3], [-33, -1], [-32, -1], [-31, -1], [-11, -5], [-10, -5],
  [-5, -1], [-3, -1], [21, -6], [22, -6], [26, -1], [28, -1], [30, -1], [44, -1],
  [ISLAND.x + 4, ISLAND.y - 1],
];
for (var co = 0; co < COINS.length; co++) {
  objects.addObject(tileObject(P, 151, COINS[co][0] * CELL, (COINS[co][1] + 1) * CELL, "coin"));
}
save(meadow, tiled.mapFormat("tmx"), "maps/endless-meadow.tmx");

// ---- the other grids' maps ---------------------------------------------------

var SG = external("tilesets/sketch-town-ground.tsx");
var SP = external("tilesets/sketch-town-props.tsx");
var HX = external("tilesets/hexagon-board.tsx");
var DG = external("tilesets/dungeon.tsx");

function gridMap(orientation, tileWidth, tileHeight, width, height, background) {
  var map = new TileMap();
  map.orientation = orientation;
  map.setTileSize(tileWidth, tileHeight);
  map.setSize(width, height);
  map.layerDataFormat = TileMap.CSV;
  map.backgroundColor = background;
  return map;
}

function addStart(map, x, y) {
  var objects = new ObjectGroup("objects");
  map.addLayer(objects);
  var point = new MapObject(MapObject.Point, "start");
  point.className = "player_start";
  point.x = x;
  point.y = y;
  objects.addObject(point);
}

// The ground tile whose corners are [top-right, bottom-right, bottom-left,
// top-left]
function cornerTile(c) {
  for (var i = 0; i < GROUND_FILES.length; i++) {
    var w = GROUND_FILES[i][1];
    if (w[0] == c[0] && w[1] == c[1] && w[2] == c[2] && w[3] == c[3]) return i;
  }
  fail("no ground tile for corners " + c);
}

// Trees and rocks, by a hash of the cell: [tile, share]
function prop(roll, trees, rocks) {
  if (roll < trees) {
    var kinds = [PROP.tree_single_N, PROP.tree_single_E, PROP.tree_multiple_N, PROP.tree_multiple_E];
    return kinds[Math.floor(roll / trees * kinds.length)];
  }
  if (roll < trees + rocks) return roll < trees + rocks / 2 ? PROP.rocks_grass_N : PROP.rocks_grass_E;
  return null;
}

// ---- the isometric town ------------------------------------------------------

// 16 x 16 isometric cells of 116 x 55 (an even width, which draws tall art
// chunk by chunk). A castle wall rings cells 2 to 10 each way, a tower at
// each corner and an open gate in each of its two front sides; trees and
// rocks stand outside it and inside, clear of the walls; a pond lies right
// of it, its water at the corners (map points) in TOWN_POND and its edges picked
// from the Wang set's corners. The ground collides where it is water, the
// props where they are walls; the props are y-sorted. The character starts
// inside, by the right-hand gate
var TOWN = 16, RING_MIN = 2, RING_MAX = 10;
var town = gridMap(TileMap.Isometric, ISO_W, ISO_H, TOWN, TOWN, "#78b9d8");
town.addTileset(SG);
town.addTileset(SP);
var TOWN_POND = new CellSet();
for (var qx = 13; qx <= 15; qx++) for (var qy = 1; qy <= 4; qy++) TOWN_POND.add(qx, qy);
TOWN_POND.add(12, 2);
TOWN_POND.add(12, 3);
function pondCorners(x, y) {
  var at = function (px, py) { return TOWN_POND.has(px, py) ? 2 : 1; };
  return [at(x + 1, y), at(x + 1, y + 1), at(x, y + 1), at(x, y)];
}
function onRing(x, y) {
  var inside = x >= RING_MIN && x <= RING_MAX && y >= RING_MIN && y <= RING_MAX;
  return inside && (x == RING_MIN || x == RING_MAX || y == RING_MIN || y == RING_MAX);
}
function nearRing(x, y) {
  for (var dy = -1; dy <= 1; dy++) for (var dx = -1; dx <= 1; dx++) if (onRing(x + dx, y + dy)) return true;
  return false;
}
var TOWN_START = [8, 6];
var townGround = tileLayer("ground", { occludes: false });
var townProps = tileLayer("props", { y_sort: true, occludes: false });
var townGroundEdit = townGround.edit(), townPropsEdit = townProps.edit();
for (var ty = 0; ty < TOWN; ty++) {
  for (var tx = 0; tx < TOWN; tx++) {
    var corners = pondCorners(tx, ty);
    townGroundEdit.setTile(tx, ty, SG.tile(cornerTile(corners)));
    var wet = corners.indexOf(2) >= 0;
    var propId = null;
    if (onRing(tx, ty)) {
      var corner = (tx == RING_MIN || tx == RING_MAX) && (ty == RING_MIN || ty == RING_MAX);
      if (corner) propId = PROP.castle_tower_N;
      // A gate's arch faces out of its side: `_N`'s down and right, `_E`'s
      // down and left
      else if (tx == RING_MAX && ty == 6) propId = PROP.castle_gateOpen_N;
      else if (ty == RING_MAX && tx == 6) propId = PROP.castle_gateOpen_E;
      else propId = ty == RING_MIN || ty == RING_MAX ? PROP.castle_wall_N : PROP.castle_wall_E;
    } else if (!wet && !nearRing(tx, ty) && Math.abs(tx - TOWN_START[0]) + Math.abs(ty - TOWN_START[1]) > 1) {
      var inside = tx > RING_MIN && tx < RING_MAX && ty > RING_MIN && ty < RING_MAX;
      propId = prop(hash(tx, ty, 21), inside ? 0.14 : 0.24, 0.05);
    }
    if (propId !== null) townPropsEdit.setTile(tx, ty, SP.tile(propId));
  }
}
townGroundEdit.apply();
townPropsEdit.apply();
town.addLayer(townGround);
town.addLayer(townProps);
// An isometric map's objects are placed in tile heights along its axes
addStart(town, (TOWN_START[0] + 0.5) * ISO_H, (TOWN_START[1] + 0.5) * ISO_H);
save(town, tiled.mapFormat("tmx"), "maps/town.tmx");

// ---- the staggered island ----------------------------------------------------

// The same island on two staggered grids of 116 x 55 cells, each 1450 x
// 675 px: rows shifted (staggered along y, odd rows half a cell right; 12
// x 24 cells) and columns shifted (along x, odd columns half a cell down; 24
// x 12). Tiled steps staggered grids in whole pixels, half a cell rounding
// down: rows 27 apart and shifted 58 along y, columns 58 apart and their
// cells 54 apart, shifted 27, along x (the 55 px tiles reach a pixel above
// those steps' 54 px diamonds). A cell is grass where its centre is inside
// the island's outline (in pixels, so both grids show one island), else
// sea, which collides; trees grow where a value noise of the centre is high
// (groves at the same places on both), rocks by a hash of the cell. The
// character starts in the middle
var ISLAND_W = 1450, ISLAND_H = 675;
// Half a cell across and along, in Tiled's whole pixels
var ISO_HALF_W = Math.floor(ISO_W / 2), ISO_HALF_H = Math.floor(ISO_H / 2);

// Smooth value noise in 0..1 on a lattice one unit apart
function noise(x, y, salt) {
  var x0 = Math.floor(x), y0 = Math.floor(y);
  var fx = x - x0, fy = y - y0;
  var sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  var at = function (dx, dy) { return hash(x0 + dx, y0 + dy, salt); };
  var top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
  var bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
  return top + (bottom - top) * sy;
}
function onIsland(px, py) {
  var dx = (px - ISLAND_W / 2) / 530, dy = (py - ISLAND_H / 2) / 235;
  var angle = Math.atan2(dy, dx);
  var edge = 1 + 0.12 * Math.sin(3 * angle + 0.5) + 0.08 * Math.sin(5 * angle + 2);
  return Math.sqrt(dx * dx + dy * dy) < edge;
}
function island(axis, width, height, path) {
  var map = gridMap(TileMap.Staggered, ISO_W, ISO_H, width, height, "#78b9d8");
  map.staggerAxis = axis;
  map.staggerIndex = TileMap.StaggerOdd;
  map.addTileset(SG);
  map.addTileset(SP);
  // A cell's centre where Tiled draws it, stepping in whole pixels
  var centre = function (x, y) {
    if (axis == TileMap.StaggerY) {
      var shift = y % 2 == 1 ? ISO_HALF_W : 0;
      return [x * 2 * ISO_HALF_W + shift + ISO_HALF_W, y * ISO_HALF_H + ISO_HALF_H];
    }
    var down = x % 2 == 1 ? ISO_HALF_H : 0;
    return [x * ISO_HALF_W + ISO_HALF_W, y * 2 * ISO_HALF_H + down + ISO_HALF_H];
  };
  var ground = tileLayer("ground", { occludes: false });
  var props = tileLayer("props", { y_sort: true, occludes: false });
  var g = ground.edit(), p = props.edit();
  for (var y = 0; y < height; y++) {
    for (var x = 0; x < width; x++) {
      var c = centre(x, y);
      if (!onIsland(c[0], c[1])) {
        g.setTile(x, y, SG.tile(GROUND.water_center_N));
        continue;
      }
      g.setTile(x, y, SG.tile(GROUND.grass_center_N));
      var middle = Math.abs(c[0] - ISLAND_W / 2) < 150 && Math.abs(c[1] - ISLAND_H / 2) < 75;
      if (middle) continue;
      var grove = noise(c[0] / 160, c[1] / 160, 31) > 0.62;
      var id = grove ? prop(hash(x, y, 32) * 0.3, 0.3, 0) : prop(hash(x, y, 33), 0.03, 0.04);
      if (id !== null) p.setTile(x, y, SP.tile(id));
    }
  }
  g.apply();
  p.apply();
  map.addLayer(ground);
  map.addLayer(props);
  // A staggered map's objects are placed in plain pixels
  addStart(map, ISLAND_W / 2, ISLAND_H / 2);
  save(map, tiled.mapFormat("tmx"), path);
}
island(TileMap.StaggerY, 12, 24, "maps/island-rows.tmx");
island(TileMap.StaggerX, 24, 12, "maps/island-columns.tmx");

// ---- the hex board -----------------------------------------------------------

// 8 x 9 pointy-topped hexagons of 60 x 70 with sides of 35 (close to
// regular, not quite: a regular hexagon 60 wide is 69.28 tall, which is why
// a turned hex leaves a pinhole of a pixel at some of its corners), in
// rows, odd rows half a hexagon right. The board is drawn from BOARD, a row
// a string: g grass, t pines, T trees, f forest, r rocks, d dirt, D dirt
// with trees, R dirt with rocks, k stone, s sand and c cacti. The sand's
// dunes are turned by a hash of the cell, a sixth of a turn at a time (the
// flags Tiled sets for 60 and 120 degrees, and both flips for a half turn),
// so its ripples run three ways; nothing else is turned, its trees and
// rocks standing up. Above the board, empty layers the page marks: the
// selected hex's range (see-through) and ring, the hovered hex's
// neighbours, and the line between them, each in rims of its own colour.
// No layer collides or casts shadows: hexagonal layers do neither
var BOARD = [
  "ttgggddD",
  "tTgggdDR",
  "fTggrddd",
  "ttgggkdd",
  "gggkkkss",
  "ggkkssss",
  "gggsscss",
  "rgsssscs",
  "ggssscss",
];
var HEX_TILES = {
  g: ["grass_05"], t: ["grass_10", "grass_12"], T: ["grass_13", "grass_11"], f: ["grass_12"],
  r: ["grass_14", "grass_15", "grass_16"], d: ["dirt_06"], D: ["dirt_11", "dirt_12", "dirt_14", "dirt_13"],
  R: ["dirt_16", "dirt_18", "dirt_17"], k: ["stone_07"], s: ["sand_07"], c: ["sand_14", "sand_13", "sand_12"],
};
var TURNS = [
  0,
  Tile.FlippedAntiDiagonally,
  Tile.RotatedHexagonal120,
  Tile.FlippedAntiDiagonally | Tile.RotatedHexagonal120,
  Tile.FlippedHorizontally | Tile.FlippedVertically | Tile.FlippedAntiDiagonally,
  Tile.FlippedHorizontally | Tile.FlippedVertically | Tile.RotatedHexagonal120,
];
// The marks' rims: beige, red and grey; the range a see-through sand wash
var MARKS = [["range", "sand_07", 0.45], ["ring", "sand_01", 1], ["neighbours", "stone_01", 1], ["line", "mars_01", 1]];
var board = gridMap(TileMap.Hexagonal, 60, 70, 8, BOARD.length, "#24313f");
board.staggerAxis = TileMap.StaggerY;
board.staggerIndex = TileMap.StaggerOdd;
board.hexSideLength = 35;
board.addTileset(HX);
var boardLayer = tileLayer("board", { collides: false, occludes: false });
var boardEdit = boardLayer.edit();
for (var by = 0; by < BOARD.length; by++) {
  for (var bx = 0; bx < BOARD[by].length; bx++) {
    var kind = BOARD[by][bx];
    var options = HEX_TILES[kind];
    if (!options) fail("no hex tile for '" + kind + "'");
    var roll = hash(bx, by, 41);
    var flags = kind == "s" ? TURNS[Math.floor(hash(bx, by, 42) * TURNS.length)] : 0;
    boardEdit.setTile(bx, by, HX.tile(hexTile(options[Math.floor(roll * options.length)])), flags);
  }
}
boardEdit.apply();
board.addLayer(boardLayer);
for (var mk = 0; mk < MARKS.length; mk++) {
  var mark = tileLayer(MARKS[mk][0], { collides: false, occludes: false, mark: hexTile(MARKS[mk][1]) });
  mark.opacity = MARKS[mk][2];
  board.addLayer(mark);
}
save(board, tiled.mapFormat("tmx"), "maps/hex-board.tmx");

// ---- the oblique dungeon -----------------------------------------------------

// 24 x 16 cells of 16 px skewed 4 px across a row (and its mirror, -4):
// Tiny Dungeon's sand floor everywhere, picked by a hash of the cell, and
// its grey brick walls round the edge, in two columns and a short row
// inside and as a few pillars; the walls collide. The character starts
// left of the first column
function dungeonMap(skew, path) {
  var map = gridMap(TileMap.Oblique, 16, 16, 24, 16, "#25131a");
  map.skewX = skew;
  map.skewY = 0;
  map.addTileset(DG);
  var floor = tileLayer("floor", { collides: false, occludes: false });
  var walls = tileLayer("walls", { occludes: false });
  var f = floor.edit(), w = walls.edit();
  var wall = function (x, y) {
    if (x == 0 || y == 0 || x == 23 || y == 15) return true;
    if (x == 8 && y >= 3 && y <= 10) return true;
    if (x == 16 && y >= 5 && y <= 12) return true;
    if (y == 8 && x >= 11 && x <= 13) return true;
    return (x == 4 && (y == 4 || y == 11)) || (x == 20 && (y == 3 || y == 12));
  };
  for (var y = 0; y < 16; y++) {
    for (var x = 0; x < 24; x++) {
      f.setTile(x, y, DG.tile(FLOORS[Math.floor(hash(x, y, 51) * FLOORS.length)]));
      if (wall(x, y)) w.setTile(x, y, DG.tile(WALL));
    }
  }
  f.apply();
  w.apply();
  map.addLayer(floor);
  map.addLayer(walls);
  // An oblique map's objects are placed in its pixels unskewed
  addStart(map, 3.5 * 16, 7.5 * 16);
  save(map, tiled.mapFormat("tmx"), path);
}
dungeonMap(4, "maps/dungeon.tmx");
dungeonMap(-4, "maps/dungeon-mirror.tmx");

tiled.log("make-maps.js: wrote the project's classes, 5 tilesets and 7 maps");
