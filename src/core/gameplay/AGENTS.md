# AGENTS.md - Gameplay Module

Gameplay on top of physics: movement presets and the input that drives them, trigger behaviours and knockback. Later pieces are added here as they land; this file lists what exists.

## Dependency Rule

`gameplay` depends on `physics`, never the reverse, and names no Rapier type (it uses `Physics` queries, `CharacterController`, `RigidBody`, messages). Components that physics must read stay in `src/core/ecs/` (`Knockback`).

## The `Movement` Set

`register_gameplay_systems` (`mod.rs`) adds one chain to `FixedSet::Movement`, which runs after the scene's `update` and `FixedSet::Game` and before `FixedSet::Physics`. Systems join the chain in this order as they are written: `place_respawn_points`, `warn_untriggered`, `collect_pickups`, `respawn_pickups`, `apply_kill_zones`, `apply_checkpoints`, `read_move_bindings`, `update_knockback`, `platformer_movement`, `top_down_movement`. Today `read_move_bindings`, `update_knockback`, `platformer_movement` and `top_down_movement` are in it. Every gameplay system's resources are `Time`, `Input` and messages, so a world with those three (`tests/integration/support.rs::gameplay_world`) runs them without a simulation.

Invalid input is logged once per entity with a `Local<EntityHashSet>` and `crate::engine_warn!(module: "Gameplay", ...)`; nothing panics.

## Movement Input

`MoveIntent` (`intent.rs`) is what every movement preset reads: a clamped `direction`, `jump_held`, and the one-shot `jump_pressed` / `dash_pressed`. `MoveBindings` (requires `MoveIntent`) fills it from the keyboard and the first gamepad: `read_move_bindings` writes every bound intent each fixed step. Input is read from the fixed view, so a press is seen once, in the first fixed step after it, with no latch of its own. Code that writes `MoveIntent` directly (AI, networking) owns the one-shots: presets clear the pressed flags they act on. `direction` is not clamped on write; presets use `clamped_direction()`.

## The Platformer Preset

