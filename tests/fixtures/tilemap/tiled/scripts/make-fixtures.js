// Writes the fixtures for tile objects taking their tile's class, image
// layers (one with no image, one with a transparent colour), the map's
// parallax origin and infinite maps (chunks at negative cells, of Tiled's
// 16 x 16, of 8 x 4 and of 24 x 24, which straddle the engine's chunks of
// 32, in every layer format, and a tile layer in a group), through Tiled's
// own scripting API, in both formats; measured.json, where Tiled puts
// the cells of isometric, staggered, hexagonal and oblique grids; and
// measured-order.json, which of two overlapping tiles it draws over the
// other on those grids.
//
// Regenerate (Tiled 1.12.2, from the repository root):
//   /Applications/Tiled.app/Contents/MacOS/Tiled \
//     --project tests/fixtures/tilemap/tiled/project.tiled-project \
//     -e tests/fixtures/tilemap/tiled/scripts/make-fixtures.js
//
// The classes (Crate, Drop) are the fixture project's.
var dir = FileInfo.path(tiled.project.fileName) + "/";

function save(asset, format, path) {
  var error = format.write(asset, dir + path);
  if (error) throw new Error("make-fixtures.js: " + path + ": " + error);
}

// The loot tileset: tiles.png's sixteen tiles; tile 1 a Crate whose hp is
// 40, tile 2 shiny with no class. The XML twin sits a folder deeper, so its
// image path has one more ".."
function loot(image) {
  var set = new Tileset("loot");
  set.setTileSize(16, 16);
  set.image = image;
  set.tile(1).className = "Crate";
  set.tile(1).setProperty("hp", 40);
  set.tile(2).setProperty("shiny", true);
  set.transparentColor = "#ff00ff";
  return set;
}
save(loot(dir + "images/tiles.png"), tiled.tilesetFormat("json"), "tilesets/loot.tsj");
save(loot(dir + "images/tiles.png"), tiled.tilesetFormat("tsx"), "xml/tilesets/loot.tsx");

// A map written by a script embeds a tileset it built; one read from a file
// keeps its file name, so the maps take the tileset from a scratch map
// that names the file
function external(folder, source) {
  var scratch = dir + folder + "/make-fixtures-scratch.tmx";
  var out = new TextFile(scratch, TextFile.WriteOnly);
  out.write(
    '<?xml version="1.0" encoding="UTF-8"?>\n' +
      '<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" ' +
      'width="1" height="1" tilewidth="16" tileheight="16" infinite="0" nextlayerid="1" nextobjectid="1">\n' +
      ' <tileset firstgid="1" source="' + source + '"/>\n' +
      "</map>\n"
  );
  out.commit();
  var map = tiled.mapFormat("tmx").read(scratch);
  File.remove(scratch);
  return map.tilesets[0];
}
var jsonSet = external("maps", "../tilesets/loot.tsj");
var xmlSet = external("xml/maps", "../tilesets/loot.tsx");

// Tile objects with no class of their own, an own property, an own class,
// and a tile with no data
function tileObjects(set, path, format) {
  var map = new TileMap();
  map.setSize(4, 4);
  map.setTileSize(16, 16);
  map.addTileset(set);
  var group = new ObjectGroup("things");
  group.drawOrder = ObjectGroup.IndexOrder;
  map.addLayer(group);
  var specs = [
    ["crate", 1, "", null],
    ["weak crate", 1, "", ["hp", 7]],
    ["gem", 2, "", null],
    ["drop crate", 1, "Drop", null],
    ["plain", 0, "", null],
  ];
  for (var i = 0; i < specs.length; i++) {
    var o = new MapObject(specs[i][0]);
    o.tile = set.tile(specs[i][1]);
    o.className = specs[i][2];
    o.x = 16 * i;
    o.y = 16;
    o.width = 16;
    o.height = 16;
    group.addObject(o);
    if (specs[i][3]) o.setProperty(specs[i][3][0], specs[i][3][1]);
  }
  save(map, format, path);
}
tileObjects(jsonSet, "maps/tile-objects.tmj", tiled.mapFormat("json"));
tileObjects(xmlSet, "xml/maps/tile-objects.tmx", tiled.mapFormat("tmx"));

// A map whose parallax origin is moved, with a parallax layer, an image
// layer Tiled made and left without an image, and one keyed to magenta
function imageLayers(path, format) {
  var map = new TileMap();
  map.setSize(4, 4);
  map.setTileSize(16, 16);
  map.parallaxOrigin = Qt.point(40, 24);
  var far = new TileLayer("far");
  far.parallaxFactor = Qt.point(0.5, 0.25);
  map.addLayer(far);
  map.addLayer(new ImageLayer("empty"));
  var keyed = new ImageLayer("keyed");
  keyed.imageFileName = dir + "images/bg.png";
  keyed.transparentColor = "#ff00ff";
  map.addLayer(keyed);
  save(map, format, path);
}
imageLayers("maps/image-layers.tmj", tiled.mapFormat("json"));
imageLayers("xml/maps/image-layers.tmx", tiled.mapFormat("tmx"));

// ---- infinite maps --------------------------------------------------------

var jsonTiles = external("maps", "../tilesets/tiles.tsj");
var xmlTiles = external("xml/maps", "../tilesets/tiles.tsx");

// The tiles every infinite fixture's "ground" layer holds: [x, y, tile id,
// flags], negative cells included, across chunks of either size
var groundTiles = [
  [-20, -3, 0, 0],
  [-1, -1, 1, Tile.FlippedHorizontally],
  [0, 0, 2, 0],
  [5, 2, 3, Tile.FlippedHorizontally | Tile.FlippedVertically | Tile.FlippedAntiDiagonally],
  [40, 10, 5, 0],
  [-17, 18, 6, Tile.FlippedVertically],
];

function paint(layer, set, tiles) {
  var edit = layer.edit();
  for (var i = 0; i < tiles.length; i++) {
    var t = tiles[i];
    edit.setTile(t[0], t[1], t[3] === null ? null : set.tile(t[2]), t[3] || 0);
  }
  edit.apply();
}

function infiniteMap(set, format) {
  var map = new TileMap();
  map.setTileSize(16, 16);
  map.infinite = true;
  map.layerDataFormat = format;
  map.addTileset(set);
  return map;
}

function groundLayer(map, set) {
  var ground = new TileLayer("ground");
  map.addLayer(ground);
  paint(ground, set, groundTiles);
  return ground;
}

