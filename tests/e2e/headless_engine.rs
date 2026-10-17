//! E2E tests for ryo2d::testing::HeadlessEngine.

use ryo2d::core::config::UserPreferenceFilter;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, HeadlessError, StepOutcome};

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Draws a 20x20 red square at (100, 50) and a 30x30 green UI rect at (10, 10).
struct Squares;

impl Scene for Squares {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(20, 20, 255, 0, 0, 255), "red");
    ctx.world.spawn((
      Transform::from_xy(100.0, 50.0),
      Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(10.0, 10.0, 30.0, 30.0), Color::rgb(0, 255, 0));
    SceneAction::Continue
  }
}

/// A square that moves 2px right per update, for determinism checks.
struct Mover {
  x: f32,
}

impl Scene for Mover {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.x += 2.0;
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(self.x, 40.0, 8.0, 8.0), Color::rgb(255, 255, 0));
    SceneAction::Continue
  }
}

/// Quits on its third update.
struct QuitsOnThird {
  updates: u32,
}

impl Scene for QuitsOnThird {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 3 {
      SceneAction::Quit
    } else {
      SceneAction::Continue
    }
  }
}

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 180).start_scene(scene))
}

#[test]
fn sprite_and_ui_appear_in_frame() {
  let Some(mut engine) = engine(Squares) else {
    return;
  };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();

  assert_eq!(frame.dimensions(), (320, 180));
  assert_eq!(frame.get_pixel(110, 60).0, RED);
  assert_eq!(frame.get_pixel(20, 20).0, GREEN);
}

#[test]
fn frame_before_first_step_is_an_error() {
  let Some(engine) = engine(Squares) else {
    return;
  };
  assert!(engine.frame().is_err());
}

#[test]
fn same_steps_give_identical_frames() {
  let Some(mut a) = engine(Mover { x: 0.0 }) else {
    return;
  };
  let Some(mut b) = engine(Mover { x: 0.0 }) else {
    return;
  };

  a.step_frames(20).unwrap();
  b.step_frames(20).unwrap();

  assert_eq!(a.frame().unwrap().as_raw(), b.frame().unwrap().as_raw());
  assert_eq!(a.frame_count(), 20);
}

#[test]
fn two_engines_share_the_gpu() {
  let Some(mut a) = engine(Squares) else { return };
  let Some(mut b) = engine(Squares) else { return };
  a.step().unwrap();
  b.step().unwrap();
  drop(a);
  b.step().unwrap();
  assert_eq!(b.frame_count(), 2);
}

#[test]
fn quit_stops_stepping() {
  let Some(mut engine) = engine(QuitsOnThird { updates: 0 }) else {
    return;
  };

  let outcome = engine.step_frames(10).unwrap();

  assert_eq!(outcome, StepOutcome::Quit);
  assert_eq!(engine.frame_count(), 2);
}

#[test]
fn step_after_quit_stays_quit() {
  let Some(mut engine) = engine(QuitsOnThird { updates: 0 }) else {
    return;
  };
  engine.step_frames(10).unwrap();

  assert_eq!(engine.step().unwrap(), StepOutcome::Quit);
  assert_eq!(engine.frame_count(), 2);
}

#[test]
fn headless_ignores_user_preferences_and_fps_overlay() {
  let data_dir = std::path::Path::new("data");
  let prefs = std::path::Path::new("data/system.json");
  let dir_existed = data_dir.exists();
  let existed = prefs.exists();
  if existed {
    eprintln!("note: data/system.json already exists; skipping the file-created check");
  }
  let Some(mut with_prefs) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 180)
      .show_fps(true)
      .with_user_preferences(UserPreferenceFilter::all())
      .start_scene(Squares),
  ) else {
    return;
  };
  let created = !existed && prefs.exists();
  if created {
    let _ = std::fs::remove_file(prefs);
    if !dir_existed {
      let _ = std::fs::remove_dir(data_dir);
    }
  }
  assert!(!created, "headless build created data/system.json");

  let Some(mut plain) = engine(Squares) else {
    return;
  };
  with_prefs.step_frames(2).unwrap();
  plain.step_frames(2).unwrap();
  assert_eq!(
    with_prefs.frame().unwrap().as_raw(),
    plain.frame().unwrap().as_raw()
  );
}

/// Paints input state into the top-left 4 cells of a 4x1 strip:
/// [0] Space held, [1] Space just pressed, [2] Space just released, [3] left mouse held.
/// Also draws a 4x4 marker at the mouse position.
#[derive(Default)]
struct InputProbe {
  held: bool,
  pressed: bool,
  released: bool,
  mouse_down: bool,
  mouse: (f32, f32),
}

