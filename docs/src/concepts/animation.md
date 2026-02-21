# Animation System

Ryo2D provides a flexible animation system for sprite-based animations. The system is designed with three different APIs to match your use case, from simple looping animations to complex state machines.

## Three APIs, Three Use Cases

### 1. Animation - Define Frame Sequences

The `Animation` struct defines a sequence of sprite frames with timing information:

```rust
use ryo2d::prelude::*;

// Load sprite frames
let frame1 = SpriteHandle::from_sprite(
    Sprite::load("coin-1.png").unwrap(), 
    "coin-1.png"
);
let frame2 = SpriteHandle::from_sprite(
    Sprite::load("coin-2.png").unwrap(), 
    "coin-2.png"
);
let frame3 = SpriteHandle::from_sprite(
    Sprite::load("coin-3.png").unwrap(), 
    "coin-3.png"
);

// Create animation with uniform frame timing
let spin = Animation::new(
    "spin",                           // Name for debugging
    vec![frame1, frame2, frame3],     // Frames
    0.1                                // Duration per frame (seconds)
);
```

**Loop Modes:**

```rust
// Play once and stop
let explosion = Animation::new("boom", frames, 0.05)
    .with_loop_mode(LoopMode::Once);

// Loop forever (default)
let idle = Animation::new("idle", frames, 0.15);

// Play forward then backward, repeat
let breathing = Animation::new("breathe", frames, 0.2)
    .with_loop_mode(LoopMode::PingPong);

// Loop a specific number of times
let triple_spin = Animation::new("spin3", frames, 0.1)
    .with_loop_mode(LoopMode::Times(3));
```

**Per-Frame Timing:**

For animations where some frames linger longer:

```rust
let blink = Animation::with_frame_durations(
    "blink",
    vec![open_eyes, closed_eyes, open_eyes],
    vec![2.0, 0.1, 0.5]  // Hold open, quick close, reopen
);
```

### 2. Animator - Simple Component for Entities

The `Animator` component is perfect for simple, always-playing animations like spinning coins, flickering flames, or idle effects:

```rust
// Spawn a spinning coin
ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Renderable::sprite(frame1.clone()),  // Initial sprite
    Animator::new(spin_animation),        // Animation component
));

// The engine automatically:
// - Updates frames each tick
// - Syncs current sprite to Renderable
// - Handles loop logic
```

**Playback Control:**

```rust
// Get mutable access to animator
if let Some(mut animator) = ctx.world.get_mut::<Animator>(entity) {
    // Pause/resume
    animator.pause();
    animator.play();
    
    // Stop and reset to first frame
    animator.stop();
    
    // Change playback speed (1.0 = normal, 2.0 = double speed)
    animator.set_speed(1.5);
    
    // Check if one-shot animation just finished
    if animator.just_finished() {
        println!("Animation complete!");
    }
}
```

### 3. AnimationController - State Machine

For characters with multiple animation states that need to transition between each other:

```rust
// Create controller
let mut controller = AnimationController::new();

// Add all animation states
controller.add_animation("idle", idle_anim);
controller.add_animation("walk", walk_anim);
controller.add_animation("run", run_anim);
controller.add_animation("jump", jump_anim.with_loop_mode(LoopMode::Once));
controller.add_animation("attack", attack_anim.with_loop_mode(LoopMode::Once));

// Set initial state
controller.set_state("idle");

// Spawn entity with controller
ctx.world.spawn((
    Transform::new(x, y),
    Renderable::sprite(initial_sprite),
    controller,
    Player::new(),
));
```

**State Transitions:**

```rust
// In your game logic - get mutable access to controller
if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(player_entity) {
    // Simple state change
    if velocity.dx != 0.0 {
        controller.set_state("walk");
    } else {
        controller.set_state("idle");
    }
    
    // Transition after current animation finishes
    if attack_button_pressed {
        controller.set_state("attack");
        // Return to idle when attack animation completes
        controller.transition_after_finish("idle");
    }
    
    // Check current state
    if controller.current_state() == Some("jump") {
        println!("Player is jumping!");
    }
    
    // Check if animation just finished
    if controller.just_finished() {
        println!("Animation cycle complete!");
    }
}
```

