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
classType("patrol", [], ClassPropertyType.MapObjectClass);
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

// ---- the tileset as the maps reference it ----------------------------------

// A map written by a script embeds a tileset it built; one read from a file
// keeps its file name, so the maps take their tileset from a scratch map
// that names the file
writeText("make-maps-scratch.tmx",
  '<?xml version="1.0" encoding="UTF-8"?>\n' +
  '<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" ' +
  'width="1" height="1" tilewidth="18" tileheight="18" infinite="0" nextlayerid="1" nextobjectid="1">\n' +
  ' <tileset firstgid="1" source="tilesets/platformer.tsx"/>\n' +
  '</map>\n');
var scratch = tiled.mapFormat("tmx").read(file("make-maps-scratch.tmx"));
File.remove(file("make-maps-scratch.tmx"));
var P = scratch.tilesets[0];

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
// beside it are ground: rows of the sheet for open above and below (a one
// cell platform), open above (a grass top), closed both ways (earth) and
// open below (a bottom edge); in each row, the tile for open on both sides,
// open on the left, closed both sides, and open on the right
function groundTile(ground, x, y) {
  var up = ground.has(x, y - 1), down = ground.has(x, y + 1);
  var left = ground.has(x - 1, y), right = ground.has(x + 1, y);
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
// a meadow from cell -88 to cell 51 with its grass at row 0, hills, a pond
// under a rope bridge, a dip, planks and coins; and a small island far up
// and left, at cell (-204, -40), alone in its chunk. The `patrol` rectangle
// covers the meadow end to end: the page walls its ends, and the runner,
// starting at `player_start`, turns before them
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
  [-88, -69, 0], [-68, -67, -1], [-66, -63, -2], [-62, -61, -1], [-60, -46, 0],
  [-36, -27, 0], [-26, -21, -1], [-20, 7, 0], [8, 9, -1], [10, 13, -2], [14, 15, -1],
  [16, 33, 0], [37, 51, 0],
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
standing(-86, 85); // a sign pointing right, at the left end
standing(49, 84); // and left, at the right end
standing(-49, 128);
standing(25, 128, Tile.FlippedHorizontally);
standing(-52, 16);
standing(12, 16);
// Sprouts on open grass, by a hash of the column
for (var sx = -88; sx <= 51; sx++) {
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
  groundEdit.setTile(x, y, P.tile(groundTile(ground, x, y)));
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

// The objects: where the runner starts and the meadow it keeps to, and the
// coins, tile objects of the coin tile taking its class
var objects = new ObjectGroup("objects");
meadow.addLayer(objects);
var start = new MapObject(MapObject.Point, "start");
start.className = "player_start";
start.x = -16 * CELL + CELL / 2;
start.y = 0;
objects.addObject(start);
start.setProperty("facing", tiled.propertyValue("Facing", "left"));
var patrol = new MapObject(MapObject.Rectangle, "patrol");
patrol.className = "patrol";
patrol.x = -88 * CELL;
patrol.y = -3 * CELL;
patrol.width = (52 - -88) * CELL;
patrol.height = 3 * CELL;
objects.addObject(patrol);
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

tiled.log("make-maps.js: wrote the project's classes, 1 tileset and 1 map");