// Three tile layers with different chunks (16 x 16, Tiled's default), one
// of which had a tile written and erased in a chunk of its own, which
// Tiled leaves out; and objects at negative coordinates
function infinite(set, path, format) {
  var map = infiniteMap(set, TileMap.CSV);
  groundLayer(map, set);
  var walls = new TileLayer("walls");
  map.addLayer(walls);
  paint(walls, set, [
    [-50, 20, 7, 0],
    [17, -17, 8, 0],
    [18, -17, 8, 0],
  ]);
  var decor = new TileLayer("decor");
  map.addLayer(decor);
  paint(decor, set, [
    [3, 3, 9, 0],
    [70, -40, 10, 0],
  ]);
  paint(decor, set, [[70, -40, 0, null]]);
  var things = new ObjectGroup("things");
  things.drawOrder = ObjectGroup.IndexOrder;
  map.addLayer(things);
  var crate = new MapObject("crate");
  crate.x = -40;
  crate.y = -24;
  crate.width = 16;
  crate.height = 16;
  things.addObject(crate);
  var spot = new MapObject("spot");
  spot.shape = MapObject.Point;
  spot.x = -8.5;
  spot.y = -100;
  things.addObject(spot);
  save(map, format, path);
}
infinite(jsonTiles, "maps/infinite.tmj", tiled.mapFormat("json"));
infinite(xmlTiles, "xml/maps/infinite.tmx", tiled.mapFormat("tmx"));

// The ground layer in every encoding, each format's own
var encodings = [
  ["csv", TileMap.CSV],
  ["base64", TileMap.Base64],
  ["zlib", TileMap.Base64Zlib],
  ["gzip", TileMap.Base64Gzip],
  ["zstd", TileMap.Base64Zstandard],
];
for (var e = 0; e < encodings.length; e++) {
  var name = "infinite-" + encodings[e][0];
  var jsonMap = infiniteMap(jsonTiles, encodings[e][1]);
  groundLayer(jsonMap, jsonTiles);
  save(jsonMap, tiled.mapFormat("json"), "maps/" + name + ".tmj");
  var xmlMap = infiniteMap(xmlTiles, encodings[e][1]);
  groundLayer(xmlMap, xmlTiles);
  save(xmlMap, tiled.mapFormat("tmx"), "xml/maps/" + name + ".tmx");
}
// XML's own <tile> elements, which JSON has no form of
var plain = infiniteMap(xmlTiles, TileMap.XML);
groundLayer(plain, xmlTiles);
save(plain, tiled.mapFormat("tmx"), "xml/maps/infinite-xml.tmx");

// The ground layer in chunks of 8 x 4 (the map's output chunk size)
function chunks8(set, path, format) {
  var map = infiniteMap(set, TileMap.CSV);
  map.chunkSize = Qt.size(8, 4);
  groundLayer(map, set);
  save(map, format, path);
}
chunks8(jsonTiles, "maps/infinite-chunks8.tmj", tiled.mapFormat("json"));
chunks8(xmlTiles, "xml/maps/infinite-chunks8.tmx", tiled.mapFormat("tmx"));

// Tiles on both sides of the engine's chunk borders at x 32 and -32, each
// pair in one of Tiled's chunks of 24 x 24 (which straddle them)
var straddleTiles = [
  [30, 5, 4, 0],
  [33, 5, 7, 0],
  [-33, -25, 8, 0],
  [-31, -25, 9, Tile.FlippedHorizontally],
];

// The ground layer and the straddling tiles in chunks of 24 x 24, and a
// tile layer inside a group, its tiles across the borders too
function chunks24(set, path, format) {
  var map = infiniteMap(set, TileMap.CSV);
  map.chunkSize = Qt.size(24, 24);
  var ground = groundLayer(map, set);
  paint(ground, set, straddleTiles);
  var level = new GroupLayer("level");
  map.addLayer(level);
  var inner = new TileLayer("inner");
  level.addLayer(inner);
  paint(inner, set, [
    [-40, 47, 11, 0],
    [31, -49, 12, Tile.FlippedVertically],
    [32, -49, 13, 0],
  ]);
  save(map, format, path);
}
chunks24(jsonTiles, "maps/infinite-chunks24.tmj", tiled.mapFormat("json"));
chunks24(xmlTiles, "xml/maps/infinite-chunks24.tmx", tiled.mapFormat("tmx"));

// ---- measure ----------------------------------------------------------------

// Where Tiled puts cells on every grid, for `MapGrid`'s tests: measured.json
// holds, for each sample grid, `tileToScreen` of the cells -3..3 a side
// (`screen`: [x, y, screen x, screen y]) and, on bounded grids, where
// `toImage()` drew a solid tile image the size of the cell at a few cells
// inside the map (`drawn`: [x, y, left column, the row below its bottom], in
// the render's pixels); for the hexagonal grid of 32 x 32 cells and side 12,
// what `screenToTile` picks at points on and near shared edges (`picks`:
// [screen x, screen y, x, y]); and which of two overlapping tiles an
// oblique map draws over the other (`oblique_order`), a tall pair one above
// the other and a wide pair side by side, for each render order.
var I = TileMap.Isometric, S = TileMap.Staggered, H = TileMap.Hexagonal, O = TileMap.Oblique;
var AX = TileMap.StaggerX, AY = TileMap.StaggerY, ODD = TileMap.StaggerOdd, EVEN = TileMap.StaggerEven;

function sampleMap(g) {
  var map = new TileMap();
  map.setTileSize(g.tile[0], g.tile[1]);
  if (g.infinite) map.infinite = true;
  else map.setSize(g.size[0], g.size[1]);
  map.orientation = g.orientation;
  if (g.axis !== undefined) map.staggerAxis = g.axis;
  if (g.index !== undefined) map.staggerIndex = g.index;
  if (g.side !== undefined) map.hexSideLength = g.side;
  if (g.skew) {
    map.skewX = g.skew[0];
    map.skewY = g.skew[1];
  }
  return map;
}

function solidImage(width, height, argb) {
  var image = new Image(width, height, Image.Format_ARGB32);
  image.fill(argb);
  return image;
}

// A map of `g` holding `tiles` ([x, y, image]) in one layer, rendered
function render(g, renderOrder, tiles) {
  var map = sampleMap(g);
  if (renderOrder !== undefined) map.renderOrder = renderOrder;
  var set = new Tileset("solid");
  map.addTileset(set);
  var layer = new TileLayer("tiles");
  map.addLayer(layer);
  var edit = layer.edit();
  for (var i = 0; i < tiles.length; i++) {
    var tile = set.addTile();
    tile.setImage(tiles[i][2]);
    edit.setTile(tiles[i][0], tiles[i][1], tile);
  }
  edit.apply();
  return map.toImage();
}

// The left column and the row below the bottom of what a render covers
function coverBottomLeft(image) {
  var left = image.width, bottom = 0;
  for (var y = 0; y < image.height; y++) {
    for (var x = 0; x < image.width; x++) {
      if ((image.pixel(x, y) >>> 24) > 0) {
        left = Math.min(left, x);
        bottom = Math.max(bottom, y + 1);
      }
    }
  }
  return [left, bottom];
}

function countPixels(image, argb) {
  var n = 0;
  for (var y = 0; y < image.height; y++)
    for (var x = 0; x < image.width; x++) if (image.pixel(x, y) >>> 0 === argb >>> 0) n++;
  return n;
}