`PlatformerMovement` (`platformer.rs`) requires `MoveIntent`, `PlatformerState` and `CharacterController::platformer()` (a top-down controller already there is warned once and skipped). `platformer_movement` writes the character's `Velocity` every step, gravity included (characters are kinematic, so physics' gravity never reaches them); the controller sweeps it in the physics step and leaves the real velocity, so the preset starts each step from what actually happened (the fall zeroed on landing, a wall's clip). It reads `CharacterState` from the last move. A non-finite `Velocity` is read as zero and warned once per entity (its own warned set, apart from the wrong-mode warning), so physics never sees it. Each step, in this order:

1. **Landed:** `CharacterState::landed` writes `MovementEvent { kind: Landed }`, refills `air_jumps_left` (to `air_jumps`) and the air dash (`dash_ready`), and ends a wall jump's hold on the run (without that, a character landing inside the hold kept the launch speed along the ground, unable to steer).
2. **Timers:** coyote is reset to `coyote_time` while on the floor and counts down in the air; the jump buffer is set to `jump_buffer` by `jump_pressed` and counts down otherwise. Both are at least one step, so zero turns the grace off without stopping jumps from the floor. The wall jump's hold and the dash cooldown count down. Wall coyote shares `coyote_time`: touching a wall in the air (`on_wall`, not `on_floor`) resets it, again at least a step, and records the wall's side (the sign of `wall_normal.x`); it counts down otherwise. The controller reports a wall only on a step the character moves into it, so wall jumps and slides need the input (or the momentum) pressing into the wall.
3. **Dash:** `dash_pressed` with `dash` set, the air dash unused (`dash_ready`), the cooldown over and no dash running starts one: along the input, normalized (the facing when there is none), which also sets the facing; the cooldown counts from the start; in the air it uses the air dash until landing; `jumping` ends; `Dashed` written. While a dash lasts (`dashing`) the velocity is `direction * speed`: the presses are cleared, `Velocity` written and the rest of the step skipped (no running, jumps, gravity or wall slide). Its last step leaves at `min(speed, run_speed)`. A jump pressed during a dash is still buffered.
4. **Run:** `velocity.x` moves toward `direction.x * run_speed` by `run_speed / accel_time` per second when speeding up the same way, `run_speed / decel_time` otherwise (a zero time is instant), scaled by `air_control` off the floor. An `air_control` of zero (or not a number) is no steering in the air at all: the speed is kept, and an instant rate is never multiplied by it (infinity times zero is NaN, which physics refuses, freezing the character). While `Knockback` is active the target is scaled by `knockback_control` (0 by default: no steering); the push itself is the controller's (see Knockback). `facing` follows the input. The whole step (speed and facing) is skipped while a wall jump's hold lasts, so the jump keeps its direction.
5. **Drop or jump**, the first that applies (so the order of everything is dash > drop-through > ground jump > wall jump > air jump):
   - **Drop-through:** down (`direction.y > 0.5`) + `jump_pressed` on a one-way floor sets `CharacterController::drop_through` and drops the buffered press and the coyote time (a jump pressed just after would pop the character back up through the platform).
   - **Ground jump:** a buffered press with coyote left: `velocity.y = -launch`, both timers cleared, `jumping` set, `Jumped` written.
   - **Wall jump** (`wall_jump` set): a buffered press with wall coyote left, in the air: `velocity = (side * launch.x, -launch.y)` away from the wall, the run held off for `lock_time`, the air jumps refilled, facing away from the wall, `jumping` set, the buffer and wall coyote cleared, `WallJumped` written.
   - **Air jump:** `jump_pressed` in the air with no coyote left and an air jump left: `velocity.y = -launch` (the ground jump's), one air jump used, `jumping` set, the buffer cleared, `AirJumped` written. It takes the press itself, not the buffer: a press with none left is only buffered, for a landing or a wall.
6. **Cut:** while `jumping` and rising, releasing `jump_held` multiplies the rise by `jump_cut`, once; `jumping` ends at the top or at the cut. It applies to every kind of jump.
7. **Gravity:** added before the move, times `fall_multiplier` while falling, capped at `max_fall_speed`.
8. **Wall slide** (`wall_slide` set): on a wall in the air, falling and pressing into it (`direction.x * wall_normal.x < 0`), the fall is capped at the slide speed and `wall_sliding` set; otherwise `wall_sliding` is false.
9. **Clear the presses:** `jump_pressed` and `dash_pressed` are consumed (see Movement Input).

**The jump:** `jump_physics(height, time_to_apex, dt)` gives gravity and the launch speed. Gravity is added before each move (semi-implicit Euler), so after `n = T / dt` steps the rise is `n·v·dt - g·dt²·n(n+1)/2`, which with `v = g·T` is `(g/2)(T² - T·dt)`. So `g = 2h / (T² - T·dt)` and `v = g·T` reach `h` at `T` at the fixed step; the continuous `g = 2h / T²` falls short by `g·T·dt/2` (2.8 px of the default 64 at 60 Hz). When `T` is not a whole number of steps the top is the last step that still rises: the default 0.38 s is 22.8 steps at 60 Hz, and the jump peaks at step 22, 64.02 px up (`the_jump_reaches_its_height_at_its_apex_time`). A jump is not snapped back to the floor: the controller snaps only moves that do not go up. `time_to_apex` is at least two steps.

**Defaults:** run 180 px/s, 0.08 s to full speed, 0.06 s to stop, 65% air control; a 64 px jump peaking at 0.38 s (gravity about 927 px/s², launch about 352 px/s at 60 Hz), falls 1.5 times heavier up to 600 px/s; 0.1 s of coyote time and jump buffer; releasing jump keeps half the rise; no steering during knockback; no air jumps, wall slide, wall jump or dash (`with_air_jumps(n)`, `with_wall_slide(max_speed)`, `with_wall_jump(launch, lock_time)`, `with_dash(speed, duration, cooldown)`; `WallJump` and `Dash` are their settings, and `Dash` is shared with the top-down preset).

`PlatformerState` keeps `facing`, `jumping`, `air_jumps_left`, `wall_sliding`, `dashing`, `dash_ready` and the crate-internal timers (coyote, buffer, wall coyote and side, the wall jump's hold, the dash's time left, cooldown and direction); `reset()` forgets all but the facing (a respawn): the air dash is ready again, and the air jumps come back at the next landing. `MovementEvent` (`events.rs`, `#[non_exhaustive]` kind: `Jumped`, `AirJumped`, `WallJumped`, `Dashed`, `Landed`) is registered with the engine's messages; written in `FixedSet::Movement`, after the scene's `update`, so `update` reads it the next step.

## The Top-Down Preset

`TopDownMovement` (`top_down.rs`) requires `MoveIntent`, `TopDownState` and `CharacterController::top_down()` (a platformer controller already there is warned once and skipped; a non-finite `Velocity` is read as zero and warned once, each with its own warned set). `top_down_movement` writes `Velocity` every step; the controller slides it along every surface and leaves the real velocity, which the next step starts from. Each step:

1. **Facing:** the clamped input's direction (`facing`, unit length, `Y` until steered); no input keeps it.
2. **Dash:** `dash_pressed` with `dash` set, the cooldown over and no dash running starts one along the facing (not the input, which can be zero), counting the cooldown from the start; `Dashed` written. While it lasts (`dashing`) the velocity is `facing * speed` and the input is ignored; its last step leaves at `min(speed, max_speed)`.
3. **Move:** otherwise the velocity moves toward `direction * max_speed` in a straight line, at `max_speed / accel_time` per second when there is input and `max_speed / decel_time` when there is none (a zero time, or a rate that is zero or infinite because the speed or time is, is instant: the target at once, never a frozen or infinite rate). The direction is clamped to length 1, so diagonals are no faster and a half stick is half speed. While `Knockback` is active the target is scaled by `knockback_control` (0 by default).
4. **Clear the presses:** `jump_pressed` and `dash_pressed` are consumed.

**Defaults:** 160 px/s, 0.08 s to full speed, 0.1 s to stop, no dash (`with_speed(max, accel_time, decel_time)`, `with_dash(speed, duration, cooldown)` using the platformer's `Dash`, `with_knockback_control(fraction)`). `TopDownState::reset()` forgets the dash and its cooldown, keeping the facing.

## Knockback

`update_knockback` (`knockback.rs`) decays every active `Knockback` and reports `KnockbackStarted` / `KnockbackFinished`. What the push does depends on the target:

| Target | Effect |
|---|---|
| `CharacterController` | Nothing here: `move_characters` (physics) passes the push as the sweep's `extra` velocity and clips it against every surface the move touched (`Knockback::clip`), so a wall absorbs the part running into it and a head-on push stops |
| Dynamic body | Each hit gives one change in velocity of its own (`direction * force` of that hit, kept in `Knockback::pending` until the system takes it; blending does not change what a hit gives), whatever the mass; physics slows it. Nothing is taken back |
| Kinematic body without a controller, fixed body | Ignored, warned once per entity |
| No body | `Transform` moves by the current knockback velocity |

## Tests

`test_util.rs`: `world(hz)` is the physics test world plus `Input` and the gameplay systems; `player(world, x, y)` spawns a 20x40 platformer character; `at`, `floor`, `no_gravity` and `run` come from the physics helpers. Single-system tests use `systems::test_util` (`world`, `run`, `messages`).
