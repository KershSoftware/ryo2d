//! Every page of the ui demo with UI fragments on and with the node cache
//! off (a full rebuild), at UI scales 1 and 1.25, and five pages at 2: two
//! engines driven by the same input, wheel and trackpad scrolling and
//! scrollbar drags among it, must draw the same pixels every frame, and the
//! UI renderer draws each page in one or two draws. The demo draws its UI
//! on its pixel-art target, where a scroll slot's contents are placed on
//! the CPU and never kept, so every page runs at 1.25 again with its UI at
//! the resolution of a window twice the game's, where the shader moves them
//! and their runs are copied. The debug spot check stays on in the engine
//! with fragments: what it captures afresh must equal what was kept, and it
//! is not counted as a copy.

use demo_kit::prelude::*;
use ryo2d::testing::PixelPair;

/// The demo's pages (`ui_demo::demo`)
const PAGES: usize = 17;

/// The Settings page, and the same screen built from a UI file: their
/// lists fit their views, so their scroll areas never move
const SETTINGS: usize = 1;
const FILE_SETTINGS: usize = 12;

/// The Layout page, whose column scrolls where the UI is small enough
const LAYOUT: usize = 5;

/// The Clipping & Modals page, whose Enter opens a modal
const CLIPPING: usize = 6;

/// The Scroll Views page: two lists and a horizontal strip
const SCROLL_VIEWS: usize = 9;

/// The File Dialogue page: a conversation whose lines move the nodes around
/// them
const FILE_DIALOGUE: usize = 14;

/// The Big list page: a closure list beside a node list
const BIG_LIST: usize = 16;

/// The pages whose scroll area must scroll at every scale they run
const SCROLLS: [usize; 3] = [CLIPPING, SCROLL_VIEWS, BIG_LIST];

/// The game's size: a whole multiple of it is a window the pixel-art target
/// is scaled into
const GAME: (u32, u32) = (1024, 768);

/// Most UI draws a page may make in a frame: the UI renderer draws once per
/// texture run, and every page's text sits on one glyph page
const MAX_DRAWS: u32 = 2;

/// How a run draws the demo: its UI scale (`None`: the default), and the
/// window's multiple of the game's size its UI is drawn at (`None`: the
/// demo's own, the UI on the pixel-art target)
#[derive(Debug, Clone, Copy)]
struct Look {
  scale: Option<f32>,
  window: Option<u32>,
}

impl Look {
  const fn at(scale: Option<f32>) -> Look {
    Look {
      scale,
      window: None,
    }
  }

  /// The two engines on page `page`
  fn pair(self, page: usize) -> Option<PixelPair> {
    let mut pair = PixelPair::fragments(move || {
      let builder = ui_demo::demo().start_page(page).engine_builder();
      let builder = builder.ui_scale(self.scale);
      match self.window {
        Some(_) => builder.ui_resolution(UiResolution::Window),
        None => builder,
      }
    })?;
    if let Some(k) = self.window {
      pair.each(|engine| engine.resize(GAME.0 * k, GAME.1 * k));
    }
    Some(pair)
  }

