use std::sync::Arc;

use glam::Vec2;

use super::*;
use crate::core::asset::Assets;
use crate::core::graphics::{Image, no_images};
use crate::core::render::UiSpace;
use crate::core::text::{GlyphStyle, Icon, PlacedIcon, TextIcons, TextLayout};
use crate::core::ui::immediate::{FragmentRun, ScrollSlot, UiCommand, UiCommands};
use crate::core::ui::{Color, Rect, UiId};

fn env() -> UiRenderEnv {
  UiRenderEnv {
    space: UiSpace {
      size: Vec2::new(320.0, 240.0),
      px_scale: 1.0,
      image: (320, 240),
      mouse_scale: 1.0,
    },
    on_target: false,
    text: true,
    fonts: 1,
    bitmaps: 0,
  }
}

fn rect(x: f32) -> UiCommand {
  UiCommand::Rect {
    rect: Rect::new(x, 10.0, 50.0, 20.0),
    fill: Color::WHITE,
    radius: 4.0,
    border: None,
    clip: None,
    layer: 0,
  }
}

fn line(points: &[Vec2]) -> UiCommand {
  UiCommand::Line {
    points: points.into(),
    at: Vec2::ZERO,
    width: 2.0,
    color: Color::WHITE,
    closed: false,
    clip: None,
    layer: 0,
  }
}

fn text(layout: &Arc<TextLayout>) -> UiCommand {
  UiCommand::Text {
    layout: layout.clone(),
    origin: Vec2::new(8.0, 8.0),
    reveal: None,
    opacity: 1.0,
    seed: 3,
    clip: None,
    layer: 0,
  }
}

/// What the commands draw from: `images`, and `icons` by name
fn from<'a>(images: &'a Assets<Image>, icons: Option<&'a TextIcons>) -> StampSources<'a> {
  StampSources { images, icons }
}

/// No images and no icons
fn none() -> StampSources<'static> {
  from(no_images(), None)
}

fn commands(list: &[UiCommand]) -> UiCommands {
  let mut commands = UiCommands::default();
  for (order, command) in list.iter().enumerate() {
    commands.push(order as u32, 0, command.clone());
  }
  commands
}

/// A retainer whose image (epoch 7) holds `list`, drawn and presented
fn drawn(list: &[UiCommand]) -> UiRetain {
  drawn_from(list, none())
}

/// [`drawn`], `list` drawing from `sources`
fn drawn_from(list: &[UiCommand], sources: StampSources<'_>) -> UiRetain {
  let mut retain = UiRetain::default();
  assert!(
    !retain.keep(&commands(list), sources, env(), 7),
    "nothing drawn yet"
  );
  retain.presented(false, true, 7);
  retain
}

#[test]
fn the_same_commands_keep_the_image() {
  let layout = Arc::new(TextLayout::default());
  let list = [rect(0.0), text(&layout)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), none(), env(), 7));
  assert!(retain.keep(&commands(&list), none(), env(), 7), "and again");
}

#[test]
fn a_changed_number_redraws() {
  let mut retain = drawn(&[rect(0.0)]);
  assert!(!retain.keep(&commands(&[rect(0.5)]), none(), env(), 7));
}

#[test]
fn another_order_redraws() {
  let mut retain = drawn(&[rect(0.0)]);
  let mut moved = UiCommands::default();
  moved.push(1, 0, rect(0.0));
  assert!(!retain.keep(&moved, none(), env(), 7));
}

#[test]
fn an_equal_layout_elsewhere_is_another_text() {
  let mut retain = drawn(&[text(&Arc::new(TextLayout::default()))]);
  let other = Arc::new(TextLayout::default());
  assert!(!retain.keep(&commands(&[text(&other)]), none(), env(), 7));
}

#[test]
fn a_text_effect_never_keeps() {
  let layout = Arc::new(TextLayout {
    styles: vec![GlyphStyle {
      color: Color::WHITE,
      outline: None,
      shadow: None,
      glow: None,
      effect: Some("wave".to_string()),
    }],
    ..TextLayout::default()
  });
  assert!(layout.has_effects());
  let list = [text(&layout)];
  let mut retain = drawn(&list);
  for _ in 0..3 {
    assert!(!retain.keep(&commands(&list), none(), env(), 7));
    retain.presented(false, true, 7);
  }
}

