---
name: ryo-controller
description: Gamepad/controller input with gilrs - buttons, sticks, triggers, actions and stick directions, pad families and button prompts, the last-used device, and scripted or headless pad input.
metadata:
  version: 2.0.0
---

# Ryo2D Gamepad/Controller Specialist

You help users implement gamepad and controller input in their Ryo2D games. This covers actions (one name for a key, a button and a stick direction), buttons, analog sticks, triggers, button prompts, menus played with the pad, and testing with a virtual pad.

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

## Actions: Keys, Buttons and Stick Directions Under One Name

Bind an action once and read it, instead of checking keys and buttons side by side. The player can rebind it (`ui.rebind` in a settings screen, see `ryo-ui`), and prompts show the binding for the device in use.

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    use Binding::{Key, Pad};
    let stick = |direction| Binding::Stick(StickDirection { stick: Stick::Left, direction });
    ctx.actions()
        .bind("jump", [Key(KeyCode::Space), Pad(GamepadButton::South)])
        .bind("left", [Key(KeyCode::KeyA), Pad(GamepadButton::DPadLeft), stick(Direction::Left)])
        .bind("right", [Key(KeyCode::KeyD), Pad(GamepadButton::DPadRight), stick(Direction::Right)])
        .bind("pause", [Key(KeyCode::Escape), Pad(GamepadButton::Start)]);
}

fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.action_pressed("jump") && self.on_ground {
        self.jump();
    }
    if ctx.input.action_held("left") { /* walk left */ }
    if ctx.input.action_pressed("pause") {
        return SceneAction::Push(Box::new(PauseMenu::default()));
    }
    SceneAction::Continue
}
```

- `action_pressed` / `action_held` / `action_released` follow the one-shot rules of what they bind (seen by the first fixed step of a frame).
- **Stick directions** are digital: `StickDirection { stick: Stick::Left | Stick::Right, direction: Direction::Up | Down | Left | Right }` goes down past 0.5 deflection and comes up below 0.4, so a stick flick is one press. Read one directly with `ctx.input.stick_direction_pressed(dir)` (`_down`, `_released`).
- **Rebinding:** `ctx.actions().rebind("jump", 0, Binding::Pad(GamepadButton::North))` replaces slot 0 (past the end it adds one); if another action of the same group (`ui.*` or the game's) had that binding, the two swap and the other's name is returned. `reset_defaults()` puts every action back. `Binding::to_config()` / `parse` are the saved text (`"Pad:South"`, `"Stick:LeftUp"`). With `EngineBuilder::with_user_preferences([UserPreference::Bindings])` the player's bindings are saved in `system.json` (`ryo-config-save`).
- **The UI's actions** are pre-bound: `ui.up/down/left/right` (arrows, D-pad, left stick), `ui.confirm` (Enter, Space, South), `ui.back` (Escape, East), `ui.next` (Tab), `ui.tab_next` / `ui.tab_prev` (E / Q and the bumpers). Menus inside a focus scope are played with them; with no scope on screen the D-pad, the stick and A stay gameplay's (`ryo-ui`, Focus and Navigation).

## Prompts and Pad Families

```rust
ui.prompt("jump");                                        // a chip or glyph for the action on the last device
ui.text(RichText::markup("Press [prompt=jump] to jump")); // the same inside text (world text too)
```

- `ctx.input.last_device()` is the device the player used last (`InputDevice::Keyboard`, `Mouse` or `Gamepad`): a key press, a mouse press, wheel or motion, or a pad press or stick past 0.5. Prompts follow it, and so does the UI's focus ring (shown for the keyboard and the pad only).
- `ctx.input.pad_family()` is `PadFamily::Xbox` (the default), `PlayStation` or `Nintendo`, from the pad's name (`PadFamily::from_name`: "dualsense", "dualshock", "playstation", "sony", "ps4", "ps5"; "nintendo", "switch", "pro controller", "joy-con"). `Binding::label(family)` is what a prompt shows: South is "A" on Xbox, "×" on PlayStation, "B" on Nintendo; a stick direction is "L↑".
- **Glyphs:** register a sprite per button in `TextIcons` under `prompt.pad.<xbox|playstation|nintendo>.<GamepadButton>` (`prompt.pad.xbox.South`), `prompt.key.<KeyCode>` (`prompt.key.Space`) or `prompt.stick.<Stick><Direction>` (`prompt.stick.LeftUp`), and prompts draw it instead of the text label.

```rust
if let Ok(a_button) = ctx.assets.load_sprite("assets/prompts/xbox_a.png") {
    ctx.world.resource_mut::<TextIcons>().insert("prompt.pad.xbox.South", a_button);
}
```

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

For a character, `MoveBindings` does this for you (keys, the left stick and the d-pad into a `MoveIntent` the movement presets read; see `ryo-character`). By hand:

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

### Jump and Pause

Use actions (above): `ctx.input.action_pressed("jump")` covers Space and A, and `action_pressed("pause")` covers Escape and Start, with the player's own bindings.

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

## Testing and Scripting Pad Input

Headless tests and scripted runs drive a **virtual pad** ("Virtual pad"), connected on first use when no real pad is (`InputEvent::PadButton` and `InputEvent::PadStick`, which carry no gilrs id).

```rust
use ryo2d::testing::HeadlessEngine;

let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else { return };
engine.step_frames(3).unwrap();
engine.tap_pad(GamepadButton::DPadDown);    // pressed for the next step only
engine.step_frames(2).unwrap();
engine.press_pad(GamepadButton::South);     // held until release_pad
engine.set_stick(Stick::Left, 0.0, 1.0);    // held at (x, y), +y up, until set again
engine.step_frames(2).unwrap();
assert!(engine.ui_focused().is_some());     // the UI's focus after the last step
```

- `ui_focusables()` lists what the D-pad can reach now and `focus_ui(id)` moves the focus, so a test can walk every widget of a menu with the pad (`ryo-ui`, Testing UI).
- `RYO_INPUT` scripts the pad in dev runs (`ryo-cli`): `N:PadSouth` taps a button (`Pad` + a `GamepadButton` name), `A-B:PadDPadDown` holds one, `A-B:LeftStick@0x-1` / `A-B:RightStick@XxY` hold a stick and centre it before frame B.

```bash
RYO_HEADLESS=1 RYO_INPUT="60:PadDPadDown 90:PadSouth 120-180:LeftStick@0x-1" \
  RYO_CAPTURE_FRAMES=100,200 RYO_CAPTURE_EXIT=1 ryo2d dev
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
