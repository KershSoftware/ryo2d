// Writes the fixtures for tile objects taking their tile's class, image
// layers (one with no image, one with a transparent colour) and the map's
// parallax origin, through Tiled's own scripting API, in both formats.
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