#[test]
fn another_environment_or_image_redraws() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  let scaled = UiRenderEnv { fonts: 2, ..env() };
  assert!(
    !retain.keep(&commands(&list), none(), scaled, 7),
    "the fonts changed"
  );
  retain.presented(false, true, 7);
  assert!(retain.keep(&commands(&list), none(), scaled, 7));
  assert!(
    !retain.keep(&commands(&list), none(), scaled, 8),
    "a new UI image"
  );
}

#[test]
fn an_unpresented_frame_redraws() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), none(), env(), 7));
  retain.lost();
  assert!(!retain.keep(&commands(&list), none(), env(), 7));
}

#[test]
fn a_frame_whose_ui_pass_did_not_draw_redraws() {
  let list = [rect(0.0)];
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(&list), none(), env(), 7));
  retain.presented(false, false, 7);
  assert!(!retain.keep(&commands(&list), none(), env(), 7));
}

#[test]
fn a_kept_frame_leaves_the_image_as_drawn() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), none(), env(), 7));
  // The `ui` pass was disabled: the image is untouched, still the commands'
  retain.presented(true, false, 7);
  assert!(retain.keep(&commands(&list), none(), env(), 7));
}

#[test]
fn no_commands_never_keep() {
  let mut retain = drawn(&[]);
  assert!(!retain.keep(&UiCommands::default(), none(), env(), 7));
}

#[test]
fn a_sprite_updated_in_place_redraws() {
  let mut images = Assets::default();
  let sprite = images.add(Image::solid_rect(4, 4, 255, 0, 0, 255));
  let list = [UiCommand::Sprite {
    sprite: sprite.clone(),
    rect: Rect::new(0.0, 0.0, 4.0, 4.0),
    uv: [0.0, 0.0, 1.0, 1.0],
    tint: Color::WHITE,
    filter: Default::default(),
    clip: None,
    layer: 0,
  }];
  let mut retain = drawn_from(&list, from(&images, None));
  assert!(retain.keep(&commands(&list), from(&images, None), env(), 7));
  *images.get_mut(&sprite).unwrap() = Image::solid_rect(4, 4, 0, 0, 255, 255);
  assert!(!retain.keep(&commands(&list), from(&images, None), env(), 7));
}

#[test]
fn a_renamed_icon_redraws() {
  let mut images = Assets::default();
  let red = images.add(Image::solid_rect(4, 4, 255, 0, 0, 255));
  let blue = images.add(Image::solid_rect(4, 4, 0, 0, 255, 255));
  let mut icons = TextIcons::default();
  icons.insert("confirm", red);
  let layout = Arc::new(TextLayout {
    icons: vec![PlacedIcon {
      icon: Icon::Named {
        name: "confirm".to_string(),
        size: None,
      },
      rect: Rect::new(0.0, 0.0, 4.0, 4.0),
      cluster: 0,
    }],
    ..TextLayout::default()
  });
  let list = [text(&layout)];
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(&list), from(&images, Some(&icons)), env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&commands(&list), from(&images, Some(&icons)), env(), 7));
  icons.insert("confirm", blue);
  assert!(!retain.keep(&commands(&list), from(&images, Some(&icons)), env(), 7));
}

#[test]
fn lines_with_equal_points_keep_and_a_moved_point_redraws() {
  let points = [Vec2::new(1.0, 2.0), Vec2::new(30.0, 40.0)];
  let mut retain = drawn(&[line(&points)]);
  // Another allocation holding the same points, as a widget makes each frame
  assert!(retain.keep(&commands(&[line(&points)]), none(), env(), 7));
  let moved = [Vec2::new(1.0, 2.0), Vec2::new(30.0, 41.0)];
  assert!(!retain.keep(&commands(&[line(&moved)]), none(), env(), 7));
}