  /// The centre of `rect` (UI units) in screen units
  fn centre(self, rect: Rect) -> (f32, f32) {
    self.point(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
  }

  /// The point `(x, y)` (UI units) in screen units: a UI unit is the
  /// scale's render pixels, and a screen unit the window's multiple of them
  fn point(self, x: f32, y: f32) -> (f32, f32) {
    let window = self.window.map_or(1.0, |k| k as f32);
    let screen = self.scale.unwrap_or(window) / window;
    (x * screen, y * screen)
  }
}

/// The scroll slot of the area the page's wheel and trackpad steps point
/// at: for the Big list, the node list's
fn scroll_area(page: usize) -> Option<usize> {
  match page {
    SETTINGS | LAYOUT | CLIPPING | SCROLL_VIEWS | FILE_SETTINGS => Some(1),
    BIG_LIST => Some(2),
    _ => None,
  }
}

/// What a page's run saw
struct Seen {
  compared: usize,
  dragged: usize,
  copied: usize,
  resolved_again: usize,
  slot_runs_copied: usize,
  draws: u32,
}

/// Open page `page` in `look`, leave it still, move the focus, point at its
/// widgets, turn the wheel and move the trackpad over its scroll area (on
/// the pages that have one), drag each scrollbar's thumb, advance the File
/// Dialogue page's conversation, press a widget and, on the modal page,
/// close the modal
fn run(page: usize, look: Look) -> Option<Seen> {
  let mut pair = look.pair(page)?;
  let what = |step: &str| format!("page {page} in {look:?}, {step}");
  pair.frames(20, &what("opening"));
  pair.frames(10, &what("still"));
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowRight] {
    pair.each(|engine| engine.tap_key(key));
    pair.frames(3, &what("moving the focus"));
  }
  let targets = pair.on.ui_focusables();
  for &(_, rect) in targets.iter().take(3) {
    let (x, y) = look.centre(rect);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &what("pointing"));
  }
  let area = scroll_area(page);
  if let Some(slot) = area {
    let slots = pair.on.ui_scroll_slots();
    let Some(&(Some(clip), _)) = slots.get(slot) else {
      panic!("{}: no scroll slot {slot} in {slots:?}", what("scrolling"));
    };
    let (x, y) = look.centre(clip);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &what("over a scroll area"));
  }
  let still = pair.on.ui_scroll_slots();
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(15, &what("scrolling"));
  let wheeled = pair.on.ui_scroll_slots();
  pair.each(|engine| engine.trackpad(0.0, -7.3));
  pair.frames(10, &what("trackpad scrolling"));
  let moved = pair.on.ui_scroll_slots();
  if let Some(slot) = area.filter(|_| SCROLLS.contains(&page)) {
    let offset = |slots: &[(Option<Rect>, [i32; 2])]| slots.get(slot).map(|&(_, offset)| offset);
    assert_ne!(
      offset(&wheeled),
      offset(&still),
      "{}: the wheel moved scroll slot {slot}",
      what("scrolling")
    );
    assert_ne!(
      offset(&moved),
      offset(&wheeled),
      "{}: the trackpad moved scroll slot {slot}",
      what("trackpad scrolling")
    );
  }
  // Drag each scrollbar's thumb: small steps, then a jump to the track's
  // end and back a third of it, which moves a long list by many views a
  // frame, past every row its interest held
  let dragged = drag_thumbs(&mut pair, look, &what("dragging a scrollbar"));
  if SCROLLS.contains(&page) {
    assert!(
      dragged > 0,
      "{}: a thumb to drag",
      what("dragging a scrollbar")
    );
    assert_ne!(
      pair.on.ui_scroll_slots(),
      moved,
      "{}: the thumbs scrolled",
      what("dragging a scrollbar")
    );
  }
  // Each line of the conversation is a text of its own size: the panel
  // around it, anchored at the bottom, moves the nodes kept around it
  if page == FILE_DIALOGUE {
    for _ in 0..4 {
      pair.each(|engine| engine.tap_key(KeyCode::Enter));
      pair.frames(20, &what("advancing the conversation"));
    }
  }
  // The earlier steps moved the focus off the Show Modal Dialog button (the
  // first widget in the page's right-hand column), so give it back
  if page == CLIPPING {
    let button = targets.iter().find(|(_, rect)| rect.x == 420.0);
    let Some(&(id, _)) = button else {
      panic!("the Show Modal Dialog button is not reachable: {targets:?}");
    };
    pair.each(|engine| engine.focus_ui(id));
    pair.frames(2, &what("focusing the modal's button"));
  }
  pair.each(|engine| engine.tap_key(KeyCode::Enter));
  pair.frames(5, &what("pressing"));
  if page == CLIPPING {
    assert!(
      pair
        .on
        .ui_snapshot()
        .lines()
        .any(|l| l.starts_with("modals ") && l != "modals []"),
      "{}: the modal opened",
      what("pressing")
    );
  }
  // Escape quits the demo from a page; only the Clipping & Modals page has
  // something to back out of (the modal its default-focused button opened)
  if page == CLIPPING {
    pair.each(|engine| engine.tap_key(KeyCode::Escape));
    pair.frames(10, &what("closing the modal"));
  } else {
    pair.frames(10, &what("settling"));
  }
  Some(Seen {
    compared: pair.compared(),
    dragged,
    copied: pair.fragments_reused(),
    resolved_again: pair.resolved_again(),
    slot_runs_copied: pair.slot_runs_copied(),
    draws: pair.most_draws(),
  })
}

