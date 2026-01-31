# Sprite Animations

This guide shows practical examples of using Ryo2D's animation system in real games.

## Example 1: Spinning Collectible Coin

A simple looping animation for a collectible item:

```rust
use ryo2d::prelude::*;

fn spawn_coin(ctx: &mut SceneContext, x: f32, y: f32) -> Entity {
    // Load coin animation frames
    let frames = vec![
        load_sprite_handle("coin-1.png"),
        load_sprite_handle("coin-2.png"),
        load_sprite_handle("coin-3.png"),
        load_sprite_handle("coin-4.png"),
    ];
    
    // Create spinning animation (150ms per frame)
    let spin_animation = Animation::new("spin", frames, 0.15);
    
    // Get initial sprite
    let initial_sprite = spin_animation.frame(0)
        .expect("Animation should have frames")
        .clone();
    
    // Spawn coin entity with Animator
    ctx.world.spawn((
        Transform::new(x, y),
        Renderable::sprite(initial_sprite),
        Animator::new(spin_animation),
        Coin,  // Custom marker component
    ))
}

// Helper to load sprites as handles
fn load_sprite_handle(path: &str) -> SpriteHandle {
    let sprite = Sprite::load(path).expect("Failed to load sprite");
    SpriteHandle::from_sprite(sprite, path)
}

#[derive(Debug, Clone, Copy)]
struct Coin;
```

The engine automatically updates the coin's sprite each frame - no game logic needed!

## Example 2: Player with Directional Movement

A character that plays different animations based on movement:

```rust
use ryo2d::prelude::*;

struct Player {
    speed: f32,
}

fn spawn_player(ctx: &mut SceneContext) -> Entity {
    // Load animation frames
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
    
    let run_frames = vec![
        load_sprite_handle("player-run-1.png"),
        load_sprite_handle("player-run-2.png"),
        load_sprite_handle("player-run-3.png"),
    ];
    
    // Create animations
    let idle = Animation::new("idle", idle_frames, 0.5);
    let walk = Animation::new("walk", walk_frames, 0.15);
    let run = Animation::new("run", run_frames, 0.1);
    
    // Build controller
    let mut controller = AnimationController::new();
    controller.add_animation("idle", idle);
    controller.add_animation("walk", walk);
    controller.add_animation("run", run);
    controller.set_state("idle");
    
    let initial_sprite = controller.current_sprite()
        .expect("Controller should have sprite")
        .clone();
    
    ctx.world.spawn((
        Transform::new(100.0, 100.0),
        Velocity::zero(),
        Renderable::sprite(initial_sprite),
        controller,
        Player { speed: 150.0 },
    ))
}

// Update function - call this in your Scene's update()
fn update_player(ctx: &mut SceneContext, player: Entity, dt: f32) {
    let mut velocity = Vec2::ZERO;
    
    // Handle input
    if ctx.input.key(KeyCode::KeyW) {
        velocity.y -= 1.0;
    }
    if ctx.input.key(KeyCode::KeyS) {
        velocity.y += 1.0;
    }
    if ctx.input.key(KeyCode::KeyA) {
        velocity.x -= 1.0;
    }
    if ctx.input.key(KeyCode::KeyD) {
        velocity.x += 1.0;
    }
    
    // Normalize diagonal movement
    if velocity.length() > 0.0 {
        velocity = velocity.normalize();
    }
    
    let speed_mult = if ctx.input.key(KeyCode::ShiftLeft) {
        2.0  // Running
    } else {
        1.0  // Walking
    };
    
    // Get player speed
    let speed = ctx.world.get::<Player>(player)
        .map(|p| p.speed)
        .unwrap_or(150.0);
    
    // Update velocity
    if let Some(mut vel) = ctx.world.get_mut::<Velocity>(player) {
        vel.dx = velocity.x * speed * speed_mult;
        vel.dy = velocity.y * speed * speed_mult;
    }
    
    // Update animation based on movement
    let velocity_magnitude = velocity.length();
    
    if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(player) {
        if velocity_magnitude > 0.0 {
            if speed_mult > 1.0 {
                controller.set_state("run");
            } else {
                controller.set_state("walk");
            }
        } else {
            controller.set_state("idle");
        }
    }
}
```

## Example 3: One-Shot Animation (Explosion)

An animation that plays once and triggers an event:

```rust
use ryo2d::prelude::*;

fn spawn_explosion(ctx: &mut SceneContext, x: f32, y: f32) -> Entity {
    // Load explosion frames
    let frames = vec![
        load_sprite_handle("explosion-1.png"),
        load_sprite_handle("explosion-2.png"),
        load_sprite_handle("explosion-3.png"),
        load_sprite_handle("explosion-4.png"),
        load_sprite_handle("explosion-5.png"),
    ];
    
    // Create one-shot animation (50ms per frame = 250ms total)
    let explosion = Animation::new("boom", frames, 0.05)
        .with_loop_mode(LoopMode::Once);
    
    let initial_sprite = explosion.frame(0)
        .expect("Animation should have frames")
        .clone();
    
    ctx.world.spawn((
        Transform::new(x, y),
        Renderable::sprite(initial_sprite).with_z_order(100),
        Animator::new(explosion),
        Explosion { lifetime: 0.25 },  // Cleanup after animation
    ))
}

#[derive(Debug, Clone, Copy)]
struct Explosion {
    lifetime: f32,
}

// System to cleanup finished explosions
fn update_explosions(ctx: &mut SceneContext, dt: f32) {
    // Collect entities to despawn
    let mut to_remove = Vec::new();
    
    let mut query = ctx.world.query::<(Entity, &Animator, &mut Explosion)>();
    for (entity, animator, mut explosion) in query.iter() {
        explosion.lifetime -= dt;
        
        // Remove when animation finishes OR lifetime expires
        if animator.just_finished() || explosion.lifetime <= 0.0 {
            to_remove.push(entity);
        }
    }
    
    // Despawn all finished explosions
    for entity in to_remove {
        ctx.world.despawn(entity);
    }
}
```