#[test]
fn a_frame_whose_only_change_is_a_slot_offset_is_not_kept() {
  let mut retain = UiRetain::default();
  let clip = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
  let mut commands = UiCommands::default();
  commands.clear();
  let slot = commands.push_slot(ScrollSlot {
    offset: [0, -10],
    clip,
    cull: None,
  });
  commands.push(0, slot, rect(0.0));
  assert!(
    !retain.keep(&commands, none(), env(), 7),
    "the first frame is drawn"
  );
  retain.presented(false, true, 7);
  commands.clear();
  let slot = commands.push_slot(ScrollSlot {
    offset: [0, -11],
    clip,
    cull: None,
  });
  commands.push(0, slot, rect(0.0));
  assert!(
    !retain.keep(&commands, none(), env(), 7),
    "a moved slot moves pixels"
  );
  retain.presented(false, true, 7);
  assert!(
    retain.keep(&commands, none(), env(), 7),
    "unchanged, it is kept"
  );
}

#[test]
fn a_draw_moved_to_another_slot_is_not_kept() {
  let clip = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
  let frame = |slot_of_rect: bool| {
    let mut commands = UiCommands::default();
    let slot = commands.push_slot(ScrollSlot {
      offset: [0, 0],
      clip,
      cull: None,
    });
    commands.push(0, if slot_of_rect { slot } else { 0 }, rect(0.0));
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(false), none(), env(), 7));
  retain.presented(false, true, 7);
  assert!(
    !retain.keep(&frame(true), none(), env(), 7),
    "the same command in a slot draws elsewhere"
  );
}

/// Where a fragment's run of two rects goes in a frame
#[derive(Clone, Copy)]
struct Placed {
  id: u64,
  version: u64,
  /// Its second rect's x
  x: f32,
  /// Its root's order
  base: u32,
  /// Its slot (the table holds one besides slot 0)
  slot: u16,
  /// The loose rects before it (the rest go after it, of the frame's two)
  before: usize,
}

const PLACED: Placed = Placed {
  id: 7,
  version: 1,
  x: 20.0,
  base: 10,
  slot: 0,
  before: 0,
};

/// A frame of two loose rects and a fragment's run placed by `at`
fn run_frame(at: Placed) -> UiCommands {
  let mut commands = UiCommands::default();
  commands.clear();
  commands.push_slot(ScrollSlot {
    offset: [0, 0],
    clip: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
    cull: None,
  });
  let loose = |commands: &mut UiCommands, k: usize| commands.push(k as u32, 0, rect(0.0));
  (0..at.before).for_each(|k| loose(&mut commands, k));
  let start = commands.as_slice().len() as u32;
  commands.push(at.base, at.slot, rect(0.0));
  commands.push(at.base + 1, at.slot, rect(at.x));
  commands.push_run(FragmentRun {
    id: UiId(at.id),
    version: at.version,
    base: at.base,
    layer: 0,
    slot: at.slot,
    draws: start..start + 2,
    bounds: None,
  });
  (at.before..2).for_each(|k| loose(&mut commands, k));
  commands
}

#[test]
fn an_unchanged_run_is_kept_without_comparing_its_commands() {
  let mut retain = UiRetain::default();
  let frame = |version: u64| run_frame(Placed { version, ..PLACED });
  assert!(
    !retain.keep(&frame(1), none(), env(), 7),
    "drawn the first time"
  );
  retain.presented(false, true, 7);
  assert!(
    retain.keep(&frame(1), none(), env(), 7),
    "the same run is kept"
  );
  // Its tag vouches for its commands: while its version is the same, its
  // fragment pushed the same ones
  let other = run_frame(Placed { x: 25.0, ..PLACED });
  assert!(
    retain.keep(&other, none(), env(), 7),
    "a run's commands are not compared"
  );
  assert!(
    !retain.keep(&frame(2), none(), env(), 7),
    "a new version is drawn"
  );
}

#[test]
fn a_run_with_another_tag_or_place_redraws() {
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&run_frame(PLACED), none(), env(), 7));
  retain.presented(false, true, 7);
  for (at, what) in [
    (Placed { id: 8, ..PLACED }, "another fragment"),
    (Placed { base: 11, ..PLACED }, "another order"),
    (Placed { slot: 1, ..PLACED }, "another slot"),
    (
      Placed {
        before: 1,
        ..PLACED
      },
      "another place in the list",
    ),
  ] {
    assert!(!retain.keep(&run_frame(at), none(), env(), 7), "{what}");
    retain.presented(false, true, 7);
    assert!(
      retain.keep(&run_frame(at), none(), env(), 7),
      "{what}, again"
    );
    // Back to the first frame
    assert!(!retain.keep(&run_frame(PLACED), none(), env(), 7));
    retain.presented(false, true, 7);
  }
}