/// Drag the thumb of each scrollbar the page shows, one after another, and
/// let it go; the bars dragged
fn drag_thumbs(pair: &mut PixelPair, look: Look, what: &str) -> usize {
  let mut dragged = 0;
  // A drag can scroll another area's bar out of view (a strip in a list):
  // the bars are read again before each
  while let Some(&(_, axis, track, thumb)) = pair.on.ui_scrollbars().get(dragged) {
    let (start, end) = if axis == 1 {
      (track.y, track.y + track.height)
    } else {
      (track.x, track.x + track.width)
    };
    let across = if axis == 1 {
      thumb.x + thumb.width / 2.0
    } else {
      thumb.y + thumb.height / 2.0
    };
    let along = if axis == 1 {
      thumb.y + thumb.height / 2.0
    } else {
      thumb.x + thumb.width / 2.0
    };
    let point = |along: f32| {
      let along = along.clamp(start + 1.0, end - 1.0);
      let (x, y) = if axis == 1 {
        (across, along)
      } else {
        (along, across)
      };
      look.point(x, y)
    };
    let (x, y) = point(along);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(1, &format!("{what}, over thumb {dragged}"));
    pair.each(|engine| engine.press_mouse(MouseButton::Left));
    pair.frames(1, &format!("{what}, pressing thumb {dragged}"));
    let third = (end - start) / 3.0;
    for to in [along + 2.0, along + 4.0, along + 8.0, end, end - third] {
      let (x, y) = point(to);
      pair.each(|engine| engine.mouse_move(x, y));
      pair.frames(1, &format!("{what}, dragging thumb {dragged}"));
    }
    pair.frames(2, &format!("{what}, holding thumb {dragged}"));
    pair.each(|engine| engine.release_mouse(MouseButton::Left));
    pair.frames(3, &format!("{what}, letting thumb {dragged} go"));
    dragged += 1;
  }
  dragged
}

/// Run each of `pages` in `look`: every page draws in at most `MAX_DRAWS`,
/// and the run copied fragments and resolved kept ones again; what it saw
/// over the pages
fn every_page(pages: &[usize], look: Look) -> Option<Seen> {
  let mut all = Seen {
    compared: 0,
    dragged: 0,
    copied: 0,
    resolved_again: 0,
    slot_runs_copied: 0,
    draws: 0,
  };
  for &page in pages {
    let seen = run(page, look)?;
    eprintln!(
      "page {page} in {look:?}: {} frames compared, {} scrollbars dragged, {} fragments copied, \
       {} resolved again, {} runs copied in scroll slots, at most {} UI draws",
      seen.compared,
      seen.dragged,
      seen.copied,
      seen.resolved_again,
      seen.slot_runs_copied,
      seen.draws
    );
    assert!(
      (1..=MAX_DRAWS).contains(&seen.draws),
      "page {page} in {look:?}: {} UI draws",
      seen.draws
    );
    all.compared += seen.compared;
    all.dragged += seen.dragged;
    all.copied += seen.copied;
    all.resolved_again += seen.resolved_again;
    all.slot_runs_copied += seen.slot_runs_copied;
  }
  eprintln!(
    "{look:?}: {} frames compared, {} scrollbars dragged, {} fragments copied, {} resolved \
     again, {} runs copied in scroll slots",
    all.compared, all.dragged, all.copied, all.resolved_again, all.slot_runs_copied
  );
  assert!(all.compared > 0);
  assert!(all.copied > 0, "{look:?}: some frame copied fragments");
  assert!(
    all.resolved_again > 0,
    "{look:?}: some frame resolved a kept fragment again"
  );
  Some(all)
}

