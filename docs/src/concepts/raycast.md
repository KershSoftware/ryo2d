# Raycasting

Ryo2D includes a GPU-accelerated raycasting system for pseudo-3D rendering in the style of Wolfenstein 3D and the Build engine. The renderer uses two WGSL shader passes — a DDA wall raycaster and an instanced billboard sprite renderer — composed into the engine's existing post-processing and UI pipeline.

## Basic Setup

Enable raycasting in your scene:

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Create a world map
    let mut world = RaycastWorld::new(32, 32);

    // Add border walls
    for x in 0..32 {
        *world.get_mut(x, 0).unwrap() = MapCell::wall(1);
        *world.get_mut(x, 31).unwrap() = MapCell::wall(1);
    }
    for y in 0..32 {
        *world.get_mut(0, y).unwrap() = MapCell::wall(1);
        *world.get_mut(31, y).unwrap() = MapCell::wall(1);
    }

    // Configure the renderer
    let rc = ctx.raycast_renderer().unwrap();
    rc.set_world(world);
    rc.set_map_info(MapInfoUniform {
        dimensions: [32, 32],
        max_distance: 32.0,
        fog_density: 0.04,
        fog_color: [0.05, 0.05, 0.1],
        diminish_factor: 0.06,            // camera-distance light falloff
        ambient_color: [0.6, 0.65, 0.8],  // global ambient tint
        ambient_intensity: 0.04,           // global ambient brightness
    });

    // Upload textures (index 0 = unused, 1+ = actual textures)
    rc.queue_wall_textures(vec![solid_black, brick_texture], 64, 64);
    rc.queue_floor_textures(vec![dark_grey, checker_texture], 64, 64);
    rc.queue_sprite_textures(vec![barrel_texture], 64, 64);

    // Enable the raycasting render path
    ctx.enable_raycast();

    // Spawn camera entity (required)
    ctx.world.spawn((
        Transform::new(4.5, 4.5),
        RaycastCamera::new(66.0), // FOV in degrees
    ));
}
```

## World Map

The world is a 2D grid of `MapCell` values. Each cell can be a wall, floor, or special type like a door or thin wall.

### MapCell

```rust
// Solid wall with texture index 1
MapCell::wall(1)

// Empty cell (floor/ceiling only)
MapCell::default()

// Custom cell with floor height variation
let mut cell = MapCell::default();
cell.floor_texture = 1;
cell.ceiling_texture = 2;
cell.floor_height = 0.2;  // raised platform
cell.light_level = 1.0;   // full brightness (default)
```

### MapFlags

Cells can have special flags:

```rust
// Door cell (blocks when closed, opens with interaction)
cell.flags = MapFlags::DOOR;

// Thin wall (can be seen through from the side)
cell.flags = MapFlags::THIN_WALL;
```

## Camera

The `RaycastCamera` component controls the player's view. Attach it to an entity with a `Transform`:

```rust
let camera = ctx.world.spawn((
    Transform::new(4.5, 4.5),
    RaycastCamera::new(66.0), // FOV in degrees
));

// Update camera direction each frame
if let Some(mut cam) = ctx.world.get_mut::<RaycastCamera>(camera) {
    cam.set_angle(player_angle);
}
```

The engine automatically syncs the camera entity's `Transform` position and `RaycastCamera` direction to GPU uniforms each frame.

## Sprites

Billboard sprites are rendered as camera-facing quads in world space. They are depth-tested against walls and sorted back-to-front before GPU upload.

```rust
// Basic sprite (texture index 0)
ctx.world.spawn((
    Transform::new(10.0, 5.0),
    RaycastSprite::new(0),
));