var sampleGrids = [
  { name: "isometric 64x32", orientation: I, tile: [64, 32], size: [5, 3] },
  { name: "isometric 64x32 infinite", orientation: I, tile: [64, 32], infinite: true },
  { name: "isometric 48x32", orientation: I, tile: [48, 32], size: [5, 3] },
  { name: "isometric 48x32 infinite", orientation: I, tile: [48, 32], infinite: true },
  { name: "staggered x odd", orientation: S, tile: [64, 32], size: [5, 5], axis: AX, index: ODD },
  { name: "staggered x even", orientation: S, tile: [64, 32], size: [5, 5], axis: AX, index: EVEN },
  { name: "staggered y odd", orientation: S, tile: [64, 32], size: [5, 5], axis: AY, index: ODD },
  { name: "staggered y even", orientation: S, tile: [64, 32], size: [5, 5], axis: AY, index: EVEN },
  { name: "hexagonal x odd side 12", orientation: H, tile: [32, 32], size: [5, 5], axis: AX, index: ODD, side: 12 },
  { name: "hexagonal x even side 12", orientation: H, tile: [32, 32], size: [5, 5], axis: AX, index: EVEN, side: 12 },
  { name: "hexagonal y odd side 12", orientation: H, tile: [32, 32], size: [5, 5], axis: AY, index: ODD, side: 12 },
  { name: "hexagonal y even side 12", orientation: H, tile: [32, 32], size: [5, 5], axis: AY, index: EVEN, side: 12 },
  { name: "hexagonal x odd side 0", orientation: H, tile: [32, 32], size: [5, 5], axis: AX, index: ODD, side: 0 },
  { name: "hexagonal x even side 0", orientation: H, tile: [32, 32], size: [5, 5], axis: AX, index: EVEN, side: 0 },
  { name: "hexagonal y odd side 0", orientation: H, tile: [32, 32], size: [5, 5], axis: AY, index: ODD, side: 0 },
  { name: "hexagonal y even side 0", orientation: H, tile: [32, 32], size: [5, 5], axis: AY, index: EVEN, side: 0 },
  { name: "oblique skew 8 12", orientation: O, tile: [32, 32], size: [5, 5], skew: [8, 12] },
  { name: "oblique skew -10 0", orientation: O, tile: [32, 32], size: [5, 5], skew: [-10, 0] },
  { name: "oblique skew 0 -12", orientation: O, tile: [32, 32], size: [5, 5], skew: [0, -12] },
  // Odd sizes, whose half cells fall on half pixels
  { name: "orthogonal 33x17", orientation: TileMap.Orthogonal, tile: [33, 17], size: [5, 5] },
  { name: "isometric 33x17", orientation: I, tile: [33, 17], size: [5, 3] },
  { name: "isometric 33x17 infinite", orientation: I, tile: [33, 17], infinite: true },
  { name: "staggered x odd 33x17", orientation: S, tile: [33, 17], size: [5, 5], axis: AX, index: ODD },
  { name: "staggered y even 33x17", orientation: S, tile: [33, 17], size: [5, 5], axis: AY, index: EVEN },
  { name: "hexagonal x odd 33x17 side 13", orientation: H, tile: [33, 17], size: [5, 5], axis: AX, index: ODD, side: 13 },
  { name: "hexagonal y odd 33x17 side 13", orientation: H, tile: [33, 17], size: [5, 5], axis: AY, index: ODD, side: 13 },
  { name: "hexagonal x even 33x17 side 12", orientation: H, tile: [33, 17], size: [5, 5], axis: AX, index: EVEN, side: 12 },
  { name: "hexagonal y even 33x17 side 12", orientation: H, tile: [33, 17], size: [5, 5], axis: AY, index: EVEN, side: 12 },
  { name: "oblique 33x17 skew 7 -5", orientation: O, tile: [33, 17], size: [5, 5], skew: [7, -5] },
  // Sides longer than the tile, which Tiled saves: rows spread apart
  { name: "hexagonal y odd side 40", orientation: H, tile: [32, 32], size: [5, 5], axis: AY, index: ODD, side: 40 },
  { name: "hexagonal x even side 41", orientation: H, tile: [32, 32], size: [5, 5], axis: AX, index: EVEN, side: 41 },
];
var drawnCells = [[1, 1], [2, 1], [1, 2], [2, 2]];
var orientationNames = {};
orientationNames[TileMap.Orthogonal] = "orthogonal";
orientationNames[I] = "isometric";
orientationNames[S] = "staggered";
orientationNames[H] = "hexagonal";
orientationNames[O] = "oblique";

function list(items) {
  return "[" + items.join(", ") + "]";
}

function measureGrid(g) {
  var map = sampleMap(g);
  var lines = [];
  var head = '  {"name": "' + g.name + '", "orientation": "' + orientationNames[g.orientation] + '"' +
    ', "tile": ' + list(g.tile) + (g.infinite ? ', "infinite": true' : ', "size": ' + list(g.size));
  if (g.axis !== undefined)
    head += ', "staggeraxis": "' + (g.axis === AX ? "x" : "y") + '", "staggerindex": "' + (g.index === ODD ? "odd" : "even") + '"';
  if (g.side !== undefined) head += ', "hexsidelength": ' + g.side;
  if (g.skew) head += ', "skew": ' + list(g.skew);
  lines.push(head + ",");
  var screen = [];
  for (var y = -3; y <= 3; y++) {
    for (var x = -3; x <= 3; x++) {
      var p = map.tileToScreen(x, y);
      screen.push(list([x, y, p.x, p.y]));
    }
  }
  lines.push('   "screen": [\n    ' + screen.join(",\n    ") + "\n   ]" + (g.infinite && !g.picks ? "" : ","));
  if (!g.infinite) {
    var drawn = [];
    for (var i = 0; i < drawnCells.length; i++) {
      var cell = drawnCells[i];
      var image = render(g, undefined, [[cell[0], cell[1], solidImage(g.tile[0], g.tile[1], 0xffff0000)]]);
      drawn.push(list(cell.concat(coverBottomLeft(image))));
    }
    lines.push('   "drawn": [' + drawn.join(", ") + "]" + (g.picks ? "," : ""));
  }
  if (g.picks) {
    var picks = [];
    for (var k = 0; k < g.picks.length; k++) {
      var t = map.screenToTile(g.picks[k][0], g.picks[k][1]);
      picks.push(list(g.picks[k].concat([t.x, t.y])));
    }
    lines.push('   "picks": [' + picks.join(", ") + "]");
  }
  return lines.join("\n") + "}";
}

// Points on and near the edges cell (0, 0) shares, and its vertices
sampleGrids[10].picks = [[20, 20], [31, 22.5], [24, 27], [16, 32], [32, 22], [32, 16], [16, 0], [0, 16], [33, 10]];

// The pair drawn over the other, by colour count: the tile drawn last shows
// whole. Tall tiles at (3, 4) and (3, 3); wide ones at (4, 3) and (3, 3)
function obliqueOrder(skew, renderOrder) {
  var g = { orientation: O, tile: [32, 32], size: [8, 8], skew: skew };
  var red = 0xffff0000, blue = 0xff0000ff;
  function over(size, a, b) {
    var image = render(g, renderOrder, [[a[0], a[1], solidImage(size[0], size[1], red)], [b[0], b[1], solidImage(size[0], size[1], blue)]]);
    var whole = size[0] * size[1];
    var reds = countPixels(image, red), blues = countPixels(image, blue);
    if (reds === whole && blues < whole) return list(a);
    if (blues === whole && reds < whole) return list(b);
    throw new Error("make-fixtures.js: oblique order: " + reds + " red and " + blues + " blue pixels");
  }
  return over([32, 64], [3, 4], [3, 3]) + ", " + over([64, 32], [4, 3], [3, 3]);
}
var renderOrders = [["right-down", TileMap.RightDown], ["right-up", TileMap.RightUp], ["left-down", TileMap.LeftDown], ["left-up", TileMap.LeftUp]];
var obliqueSkews = [[8, 12], [-10, 0], [0, -12]];