#[test]
fn every_page_draws_as_a_rebuild_with_fragments() {
  let pages: Vec<usize> = (0..PAGES).collect();
  every_page(&pages, Look::at(None));
}

#[test]
fn every_page_draws_as_a_rebuild_with_fragments_at_1_25() {
  let pages: Vec<usize> = (0..PAGES).collect();
  every_page(&pages, Look::at(Some(1.25)));
}

#[test]
fn every_page_draws_as_a_rebuild_with_fragments_at_1_25_off_the_target() {
  let pages: Vec<usize> = (0..PAGES).collect();
  let look = Look {
    scale: Some(1.25),
    window: Some(2),
  };
  let Some(seen) = every_page(&pages, look) else {
    return;
  };
  assert!(
    seen.slot_runs_copied > 0,
    "off the pixel-art target, runs in the shader's scroll slots were copied"
  );
}

#[test]
fn five_pages_draw_as_a_rebuild_with_fragments_at_2() {
  let pages = [SETTINGS, CLIPPING, FILE_SETTINGS, FILE_DIALOGUE, BIG_LIST];
  every_page(&pages, Look::at(Some(2.0)));
}

/// The node list's rows (the right-hand list's), top to bottom, in UI
/// units
fn node_rows(pair: &PixelPair) -> Vec<Rect> {
  let targets = pair.on.ui_focusables();
  let middle = targets
    .iter()
    .map(|(_, rect)| rect.x + rect.width)
    .fold(0.0, f32::max)
    / 2.0;
  let mut rows: Vec<Rect> = targets
    .into_iter()
    .map(|(_, rect)| rect)
    .filter(|rect| rect.x > middle && rect.height > 20.0)
    .collect();
  rows.sort_by(|a, b| a.y.total_cmp(&b.y));
  rows
}

#[test]
fn hover_across_a_trackpad_scrolled_list_at_1_25() {
  // The Big list page at UI scale 1.25, as the demo draws it (the UI on the
  // pixel-art target, where a scroll slot's contents are placed on the
  // CPU) and at the resolution of a window twice the game's (where the
  // shader moves them): the node list scrolled to a fractional offset by
  // the trackpad, then the pointer moved across eight of its rows, one row
  // a frame
  for window in [None, Some(2)] {
    let look = Look {
      scale: Some(1.25),
      window,
    };
    let Some(mut pair) = look.pair(BIG_LIST) else {
      return;
    };
    let what = |step: &str| format!("{look:?}, {step}");
    pair.frames(20, &what("opening"));
    let rows = node_rows(&pair);
    assert!(
      rows.len() > 8,
      "{}: the node list's rows {rows:?}",
      what("opened")
    );
    let (x, y) = look.centre(rows[1]);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &what("over the list"));
    // 11.7 logical pixels in all: the list rests between render pixels,
    // drawn at the whole pixel nearest
    for dy in [-3.7, -5.1, -2.9] {
      pair.each(|engine| engine.trackpad(0.0, dy));
      pair.frames(1, &what("trackpad scrolling"));
    }
    pair.frames(10, &what("scrolled"));
    let scrolled = node_rows(&pair);
    assert!(
      scrolled[1].y < rows[1].y,
      "{}: the trackpad scrolled the list",
      what("scrolled")
    );
    let first = pair.fragments_reused();
    for row in scrolled.iter().skip(1).take(8) {
      let (x, y) = look.centre(*row);
      pair.each(|engine| engine.mouse_move(x, y));
      pair.frames(1, &what("hovering a row"));
    }
    pair.frames(5, &what("over the last row"));
    assert!(
      pair.fragments_reused() > first,
      "{}: the rows not hovered were copied",
      what("hovering")
    );
    if window.is_some() {
      assert!(
        pair.slot_runs_copied() > 0,
        "{}: the list's runs were copied in its scroll slot",
        what("hovering")
      );
    }
  }
}
