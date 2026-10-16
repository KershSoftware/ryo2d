# Input

- `Input` is a world resource fed by queued `InputEvent`s, published in `First` by `begin_frame`.
- One-shots (presses, releases, the wheel) read the frame view outside the fixed steps, the first fixed step's view inside them, and nothing in later steps. A frame with no fixed step carries them to the next first step; a pause drops them (`end_fixed_steps(step > 0 || paused)`).
- `last_device()` follows the newest key press, mouse press/wheel/motion (2+ units), or pad press/stick (past 0.5): prompts and the UI's focus ring use it.
- Only the first connected gamepad is tracked. `InputEvent::PadButton`/`PadStick` are id-less pad events for scripted and headless input; they connect a "Virtual pad" when none is connected.
- `RYO_INPUT` (`input_script.rs`) scripts keys, the mouse, the wheel, pad buttons and sticks for dev captures.