#[test]
fn draws_beside_a_run_are_compared_one_by_one() {
  let frame = |x: f32| {
    let mut commands = UiCommands::default();
    commands.push(0, 0, rect(0.0));
    commands.push(10, 0, rect(0.0));
    commands.push(11, 0, rect(20.0));
    commands.push_run(FragmentRun {
      id: UiId(7),
      version: 1,
      base: 10,
      layer: 0,
      slot: 0,
      draws: 1..3,
      bounds: None,
    });
    commands.push(12, 0, rect(x));
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(0.0), none(), env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(0.0), none(), env(), 7));
  assert!(
    !retain.keep(&frame(0.5), none(), env(), 7),
    "a loose draw changed"
  );
}

#[test]
fn a_sprite_in_a_run_updated_in_place_redraws() {
  let mut images = Assets::default();
  let sprite = images.add(Image::solid_rect(4, 4, 255, 0, 0, 255));
  let frame = |x: f32| {
    let mut commands = UiCommands::default();
    commands.push(0, 0, rect(x));
    commands.push(10, 0, rect(0.0));
    commands.push(
      11,
      0,
      UiCommand::Sprite {
        sprite: sprite.clone(),
        rect: Rect::new(0.0, 0.0, 4.0, 4.0),
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: Color::WHITE,
        filter: Default::default(),
        clip: None,
        layer: 0,
      },
    );
    commands.push_run(FragmentRun {
      id: UiId(7),
      version: 1,
      base: 10,
      layer: 0,
      slot: 0,
      draws: 1..3,
      bounds: None,
    });
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(0.0), from(&images, None), env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(0.0), from(&images, None), env(), 7));
  *images.get_mut(&sprite).unwrap() = Image::solid_rect(4, 4, 0, 0, 255, 255);
  assert!(
    !retain.keep(&frame(0.0), from(&images, None), env(), 7),
    "the run's sprite changed"
  );
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(0.0), from(&images, None), env(), 7));
  // Drawn again for a loose draw, the run (seen before with its version)
  // keeps watching its sprite
  assert!(!retain.keep(&frame(1.0), from(&images, None), env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(1.0), from(&images, None), env(), 7));
  *images.get_mut(&sprite).unwrap() = Image::solid_rect(4, 4, 0, 255, 0, 255);
  assert!(
    !retain.keep(&frame(1.0), from(&images, None), env(), 7),
    "the run's sprite changed again"
  );
}

#[test]
fn a_run_with_a_text_effect_never_keeps() {
  let layout = Arc::new(TextLayout {
    styles: vec![GlyphStyle {
      color: Color::WHITE,
      outline: None,
      shadow: None,
      glow: None,
      effect: Some("wave".to_string()),
    }],
    ..TextLayout::default()
  });
  let frame = || {
    let mut commands = UiCommands::default();
    commands.push(10, 0, rect(0.0));
    commands.push(11, 0, text(&layout));
    commands.push_run(FragmentRun {
      id: UiId(7),
      version: 1,
      base: 10,
      layer: 0,
      slot: 0,
      draws: 0..2,
      bounds: None,
    });
    commands
  };
  let mut retain = UiRetain::default();
  for _ in 0..3 {
    assert!(!retain.keep(&frame(), none(), env(), 7));
    retain.presented(false, true, 7);
  }
}

/// The fragment runs' kept instances (`UiRunCache`), apart from building
mod runs {
  use bytemuck::Zeroable;

  use super::*;
  use crate::core::graphics::{GlyphPage, TextureId};
  use crate::core::render::ui_renderer::{TexNeed, UiInstance};
  use crate::core::text::GlyphFormat;

  const PAGE: GlyphPage = GlyphPage {
    format: GlyphFormat::Mask,
    index: 2,
  };

  /// Run 7 at `version`, its one draw in slot 0
  fn tag(version: u64) -> FragmentRun {
    FragmentRun {
      id: UiId(7),
      version,
      base: 0,
      layer: 0,
      slot: 0,
      draws: 0..1,
      bounds: None,
    }
  }

