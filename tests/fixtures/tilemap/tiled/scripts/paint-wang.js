// Paints grass into the dirt maps through Tiled's own Wang brush and saves
// what the Wang tests compare against.
//
// Regenerate (Tiled 1.12.2, from the repository root):
//   /Applications/Tiled.app/Contents/MacOS/Tiled -e tests/fixtures/tilemap/tiled/scripts/paint-wang.js
//
// Reads maps/wang-dirt.tmj and xml/maps/wang-dirt.tmx (5x5 of all-dirt tiles
// of the `ground` corner set) and writes maps/wang-grass.tmj and
// xml/maps/wang-grass.tmx. The brush paints the four corners of cell (2, 2)
// with grass, as one click of Tiled's terrain brush does, and Tiled's own
// filler picks the tiles of the 3x3 around it.
var dir = "tests/fixtures/tilemap/tiled/";
var jobs = [
  [dir + "maps/wang-dirt.tmj", dir + "maps/wang-grass.tmj"],
  [dir + "xml/maps/wang-dirt.tmx", dir + "xml/maps/wang-grass.tmx"],
];
for (var i = 0; i < jobs.length; i++) {
  // -e runs without an editor, so the maps are read, not opened
  var map = tiled.mapFormatForFile(jobs[i][0]).read(jobs[i][0]);
  var layer = map.layerAt(0);
  var set = null;
  for (var t = 0; t < map.tilesets.length; t++) {
    for (var w = 0; w < map.tilesets[t].wangSets.length; w++) {
      if (map.tilesets[t].wangSets[w].name == "ground") set = map.tilesets[t].wangSets[w];
    }
  }
  var edit = layer.wangEdit(set);
  var x = 2, y = 2;
  edit.setCorner(x, y, 1);
  edit.setCorner(x + 1, y, 1);
  edit.setCorner(x, y + 1, 1);
  edit.setCorner(x + 1, y + 1, 1);
  edit.apply();
  var error = tiled.mapFormatForFile(jobs[i][1]).write(map, jobs[i][1]);
  if (error) tiled.log("error: " + error);
}