impl Scene for InputProbe {
  // The UI pass runs first each frame, so the probe reads that frame's input
  // where it draws
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let input = ui.input();
    self.held = input.key_down(KeyCode::Space);
    self.pressed = input.key_pressed(KeyCode::Space);
    self.released = input.key_released(KeyCode::Space);
    self.mouse_down = input.mouse_down(MouseButton::Left);
    self.mouse = input.mouse_position();
    for (i, on) in [self.held, self.pressed, self.released, self.mouse_down]
      .into_iter()
      .enumerate()
    {
      if on {
        ui.draw_rect(
          Rect::new(i as f32 * 10.0, 0.0, 10.0, 10.0),
          Color::rgb(255, 255, 255),
        );
      }
    }
    ui.draw_rect(
      Rect::new(self.mouse.0, self.mouse.1, 4.0, 4.0),
      Color::rgb(255, 0, 255),
    );
    SceneAction::Continue
  }
}

/// Which of the four indicator cells are lit in the current frame.
fn cells(engine: &HeadlessEngine) -> [bool; 4] {
  let frame = engine.frame().unwrap();
  [0, 1, 2, 3].map(|i| frame.get_pixel(i * 10 + 5, 5).0 == [255, 255, 255, 255])
}

#[test]
fn press_key_is_just_pressed_once() {
  let Some(mut engine) = engine(InputProbe::default()) else {
    return;
  };

  engine.press_key(KeyCode::Space);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [true, true, false, false]);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [true, false, false, false]);
}

#[test]
fn release_key_is_just_released_once() {
  let Some(mut engine) = engine(InputProbe::default()) else {
    return;
  };
  engine.press_key(KeyCode::Space);
  engine.step_frames(2).unwrap();

  engine.release_key(KeyCode::Space);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [false, false, true, false]);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [false, false, false, false]);
}

#[test]
fn tap_key_presses_for_one_step() {
  let Some(mut engine) = engine(InputProbe::default()) else {
    return;
  };

  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [true, true, false, false]);
  engine.step().unwrap();
  assert_eq!(cells(&engine), [false, false, true, false]);
}

#[test]
fn mouse_move_and_button_reach_the_scene() {
  let Some(mut engine) = engine(InputProbe::default()) else {
    return;
  };

  engine.mouse_move(200.0, 100.0);
  engine.press_mouse(MouseButton::Left);
  engine.step().unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(201, 101).0, [255, 0, 255, 255]);
  assert!(cells(&engine)[3]);

  engine.release_mouse(MouseButton::Left);
  engine.step().unwrap();
  assert!(!cells(&engine)[3]);
}

#[test]
fn engine_debug_keys_are_inert_headless() {
  let Some(mut tapped) = engine(Squares) else {
    return;
  };
  let Some(mut plain) = engine(Squares) else {
    return;
  };
  let captures_existed = std::path::Path::new("captures").exists();

  tapped.tap_key(KeyCode::F1); // would cycle the FPS/timing overlays on
  tapped.step_frames(2).unwrap();
  tapped.tap_key(KeyCode::F12); // would write captures/frame-*.png
  tapped.step_frames(2).unwrap();
  plain.step_frames(4).unwrap();

  let wrote_capture = !captures_existed && std::path::Path::new("captures").exists();
  if wrote_capture {
    let _ = std::fs::remove_dir_all("captures");
  }
  assert!(!wrote_capture, "F12 wrote a capture in a headless run");
  assert_eq!(
    tapped.frame().unwrap().as_raw(),
    plain.frame().unwrap().as_raw()
  );
}

/// Requests UI scale 2 and draws a rect sized by the live UI scale. The
/// scale is render pixels per UI unit, so the rect's coordinates are scaled
/// too: 20 UI units are 40 pixels.
struct WantsUiScale;

impl Scene for WantsUiScale {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.request_ui_scale(Some(2.0));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let side = 10.0 * ui.ui_scale();
    ui.draw_rect(Rect::new(0.0, 0.0, side, side), Color::rgb(0, 255, 255));
    SceneAction::Continue
  }
}

#[test]
fn requested_ui_scale_applies_headless() {
  let Some(mut engine) = engine(WantsUiScale) else {
    return;
  };

  engine.step_frames(3).unwrap();

  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(35, 35).0, [0, 255, 255, 255]);
  assert_ne!(frame.get_pixel(45, 45).0, [0, 255, 255, 255]);
}

