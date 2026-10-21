//! Every page of the ui demo on the old UI path and the UI renderer: two
//! engines driven by the same input must draw the same pixels every frame

use demo_kit::prelude::*;
use ryo2d::testing::PixelPair;

/// The demo's pages (`ui_demo::demo`)
const PAGES: usize = 17;

/// The Clipping & Modals page, whose Enter opens a modal
const CLIPPING: usize = 6;

/// Run `n` frames of the pair, noting the most draws the new path made in a
/// frame it drew
fn frames(pair: &mut PixelPair, most: &mut u32, n: u32, what: &str) {
  for _ in 0..n {
    pair.frames(1, what);
    *most = (*most).max(pair.b().ui_draw_count());
  }
}

/// Most draws a page may make: its distinct textures (the pages show the
/// one UI atlas) plus one
const MAX_DRAWS: u32 = 2;

/// Open page `page`, leave it still, move the focus, point at and scroll
/// over its widgets, press one and, on the modal page, close the modal; the frames compared, kept and
/// the texts reused
fn run(page: usize) -> Option<(usize, u32)> {
  let mut pair = PixelPair::between(
    || ui_demo::demo().start_page(page).engine_builder(),
    |b| b.ui_dedicated(false),
    |b| b.ui_dedicated(true),
  )?;
  let mut most = 0;
  let what = |step: &str| format!("page {page}, {step}");
  frames(&mut pair, &mut most, 20, &what("opening"));
  frames(&mut pair, &mut most, 10, &what("still"));
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowRight] {
    pair.each(|engine| engine.tap_key(key));
    frames(&mut pair, &mut most, 3, &what("moving the focus"));
  }
  let targets = pair.on.ui_focusables();
  for (_, rect) in targets.iter().take(3) {
    let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    pair.each(|engine| engine.mouse_move(x, y));
    frames(&mut pair, &mut most, 3, &what("pointing"));
  }
  pair.each(|engine| engine.wheel(0.0, -3.0));
  frames(&mut pair, &mut most, 15, &what("scrolling"));
  // The earlier steps moved the focus off the Show Modal Dialog button (the
  // first widget in the page's right-hand column), so give it back
  if page == CLIPPING {
    let button = targets.iter().find(|(_, rect)| rect.x == 420.0);
    let Some(&(id, _)) = button else {
      panic!("the Show Modal Dialog button is not reachable: {targets:?}");
    };
    pair.each(|engine| engine.focus_ui(id));
    frames(
      &mut pair,
      &mut most,
      2,
      &what("focusing the modal's button"),
    );
  }
  pair.each(|engine| engine.tap_key(KeyCode::Enter));
  frames(&mut pair, &mut most, 5, &what("pressing"));
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
    frames(&mut pair, &mut most, 10, &what("closing the modal"));
  } else {
    frames(&mut pair, &mut most, 10, &what("settling"));
  }
  Some((pair.compared(), most))
}

#[test]
fn every_page_looks_the_same_on_the_ui_renderer() {
  let mut compared = 0;
  for page in 0..PAGES {
    let Some((c, draws)) = run(page) else {
      return;
    };
    eprintln!("page {page}: {c} frames compared, at most {draws} UI draws");
    assert!(
      (1..=MAX_DRAWS).contains(&draws),
      "page {page}: {draws} UI draws"
    );
    compared += c;
  }
  assert!(compared > 0);
}
