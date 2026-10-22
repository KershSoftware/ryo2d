//! Every page of the ui demo with UI fragments on and with the node cache
//! off (a full rebuild), at UI scales 1 and 1.25, and four pages at 2: two
//! engines driven by the same input, wheel and trackpad scrolling among it,
//! must draw the same pixels every frame, and the UI renderer draws each
//! page in one or two draws. The demo draws its UI on its pixel-art target,
//! where a scroll slot's contents are placed on the CPU and never kept, so
//! every page runs at 1.25 at the window's resolution too, where the shader
//! moves them and their runs are copied. The debug spot check stays on in
//! the engine with fragments: what it captures afresh must equal what was
//! kept, and it is not counted as a copy.

use demo_kit::prelude::*;
use ryo2d::testing::PixelPair;

/// The demo's pages (`ui_demo::demo`)
const PAGES: usize = 17;

/// The Settings page, and the same screen built from a UI file
const SETTINGS: usize = 1;
const FILE_SETTINGS: usize = 12;

/// The Clipping & Modals page, whose Enter opens a modal
const CLIPPING: usize = 6;

/// The Big list page: a closure list beside a node list
const BIG_LIST: usize = 16;

/// Most UI draws a page may make in a frame: the UI renderer draws once per
/// texture run, and every page's text sits on one glyph page
const MAX_DRAWS: u32 = 2;

/// The two engines on page `page` at UI scale `scale` (`None`: the
/// default), the UI drawn at `resolution`
fn pair(page: usize, scale: Option<f32>, resolution: UiResolution) -> Option<PixelPair> {
  PixelPair::fragments(move || {
    ui_demo::demo()
      .start_page(page)
      .engine_builder()
      .ui_scale(scale)
      .ui_resolution(resolution)
  })
}

/// The centre of `rect` (UI units) in screen units, at UI scale `scale`
fn centre(rect: Rect, scale: f32) -> (f32, f32) {
  (
    (rect.x + rect.width / 2.0) * scale,
    (rect.y + rect.height / 2.0) * scale,
  )
}

/// Open page `page` at UI scale `scale` and `resolution`, leave it still,
/// move the focus, point at its widgets, turn the wheel and move the
/// trackpad over them, press one and, on the modal page, close the modal;
/// the frames compared and the fragments copied, and the most UI draws a
/// frame made
fn run(page: usize, scale: Option<f32>, resolution: UiResolution) -> Option<(usize, usize, u32)> {
  let mut pair = pair(page, scale, resolution)?;
  let units = scale.unwrap_or(1.0);
  let what = |step: &str| format!("page {page} at scale {units} ({resolution:?}), {step}");
  pair.frames(20, &what("opening"));
  pair.frames(10, &what("still"));
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowRight] {
    pair.each(|engine| engine.tap_key(key));
    pair.frames(3, &what("moving the focus"));
  }
  let targets = pair.on.ui_focusables();
  for &(_, rect) in targets.iter().take(3) {
    let (x, y) = centre(rect, units);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &what("pointing"));
  }
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(15, &what("scrolling"));
  pair.each(|engine| engine.trackpad(0.0, -7.3));
  pair.frames(10, &what("trackpad scrolling"));
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
  Some((pair.compared(), pair.fragments_reused(), pair.most_draws()))
}

/// Run each of `pages` at UI scale `scale` and `resolution`: every page
/// draws in at most `MAX_DRAWS`, and the run copied fragments
fn every_page(pages: &[usize], scale: Option<f32>, resolution: UiResolution) {
  let (mut compared, mut reused) = (0, 0);
  for &page in pages {
    let Some((c, r, draws)) = run(page, scale, resolution) else {
      return;
    };
    eprintln!(
      "page {page} at scale {scale:?} ({resolution:?}): {c} frames compared, \
       {r} fragments copied, at most {draws} UI draws"
    );
    assert!(
      (1..=MAX_DRAWS).contains(&draws),
      "page {page} at scale {scale:?} ({resolution:?}): {draws} UI draws"
    );
    (compared, reused) = (compared + c, reused + r);
  }
  eprintln!(
    "scale {scale:?} ({resolution:?}): {compared} frames compared, {reused} fragments copied"
  );
  assert!(compared > 0);
  assert!(
    reused > 0,
    "scale {scale:?} ({resolution:?}): some frame copied fragments"
  );
}

#[test]
fn every_page_draws_as_a_rebuild_with_fragments() {
  let pages: Vec<usize> = (0..PAGES).collect();
  every_page(&pages, None, UiResolution::Render);
}

#[test]
fn every_page_draws_as_a_rebuild_with_fragments_at_1_25() {
  let pages: Vec<usize> = (0..PAGES).collect();
  every_page(&pages, Some(1.25), UiResolution::Render);
}

#[test]
fn every_page_draws_as_a_rebuild_with_fragments_at_1_25_off_the_target() {
  let pages: Vec<usize> = (0..PAGES).collect();
  every_page(&pages, Some(1.25), UiResolution::Window);
}

#[test]
fn four_pages_draw_as_a_rebuild_with_fragments_at_2() {
  let pages = [SETTINGS, CLIPPING, FILE_SETTINGS, BIG_LIST];
  every_page(&pages, Some(2.0), UiResolution::Render);
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
  // CPU) and at the window's resolution (where the shader moves them): the
  // node list scrolled to a fractional offset by the trackpad, then the
  // pointer moved across eight of its rows, one row a frame
  for resolution in [UiResolution::Render, UiResolution::Window] {
    let scale = 1.25;
    let make = move || {
      ui_demo::demo()
        .start_page(BIG_LIST)
        .engine_builder()
        .ui_scale(Some(scale))
        .ui_resolution(resolution)
    };
    let Some(mut pair) = PixelPair::fragments(make) else {
      return;
    };
    let what = |step: &str| format!("{resolution:?}, {step}");
    pair.frames(20, &what("opening"));
    let rows = node_rows(&pair);
    assert!(
      rows.len() > 8,
      "{}: the node list's rows {rows:?}",
      what("opened")
    );
    let (x, y) = centre(rows[1], scale);
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
      let (x, y) = centre(*row, scale);
      pair.each(|engine| engine.mouse_move(x, y));
      pair.frames(1, &what("hovering a row"));
    }
    pair.frames(5, &what("over the last row"));
    assert!(
      pair.fragments_reused() > first,
      "{}: the rows not hovered were copied",
      what("hovering")
    );
  }
}