  /// Keep run 7 as built at `version` in `env`: a glyph on `PAGE` (at
  /// generation 5) and a sprite of texture 3 (its command's stamp 9)
  fn keep(cache: &mut UiRunCache, version: u64, env: UiRenderEnv, animated: bool) {
    let Some(mut kept) = cache.spare() else {
      return;
    };
    kept.version = version;
    kept.env = env;
    kept.instances.push(UiInstance::zeroed());
    kept.needs.push(TexNeed::Glyph(PAGE));
    kept.resident.push((0, TextureId::for_test(3)));
    kept.pages.push((PAGE, 5));
    kept.stamps.push((0, 9));
    kept.animated = animated;
    cache.store(UiId(7), kept);
  }

  /// A cache that kept run 7 at version 1 in the frame before
  fn holding(animated: bool) -> UiRunCache {
    let mut cache = UiRunCache::default();
    cache.begin(env(), true);
    keep(&mut cache, 1, env(), animated);
    cache.end();
    cache.begin(env(), true);
    cache
  }

  /// Whether the cache gives run 7 at `version`, drawn in `slot`, with
  /// `PAGE` at generation `page` (`None`: no such page), texture 3 live or
  /// not and its command's stamp `stamp`
  fn found(
    cache: &mut UiRunCache,
    version: u64,
    slot: u8,
    page: Option<u64>,
    live: bool,
    stamp: u64,
  ) -> bool {
    cache
      .get(&tag(version), slot, |_| page, |_| live, |_| stamp)
      .is_some()
  }

  #[test]
  fn a_kept_run_is_copied_while_it_is_the_same() {
    let mut cache = UiRunCache::default();
    cache.begin(env(), true);
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9), "nothing kept");
    keep(&mut cache, 1, env(), false);
    cache.end();
    cache.begin(env(), true);
    let kept = cache
      .get(&tag(1), 0, |_| Some(5), |_| true, |_| 9)
      .expect("the same run");
    assert_eq!(kept.instances.len(), 1);
    assert_eq!(cache.stats(), (1, 0));
    assert_eq!(cache.slot_copied(), 0, "copied into slot 0");
    // Drawn once a frame: a second run with its id is built
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9));
    cache.end();
    cache.begin(env(), true);
    assert!(found(&mut cache, 1, 0, Some(5), true, 9), "and again");
  }

  /// What run 7 is asked with (see `found`)
  struct Asked {
    version: u64,
    slot: u8,
    page: Option<u64>,
    live: bool,
    stamp: u64,
  }

  /// Run 7 asked as it was kept
  const SAME: Asked = Asked {
    version: 1,
    slot: 0,
    page: Some(5),
    live: true,
    stamp: 9,
  };

  #[test]
  fn a_kept_run_is_built_again_when_anything_it_was_built_from_changed() {
    let cases = [
      ("a new version", Asked { version: 2, ..SAME }),
      (
        "its page cleared",
        Asked {
          page: Some(6),
          ..SAME
        },
      ),
      ("its page gone", Asked { page: None, ..SAME }),
      (
        "its texture forgotten",
        Asked {
          live: false,
          ..SAME
        },
      ),
      ("a sprite changed in place", Asked { stamp: 10, ..SAME }),
      (
        "built in slot 0, drawn in a slot",
        Asked { slot: 4, ..SAME },
      ),
    ];
    for (what, asked) in cases {
      let mut cache = holding(false);
      let Asked {
        version,
        slot,
        page,
        live,
        stamp,
      } = asked;
      assert!(
        !found(&mut cache, version, slot, page, live, stamp),
        "{what}"
      );
    }
    let mut cache = holding(false);
    assert!(found(&mut cache, 1, 0, Some(5), true, 9), "the same");
    // A text with an effect reads the clock
    let mut cache = holding(true);
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9), "animated");
  }

  #[test]
  fn a_run_built_in_a_slot_is_copied_into_another_slot_but_not_slot_0() {
    let mut cache = UiRunCache::default();
    cache.begin(env(), true);
    let mut kept = cache.spare().expect("retention on");
    kept.version = 1;
    kept.slot = 3;
    cache.store(UiId(7), kept);
    cache.end();
    cache.begin(env(), true);
    assert!(
      found(&mut cache, 1, 5, Some(5), true, 9),
      "slot indices are per frame"
    );
    assert_eq!(cache.slot_copied(), 1, "copied into a shader slot");
    cache.end();
    cache.begin(env(), true);
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9));
  }

  #[test]
  fn a_run_not_drawn_in_a_frame_or_built_in_another_environment_is_dropped() {
    let mut cache = holding(false);
    cache.end();
    cache.begin(env(), true);
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9), "not drawn");

    let mut cache = holding(false);
    cache.end();
    let scaled = UiRenderEnv {
      space: UiSpace {
        px_scale: 2.0,
        ..env().space
      },
      ..env()
    };
    cache.begin(scaled, true);
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9), "another env");
    // Kept in one environment, asked in another (as `begin` never lets)
    let mut cache = UiRunCache::default();
    cache.begin(env(), true);
    keep(&mut cache, 1, scaled, false);
    cache.end();
    cache.begin(env(), true);
    assert!(
      !found(&mut cache, 1, 0, Some(5), true, 9),
      "built elsewhere"
    );
  }

  #[test]
  fn with_retention_off_nothing_is_kept() {
    let mut cache = holding(false);
    cache.end();
    cache.begin(env(), false);
    assert!(cache.spare().is_none(), "no run is kept");
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9));
    keep(&mut cache, 1, env(), false);
    cache.end();
    cache.begin(env(), true);
    assert!(!found(&mut cache, 1, 0, Some(5), true, 9), "cleared");
  }
}

