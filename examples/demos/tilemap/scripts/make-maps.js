// Writes the tilemap demo's Tiled files through Tiled's own scripting API:
// the property types of project.tiled-project, the four tilesets and the
// five maps. make-maps runs it from assets/, after writing the images it
// reads and an empty project:
//
//   cargo run -p tilemap-demo --bin make-maps
//
// which comes down to (Tiled 1.12.2, from examples/demos/tilemap/assets):
//
//   Tiled --project project.tiled-project --evaluate ../scripts/make-maps.js
//
// Every choice is made here, by rule or by a hash of the cell, so a run
// writes the same files each time. Terrain tiles are picked the way a corner
// Wang set reads them: a corner is solid when the cells around it are (the
// ground), or open when any cell around it is floor (the dungeon), and the
// tile is the one whose corners match.

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

// One of [[id, probability], ...], weighted, by the cell's hash
function pick(choices, x, y, salt) {
  var total = 0;
  for (var i = 0; i < choices.length; i++) total += choices[i][1];
  var at = hash(x, y, salt) * total;
  for (var j = 0; j < choices.length; j++) {
    at -= choices[j][1];
    if (at < 0) return choices[j][0];
  }
  return choices[choices.length - 1][0];
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

function polygon(points, properties) {
  var shape = new MapObject(MapObject.Polygon, "");
  var out = [];
  for (var i = 0; i < points.length; i++) out.push(Qt.point(points[i][0], points[i][1]));
  shape.polygon = out;
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

// A corner Wang set from [[tile, [top-right, bottom-right, bottom-left,
// top-left]], ...]
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

// For each corner key "tr,br,bl,tl", the set's tiles with those corners and
// their probabilities
function cornerTable(tiles, probability) {
  var table = {};
  for (var i = 0; i < tiles.length; i++) {
    var key = tiles[i][1].join(",");
    (table[key] = table[key] || []).push([tiles[i][0], probability[tiles[i][0]] || 1]);
  }
  return table;
}

function writeText(path, text) {
  var out = new TextFile(file(path), TextFile.WriteOnly);
  out.write(text);
  out.commit();
}

function readText(path) {
  var input = new TextFile(file(path), TextFile.ReadOnly);
  var text = input.readAll();
  input.close();
  return text;
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
classType("switch", [["on", false]], ClassPropertyType.MapObjectClass);
classType("door", [["open", false], ["switch", tiled.objectRef(0)]], ClassPropertyType.MapObjectClass);
classType("spikes", [["damage", 1]], ClassPropertyType.TileClass);

// ---- tilesets --------------------------------------------------------------

// Pixel Platformer, 18 px: terrain and blocks solid, the rope bridge and the
// planks one-way, the pine's trunk solid (for the top-down map), the coin,
// water and waterfall animated, coins and spikes of a class
var platformer = new Tileset("platformer");
platformer.setTileSize(18, 18);
platformer.imageFileName = file("kenney-pixel-platformer/tilemap_packed.png");
solid(platformer, [].concat(
  range(0, 6), range(9, 11), range(20, 26), range(28, 31), range(40, 43), range(47, 50),
  range(60, 63), range(80, 83), range(100, 104), range(120, 123), range(140, 143), [130, 150]));
shapes(platformer, [90, 91, 92], function () { return [rect(0, 6, 18, 6, { one_way: true })]; });
shapes(platformer, [146, 147], function () { return [rect(0, 0, 18, 10, { one_way: true })]; });
shapes(platformer, [126], function () { return [rect(7, 14, 4, 4)]; });
animate(platformer, 151, [[151, 300], [152, 150]]);
animate(platformer, 33, [[33, 500], [53, 500]]);
animate(platformer, 34, [[34, 150], [35, 150]]);
animate(platformer, 54, [[54, 150], [55, 150]]);
animate(platformer, 74, [[74, 150], [75, 150]]);
platformer.tile(151).className = "coin";
platformer.tile(68).className = "spikes";
save(platformer, tiled.tilesetFormat("tsx"), "tilesets/platformer.tsx");

// The ground (images/ground.png, made by make-maps from Pixel Platformer):
// a corner Wang set with every corner combination, two slopes and their feet
var GROUND_TILES = [
  [0, [0, 1, 0, 0]], [1, [0, 1, 1, 0]], [2, [0, 0, 1, 0]], [3, [1, 0, 1, 1]], [4, [1, 1, 0, 1]],
  [5, [1, 1, 0, 0]], [6, [1, 1, 1, 1]], [7, [0, 0, 1, 1]], [8, [0, 1, 1, 1]], [9, [1, 1, 1, 0]],
  [10, [1, 0, 0, 0]], [11, [1, 0, 0, 1]], [12, [0, 0, 0, 1]], [13, [0, 1, 0, 1]], [14, [1, 0, 1, 0]],
  [15, [1, 1, 1, 1]],
];
var GROUND_PROBABILITY = { 15: 0.5 };
var ground = new Tileset("ground");
ground.setTileSize(18, 18);
ground.imageFileName = file("images/ground.png");
solid(ground, range(0, 15).concat([18, 19]));
shapes(ground, [16], function () { return [polygon([[18, 0], [18, 18], [0, 18]])]; });
shapes(ground, [17], function () { return [polygon([[0, 0], [18, 18], [0, 18]])]; });
ground.tile(15).probability = GROUND_PROBABILITY[15];
wangSet(ground, "ground", ["ground"], GROUND_TILES, 6);
save(ground, tiled.tilesetFormat("tsx"), "tilesets/ground.tsx");

// Tiny Dungeon, 16 px: walls, doors and fountains solid, statues solid at
// their base and casting shadows with their body, the gargoyle and the drain
// running
var dungeon = new Tileset("dungeon");
dungeon.setTileSize(16, 16);
dungeon.imageFileName = file("kenney-tiny-dungeon/tilemap_packed.png");
solid(dungeon, [].concat(range(0, 11), range(12, 23), range(24, 28), range(33, 35), [40], range(45, 47), range(57, 59)));
shapes(dungeon, [64], function () {
  return [rect(5, 8, 6, 7), rect(2, 1, 12, 14, { occluder: true })];
});
shapes(dungeon, [65], function () {
  return [rect(2, 7, 12, 8), rect(2, 1, 12, 14, { occluder: true })];
});
shapes(dungeon, [63, 75], function () { return [rect(0, 2, 16, 14)]; });
shapes(dungeon, [72], function () { return [rect(1, 4, 14, 10)]; });
shapes(dungeon, [82], function () { return [rect(3, 4, 10, 11)]; });
shapes(dungeon, [89], function () { return [rect(1, 5, 14, 10)]; });
animate(dungeon, 19, [[19, 600], [20, 600]]);
animate(dungeon, 43, [[43, 400], [44, 400]]);
save(dungeon, tiled.tilesetFormat("json"), "tilesets/dungeon.tsj");

// The dungeon's terrain (images/dungeon-terrain.png, made by make-maps from
// Tiny Dungeon): a corner Wang set of wall (1) and floor (2), floor the
// higher so procgen's corridors keep their floor; walls one cell thick,
// steps (one-way, for the LDtk level), two slopes, and wall tops open
// above (for the LDtk level's walls, seen from the side)
var W = 1, F = 2;
var TERRAIN_TILES = [
  [0, [W, F, W, W]], [1, [W, F, F, W]], [2, [W, W, F, W]], [3, [F, W, F, F]], [4, [F, F, W, F]],
  [5, [W, W, W, W]], [6, [W, W, W, W]], [7, [W, W, W, W]],
  [8, [F, F, W, W]], [9, [F, F, F, F]], [10, [W, W, F, F]], [11, [W, F, F, F]], [12, [F, F, F, W]],
  [13, [F, F, F, F]], [14, [F, F, F, F]], [15, [F, W, F, W]],
  [16, [F, W, W, W]], [17, [F, W, W, F]], [18, [W, W, W, F]], [19, [W, F, W, F]],
];
var TERRAIN_PROBABILITY = { 6: 0.1, 7: 0.1, 13: 0.5, 14: 0.1 };
var terrain = new Tileset("dungeon-terrain");
terrain.setTileSize(16, 16);
terrain.imageFileName = file("images/dungeon-terrain.png");
solid(terrain, [].concat(range(0, 8), range(10, 12), range(15, 26), range(33, 37)));
shapes(terrain, range(27, 30), function () { return [rect(0, 0, 16, 16, { one_way: true })]; });
shapes(terrain, [31], function () { return [polygon([[16, 0], [16, 16], [0, 16]])]; });
shapes(terrain, [32], function () { return [polygon([[0, 0], [16, 16], [0, 16]])]; });
for (var id in TERRAIN_PROBABILITY) terrain.tile(Number(id)).probability = TERRAIN_PROBABILITY[id];
wangSet(terrain, "dungeon", ["wall", "floor"], TERRAIN_TILES, 9);
save(terrain, tiled.tilesetFormat("json"), "tilesets/dungeon-terrain.tsj");

// ---- the tilesets as the maps reference them -------------------------------

// A map written by a script embeds a tileset it built; one read from a file
// keeps its file name, so the maps take their tilesets from a scratch map
// that names the four files
writeText("make-maps-scratch.tmx",
  '<?xml version="1.0" encoding="UTF-8"?>\n' +
  '<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" ' +
  'width="1" height="1" tilewidth="16" tileheight="16" infinite="0" nextlayerid="1" nextobjectid="1">\n' +
  ' <tileset firstgid="1" source="tilesets/platformer.tsx"/>\n' +
  ' <tileset firstgid="181" source="tilesets/ground.tsx"/>\n' +
  ' <tileset firstgid="201" source="tilesets/dungeon.tsj"/>\n' +
  ' <tileset firstgid="333" source="tilesets/dungeon-terrain.tsj"/>\n' +
  '</map>\n');
var scratch = tiled.mapFormat("tmx").read(file("make-maps-scratch.tmx"));
File.remove(file("make-maps-scratch.tmx"));
var P = scratch.tilesets[0], G = scratch.tilesets[1], D = scratch.tilesets[2], T = scratch.tilesets[3];

function newMap(width, height, tile, tilesets, background) {
  var map = new TileMap();
  map.setSize(width, height);
  map.setTileSize(tile, tile);
  map.layerDataFormat = TileMap.CSV;
  if (background) map.backgroundColor = background;
  for (var i = 0; i < tilesets.length; i++) map.addTileset(tilesets[i]);
  return map;
}

function tileLayer(map, name, properties) {
  var layer = new TileLayer(name);
  layer.width = map.width;
  layer.height = map.height;
  for (var key in properties || {}) layer.setProperty(key, properties[key]);
  return layer;
}

// Fills `layer` from rows of characters: `place(c, x, y)` returns [tileset,
// id, flags] for a cell, or null
function paint(layer, rows, place) {
  var edit = layer.edit();
  for (var y = 0; y < rows.length; y++) {
    for (var x = 0; x < rows[y].length; x++) {
      var cell = place(rows[y][x], x, y);
      if (cell) edit.setTile(x, y, cell[0].tile(cell[1]), cell[2] || 0);
    }
  }
  edit.apply();
}

function at(rows, x, y) {
  // outside the map, the nearest cell inside
  y = Math.max(0, Math.min(rows.length - 1, y));
  x = Math.max(0, Math.min(rows[y].length - 1, x));
  return rows[y][x];
}

function check(rows, width, height, what) {
  if (rows.length != height) fail(what + ": " + rows.length + " rows, not " + height);
  for (var y = 0; y < rows.length; y++) {
    if (rows[y].length != width) fail(what + ": row " + y + " is " + rows[y].length + " wide");
  }
}

function tileObject(tileset, id, x, y, name, className) {
  var object = new MapObject(name);
  object.tile = tileset.tile(id);
  object.className = className;
  object.x = x;
  object.y = y;
  object.width = tileset.tileWidth;
  object.height = tileset.tileHeight;
  return object;
}

function point(x, y, name, className) {
  var object = new MapObject(MapObject.Point, name);
  object.className = className;
  object.x = x;
  object.y = y;
  return object;
}

// ---- the platformer --------------------------------------------------------

// 48 x 16 cells of 18 px. `#` ground, `/` `\` slopes, `p` a grass ledge one
// cell high, `=` the rope bridge, `-` planks, `~` water, `w` deep water, `v`
// a waterfall, `D` the door, `^` spikes
var PLATFORMER = [
  "............................................##..",
  "............................................##..",
  "............................................##..",
  "............................................##..",
  "............................................##..",
  "............................................##..",
  "............................................##..",
  ".....................................pppp...##..",
  "............................................##..",
  "......./##\\...----..........................##..",
  "....../####\\..............v#########........D...",
  "...../######\\..^^^........v#########........D...",
  "####################======v#####################",
  "####################~~~~~~~#####################",
  "####################wwwwwww#####################",
  "################################################",
];
check(PLATFORMER, 48, 16, "the platformer");

function isGround(c) {
  return c == "#" || c == "/" || c == "\\";
}

// A corner of the ground is ground when all four cells around it are
function groundCorner(rows, x, y) {
  return isGround(at(rows, x - 1, y - 1)) && isGround(at(rows, x, y - 1)) &&
    isGround(at(rows, x - 1, y)) && isGround(at(rows, x, y)) ? 1 : 0;
}

var groundTable = cornerTable(GROUND_TILES, GROUND_PROBABILITY);
var map = newMap(48, 16, 18, [P, G], "#7ff0a5");

var backdrop = new ImageLayer("backdrop");
backdrop.imageFileName = file("images/backdrop.png");
backdrop.repeatX = true;
backdrop.parallaxFactor = Qt.point(0.5, 1);
backdrop.offset = Qt.point(0, 12 * 18 - 60);
map.addLayer(backdrop);

var scenery = tileLayer(map, "scenery", { collides: false });
var sceneryEdit = scenery.edit();
var SCENERY = [
  // x, y, tile, flags
  [1, 11, 126, 0], [3, 11, 87, Tile.FlippedHorizontally], [13, 11, 124, 0], [18, 11, 125, 0],
  [29, 9, 126, 0], [34, 9, 128, 0], [36, 11, 125, Tile.FlippedHorizontally], [37, 11, 126, 0],
  [41, 11, 126, 0], [47, 11, 88, 0],
];
for (var i = 0; i < SCENERY.length; i++) {
  var s = SCENERY[i];
  sceneryEdit.setTile(s[0], s[1], P.tile(s[2]), s[3]);
}
sceneryEdit.apply();
map.addLayer(scenery);

var water = tileLayer(map, "water", { collides: false });
paint(water, PLATFORMER, function (c, x, y) {
  if (c == "~") return [P, 33];
  if (c == "w") return [P, 73];
  if (c == "v") {
    if (at(PLATFORMER, x, y - 1) != "v") return [P, 34];
    if (at(PLATFORMER, x, y + 1) != "v") return [P, 74];
    return [P, 54];
  }
  return null;
});
map.addLayer(water);

var groundLayer = tileLayer(map, "ground");
paint(groundLayer, PLATFORMER, function (c, x, y) {
  if (c == "/") return [G, 16];
  if (c == "\\") return [G, 17];
  if (c == "#") {
    var above = at(PLATFORMER, x, y - 1);
    if (y > 0 && above == "/") return [G, 18];
    if (y > 0 && above == "\\") return [G, 19];
    var key = [groundCorner(PLATFORMER, x + 1, y), groundCorner(PLATFORMER, x + 1, y + 1),
      groundCorner(PLATFORMER, x, y + 1), groundCorner(PLATFORMER, x, y)].join(",");
    var choices = groundTable[key];
    if (!choices) fail("the platformer: ground at (" + x + ", " + y + ") is not part of two by two ground");
    return [G, pick(choices, x, y, 1)];
  }
  var left = at(PLATFORMER, x - 1, y) == c && x > 0, right = at(PLATFORMER, x + 1, y) == c && x < 47;
  if (c == "p") return [P, left ? (right ? 2 : 3) : (right ? 1 : 0)];
  if (c == "=") return [P, left ? (right ? 91 : 92) : 90];
  if (c == "-") return [P, 146];
  return null;
});
map.addLayer(groundLayer);

var doors = tileLayer(map, "doors");
paint(doors, PLATFORMER, function (c, x, y) {
  if (c != "D") return null;
  return [P, at(PLATFORMER, x, y - 1) == "D" ? 150 : 130];
});
map.addLayer(doors);

var hazards = tileLayer(map, "hazards");
paint(hazards, PLATFORMER, function (c) { return c == "^" ? [P, 68] : null; });
map.addLayer(hazards);

var objects = new ObjectGroup("objects");
map.addLayer(objects);
objects.addObject(point(2 * 18 + 9, 12 * 18, "start", "player_start"));
var COINS = [
  [4, 10], [8, 7], [9, 7], [14, 7], [15, 7], [16, 7], [22, 10], [23, 10], [30, 8], [33, 8], [38, 5], [39, 5],
];
// The coins have no class of their own: they take the coin tile's, with
// its value (1, the class's default), as Tiled shows them
for (var c = 0; c < COINS.length; c++) {
  objects.addObject(tileObject(P, 151, COINS[c][0] * 18, (COINS[c][1] + 1) * 18, "coin", ""));
}
var prize = tileObject(P, 151, 46 * 18, 12 * 18, "prize", "");
objects.addObject(prize);
prize.setProperty("value", 10);
var lever = tileObject(P, 64, 31 * 18, 10 * 18, "lever", "switch");
objects.addObject(lever);
var door = new MapObject(MapObject.Rectangle, "door");
door.className = "door";
door.x = 44 * 18;
door.y = 10 * 18;
door.width = 18;
door.height = 36;
objects.addObject(door);
door.setProperty("switch", lever);

// Tiled writes every layer of a map in the map's one format: the ground is
// written base64 zlib in a second copy and its data set into the CSV one
save(map, tiled.mapFormat("tmx"), "maps/platformer.tmx");
map.layerDataFormat = TileMap.Base64Zlib;
save(map, tiled.mapFormat("tmx"), "maps/make-maps-zlib.tmx");
function groundData(text) {
  var start = text.indexOf('name="ground"');
  var from = text.indexOf("<data", start), to = text.indexOf("</data>", from) + "</data>".length;
  if (start < 0 || from < 0) fail("no ground data in the platformer");
  return [from, to, text.slice(from, to)];
}
var csv = readText("maps/platformer.tmx");
var zlib = groundData(readText("maps/make-maps-zlib.tmx"))[2];
File.remove(file("maps/make-maps-zlib.tmx"));
var spot = groundData(csv);
writeText("maps/platformer.tmx", csv.slice(0, spot[0]) + zlib + csv.slice(spot[1]));

// ---- the top-down garden ---------------------------------------------------

// 30 x 20 cells of 16 px: sand paths through a lawn (the map's background
// colour, the backdrop's ground), pines and statues sorted by their bottom
// edge, clouds drifting above with their own parallax
var GARDEN = [];
for (var y = 0; y < 20; y++) {
  var row = "";
  for (var x = 0; x < 30; x++) {
    var path = y == 9 || y == 10 || ((x == 14 || x == 15) && y >= 3) || (y >= 7 && y <= 12 && x >= 11 && x <= 18);
    row += path ? "." : " ";
  }
  GARDEN.push(row);
}
var garden = newMap(30, 20, 16, [D, P], "#2eb082");
var sky = new ImageLayer("backdrop");
sky.imageFileName = file("images/backdrop.png");
sky.repeatX = true;
sky.parallaxFactor = Qt.point(0.5, 0.5);
sky.offset = Qt.point(0, -40);
garden.addLayer(sky);
var paths = tileLayer(garden, "paths");
paint(paths, GARDEN, function (c, x, y) {
  return c == "." ? [D, pick([[48, 6], [49, 3], [42, 1]], x, y, 2)] : null;
});
garden.addLayer(paths);
var trees = tileLayer(garden, "trees", { y_sort: true });
var treesEdit = trees.edit();
var PINES = [
  [2, 2], [5, 3], [9, 1], [20, 2], [25, 4], [27, 1], [1, 7], [6, 6], [22, 6], [28, 8],
  [3, 14], [7, 16], [10, 13], [19, 15], [23, 17], [26, 13],
];
for (var t = 0; t < PINES.length; t++) treesEdit.setTile(PINES[t][0], PINES[t][1], P.tile(126));
var STATUES = [[11, 7, 64], [18, 7, 64], [11, 12, 65], [18, 12, 65]];
for (var u = 0; u < STATUES.length; u++) treesEdit.setTile(STATUES[u][0], STATUES[u][1], D.tile(STATUES[u][2]));
treesEdit.apply();
garden.addLayer(trees);
var clouds = tileLayer(garden, "clouds", { collides: false, occludes: false, lit: false });
clouds.parallaxFactor = Qt.point(1.25, 1.25);
clouds.opacity = 0.85;
var cloudsEdit = clouds.edit();
var CLOUDS = [
  [4, 3, 153], [5, 3, 154], [6, 3, 155], [12, 1, 156], [17, 6, 153], [18, 6, 154], [19, 6, 154],
  [20, 6, 155], [26, 10, 156], [20, 15, 153], [21, 15, 154], [22, 15, 155], [2, 17, 156],
];
for (var k = 0; k < CLOUDS.length; k++) cloudsEdit.setTile(CLOUDS[k][0], CLOUDS[k][1], P.tile(CLOUDS[k][2]));
cloudsEdit.apply();
garden.addLayer(clouds);
var gardenObjects = new ObjectGroup("objects");
garden.addLayer(gardenObjects);
gardenObjects.addObject(point(15 * 16, 10 * 16, "start", "player_start"));
save(garden, tiled.mapFormat("json"), "maps/topdown.tmj");

// ---- three rooms -----------------------------------------------------------

// 20 x 12 cells of 16 px each, side by side in rooms/rooms.world: floor
// inside a wall two cells thick, doorways where the rooms meet, brick faces
// under the north wall
var terrainTable = cornerTable(TERRAIN_TILES, TERRAIN_PROBABILITY);

function roomRows(west, east) {
  var rows = [];
  for (var y = 0; y < 12; y++) {
    var row = "";
    for (var x = 0; x < 20; x++) {
      var inside = x >= 2 && x <= 17 && y >= 2 && y <= 9;
      var doorway = (y == 5 || y == 6) && ((west && x < 2) || (east && x > 17));
      row += inside || doorway ? "." : "#";
    }
    rows.push(row);
  }
  return rows;
}

// A corner is floor when any cell around it is floor: the higher colour
// covers the lower, as procgen's terrain maps read a grid
function floorCorner(rows, x, y) {
  return at(rows, x - 1, y - 1) == "." || at(rows, x, y - 1) == "." ||
    at(rows, x - 1, y) == "." || at(rows, x, y) == "." ? F : W;
}

function room(rows, file, format, props, extra) {
  var r = newMap(20, 12, 16, [T, D]);
  var land = tileLayer(r, "terrain", { occludes: true });
  paint(land, rows, function (c, x, y) {
    var key = [floorCorner(rows, x + 1, y), floorCorner(rows, x + 1, y + 1),
      floorCorner(rows, x, y + 1), floorCorner(rows, x, y)].join(",");
    var choices = terrainTable[key];
    if (!choices) fail(file + ": no terrain tile for corners " + key);
    return [T, pick(choices, x, y, 3)];
  });
  r.addLayer(land);
  var faces = tileLayer(r, "faces");
  paint(faces, rows, function (c, x, y) {
    if (c != "." || y == 0 || rows[y - 1][x] != "#" || x < 2 || x > 17) return null;
    return [D, extra.fountain == x ? 19 : 40];
  });
  r.addLayer(faces);
  var propsLayer = tileLayer(r, "props", { y_sort: true, occludes: false });
  var propsEdit = propsLayer.edit();
  for (var i = 0; i < props.length; i++) propsEdit.setTile(props[i][0], props[i][1], D.tile(props[i][2]));
  propsEdit.apply();
  r.addLayer(propsLayer);
  var group = new ObjectGroup("objects");
  r.addLayer(group);
  if (extra.start) group.addObject(point(extra.start[0], extra.start[1], "start", "player_start"));
  save(r, tiled.mapFormat(format), "rooms/" + file);
}

room(roomRows(false, true), "room-1.tmj", "json", [[3, 8, 82], [4, 8, 82], [15, 3, 63]], { start: [5 * 16, 7 * 16] });
room(roomRows(true, true), "room-2.tmx", "tmx", [[6, 5, 64], [13, 5, 64], [6, 8, 65], [13, 8, 65]], { fountain: 9 });
room(roomRows(true, false), "room-3.tmj", "json", [[8, 4, 89], [11, 4, 89], [9, 7, 72], [16, 8, 82]], {});

tiled.log("make-maps.js: wrote the project's classes, 4 tilesets and 5 maps");