var measured = [];
for (var m = 0; m < sampleGrids.length; m++) measured.push(measureGrid(sampleGrids[m]));
var orders = [];
for (var s = 0; s < obliqueSkews.length; s++) {
  for (var r = 0; r < renderOrders.length; r++) {
    orders.push('  {"skew": ' + list(obliqueSkews[s]) + ', "renderorder": "' + renderOrders[r][0] + '", "over": [' +
      obliqueOrder(obliqueSkews[s], renderOrders[r][1]) + "]}");
  }
}
var measuredFile = new TextFile(dir + "measured.json", TextFile.WriteOnly);
measuredFile.write(
  '{"tiled": "' + tiled.version + '",\n "grids": [\n' + measured.join(",\n") + "\n ],\n" +
    ' "oblique_order": [\n' + orders.join(",\n") + "\n ]}\n"
);
measuredFile.commit();

// ---- shapes -----------------------------------------------------------------

// Maps of every grid, in both formats (`maps/shape-*.tmj`, `xml/maps/shape-*.tmx`):
// the grids `measure` samples, each filled with tiles shaped like its cells
// (diamonds, hexagons whose corners are clear, rectangles) in four colours,
// tile (x + 2y) mod 4 at every cell but the map's outer ring, which is left
// empty so no tile reaches past the edge of Tiled's render; infinite maps
// of isometric, staggered, hexagonal and oblique grids with tiles at
// negative cells across chunks; a hexagonal map holding a marker tile with
// each of the sixteen flag combinations (flips and 60 and 120 degree
// rotations); a hexagonal map with a layer that collides; and a world
// listing an isometric map and an infinite hexagonal one, by their tile
// origins (written as text: Tiled's scripting cannot load or save a world
// headless). `renders/shape-*.png` is `toImage()` of each bounded map but
// the colliding one: what Tiled draws. Every tile pixel is opaque or clear.
var RED = 0xffff0000, GREEN = 0xff00ff00, BLUE = 0xff0000ff, YELLOW = 0xffffff00, WHITE = 0xffffffff;
var shapeColours = [RED, GREEN, BLUE, YELLOW];

// The outline of a tile image `w` x `h` shaped like a cell of its grid
function outline(kind, w, h, side) {
  if (kind === "diamond") return [[w / 2, 0], [w, h / 2], [w / 2, h], [0, h / 2]];
  if (kind === "hex-y") {
    var s = Math.max(0, (h - side) / 2);
    return [[w / 2, 0], [w, s], [w, h - s], [w / 2, h], [0, h - s], [0, s]];
  }
  if (kind === "hex-x") {
    var t = Math.max(0, (w - side) / 2);
    return [[t, 0], [w - t, 0], [w, h / 2], [w - t, h], [t, h], [0, h / 2]];
  }
  return [[0, 0], [w, 0], [w, h], [0, h]];
}

// Whether pixel (x, y)'s centre is inside the convex outline `points`
function covers(points, x, y) {
  var px = x + 0.5, py = y + 0.5, sign = 0;
  for (var i = 0; i < points.length; i++) {
    var a = points[i], b = points[(i + 1) % points.length];
    var cross = (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0]);
    if (cross === 0) continue;
    var s = cross > 0 ? 1 : -1;
    if (sign === 0) sign = s;
    else if (s !== sign) return false;
  }
  return true;
}

// An embedded tileset of `colours.length` tiles in a row, each shaped by
// `kind`, its image saved to `images/<file>.png`; `paint(x, y, colour)` may
// colour a pixel otherwise
var savedImages = {};
function shapeTileset(file, kind, w, h, side, colours, paint) {
  var path = "images/" + file + ".png";
  if (!savedImages[path]) {
    var image = new Image(w * colours.length, h, Image.Format_ARGB32);
    image.fill(0);
    var points = outline(kind, w, h, side);
    for (var t = 0; t < colours.length; t++) {
      for (var y = 0; y < h; y++) {
        for (var x = 0; x < w; x++) {
          if (covers(points, x, y)) image.setPixel(t * w + x, y, paint ? paint(x, y, colours[t]) : colours[t]);
        }
      }
    }
    if (!image.save(dir + path)) throw new Error("make-fixtures.js: " + path + ": not saved");
    savedImages[path] = true;
  }
  var set = new Tileset(file);
  set.setTileSize(w, h);
  set.image = dir + path;
  return set;
}

function mod4(n) {
  return ((n % 4) + 4) % 4;
}

// A map of grid `g` and its tileset, its layers in CSV
function shapeMap(g) {
  var map = sampleMap(g);
  map.layerDataFormat = TileMap.CSV;
  var kind = g.kind || "rect";
  var name = "shape-" + kind + "-" + g.tile[0] + "x" + g.tile[1] + (g.side !== undefined ? "-" + g.side : "");
  var set = shapeTileset(name, kind, g.tile[0], g.tile[1], g.side || 0, shapeColours);
  map.addTileset(set);
  return [map, set];
}

// Tile (x + 2y) mod 4 at every cell of a bounded map but its outer ring
function fill(layer, set, size) {
  var edit = layer.edit();
  for (var y = 1; y < size[1] - 1; y++)
    for (var x = 1; x < size[0] - 1; x++) edit.setTile(x, y, set.tile(mod4(x + 2 * y)));
  edit.apply();
}

