# Future Tilemap Features

Features identified during the tilemap system design (2026-02-21) that are out of scope for the initial implementation. Refer to these when extending the tilemap system later.

## Infinite Maps (Tiled Chunked Format)

Tiled supports "infinite" maps where tile data is stored in sparse chunks rather than a fixed-size grid. The map has no predefined width/height — chunks are allocated on demand as the user paints tiles.

**What it would require:**
- Parse Tiled's chunked layer format (`"infinite": true` in .tmj)
- Replace the flat `Vec<Option<TileId>>` in `TilemapLayer` with a `HashMap<(i32, i32), Vec<Option<TileId>>>` keyed by chunk coordinates
- Tile lookups become two-step: find chunk, then index within chunk
- Collision generation needs to handle sparse regions

**When you'd want this:** Very large or procedurally-expanded maps where pre-allocating the full grid is wasteful.

## Image Layers

Tiled image layers embed a single image (not tiles) positioned within the map. Used for static backgrounds, foreground overlays, etc.

**Why it's low priority:** Ryo2D's `ParallaxBackground` system already handles layered background images with scroll factors, and it's GPU-accelerated. Image layers in Tiled are functionally equivalent but less capable (no scroll speed control).

**If needed:** Parse `"type": "imagelayer"` from .tmj, load the image as a `SpriteHandle`, spawn as a `Renderable` entity at the layer's offset with the layer's z-order.

## Wang Tiles

Tiled's Wang tile system (terrain sets) uses edge/corner matching rules to auto-select tiles. It's Tiled's built-in auto-tiling mechanism.

**Relationship to our auto-tiling:** Our bitmask auto-tiling system (part of the tilemap design) handles the same use case with a different approach. Wang tiles could be supported as an alternative rule source — import Tiled's Wang set definitions and convert them to our bitmask rules.

**What it would require:**
- Parse `"wangsets"` from .tsj tileset files
- Convert Wang edge/corner constraints to bitmask lookup entries
- Handle both edge-type and corner-type Wang sets

## 2D Engine Feature Gaps (Non-Tilemap)

Other features identified from researching Godot, Unity, GameMaker, Defold, Bevy, Love2D, Phaser, and Pygame that Ryo2D doesn't have yet:

### Skeletal/Bone Animation
Bone-based 2D rigging with IK (inverse kinematics). Godot has Skeleton2D, Unity has 2D Animation package, GameMaker supports Spine import. Would enable smoother character animation without per-frame spritesheets.

### Localization / i18n
Translation system with string tables, plural rules, and locale switching. Godot has built-in CSV/PO support, Unity has a localization package. Essential for shipping games in multiple languages.

### Accessibility
Screen reader support, focus navigation, text-to-speech. Godot 4.5+ leads here with AccessKit integration. Increasingly expected for commercial releases.

### Sprite Atlas Packing
Automatic texture atlas generation at build time — pack individual sprites into atlases to reduce draw calls. Godot, Unity, and Defold all do this automatically.

### Normal Map Lighting
Apply normal maps to 2D sprites so they react to the lighting system with depth/surface detail. Godot and Unity both support this on their 2D light systems.

### 2D Joints
Expose Rapier2D's joint system (hinge, spring, distance, fixed, prismatic). We integrate Rapier for rigid bodies and colliders but don't expose joints yet. Godot, Unity, and GameMaker all have joint APIs.

### Additional Collision Shapes
Capsule, polygon, edge/segment, and composite colliders. We currently support AABB and circle. Rapier already supports these shapes — just needs API exposure.

### Path-Based Tweening
Tween objects along Bezier/spline paths (not just linear interpolation). Phaser has tween-on-path, Godot has Path2D + PathFollow2D.

### Texture Compression
Platform-specific texture compression at build time (DXT, ETC2, ASTC). Reduces GPU memory usage significantly. Defold, Unity, and Godot handle this in their export pipelines.