// Scaled sprite with vertical offset
let mut sprite = RaycastSprite::new(0);
sprite.scale = (0.5, 0.5);    // half size
sprite.v_offset = -0.25;       // shift down (bottom sits on floor)
ctx.world.spawn((Transform::new(10.0, 5.0), sprite));
```

## Lights

The raycasting renderer uses a six-layer lighting pipeline applied per-pixel: bilinear lightmap ambient, global ambient, dynamic point lights with shadow rays, camera-distance diminishing, and distance fog. Sprites share the same light buffer for environmental consistency.

### Dynamic Point Lights

Point lights affect walls, floors, ceilings, and sprites. Each light has a world-space height and configurable falloff curve:

```rust
// Basic light (defaults: height=0.5, falloff=2.0 quadratic)
ctx.world.spawn((
    Transform::new(23.0, 23.0),
    RaycastLight::new(5.0, (1.0, 0.78, 0.39), 1.5),
    //                radius  RGB colour       intensity
));

// Light with custom height and falloff
let mut light = RaycastLight::new(4.0, (1.0, 0.9, 0.7), 1.0);
light.height = 0.8;   // near ceiling (0.0=floor, 0.5=mid, 1.0=ceiling)
light.falloff = 3.0;   // cubic falloff (tighter pool than quadratic)
ctx.world.spawn((Transform::new(18.5, 24.0), light));
```

Falloff curves: `1.0` = linear, `2.0` = quadratic (default, soft spread), `3.0` = cubic (tight pool).

### Lightmap Ambient

Each cell has a `light_level` field (0.0 to 1.0) that controls ambient brightness. These values are uploaded as an R8Unorm texture and bilinear-sampled, producing smooth transitions across tile boundaries instead of hard edges:

```rust
// Dark room
let cell = world.get_mut(x, y).unwrap();
cell.light_level = 0.15;

// Transition zone (gradual ramp)
cell.light_level = 0.7;
```

### Global Ambient and Diminishing

`MapInfoUniform` controls global ambient light (prevents pure black) and camera-distance diminishing (Doom-style depth darkening):

```rust
rc.set_map_info(MapInfoUniform {
    // ...
    diminish_factor: 0.06,            // 0.0=none, higher=more depth darkening
    ambient_color: [0.6, 0.65, 0.8],  // blue tint for cave/outdoor feel
    ambient_intensity: 0.04,           // prevents pure black without flattening contrast
});
```

## Doors

Doors are map cells with the `DOOR` flag and a corresponding `RaycastDoor` ECS entity that manages the open/close state machine:

```rust
// Mark cell as door in the map
let cell = world.get_mut(15, 12).unwrap();
cell.wall_texture = 2;
cell.flags = MapFlags::DOOR;

// Spawn door entity
let door = ctx.world.spawn((
    RaycastDoor::new(15, 12, DoorSpeed::Normal),
));

// Activate door (e.g., on player interaction)
if let Some(mut door) = ctx.world.get_mut::<RaycastDoor>(door_entity) {
    door.activate(); // toggles Closed→Opening or Open→Closing
}
```

Door state machines cycle through: `Closed → Opening → Open → Closing → Closed`. Use `door_system()` each frame to tick animations and sync door state to the world map.

## Textures

All textures are uploaded as `texture_2d_array` resources, indexed by texture ID in the shader. Textures are provided as raw RGBA byte vectors:

```rust
let rc = ctx.raycast_renderer().unwrap();

// Wall textures (index 0 = unused/air, 1+ = visible walls)
rc.queue_wall_textures(vec![black_rgba, brick_rgba, stone_rgba], 64, 64);

// Floor/ceiling textures
rc.queue_floor_textures(vec![dark_rgba, checker_rgba], 64, 64);

// Sprite textures
rc.queue_sprite_textures(vec![barrel_rgba, torch_rgba], 64, 64);
```

## Scene Cleanup

The `SceneCleanup::RAYCAST` flag automatically disables the renderer and drops the world map on scene transitions. This is included in the default cleanup set.

## Fog

Distance fog is controlled by `MapInfoUniform`:

```rust
rc.set_map_info(MapInfoUniform {
    dimensions: [32, 32],
    max_distance: 32.0,              // max ray distance
    fog_density: 0.04,               // higher = more fog
    fog_color: [0.05, 0.05, 0.1],   // dark blue fog
    diminish_factor: 0.06,
    ambient_color: [0.6, 0.65, 0.8],
    ambient_intensity: 0.04,
});
```