var shapeGrids = [
  { name: "iso", orientation: I, tile: [64, 32], size: [6, 4], kind: "diamond" },
  { name: "iso-48x32", orientation: I, tile: [48, 32], size: [6, 4], kind: "diamond" },
  { name: "stag-x-odd", orientation: S, tile: [64, 32], size: [6, 6], axis: AX, index: ODD, kind: "diamond" },
  { name: "stag-x-even", orientation: S, tile: [64, 32], size: [6, 6], axis: AX, index: EVEN, kind: "diamond" },
  { name: "stag-y-odd", orientation: S, tile: [64, 32], size: [6, 6], axis: AY, index: ODD, kind: "diamond" },
  { name: "stag-y-even", orientation: S, tile: [64, 32], size: [6, 6], axis: AY, index: EVEN, kind: "diamond" },
  { name: "hex-x-odd", orientation: H, tile: [32, 32], size: [6, 6], axis: AX, index: ODD, side: 12, kind: "hex-x" },
  { name: "hex-x-even", orientation: H, tile: [32, 32], size: [6, 6], axis: AX, index: EVEN, side: 12, kind: "hex-x" },
  { name: "hex-y-odd", orientation: H, tile: [32, 32], size: [6, 6], axis: AY, index: ODD, side: 12, kind: "hex-y" },
  { name: "hex-y-even", orientation: H, tile: [32, 32], size: [6, 6], axis: AY, index: EVEN, side: 12, kind: "hex-y" },
  // Side 0: the staggered grid of its stagger, with diamond tiles
  { name: "hex-x-side0", orientation: H, tile: [32, 32], size: [6, 6], axis: AX, index: EVEN, side: 0, kind: "hex-x" },
  { name: "hex-y-side0", orientation: H, tile: [32, 32], size: [6, 6], axis: AY, index: ODD, side: 0, kind: "hex-y" },
  // A side longer than the tile: rows spread apart, the tiles are boxes
  { name: "hex-long-side", orientation: H, tile: [32, 32], size: [6, 6], axis: AY, index: ODD, side: 40, kind: "hex-y" },
  { name: "oblique", orientation: O, tile: [32, 32], size: [6, 6], skew: [8, 12] },
  { name: "oblique-neg-x", orientation: O, tile: [32, 32], size: [6, 6], skew: [-10, 0] },
  { name: "oblique-neg-y", orientation: O, tile: [32, 32], size: [6, 6], skew: [0, -12] },
  // Odd sizes, whose half cells fall on half pixels
  { name: "odd-ortho", orientation: TileMap.Orthogonal, tile: [33, 17], size: [6, 6] },
  { name: "odd-iso", orientation: I, tile: [33, 17], size: [6, 4], kind: "diamond" },
  { name: "odd-stag-x-odd", orientation: S, tile: [33, 17], size: [6, 6], axis: AX, index: ODD, kind: "diamond" },
  { name: "odd-stag-y-even", orientation: S, tile: [33, 17], size: [6, 6], axis: AY, index: EVEN, kind: "diamond" },
  { name: "odd-hex-x-odd", orientation: H, tile: [33, 17], size: [6, 6], axis: AX, index: ODD, side: 13, kind: "hex-x" },
  { name: "odd-hex-y-odd", orientation: H, tile: [33, 17], size: [6, 6], axis: AY, index: ODD, side: 13, kind: "hex-y" },
  { name: "odd-oblique", orientation: O, tile: [33, 17], size: [6, 6], skew: [7, -5] },
];

function saveBoth(name, build) {
  save(build(), tiled.mapFormat("json"), "maps/" + name + ".tmj");
  save(build(), tiled.mapFormat("tmx"), "xml/maps/" + name + ".tmx");
}

function renderTo(name, map) {
  var path = "renders/" + name + ".png";
  if (!map.toImage().save(dir + path)) throw new Error("make-fixtures.js: " + path + ": not saved");
}

File.makePath(dir + "renders");

for (var b = 0; b < shapeGrids.length; b++) {
  (function (g) {
    var build = function () {
      var made = shapeMap(g);
      var layer = new TileLayer("ground");
      made[0].addLayer(layer);
      fill(layer, made[1], g.size);
      return made[0];
    };
    saveBoth("shape-" + g.name, build);
    renderTo("shape-" + g.name, build());
  })(shapeGrids[b]);
}

// Infinite maps: tiles at negative cells, across Tiled's chunks and the engine's
var shapeInfinite = [
  { name: "infinite-iso", orientation: I, tile: [64, 32], infinite: true, kind: "diamond" },
  { name: "infinite-stag", orientation: S, tile: [64, 32], infinite: true, axis: AY, index: ODD, kind: "diamond" },
  { name: "infinite-hex", orientation: H, tile: [32, 32], infinite: true, axis: AY, index: ODD, side: 12, kind: "hex-y" },
  { name: "infinite-oblique", orientation: O, tile: [32, 32], infinite: true, skew: [8, 12] },
];
var shapeInfiniteCells = [[-20, -3], [-1, -1], [0, 0], [5, 2], [40, 10], [-17, 18], [-33, -40]];
for (var n = 0; n < shapeInfinite.length; n++) {
  (function (g) {
    saveBoth("shape-" + g.name, function () {
      var made = shapeMap(g);
      var layer = new TileLayer("ground");
      made[0].addLayer(layer);
      var edit = layer.edit();
      for (var i = 0; i < shapeInfiniteCells.length; i++) {
        var c = shapeInfiniteCells[i];
        edit.setTile(c[0], c[1], made[1].tile(mod4(c[0] + 2 * c[1])));
      }
      edit.apply();
      return made[0];
    });
  })(shapeInfinite[n]);
}

// The hex rotations: a white hexagon with its top-left quarter red, near
// regular (56 x 64, side 32), at cell (1 + i mod 4, 1 + i / 4) with flags
// i: 1 flips it horizontally, 2 vertically, 4 rotates it 60 degrees
// clockwise and 8 rotates it 120
var rotations = { orientation: H, tile: [56, 64], size: [6, 6], axis: AY, index: ODD, side: 32 };
function hexRotations() {
  var map = sampleMap(rotations);
  map.layerDataFormat = TileMap.CSV;
  var set = shapeTileset("shape-marker-56x64-32", "hex-y", 56, 64, 32, [WHITE], function (x, y, colour) {
    return x < 28 && y < 32 ? RED : colour;
  });
  map.addTileset(set);
  var layer = new TileLayer("markers");
  map.addLayer(layer);
  var edit = layer.edit();
  for (var i = 0; i < 16; i++) {
    var flags = (i & 1 ? Tile.FlippedHorizontally : 0) | (i & 2 ? Tile.FlippedVertically : 0) |
      (i & 4 ? Tile.FlippedAntiDiagonally : 0) | (i & 8 ? Tile.RotatedHexagonal120 : 0);
    edit.setTile(1 + (i % 4), 1 + Math.floor(i / 4), set.tile(0), flags);
  }
  edit.apply();
  return map;
}
saveBoth("shape-hex-rotations", hexRotations);
renderTo("shape-hex-rotations", hexRotations());