#[test]
fn invalid_resolution_is_an_error_not_a_panic() {
  for (width, height) in [(0, 180), (320, 0), (8193, 180), (320, 8193)] {
    let result = HeadlessEngine::new(
      EngineBuilder::new()
        .resolution(width, height)
        .start_scene(Squares),
    );
    assert!(
      matches!(
        result,
        Err(HeadlessError::Render(_)) | Err(HeadlessError::NoGpu)
      ),
      "{width}x{height} should be rejected"
    );
  }
}

/// A white horizontal line across the middle of the screen, optionally under a bright light.
struct LineScene {
  with_light: bool,
}

impl Scene for LineScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if self.with_light {
      ctx.world.spawn((
        Transform::from_xy(160.0, 90.0),
        Light2D::point(Color::rgb(255, 255, 255), 400.0).with_intensity(2.0),
      ));
    }
    let line = Line2D::new(
      vec![Vec2::new(40.0, 90.0), Vec2::new(280.0, 90.0)],
      Color::rgb(255, 255, 255),
      8.0,
    );
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// Brightness of the line's centre minus the background 20px above it.
fn line_contrast(engine: &HeadlessEngine) -> (i32, [u8; 4], [u8; 4]) {
  let frame = engine.frame().unwrap();
  let on_line = frame.get_pixel(160, 90).0;
  let beside = frame.get_pixel(160, 70).0;
  let sum = |p: [u8; 4]| i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2]);
  (sum(on_line) - sum(beside), on_line, beside)
}

#[test]
fn lines_render_without_lighting() {
  let Some(mut engine) = engine(LineScene { with_light: false }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let (contrast, on_line, beside) = line_contrast(&engine);
  assert!(
    contrast > 150,
    "line missing: on {on_line:?}, beside {beside:?}"
  );
}

#[test]
fn lines_render_when_lighting_is_on() {
  let Some(mut engine) = engine(LineScene { with_light: true }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let (contrast, on_line, beside) = line_contrast(&engine);
  assert!(
    contrast > 150,
    "line missing under lighting: on {on_line:?}, beside {beside:?}"
  );
}

/// Counts every `CollisionStarted` it reads and pauses physics after the first.
struct PauseOnHit {
  started: std::sync::Arc<std::sync::atomic::AtomicUsize>,
  paused: bool,
}

impl Scene for PauseOnHit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // A falling body overlapping a fixed floor
    ctx.world.spawn((
      Transform::from_xy(100.0, 100.0),
      RigidBody::dynamic(),
      Collider::rect(20.0, 20.0),
    ));
    ctx.world.spawn((
      Transform::from_xy(100.0, 105.0),
      RigidBody::fixed(),
      Collider::rect(80.0, 20.0),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let seen = ctx.messages::<CollisionStarted>().len();
    if seen > 0 {
      self
        .started
        .fetch_add(seen, std::sync::atomic::Ordering::SeqCst);
      self.paused = true;
    }
    SceneAction::Continue
  }

  fn is_physics_paused(&self) -> bool {
    self.paused
  }
}

#[test]
fn paused_physics_does_not_repeat_collision_events() {
  let started = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let Some(mut engine) = engine(PauseOnHit {
    started: started.clone(),
    paused: false,
  }) else {
    return;
  };

  engine.step_frames(10).unwrap();

  assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// A pitch-dark scene with one normal line (y=60) and one unlit line (y=120).
struct DarkLines;

impl Scene for DarkLines {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.lighting().set_ambient(AmbientLight::dark());
    // A tiny light far from the lines, so lighting is on but they stay dark
    ctx.world.spawn((
      Transform::from_xy(310.0, 5.0),
      Light2D::point(Color::rgb(255, 255, 255), 4.0),
    ));
    let white_line = |y: f32| {
      Line2D::new(
        vec![Vec2::new(40.0, y), Vec2::new(280.0, y)],
        Color::rgb(255, 255, 255),
        8.0,
      )
    };
    ctx
      .world
      .spawn((Transform::from_xy(0.0, 0.0), white_line(60.0)));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      white_line(120.0).with_lighting(false),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn unlit_lines_stay_bright_in_the_dark() {
  let Some(mut engine) = engine(DarkLines) else {
    return;
  };
  engine.step_frames(3).unwrap();

  let frame = engine.frame().unwrap();
  let brightness = |p: [u8; 4]| i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2]);
  let lit = frame.get_pixel(160, 60).0;
  let unlit = frame.get_pixel(160, 120).0;
  assert!(brightness(lit) < 60, "lit line should be dark: {lit:?}");
  assert!(
    brightness(unlit) > 600,
    "unlit line should be bright: {unlit:?}"
  );
}

/// A white unlit line across the middle of the screen, in a scene without lights.
struct UnlitLineNoLights;

impl Scene for UnlitLineNoLights {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let line = Line2D::new(
      vec![Vec2::new(40.0, 90.0), Vec2::new(280.0, 90.0)],
      Color::rgb(255, 255, 255),
      8.0,
    )
    .with_lighting(false);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn unlit_lines_render_without_lighting() {
  let Some(mut engine) = engine(UnlitLineNoLights) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let (contrast, on_line, beside) = line_contrast(&engine);
  assert!(
    contrast > 150,
    "unlit line missing without lighting: on {on_line:?}, beside {beside:?}"
  );
}

/// A mid-grey sprite must come out mid-grey on a target without an sRGB
/// format: the composite encodes to sRGB itself, so the output is not dark.
#[test]
fn a_non_srgb_target_still_shows_srgb_colours() {
  use ryo2d::testing::offscreen_with_format;
  struct Grey;
  impl Scene for Grey {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let grey = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, 128, 128, 128, 255), "grey");
      ctx
        .world
        .spawn((Transform::from_xy(16.0, 16.0), Renderable::sprite(grey)));
    }
  }
  let builder = EngineBuilder::new().resolution(32, 32).start_scene(Grey);
  let Some(mut engine) = HeadlessEngine::new_or_skip_with(
    builder,
    offscreen_with_format(wgpu::TextureFormat::Rgba8Unorm),
  ) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let pixel = engine.frame().unwrap().get_pixel(16, 16).0;
  assert!((i32::from(pixel[0]) - 128).abs() <= 2, "{pixel:?}");
}