#[test]
fn held_runs_are_found_after_many_leave() {
  // Forty runs, then the last twenty of them: a list's top rows leaving
  // its interest at once, more of them than the look-ahead holds. Every run
  // left is found among those held before, and none is looked through again
  let frame = |from: u32| {
    let mut commands = UiCommands::default();
    for k in from..40 {
      let at = commands.position();
      commands.push(k, 0, rect(k as f32));
      commands.push_run(FragmentRun {
        id: UiId(u64::from(k) + 1),
        version: u64::from(k) + 1,
        base: k,
        layer: 0,
        slot: 0,
        draws: at..at + 1,
        bounds: None,
      });
    }
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(0), none(), env(), 1));
  assert_eq!(retain.unseen, 40, "every run is new");
  assert!(!retain.keep(&frame(20), none(), env(), 1));
  assert_eq!(retain.unseen, 0, "every run left was held before");
  // A new run among them, then the rest: found still
  let mut commands = frame(30);
  let at = commands.position();
  commands.push(50, 0, rect(50.0));
  commands.push_run(FragmentRun {
    id: UiId(99),
    version: 99,
    base: 50,
    layer: 0,
    slot: 0,
    draws: at..at + 1,
    bounds: None,
  });
  assert!(!retain.keep(&commands, none(), env(), 1));
  assert_eq!(retain.unseen, 1, "only the new run");
}

#[test]
fn held_runs_are_found_in_linear_time_among_new_ones() {
  // Three hundred runs, then each of them followed by a new one (rows with
  // a timer on every other one): the new runs are looked for past the
  // look-ahead in all within as many runs as were held, so a hold's lookups
  // stay linear in its runs
  let run = |commands: &mut UiCommands, id: u64, k: u32| {
    let at = commands.position();
    commands.push(k, 0, rect(k as f32));
    commands.push_run(FragmentRun {
      id: UiId(id),
      version: id,
      base: k,
      layer: 0,
      slot: 0,
      draws: at..at + 1,
      bounds: None,
    });
  };
  let mut held = UiCommands::default();
  for k in 0..300 {
    run(&mut held, u64::from(k) + 1, 2 * k);
  }
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&held, none(), env(), 1));
  let mut mixed = UiCommands::default();
  for k in 0..300 {
    run(&mut mixed, u64::from(k) + 1, 2 * k);
    run(&mut mixed, u64::from(k) + 1001, 2 * k + 1);
  }
  assert!(!retain.keep(&mixed, none(), env(), 1));
  assert_eq!(retain.unseen, 300, "only the new runs");
  let visited = retain.far_visited.get();
  assert!(visited <= 300, "{visited} runs visited past the look-ahead");
}