// Tall tiles across a chunk border: plain layers of opaque tiles larger than
// their cells, from two tilesets (red and green, blue and yellow; the
// tileset by (x + y) mod 2, the colour by x mod 2), straddling the border
// the engine's chunks of 32 cells have at x 32 (y 32 on the orthogonal
// map), each overlapping its neighbours: Tiled draws them in its order
// whatever their tileset or chunk. 24 x 32 tiles in an isometric map's
// 16 x 8 cells reach past their cells' sides; 16 x 32 ones there, 16 x 24
// ones in a staggered map's and 16 x 40 ones in an orthogonal map's 16 px
// cells reach only above them
var tallMaps = [
  { name: "shape-tall-chunk-border", grid: { orientation: I, tile: [16, 8], size: [36, 4] }, tile: [24, 32], cells: [29, 34, 1, 2] },
  { name: "shape-tall-iso-upright", grid: { orientation: I, tile: [16, 8], size: [36, 4] }, tile: [16, 32], cells: [29, 34, 1, 2] },
  { name: "shape-tall-stag-upright", grid: { orientation: S, tile: [16, 8], size: [36, 10], axis: AY, index: ODD }, tile: [16, 24], cells: [29, 34, 4, 7] },
  { name: "shape-tall-ortho-upright", grid: { orientation: TileMap.Orthogonal, tile: [16, 16], size: [6, 36] }, tile: [16, 40], cells: [1, 4, 30, 34] },
];
function tallMap(t) {
  var map = sampleMap(t.grid);
  map.layerDataFormat = TileMap.CSV;
  var w = t.tile[0], h = t.tile[1];
  var sets = [
    shapeTileset("shape-tall-a-" + w + "x" + h, "rect", w, h, 0, [RED, GREEN]),
    shapeTileset("shape-tall-b-" + w + "x" + h, "rect", w, h, 0, [BLUE, YELLOW]),
  ];
  map.addTileset(sets[0]);
  map.addTileset(sets[1]);
  var layer = new TileLayer("ground");
  map.addLayer(layer);
  var edit = layer.edit();
  for (var y = t.cells[2]; y <= t.cells[3]; y++)
    for (var x = t.cells[0]; x <= t.cells[1]; x++) edit.setTile(x, y, sets[(x + y) % 2].tile(x % 2));
  edit.apply();
  return map;
}
for (var tm = 0; tm < tallMaps.length; tm++) {
  (function (t) {
    saveBoth(t.name, function () { return tallMap(t); });
    renderTo(t.name, tallMap(t));
  })(tallMaps[tm]);
}

// A hexagonal map whose "walls" layer collides, which the engine refuses,
// over a "ground" layer that says nothing of collision
saveBoth("shape-hex-collides", function () {
  var made = shapeMap({ orientation: H, tile: [32, 32], size: [4, 4], axis: AY, index: ODD, side: 12, kind: "hex-y" });
  var ground = new TileLayer("ground");
  made[0].addLayer(ground);
  fill(ground, made[1], [4, 4]);
  var walls = new TileLayer("walls");
  made[0].addLayer(walls);
  walls.setProperty("collides", true);
  var edit = walls.edit();
  edit.setTile(1, 1, made[1].tile(2));
  edit.apply();
  return made[0];
});

var shapeWorld = new TextFile(dir + "worlds/shape.world", TextFile.WriteOnly);
shapeWorld.write(
  "{\n" +
    '  "maps": [\n' +
    '    { "fileName": "../maps/shape-iso.tmj", "x": -400, "y": -200 },\n' +
    '    { "fileName": "../maps/shape-infinite-hex.tmj", "x": 1000, "y": 500 }\n' +
    "  ],\n" +
    '  "onlyShowAdjacentMaps": false,\n' +
    '  "type": "world"\n' +
    "}\n"
);
shapeWorld.commit();

// ---- objects on every grid ----------------------------------------------------

// Maps of an isometric, an oblique, a staggered and a hexagonal grid
// (`maps/shape-objects-*.tmj`, `xml/maps/shape-objects-*.tmx`), each with the
// same objects in Tiled's own object space: a point, a rectangle, an
// ellipse, a polygon, a tile object, a rotated rectangle and a rotated tile
// object; isometric ones whose tileset aligns its objects at the bottom left
// or the centre, or is a file that says nothing of it (so Tiled aligns them
// at the bottom centre). measured-objects.json holds, for each map and
// object, where Tiled puts its anchor (`pixelToScreen`) and the box of
// pixels `toImage()` draws it in (`box`: left, top, right and bottom
// edges), the object drawn alone: a rectangle, ellipse or polygon's outline
// is a pen of about 3 pixels wide round its corners, a tile object's
// picture is exact.
var objectSpecs = [
  { name: "point", id: 1, shape: "point", x: 80, y: 48 },
  { name: "rect", id: 2, shape: "rect", x: 32, y: 32, w: 32, h: 64 },
  { name: "ellipse", id: 3, shape: "ellipse", x: 96, y: 32, w: 64, h: 32 },
  { name: "polygon", id: 4, shape: "polygon", x: 32, y: 96, points: [[0, 0], [16, 0], [16, 16], [0, 8]] },
  { name: "tile", id: 5, shape: "tile", x: 64, y: 96, w: 64, h: 64 },
  { name: "turned rect", id: 6, shape: "rect", x: 96, y: 96, w: 64, h: 32, rotation: 30 },
  { name: "turned tile", id: 7, shape: "tile", x: 128, y: 128, w: 64, h: 64, rotation: 30 },
];
var objectGrids = [
  { name: "iso", orientation: I, tile: [64, 32], size: [6, 6] },
  { name: "oblique", orientation: O, tile: [32, 32], size: [8, 8], skew: [8, 12] },
  { name: "stag", orientation: S, tile: [64, 32], size: [12, 12], axis: AY, index: ODD },
  { name: "hex", orientation: H, tile: [32, 32], size: [12, 12], axis: AY, index: ODD, side: 12 },
];
var objectTile = "shape-objects-tile";

// A map of `g` with the objects `specs` names in one layer; `set` is its
// tileset, with `alignment` for tile objects when given
function objectsMap(g, specs, set) {
  var map = sampleMap(g);
  map.addTileset(set);
  var layer = new ObjectGroup("things");
  layer.drawOrder = ObjectGroup.IndexOrder;
  map.addLayer(layer);
  for (var i = 0; i < specs.length; i++) {
    var spec = specs[i];
    var o = new MapObject(spec.name);
    o.x = spec.x;
    o.y = spec.y;
    if (spec.shape === "point") o.shape = MapObject.Point;
    else if (spec.shape === "ellipse") o.shape = MapObject.Ellipse;
    else if (spec.shape === "polygon") {
      o.shape = MapObject.Polygon;
      o.polygon = spec.points.map(function (p) { return Qt.point(p[0], p[1]); });
    } else {
      o.shape = MapObject.Rectangle;
      if (spec.shape === "tile") o.tile = set.tile(0);
    }
    if (spec.w !== undefined) {
      o.width = spec.w;
      o.height = spec.h;
    }
    if (spec.rotation) o.rotation = spec.rotation;
    layer.addObject(o);
  }
  return map;
}

function objectSet(alignment) {
  var set = shapeTileset(objectTile, "rect", 64, 64, 0, [RED]);
  if (alignment !== undefined) set.objectAlignment = alignment;
  return set;
}

// The box of pixels a render covers, or null for none
function coverBox(image) {
  var left = image.width, top = image.height, right = 0, bottom = 0;
  for (var y = 0; y < image.height; y++) {
    for (var x = 0; x < image.width; x++) {
      if ((image.pixel(x, y) >>> 24) > 0) {
        left = Math.min(left, x);
        top = Math.min(top, y);
        right = Math.max(right, x + 1);
        bottom = Math.max(bottom, y + 1);
      }
    }
  }
  return right ? [left, top, right, bottom] : null;
}