fn red_square_at(x: f32, y: f32, size: u32) -> impl FnMut(&mut SceneContext) + Send + 'static {
  move |ctx| {
    let red = SpriteHandle::from_sprite(Sprite::solid_rect(size, size, 220, 30, 40, 255), "red");
    ctx
      .world
      .spawn((Transform::from_xy(x, y), Renderable::sprite(red)));
  }
}

struct Setup(Box<dyn FnMut(&mut SceneContext) + Send>);
impl Scene for Setup {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    (self.0)(ctx);
  }
}

#[test]
fn scale_factor_two_renders_twice_the_pixels() {
  let builder = EngineBuilder::new()
    .resolution(32, 32)
    .start_scene(Setup(Box::new(red_square_at(16.0, 16.0, 10))));
  let Some(mut engine) = HeadlessEngine::new_or_skip_with_scale_factor(builder, 2.0) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.dimensions(), (64, 64));
  // 10 x 10 screen units around (16, 16) is 20 x 20 pixels around (32, 32)
  assert_eq!(frame.get_pixel(32, 32).0, [220, 30, 40, 255]);
  assert_eq!(frame.get_pixel(23, 32).0, [220, 30, 40, 255]);
  assert_ne!(frame.get_pixel(21, 32).0, [220, 30, 40, 255]);
  assert_ne!(frame.get_pixel(43, 32).0, [220, 30, 40, 255]);
}

#[test]
fn pixel_art_letterboxes_by_a_whole_number() {
  let builder = EngineBuilder::new()
    .pixel_art(32, 32)
    .start_scene(Setup(Box::new(red_square_at(16.0, 16.0, 32))));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.resize(100, 80); // scale 2: a 64 x 64 image at (18, 8)
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().dimensions(), (32, 32));
  let window = engine.window_frame().unwrap();
  assert_eq!(window.dimensions(), (100, 80));
  assert_eq!(window.get_pixel(5, 40).0, [0, 0, 0, 255], "left bar");
  assert_eq!(window.get_pixel(50, 40).0, [220, 30, 40, 255], "the art");
  assert_eq!(window.get_pixel(50, 4).0, [0, 0, 0, 255], "top bar");
}

#[test]
fn the_pointer_over_a_bar_still_maps_to_screen_space() {
  let builder = EngineBuilder::new()
    .pixel_art(32, 32)
    .start_scene(Setup(Box::new(|_| {})));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.resize(100, 80);
  engine.mouse_move_window(10.0, 40.0);
  engine.step_frames(1).unwrap();
  let (x, y) = engine.world().resource::<Input>().mouse_position();
  assert_eq!((x, y), (-4.0, 16.0));
}

