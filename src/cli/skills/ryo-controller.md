---
name: ryo-controller
description: Gamepad/controller input handling with gilrs - buttons, sticks, triggers.
metadata:
  version: 1.0.0
---

# Ryo2D Gamepad/Controller Specialist

You help users implement gamepad and controller input in their Ryo2D games. This covers buttons, analog sticks, triggers, and common input patterns.

**For complete API signatures, see the `ryo-api` skill.**

## Core Concept

Ryo2D uses the `gilrs` crate for cross-platform gamepad support:

- **Automatic detection** - Controllers are detected on plug/unplug
- **First gamepad only** - Currently supports single player (first connected gamepad)
- **Graceful fallback** - Games work without a controller (keyboard fallback)
- **Radial deadzone** - 15% deadzone on analog sticks, automatically applied

## GamepadButton Enum

Uses positional naming that works across controllers:

| Button | PlayStation | Xbox |
|--------|-------------|------|
| `South` | Cross (X) | A |
| `East` | Circle (O) | B |
| `West` | Square | X |
| `North` | Triangle | Y |
| `LeftBumper` | L1 | LB |
| `RightBumper` | R1 | RB |
| `LeftTrigger` | L2 (digital) | LT (digital) |
| `RightTrigger` | R2 (digital) | RT (digital) |
| `LeftStick` | L3 | LS |
| `RightStick` | R3 | RS |
| `Select` | Share | Back |
| `Start` | Options | Menu |
| `Guide` | PS Button | Xbox Button |
| `DPadUp/Down/Left/Right` | D-pad | D-pad |

## Basic Usage

### Checking Connection

```rust
if ctx.input.gamepad_connected() {
    let name = ctx.input.gamepad_name();
    println!("Controller: {}", name);
}
```

### Button Input

Same pattern as keyboard - `down`, `pressed`, `released`:

```rust
// Button held down (continuous)
if ctx.input.gamepad_button_down(GamepadButton::South) {
    // Jump button held
}

// Button just pressed this frame (one-shot)
if ctx.input.gamepad_button_pressed(GamepadButton::South) {
    self.jump();
}

// Button just released this frame
if ctx.input.gamepad_button_released(GamepadButton::East) {
    self.release_charge_attack();
}
```

### Analog Sticks

Values range from -1.0 to 1.0 with deadzone applied. Stick Y is positive when pushed **up**, the opposite of screen Y (which grows downward):

```rust
// Get both axes as tuple
let (x, y) = ctx.input.left_stick();

// Or individual axes
let move_x = ctx.input.left_stick_x();
let move_y = ctx.input.left_stick_y();

// Right stick (for camera, aiming, etc.)
let (aim_x, aim_y) = ctx.input.right_stick();
```

### Triggers

Values range from 0.0 (released) to 1.0 (fully pressed):

```rust
let left = ctx.input.left_trigger();   // 0.0 to 1.0
let right = ctx.input.right_trigger(); // 0.0 to 1.0

// Example: accelerate based on trigger pressure
player_speed = base_speed + (ctx.input.right_trigger() * boost_speed);
```

### Raw Stick Values (Debug)

For visualization/debugging, get raw values without deadzone:

```rust
let (raw_x, raw_y) = ctx.input.left_stick_raw();
let (raw_rx, raw_ry) = ctx.input.right_stick_raw();
```

## Common Patterns

