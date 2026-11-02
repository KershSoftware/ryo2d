// Writes the fixtures for tile objects taking their tile's class, image
// layers (one with no image, one with a transparent colour), the map's
// parallax origin and infinite maps (chunks at negative cells, of Tiled's
// 16 x 16, of 8 x 4 and of 24 x 24, which straddle the engine's chunks of
// 32, in every layer format, and a tile layer in a group), through Tiled's
// own scripting API, in both formats; and measured.json, where Tiled puts
// the cells of isometric, staggered, hexagonal and oblique grids.
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