## Example 4: Character with Attack Animation

Shows transition-after-finish for returning to idle state:

```rust
use ryo2d::prelude::*;

struct Player {
    can_attack: bool,
}

fn spawn_player_with_attack(ctx: &mut SceneContext) -> Entity {
    // Load frames
    let idle_frames = load_animation_frames("idle", 2);
    let attack_frames = load_animation_frames("attack", 5);
    
    // Create animations
    let idle = Animation::new("idle", idle_frames, 0.2);
    // Attack plays once
    let attack = Animation::new("attack", attack_frames, 0.08)
        .with_loop_mode(LoopMode::Once);
    
    // Build controller
    let mut controller = AnimationController::new();
    controller.add_animation("idle", idle);
    controller.add_animation("attack", attack);
    controller.set_state("idle");
    
    let initial_sprite = controller.current_sprite().unwrap().clone();
    
    ctx.world.spawn((
        Transform::new(100.0, 100.0),
        Renderable::sprite(initial_sprite),
        controller,
        Player { can_attack: true },
    ))
}

fn handle_player_attack(ctx: &mut SceneContext, player: Entity) {
    // Check if attack button pressed
    if !ctx.input.key_pressed(KeyCode::Space) {
        return;
    }
    
    // Get controller and player state
    let can_attack = ctx.world.get::<Player>(player)
        .map(|p| p.can_attack)
        .unwrap_or(false);
    
    if !can_attack {
        return;
    }
    
    // Trigger attack animation
    if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(player) {
        // Only attack if not already attacking
        if controller.current_state() != Some("attack") {
            controller.set_state("attack");
            // Automatically return to idle when attack finishes
            controller.transition_after_finish("idle");
        }
    }
}

// Helper to load multiple frames with pattern
fn load_animation_frames(name: &str, count: usize) -> Vec<SpriteHandle> {
    (1..=count)
        .map(|i| {
            let path = format!("{}-{}.png", name, i);
            load_sprite_handle(&path)
        })
        .collect()
}

fn load_sprite_handle(path: &str) -> SpriteHandle {
    let sprite = Sprite::load(path).expect("Failed to load sprite");
    SpriteHandle::from_sprite(sprite, path)
}
```

## Example 5: Ping-Pong Animation (Breathing Effect)

An animation that plays forward then backward continuously:

```rust
use ryo2d::prelude::*;

fn spawn_breathing_monster(ctx: &mut SceneContext, x: f32, y: f32) -> Entity {
    // Load frames - monster expanding/contracting
    let frames = vec![
        load_sprite_handle("monster-small.png"),
        load_sprite_handle("monster-medium.png"),
        load_sprite_handle("monster-large.png"),
    ];
    
    // Ping-pong mode: small → medium → large → medium → small → repeat
    let breathe = Animation::new("breathe", frames, 0.3)
        .with_loop_mode(LoopMode::PingPong);
    
    let initial_sprite = breathe.frame(0).unwrap().clone();
    
    ctx.world.spawn((
        Transform::new(x, y),
        Renderable::sprite(initial_sprite),
        Animator::new(breathe),
        Monster,
    ))
}

#[derive(Debug, Clone, Copy)]
struct Monster;
```

## Example 6: Dynamic Animation Speed

Adjust animation speed based on game state:

```rust
use ryo2d::prelude::*;

fn update_running_animation(ctx: &mut SceneContext, player: Entity) {
    // Get player velocity
    let velocity = ctx.world.get::<Velocity>(player)
        .map(|v| v.dx.abs())
        .unwrap_or(0.0);
    
    // Scale animation speed with movement speed
    if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(player) {
        let speed_scale = (velocity / 100.0).clamp(0.5, 2.0);
        controller.set_speed(speed_scale);
    }
}
```

## Best Practices

1. **Preload Frames** - Load all animation frames at scene startup, not during gameplay
2. **Reuse Animations** - Multiple entities can share the same `Animation` definition
3. **State Before Transition** - Always set the state before setting up a transition
4. **Check Current State** - Use `current_state()` to avoid redundant state changes
5. **One-Shot Cleanup** - Remember to despawn entities with `LoopMode::Once` animations
6. **Animation Speed** - Use `set_speed()` for slow-motion or fast-forward effects

## Debugging Tips

```rust
// Check what animation is playing
if let Some(animator) = ctx.world.get::<Animator>(entity) {
    println!("Current animation: {}", animator.animation_name());
    println!("Frame: {}/{}", 
        animator.current_frame_index() + 1,
        animator.animation().frame_count()
    );
}

// Check controller state
if let Some(controller) = ctx.world.get::<AnimationController>(entity) {
    if let Some(state) = controller.current_state() {
        println!("Animation state: {}", state);
    }
}
```

## See Also

- [Animation System Concept](../concepts/animation.md) - Full API documentation
- [Sprites & Rendering](../concepts/sprites.md) - Understanding sprite rendering
- [Flappy Bird Example](https://github.com/your-repo/examples/flappy) - Real-world usage