#[test]
fn a_minimised_window_skips_frames_and_comes_back() {
  let builder = EngineBuilder::new()
    .resolution(32, 32)
    .start_scene(Setup(Box::new(red_square_at(16.0, 16.0, 10))));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(1).unwrap();
  engine.resize(0, 0);
  engine.step_frames(3).unwrap();
  engine.resize(32, 32);
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(16, 16).0,
    [220, 30, 40, 255]
  );
}

#[test]
fn pad_and_wheel_helpers_reach_input() {
  #[derive(Default)]
  struct Watch;
  impl Scene for Watch {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(Watch))
  else {
    return;
  };
  engine.tap_pad(GamepadButton::North);
  engine.set_stick(Stick::Left, 0.0, 1.0);
  engine.wheel(0.0, 1.0);
  engine.step().unwrap();
  let input = engine.world().resource::<Input>();
  assert!(input.gamepad_button_down(GamepadButton::North));
  assert_eq!(input.left_stick_raw(), (0.0, 1.0));
  assert_eq!(
    input.last_device(),
    InputDevice::Mouse,
    "the wheel came last"
  );
  engine.step().unwrap();
  assert!(
    !engine
      .world()
      .resource::<Input>()
      .gamepad_button_down(GamepadButton::North),
    "a tap is released after one step"
  );
}

/// A menu whose button switches, with a fade, to a scene that records which
/// click asked for it
struct FadingMenu {
  clicks: u32,
  entered: std::rc::Rc<std::cell::Cell<u32>>,
}

/// Records, when it enters, which click of the menu made it
struct Entered {
  click: u32,
  entered: std::rc::Rc<std::cell::Cell<u32>>,
}

impl Scene for FadingMenu {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if ui
      .button("Go")
      .at(Rect::new(10.0, 10.0, 60.0, 30.0))
      .clicked()
    {
      self.clicks += 1;
      return SceneAction::SwitchWith(
        Box::new(Entered {
          click: self.clicks,
          entered: self.entered.clone(),
        }),
        Transition::fade(std::time::Duration::from_millis(300)),
      );
    }
    SceneAction::Continue
  }
}

impl Scene for Entered {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    self.entered.set(self.click);
  }
}

#[test]
fn a_click_during_a_fade_starts_no_second_switch() {
  let entered = std::rc::Rc::new(std::cell::Cell::new(0));
  let Some(mut engine) = engine(FadingMenu {
    clicks: 0,
    entered: entered.clone(),
  }) else {
    return;
  };
  let click = |engine: &mut HeadlessEngine| {
    engine.mouse_move(40.0, 25.0);
    engine.press_mouse(MouseButton::Left);
    engine.step().unwrap();
    engine.release_mouse(MouseButton::Left);
    engine.step().unwrap();
  };
  engine.step_frames(2).unwrap();
  // The switch starts: 300 ms fading out (18 frames at 60 Hz), then in
  click(&mut engine);
  // Clicked again while it fades out and the menu still shows
  click(&mut engine);
  engine.step_frames(60).unwrap();
  assert_eq!(entered.get(), 1, "the first click's switch, not a second");
}

#[test]
fn ui_focus_can_be_read_and_set_headlessly() {
  struct Menu;
  impl Scene for Menu {
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      ui.column().focus_scope().show(|ui| {
        ui.button("A").default_focus().show();
        ui.button("B").show();
      });
      SceneAction::Continue
    }
  }
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(200, 150).start_scene(Menu))
  else {
    return;
  };
  engine.step_frames(3).unwrap();
  let targets = engine.ui_focusables();
  assert_eq!(targets.len(), 2);
  assert_eq!(engine.ui_focused(), Some(targets[0].0));
  engine.focus_ui(targets[1].0);
  engine.step_frames(1).unwrap();
  assert_eq!(engine.ui_focused(), Some(targets[1].0));
}

