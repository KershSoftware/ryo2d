//! The engine scene that hosts one page plus the shell

use std::rc::Rc;

use ryo2d::prelude::*;

use crate::chrome::{self, View};
use crate::controls::reserved_key;
use crate::demo::DemoDef;
use crate::page::DemoPage;
use crate::settings::{Row, Settings};

/// One page of a demo, with the shell's state
pub(crate) struct PageScene {
  def: Rc<DemoDef>,
  index: usize,
  page: Box<dyn DemoPage>,
  help_open: bool,
  selected: usize,
  /// The first settings row the panel showed on the last UI frame
  scroll: usize,
  rows: Vec<Row>,
  /// A widget had keyboard focus on the last UI frame (the shell ignores its
  /// keys while one does)
  keyboard_wanted: bool,
  /// Where the settings panel was drawn on the last UI frame
  panel: Option<Rect>,
  /// The settings panel's focusable widget on the last UI frame
  panel_id: Option<UiId>,
}

impl PageScene {
  pub(crate) fn new(def: Rc<DemoDef>, index: usize) -> Self {
    let page = (def.pages[index])();
    Self {
      def,
      index,
      page,
      help_open: false,
      selected: 0,
      scroll: 0,
      rows: Vec::new(),
      keyboard_wanted: false,
      panel: None,
      panel_id: None,
    }
  }

  /// A fresh scene for page `index`; the engine's scene cleanup runs between
  fn switch_to(&self, index: usize) -> SceneAction {
    SceneAction::Switch(Box::new(PageScene::new(self.def.clone(), index)))
  }

  /// A fresh scene for this page with the same row selected and in view
  fn reset(&self) -> SceneAction {
    let mut scene = PageScene::new(self.def.clone(), self.index);
    scene.selected = self.selected;
    scene.scroll = self.scroll;
    SceneAction::Switch(Box::new(scene))
  }

  /// Record the page's settings rows, applying `adjust` steps to the
  /// selected one
  fn read_rows(&mut self, adjust: i32) {
    let mut settings = Settings::new(self.selected, adjust);
    self.page.settings(&mut settings);
    self.rows = settings.into_rows();
    self.selected = self.selected.min(self.rows.len().saturating_sub(1));
  }

  /// Handle the shell's keys, or return a scene action to return instead of
  /// drawing the page. The keys it acts on are consumed, so neither the
  /// page's widgets nor its update see them.
  fn shell_keys(&mut self, ui: &mut UiContext) -> Result<(), SceneAction> {
    let help = take(ui, KeyCode::KeyH);
    // Escape closes help first; otherwise a page that wants it gets it
    let escape = (self.help_open || !self.page.wants_escape()) && take(ui, KeyCode::Escape);
    // `|` so both aliases are consumed
    let next = take(ui, KeyCode::BracketRight) | take(ui, KeyCode::PageDown);
    let prev = take(ui, KeyCode::BracketLeft) | take(ui, KeyCode::PageUp);
    let reset = take(ui, KeyCode::KeyR);
    // A key the shell acts on must not also be typed into a field, such as
    // one the next page focuses on entry
    if help || escape || next || prev || reset {
      ui.input().consume_text();
    }

    if help {
      self.help_open = !self.help_open;
    }
    if escape {
      if !self.help_open {
        return Err(SceneAction::Quit);
      }
      self.help_open = false;
    }

    // One page has no other page to go to: its keys are still taken, but
    // switching to itself would reset it
    let count = self.def.pages.len();
    if next && count > 1 {
      return Err(self.switch_to((self.index + 1) % count));
    }
    if prev && count > 1 {
      return Err(self.switch_to((self.index + count - 1) % count));
    }
    if reset {
      return Err(self.reset());
    }
    Ok(())
  }
}

/// Whether `key` was pressed this frame, consuming it if so.
fn take(ui: &mut UiContext, key: KeyCode) -> bool {
  let pressed = ui.input().key_pressed(key);
  if pressed {
    ui.consume_key(key);
  }
  pressed
}

impl Scene for PageScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.page.enter(ctx);
    // The page's UI may draw before its first update: show its panel already
    self.read_rows(0);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    self.page.exit(ctx);
  }

  /// Pages also stop music, so one page's music can't play over the next
  fn cleanup_on_exit(&self) -> SceneCleanup {
    SceneCleanup::ALL
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // The page sees no input under help, and no pointer over the panel (both
    // as drawn by this frame's UI pass, which runs first)
    let input = &mut *ctx.input;
    let (mx, my) = input.mouse_position();
    if self.help_open {
      input.block_all();
    } else if self.panel.is_some_and(|panel| panel.contains(mx, my)) {
      input.block_pointer();
    }
    self.page.update(ctx, dt);
    ctx.input.unblock();
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    // Typing into a widget, or picking from an open list: the shell's keys
    // belong to it
    if !self.keyboard_wanted
      && let Err(action) = self.shell_keys(ui)
    {
      return action;
    }
    // The panel takes the arrows and the D-pad through UI focus, with the
    // UI's hold-to-repeat
    let last = self.rows.len().saturating_sub(1);
    let adjust = match self.panel_id.and_then(|id| ui.nav_step(id)) {
      Some(Direction::Down) => {
        self.selected = (self.selected + 1).min(last);
        0
      }
      Some(Direction::Up) => {
        self.selected = self.selected.saturating_sub(1);
        0
      }
      Some(Direction::Right) => 1,
      Some(Direction::Left) => -1,
      None => 0,
    };
    self.read_rows(adjust);

    // The legend's lines are measured first, so the page's content_area ends
    // above them all. Help is a modal over the page: its dim takes the
    // pointer from the page's widgets
    chrome::measure_legend(ui, &self.page.controls());
    self.page.ui(ui, ctx);
    self.keyboard_wanted = ui.wants_keyboard();
    let controls = self.page.controls();
    let wants_escape = self.page.wants_escape();
    // Startup checks each page's first controls; this catches ones that
    // change later, such as with a setting
    if cfg!(debug_assertions)
      && let Some(key) = reserved_key(&controls, wants_escape)
    {
      panic!(
        "demo \"{}\", page \"{}\": {key:?} is reserved by the demo shell",
        self.def.title,
        self.page.name()
      );
    }
    let view = View {
      title: &self.def.title,
      index: self.index,
      count: self.def.pages.len(),
      page_name: self.page.name(),
      description: self.page.description(),
      rows: &self.rows,
      selected: self.selected,
      scroll: self.scroll,
      controls: &controls,
      escape_quits: !wants_escape,
      help_open: self.help_open,
    };
    let drawn = chrome::draw(ui, &view);
    self.panel = drawn.panel;
    self.panel_id = drawn.panel_id;
    self.scroll = drawn.scroll;
    // A click on a row picks it from the next frame
    if let Some(row) = drawn.clicked_row {
      self.selected = row;
    }
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    self.page.render(gpu);
  }
}