### Combined Keyboard + Gamepad Movement

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let mut move_x = 0.0;
    let mut move_y = 0.0;

    // Keyboard input
    if ctx.input.key_down(KeyCode::KeyA) { move_x -= 1.0; }
    if ctx.input.key_down(KeyCode::KeyD) { move_x += 1.0; }
    if ctx.input.key_down(KeyCode::KeyW) { move_y -= 1.0; }
    if ctx.input.key_down(KeyCode::KeyS) { move_y += 1.0; }

    // Gamepad stick (additive; stick Y is up-positive, so negate it)
    move_x += ctx.input.left_stick_x();
    move_y -= ctx.input.left_stick_y();

    // Gamepad D-pad (additive)
    if ctx.input.gamepad_button_down(GamepadButton::DPadLeft) { move_x -= 1.0; }
    if ctx.input.gamepad_button_down(GamepadButton::DPadRight) { move_x += 1.0; }
    if ctx.input.gamepad_button_down(GamepadButton::DPadUp) { move_y -= 1.0; }
    if ctx.input.gamepad_button_down(GamepadButton::DPadDown) { move_y += 1.0; }

    // Clamp to unit circle to prevent diagonal speed boost
    let len = (move_x * move_x + move_y * move_y).sqrt();
    if len > 1.0 {
        move_x /= len;
        move_y /= len;
    }

    // Apply movement
    if let Some(mut vel) = ctx.world.get_mut::<Velocity>(self.player) {
        vel.dx = move_x * PLAYER_SPEED;
        vel.dy = move_y * PLAYER_SPEED;
    }

    SceneAction::Continue
}
```

### Jump with Multiple Inputs

```rust
let jump_pressed = ctx.input.key_pressed(KeyCode::Space)
    || ctx.input.key_pressed(KeyCode::KeyW)
    || ctx.input.gamepad_button_pressed(GamepadButton::South);

if jump_pressed && self.on_ground {
    self.jump();
}
```

### Pause Menu Toggle

```rust
let pause_pressed = ctx.input.key_pressed(KeyCode::Escape)
    || ctx.input.gamepad_button_pressed(GamepadButton::Start);

if pause_pressed {
    return SceneAction::Push(Box::new(PauseMenu::new()));
}
```

### Analog Aim Direction

```rust
// Get aim direction from right stick
let (aim_x, aim_y) = ctx.input.right_stick();

// Only aim if stick is moved
if aim_x.abs() > 0.1 || aim_y.abs() > 0.1 {
    let angle = (-aim_y).atan2(aim_x);  // negate: stick Y is up-positive
    self.aim_direction = angle;
}
```

### Trigger-Based Actions

```rust
// Gradual speed based on trigger
let speed = BASE_SPEED + ctx.input.right_trigger() * BOOST_SPEED;

// Fire weapon when trigger crosses threshold
if ctx.input.right_trigger() > 0.8 && !self.was_firing {
    self.fire_weapon();
    self.was_firing = true;
} else if ctx.input.right_trigger() < 0.2 {
    self.was_firing = false;
}
```

## Deadzone Explained

The 15% radial deadzone prevents stick drift:

```
         1.0
          |
    ------+------
   /      |      \
  /   deadzone    \
 |    (0 output)   |
  \       |       /
   \      |      /
    ------+------
          |
        -1.0

- Inside deadzone (magnitude < 0.15): outputs (0, 0)
- Outside deadzone: rescaled to use full 0.0-1.0 range
```

The deadzone is applied radially (to the combined x,y magnitude), not per-axis, which feels more natural for circular stick movement.

## Hot-Plugging

Controllers can be connected/disconnected during gameplay:

```rust
// Check whether a controller is connected (poll each frame; it is state, not an event)
if ctx.input.gamepad_connected() {
    // Show "Controller connected" message
    self.show_notification(&format!("Controller: {}", ctx.input.gamepad_name()));
}

// If controller disconnected, pause and show message
if !ctx.input.gamepad_connected() && self.was_using_gamepad {
    return SceneAction::Push(Box::new(ReconnectPrompt::new()));
}
```

## Controller Demo

The engine includes a controller demo for testing:

```bash
cd examples/demos/controller
cargo run
```

This visualizes:
- All button states (highlighted when pressed)
- Stick positions (raw vs deadzone-applied)
- Trigger values as progress bars
- Button press history

## Supported Controllers

Tested and working:
- PS5 DualSense (USB and Bluetooth)
- PS4 DualShock 4
- Xbox One/Series controllers
- Generic XInput gamepads

The gilrs crate handles controller mapping automatically via the SDL gamepad database.
