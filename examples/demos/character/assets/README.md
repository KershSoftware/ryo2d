# Character demo assets

The platformer page's level is `ldtk/platformer.ldtk`, an LDtk 1.5.3 project of one level, `Platformer`: an IntGrid layer `Collisions` (`solid`, `one_way`, `slope_bottom_right`, `slope_bottom_left`) whose auto-layer rules draw `images/dungeon-terrain.png` over it, and an `Entities` layer.

Nothing here is drawn or placed by hand but this README. The tilemap demo's `make-maps` writes `ldtk/platformer.ldtk` and copies `images/dungeon-terrain.png` and `kenney-tiny-dungeon/License.txt` from its own assets, so the level has one source (`examples/demos/tilemap/src/bin/make-maps/ldtk.rs`):

```sh
cargo run -p tilemap-demo --bin make-maps
```

The art is Tiny Dungeon by Kenney, CC0 (`kenney-tiny-dungeon/License.txt`): `images/dungeon-terrain.png` is made from its `Tilemap/tilemap_packed.png` by the tilemap demo's `make-maps`, which copies it here.