## Complete Example: Animated Character

Here's a full example showing how to create a player with animated states:

```rust
use ryo2d::prelude::*;

fn spawn_player(ctx: &mut SceneContext) -> Entity {
    // Load sprites
    let idle_frames = vec![
        load_sprite_handle("player-idle-1.png"),
        load_sprite_handle("player-idle-2.png"),
    ];
    
    let walk_frames = vec![
        load_sprite_handle("player-walk-1.png"),
        load_sprite_handle("player-walk-2.png"),
        load_sprite_handle("player-walk-3.png"),
        load_sprite_handle("player-walk-4.png"),
    ];
    
    // Create animations
    let idle = Animation::new("idle", idle_frames, 0.5);
    let walk = Animation::new("walk", walk_frames, 0.15);
    
    // Build controller
    let mut controller = AnimationController::new();
    controller.add_animation("idle", idle);
    controller.add_animation("walk", walk);
    controller.set_state("idle");
    
    let initial_sprite = controller.current_sprite()
        .expect("Controller should have sprite")
        .clone();
    
    // Spawn entity
    ctx.world.spawn((
        Transform::new(100.0, 100.0),
        Velocity::zero(),
        Renderable::sprite(initial_sprite),
        controller,
    ))
}

fn update_player_animation(ctx: &mut SceneContext, player: Entity) {
    // Get velocity to determine state
    let velocity = ctx.world.get::<Velocity>(player)
        .map(|v| v.dx.abs())
        .unwrap_or(0.0);
    
    // Update animation state based on movement
    if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(player) {
        if velocity > 0.1 {
            controller.set_state("walk");
        } else {
            controller.set_state("idle");
        }
    }
}

// Helper function
fn load_sprite_handle(path: &str) -> SpriteHandle {
    let sprite = Sprite::load(path).expect("Failed to load sprite");
    SpriteHandle::from_sprite(sprite, path)
}
```

## Real-World Example: Flappy Bird

The Flappy Bird example game uses `AnimationController` to manage the bird's animations:

```rust
// Create controller with flapping and hurt animations
let mut controller = AnimationController::new();
controller.add_animation("flap", flap_animation);
controller.add_animation("hurt", hurt_animation);
controller.set_state("flap");

// Spawn bird with controller
ctx.world.spawn((
    Transform::new(x, y),
    Renderable::sprite(initial_sprite),
    controller,
    Bird::new(),
));

// Switch to hurt animation on death
if bird.is_dead {
    if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(bird_entity) {
        controller.set_state("hurt");
    }
}
```

## When to Use Which API

| Use Case | API | Example |
|----------|-----|---------|
| Simple looping effect | `Animator` | Spinning coin, flickering torch |
| Character states | `AnimationController` | Player idle/walk/run/jump |
| One-shot effects | `Animator` with `LoopMode::Once` | Explosion, hit flash |
| Complex transitions | `AnimationController` with `transition_after_finish` | Attack → idle, jump → fall |

## Engine Integration

The animation system is fully integrated into the engine:

1. **Automatic Updates** - The engine updates `Animator` and `AnimationController` components each frame
2. **Sprite Syncing** - Current animation frames are automatically synced to the entity's `Renderable`
3. **No Manual Work** - You never need to manually set sprites or update frame counters
4. **Game Logic Focus** - Just set animation states in response to game events

## Performance Notes

- Animations use `SpriteHandle` for efficient sprite sharing
- Frame updates happen once per frame, not per entity
- State changes are cheap - only allocate when adding new animations
- Multiple entities can share the same `Animation` definition

## See Also

- [Sprites & Rendering](./sprites.md) - How sprites work in Ryo2D
- [Asset Management](./assets.md) - Loading and caching sprite assets
- [ECS Architecture](./ecs.md) - Understanding components and entities