#[test]
fn a_lower_scenes_button_ignores_the_pointer() {
  use std::sync::{Arc, Mutex};
  type Clicks = Arc<Mutex<u32>>;

  struct Lower {
    clicks: Clicks,
    pushed: bool,
  }
  impl Scene for Lower {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      if self.pushed {
        return SceneAction::Continue;
      }
      self.pushed = true;
      SceneAction::Push(Box::new(Upper))
    }
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      if ui
        .button("Under")
        .at(Rect::new(10.0, 10.0, 80.0, 30.0))
        .clicked()
        && let Ok(mut n) = self.clicks.lock()
      {
        *n += 1;
      }
      SceneAction::Continue
    }
  }
  /// Transparent, so the scene under it still draws; P pops it
  struct Upper;
  impl Scene for Upper {
    fn is_transparent(&self) -> bool {
      true
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      if ctx.input.key_pressed(KeyCode::KeyP) {
        SceneAction::Pop
      } else {
        SceneAction::Continue
      }
    }
  }
  fn click(engine: &mut HeadlessEngine) {
    engine.mouse_move(20.0, 20.0);
    engine.press_mouse(MouseButton::Left);
    engine.step().unwrap();
    engine.release_mouse(MouseButton::Left);
    engine.step_frames(2).unwrap();
  }

  let clicks = Clicks::default();
  let lower = Lower {
    clicks: clicks.clone(),
    pushed: false,
  };
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(200, 150).start_scene(lower))
  else {
    return;
  };
  engine.step_frames(3).unwrap();
  click(&mut engine);
  click(&mut engine);
  assert_eq!(
    *clicks.lock().unwrap(),
    0,
    "the scene above has the pointer"
  );
  engine.tap_key(KeyCode::KeyP);
  engine.step_frames(3).unwrap();
  click(&mut engine);
  assert_eq!(*clicks.lock().unwrap(), 1, "on top again, the button works");
}

/// What a wheel scene draws over its left half
#[derive(Clone, Copy)]
enum WheelOver {
  /// A panel with a label in it
  Panel,
  /// A modal, its dim over the whole UI
  Modal,
  /// A scroll list of 30 rows at its top, so a wheel up cannot move it
  ListAtItsTop,
}

/// Draws `over` in the left half of a 200 x 150 UI, nothing in the right
/// half; gameplay adds up the wheel it sees in `update`
struct Wheel {
  over: WheelOver,
  seen: std::sync::Arc<std::sync::Mutex<f32>>,
}

impl Scene for Wheel {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let (_, y) = ctx.input.scroll_delta();
    if let Ok(mut seen) = self.seen.lock() {
      *seen += y.abs();
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let half = Rect::new(0.0, 0.0, 100.0, 150.0);
    match self.over {
      WheelOver::Panel => {
        ui.panel().at(half).show(|ui| {
          ui.text("A panel");
        });
      }
      WheelOver::Modal => {
        ui.modal().show(|ui| {
          ui.text("A modal");
        });
      }
      WheelOver::ListAtItsTop => {
        ui.scroll().at(half).show(|ui| {
          for i in 0..30 {
            ui.button(format!("Row {i}")).id(i).fill_width().show();
          }
        });
      }
    }
    SceneAction::Continue
  }
}

/// How much wheel gameplay saw after one line of wheel up with the pointer
/// at `(x, y)` over `over`
fn wheel_seen(over: WheelOver, x: f32, y: f32) -> Option<f32> {
  let seen = std::sync::Arc::new(std::sync::Mutex::new(0.0));
  let scene = Wheel {
    over,
    seen: std::sync::Arc::clone(&seen),
  };
  let mut engine =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(200, 150).start_scene(scene))?;
  engine.mouse_move(x, y);
  engine.step_frames(3).unwrap();
  engine.wheel(0.0, 1.0);
  engine.step_frames(2).unwrap();
  let seen = *seen.lock().unwrap();
  Some(seen)
}

#[test]
fn the_wheel_over_a_panel_or_a_modal_never_reaches_gameplay() {
  for (over, what) in [(WheelOver::Panel, "panel"), (WheelOver::Modal, "modal")] {
    let Some(seen) = wheel_seen(over, 50.0, 75.0) else {
      return;
    };
    assert_eq!(seen, 0.0, "the wheel over the {what} is the UI's");
  }
}

#[test]
fn the_wheel_over_a_list_at_its_edge_never_reaches_gameplay() {
  let Some(seen) = wheel_seen(WheelOver::ListAtItsTop, 50.0, 75.0) else {
    return;
  };
  assert_eq!(
    seen, 0.0,
    "the list does not move, and gameplay sees nothing"
  );
}

#[test]
fn the_wheel_over_no_ui_reaches_gameplay() {
  for over in [WheelOver::Panel, WheelOver::ListAtItsTop] {
    let Some(seen) = wheel_seen(over, 150.0, 75.0) else {
      return;
    };
    assert_eq!(seen, 1.0, "the wheel beside the UI is the game's");
  }
}
