//! Every page of the ui demo with UI render retention on and off: two
//! engines driven by the same input must draw the same pixels every frame,
//! and the UI renderer draws each page in one or two draws

use demo_kit::prelude::*;
use ryo2d::testing::PixelPair;

/// The demo's pages (`ui_demo::demo`)
const PAGES: usize = 17;

/// The Clipping & Modals page, whose Enter opens a modal
const CLIPPING: usize = 6;

/// Most UI draws a page may make in a frame: the UI renderer draws once per
/// texture run, and every page's text sits on one glyph page
const MAX_DRAWS: u32 = 2;

/// Open page `page`, leave it still, move the focus, point at and scroll
/// over its widgets, press one and, on the modal page, close the modal; the frames compared, kept and
/// the texts reused, and the most UI draws a frame made
fn run(page: usize) -> Option<(usize, usize, usize, u32)> {
  let mut pair = PixelPair::new(|| ui_demo::demo().start_page(page).engine_builder())?;
  let what = |step: &str| format!("page {page}, {step}");
  pair.frames(20, &what("opening"));
  pair.frames(10, &what("still"));
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowRight] {
    pair.each(|engine| engine.tap_key(key));
    pair.frames(3, &what("moving the focus"));
  }
  let targets = pair.on.ui_focusables();
  for (_, rect) in targets.iter().take(3) {
    let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &what("pointing"));
  }
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(15, &what("scrolling"));
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
      "the modal opened"
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
  Some((
    pair.compared(),
    pair.kept(),
    pair.reused(),
    pair.most_draws(),
  ))
}

#[test]
fn every_page_looks_the_same_retained() {
  let (mut compared, mut kept, mut reused) = (0, 0, 0);
  for page in 0..PAGES {
    let Some((c, k, r, draws)) = run(page) else {
      return;
    };
    eprintln!(
      "page {page}: {c} frames compared, {k} kept, {r} texts reused, at most {draws} UI draws"
    );
    assert!(
      (1..=MAX_DRAWS).contains(&draws),
      "page {page}: {draws} UI draws"
    );
    (compared, kept, reused) = (compared + c, kept + k, reused + r);
  }
  assert!(compared > 0);
  assert!(kept > 0, "some unchanged frame kept the UI image");
  assert!(reused > 0, "some changing frame reused texts");
}