var objectMeasures = [];
// Every object of `specs` alone on a map of `g` named `name`
function measureObjects(name, g, specs, alignment) {
  for (var i = 0; i < specs.length; i++) {
    var map = objectsMap(g, [specs[i]], objectSet(alignment));
    var anchor = map.pixelToScreen(specs[i].x, specs[i].y);
    var image = map.toImage();
    var box = specs[i].shape === "point" ? null : coverBox(image);
    if (box && (box[0] <= 0 || box[1] <= 0 || box[2] >= image.width || box[3] >= image.height))
      throw new Error("make-fixtures.js: " + name + ", object " + specs[i].id + " reaches the edge of its render");
    objectMeasures.push('  {"map": "' + name + '", "id": ' + specs[i].id + ', "anchor": ' + list([anchor.x, anchor.y]) +
      ', "box": ' + (box ? list(box) : "null") + "}");
  }
}

for (var q = 0; q < objectGrids.length; q++) {
  (function (g) {
    var name = "shape-objects-" + g.name;
    saveBoth(name, function () { return objectsMap(g, objectSpecs, objectSet()); });
    measureObjects(name, g, objectSpecs);
  })(objectGrids[q]);
}

// The tile object alone, aligned at the bottom left and at the centre, and
// by a tileset file that is silent
var alignedTile = [objectSpecs[4]];
var isoGrid = objectGrids[0];
var alignments = [["bottomleft", Tileset.BottomLeft], ["center", Tileset.Center]];
for (var al = 0; al < alignments.length; al++) {
  (function (word, value) {
    var name = "shape-objects-iso-" + word;
    saveBoth(name, function () { return objectsMap(isoGrid, alignedTile, objectSet(value)); });
    measureObjects(name, isoGrid, alignedTile, value);
  })(alignments[al][0], alignments[al][1]);
}
save(objectSet(), tiled.tilesetFormat("json"), "tilesets/" + objectTile + ".tsj");
save(objectSet(), tiled.tilesetFormat("tsx"), "xml/tilesets/" + objectTile + ".tsx");
save(objectsMap(isoGrid, alignedTile, external("maps", "../tilesets/" + objectTile + ".tsj")), tiled.mapFormat("json"), "maps/shape-objects-iso-external.tmj");
save(objectsMap(isoGrid, alignedTile, external("xml/maps", "../tilesets/" + objectTile + ".tsx")), tiled.mapFormat("tmx"), "xml/maps/shape-objects-iso-external.tmx");
measureObjects("shape-objects-iso-external", isoGrid, alignedTile);

var objectsFile = new TextFile(dir + "measured-objects.json", TextFile.WriteOnly);
objectsFile.write('{"tiled": "' + tiled.version + '",\n "objects": [\n' + objectMeasures.join(",\n") + "\n ]}\n");
objectsFile.commit();

// ---- paint order --------------------------------------------------------------

// Which of two overlapping tiles Tiled draws over the other inside one layer
// on isometric, staggered and hexagonal maps (`measured-order.json`): pairs
// of solid tiles larger than their cells, in the middle of a 16 x 16 map so
// neither reaches the render's edge, `over` the cell whose tile shows whole.
// An isometric map draws its diagonals (cells of equal x + y) top to bottom,
// each left to right; the other grids their rows
var paintGrids = [
  { name: "isometric 16x8", orientation: I, tile: [16, 8], size: [16, 16] },
  { name: "staggered y odd 16x8", orientation: S, tile: [16, 8], size: [16, 16], axis: AY, index: ODD },
  { name: "staggered x odd 16x8", orientation: S, tile: [16, 8], size: [16, 16], axis: AX, index: ODD },
  { name: "hexagonal y odd 16x16 side 8", orientation: H, tile: [16, 16], size: [16, 16], axis: AY, index: ODD, side: 8 },
  { name: "hexagonal x even 16x16 side 8", orientation: H, tile: [16, 16], size: [16, 16], axis: AX, index: EVEN, side: 8 },
];
// [grid, cell a, cell b, tile size]
var paintPairs = [
  [0, [8, 7], [7, 8], [32, 24]],
  [0, [9, 7], [7, 8], [32, 24]],
  [0, [8, 8], [8, 7], [32, 24]],
  [0, [8, 8], [9, 8], [32, 24]],
  [0, [7, 9], [9, 6], [64, 24]],
  [1, [7, 8], [8, 7], [32, 24]],
  [1, [7, 8], [8, 8], [32, 24]],
  [2, [7, 8], [8, 7], [32, 24]],
  [2, [8, 8], [9, 8], [32, 24]],
  [3, [7, 8], [8, 7], [32, 32]],
  [3, [7, 8], [8, 8], [32, 32]],
  [4, [7, 8], [8, 7], [32, 32]],
  [4, [8, 8], [9, 8], [32, 32]],
];
var paintOrders = [];
for (var pp = 0; pp < paintPairs.length; pp++) {
  var spec = paintPairs[pp];
  var pg = paintGrids[spec[0]], size = spec[3];
  var redArgb = 0xffff0000, blueArgb = 0xff0000ff;
  var drawn = render(pg, undefined, [
    [spec[1][0], spec[1][1], solidImage(size[0], size[1], redArgb)],
    [spec[2][0], spec[2][1], solidImage(size[0], size[1], blueArgb)],
  ]);
  var whole = size[0] * size[1];
  var reds = countPixels(drawn, redArgb), blues = countPixels(drawn, blueArgb);
  var over;
  if (reds === whole && blues < whole) over = spec[1];
  else if (blues === whole && reds < whole) over = spec[2];
  else throw new Error("make-fixtures.js: paint order: " + pg.name + ": " + reds + " red and " + blues + " blue pixels");
  paintOrders.push('  {"grid": "' + pg.name + '", "a": ' + list(spec[1]) + ', "b": ' + list(spec[2]) + ', "over": ' + list(over) + "}");
}
var orderGrids = [];
for (var og = 0; og < paintGrids.length; og++) {
  var g = paintGrids[og];
  var head = '  {"name": "' + g.name + '", "orientation": "' + orientationNames[g.orientation] + '", "tile": ' + list(g.tile);
  if (g.axis !== undefined)
    head += ', "staggeraxis": "' + (g.axis === AX ? "x" : "y") + '", "staggerindex": "' + (g.index === ODD ? "odd" : "even") + '"';
  if (g.side !== undefined) head += ', "hexsidelength": ' + g.side;
  orderGrids.push(head + "}");
}
var orderFile = new TextFile(dir + "measured-order.json", TextFile.WriteOnly);
orderFile.write(
  '{"tiled": "' + tiled.version + '",\n "grids": [\n' + orderGrids.join(",\n") + "\n ],\n" +
    ' "pairs": [\n' + paintOrders.join(",\n") + "\n ]}\n"
);
orderFile.commit();

// ---- turns about their box, and tall tiles on an odd grid ---------------------

