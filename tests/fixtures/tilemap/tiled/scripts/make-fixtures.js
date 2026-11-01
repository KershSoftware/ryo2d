// Writes the fixtures for tile objects taking their tile's class, image
// layers (one with no image, one with a transparent colour), the map's
// parallax origin and infinite maps (chunks at negative cells, of Tiled's
// 16 x 16 and of 8 x 4, in every layer format), through Tiled's own
// scripting API, in both formats.
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