// Hexagonal maps of turned marker tiles that are not their box's size: a
// white rectangle with its top-left quarter red, at cell (2 + 4 (i mod 4),
// 2 + 4 (i / 4)) with flags i, as in the hex rotation fixture. On even
// cells (32 x 32, side 16) a marker 48 x 40, larger than its box, and one
// of the box's size whose tileset has an offset (6, -4); on odd cells
// (33 x 33, side 15: the box Tiled steps by is 32 across) a marker the
// cell's size, a pixel wider than its box, and one of 45 x 41 with an
// offset (5, -3). Where Tiled turns each about shows in its render
var turnMaps = [
  { name: "shape-hex-turns-large", tile: [32, 32], side: 16, marker: [48, 40] },
  { name: "shape-hex-turns-offset", tile: [32, 32], side: 16, marker: [32, 32], offset: [6, -4] },
  { name: "shape-hex-turns-odd", tile: [33, 33], side: 15, marker: [33, 33] },
  { name: "shape-hex-turns-odd-offset", tile: [33, 33], side: 15, marker: [45, 41], offset: [5, -3] },
];
function turnMap(t) {
  var map = sampleMap({ orientation: H, tile: t.tile, size: [17, 17], axis: AY, index: ODD, side: t.side });
  map.layerDataFormat = TileMap.CSV;
  var w = t.marker[0], h = t.marker[1];
  var file = "shape-marker-" + w + "x" + h + (t.offset ? "-offset" : "");
  var set = shapeTileset(file, "rect", w, h, 0, [WHITE], function (x, y, colour) {
    return 2 * x < w && 2 * y < h ? RED : colour;
  });
  if (t.offset) set.tileOffset = Qt.point(t.offset[0], t.offset[1]);
  map.addTileset(set);
  var layer = new TileLayer("markers");
  map.addLayer(layer);
  var edit = layer.edit();
  for (var i = 0; i < 16; i++) {
    var flags = (i & 1 ? Tile.FlippedHorizontally : 0) | (i & 2 ? Tile.FlippedVertically : 0) |
      (i & 4 ? Tile.FlippedAntiDiagonally : 0) | (i & 8 ? Tile.RotatedHexagonal120 : 0);
    edit.setTile(2 + 4 * (i % 4), 2 + 4 * Math.floor(i / 4), set.tile(0), flags);
  }
  edit.apply();
  return map;
}
for (var tt = 0; tt < turnMaps.length; tt++) {
  (function (t) {
    saveBoth(t.name, function () { return turnMap(t); });
    renderTo(t.name, turnMap(t));
  })(turnMaps[tt]);
}

// Tall tiles on an isometric map of odd cells (33 x 17), each alone: images
// of either width parity and either height parity (33 x 40 red, 33 x 41
// green, 32 x 40 blue, 32 x 41 yellow), each at a cell whose anchor's y is
// a whole pixel and at one where it is a half, so the render shows where
// Tiled's raster puts images whose height is not the cell's
function oddTall() {
  var map = sampleMap({ orientation: I, tile: [33, 17], size: [12, 12] });
  map.layerDataFormat = TileMap.CSV;
  var sizes = [[33, 40, RED], [33, 41, GREEN], [32, 40, BLUE], [32, 41, YELLOW]];
  var cells = [[[3, 3], [6, 3]], [[9, 3], [3, 6]], [[6, 6], [9, 6]], [[3, 9], [6, 9]]];
  var layer = new TileLayer("ground");
  var sets = [];
  for (var k = 0; k < sizes.length; k++) {
    var s = sizes[k];
    sets.push(shapeTileset("shape-tall-odd-" + s[0] + "x" + s[1], "rect", s[0], s[1], 0, [s[2]]));
    map.addTileset(sets[k]);
  }
  map.addLayer(layer);
  var edit = layer.edit();
  for (var k = 0; k < sizes.length; k++)
    for (var c = 0; c < 2; c++) edit.setTile(cells[k][c][0], cells[k][c][1], sets[k].tile(0));
  edit.apply();
  return map;
}
saveBoth("shape-tall-odd-iso", oddTall);
renderTo("shape-tall-odd-iso", oddTall());

// ---- the raster of isometric maps of odd cells --------------------------------

// Where toImage() draws a tile on isometric maps of odd cells
// (`measured-raster.json`): one solid tile alone on a 12 x 12 map, at a
// cell whose anchor's x is whole and one where it is a half (y the other),
// for tilesets whose tile size is below, at and past the cell's width
// across or down: an atlas (its image a file) and an image collection
// read back from a saved map (Tiled writes its largest image as its tile
// size). Each row: [cell width, cell height, kind, tileset width, tileset
// height, image width, image height, x, y, anchor x, anchor y, left,
// bottom], the anchor `tileToScreen`'s top vertex less half a cell across
// and plus a cell down, left and bottom the render's opaque pixels'
var rasterScratch = dir + "maps/make-fixtures-scratch";
function rasterRow(cell, kind, image, at) {
  var map = new TileMap();
  map.setTileSize(cell[0], cell[1]);
  map.setSize(12, 12);
  map.orientation = I;
  var set = new Tileset("raster");
  var tile;
  if (kind === "atlas") {
    solidImage(image[0], image[1], RED).save(rasterScratch + ".png");
    set.setTileSize(image[0], image[1]);
    set.image = rasterScratch + ".png";
    tile = set.tile(0);
  } else {
    tile = set.addTile();
    tile.setImage(solidImage(image[0], image[1], RED));
  }
  map.addTileset(set);
  var layer = new TileLayer("tiles");
  map.addLayer(layer);
  var edit = layer.edit();
  edit.setTile(at[0], at[1], tile);
  edit.apply();
  if (kind === "collection") {
    tiled.mapFormat("tmx").write(map, rasterScratch + ".tmx");
    map = tiled.mapFormat("tmx").read(rasterScratch + ".tmx");
  }
  var cover = coverBox(map.toImage());
  var top = map.tileToScreen(at[0], at[1]);
  var tileset = map.tilesets[0];
  return list([cell[0], cell[1], '"' + kind + '"', tileset.tileWidth, tileset.tileHeight, image[0], image[1],
    at[0], at[1], top.x - cell[0] / 2, top.y + cell[1], cover[0], cover[3]]);
}
var rasterCells = [[33, 17], [33, 15], [31, 17], [21, 11], [33, 16], [32, 17]];
var rasterRows = [];
for (var rc = 0; rc < rasterCells.length; rc++) {
  var cell = rasterCells[rc], tw = cell[0], th = cell[1];
  var atlasImages = [[tw - 1, 8], [tw, 8], [tw + 1, 8], [8, tw], [8, tw + 1], [tw + 3, 2 * th + 5]];
  var collectionImages = [[tw, 8], [tw + 1, 8], [8, tw + 1]];
  var ats = [[6, 5], [7, 5]];
  for (var ai = 0; ai < atlasImages.length; ai++)
    for (var at = 0; at < ats.length; at++) rasterRows.push(rasterRow(cell, "atlas", atlasImages[ai], ats[at]));
  for (var ci = 0; ci < collectionImages.length; ci++)
    for (var at2 = 0; at2 < ats.length; at2++) rasterRows.push(rasterRow(cell, "collection", collectionImages[ci], ats[at2]));
}
File.remove(rasterScratch + ".png");
File.remove(rasterScratch + ".tmx");
var rasterFile = new TextFile(dir + "measured-raster.json", TextFile.WriteOnly);
rasterFile.write('{"tiled": "' + tiled.version + '",\n "rows": [\n  ' + rasterRows.join(",\n  ") + "\n ]}\n");
rasterFile.commit();
