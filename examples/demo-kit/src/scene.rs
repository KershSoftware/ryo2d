//! The engine scene that hosts one page plus the shell

use std::rc::Rc;

use ryo2d::prelude::*;

use crate::chrome::{self, View};
use crate::controls::reserved_key;
use crate::demo::DemoDef;
use crate::hold::HoldRepeat;
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
  left: HoldRepeat,
  right: HoldRepeat,
  /// A widget had keyboard focus on the last UI frame
  keyboard_wanted: bool,
  /// Where the settings panel was drawn on the last UI frame
  panel: Option<Rect>,
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
      left: HoldRepeat::default(),
      right: HoldRepeat::default(),
      keyboard_wanted: false,
      panel: None,
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

  /// Handle the shell's keys; returns the adjustment steps for the panel, or
  /// a scene action to return instead of updating the page
  fn shell_keys(&mut self, ctx: &mut SceneContext) -> Result<i32, SceneAction> {
    let dt = ctx.time.fixed_dt;
    let input = &mut *ctx.input;

    let help = input.key_pressed_and_consume(KeyCode::KeyH);
    let escape = input.key_pressed_and_consume(KeyCode::Escape);
    // `|` so both aliases are consumed
    let next = input.key_pressed_and_consume(KeyCode::BracketRight)
      | input.key_pressed_and_consume(KeyCode::PageDown);
    let prev = input.key_pressed_and_consume(KeyCode::BracketLeft)
      | input.key_pressed_and_consume(KeyCode::PageUp);
    let reset = input.key_pressed_and_consume(KeyCode::KeyR);
    // A key the shell acts on must not also be typed into a field, such as
    // one the next page focuses on entry
    if help || escape || next || prev || reset {
      input.consume_text_input();
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

    let count = self.def.pages.len();
    if next {
      return Err(self.switch_to((self.index + 1) % count));
    }
    if prev {
      return Err(self.switch_to((self.index + count - 1) % count));
    }
    if reset {
      return Err(self.reset());
    }

    if self.help_open {
      // Help is modal: arrows change neither selection nor values
      self.left = HoldRepeat::default();
      self.right = HoldRepeat::default();
      return Ok(0);
    }
    if input.key_pressed_and_consume(KeyCode::ArrowDown) && !self.rows.is_empty() {
      self.selected = (self.selected + 1).min(self.rows.len() - 1);
    }
    if input.key_pressed_and_consume(KeyCode::ArrowUp) {
      self.selected = self.selected.saturating_sub(1);
    }
    let right_pressed = input.key_pressed_and_consume(KeyCode::ArrowRight);
    let left_pressed = input.key_pressed_and_consume(KeyCode::ArrowLeft);
    let right = self
      .right
      .steps(right_pressed, input.key_down(KeyCode::ArrowRight), dt);
    let left = self
      .left
      .steps(left_pressed, input.key_down(KeyCode::ArrowLeft), dt);
    Ok(right as i32 - left as i32)
  }
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
    let adjust = if self.keyboard_wanted {
      // Typing into a widget: the shell's keys belong to it
      self.left = HoldRepeat::default();
      self.right = HoldRepeat::default();
      0
    } else {
      match self.shell_keys(ctx) {
        Ok(adjust) => adjust,
        Err(action) => return action,
      }
    };

    self.read_rows(adjust);

    // The page sees no input under help, and no pointer over the panel
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

  fn render(&mut self, ctx: &mut SceneContext, alpha: f32) {
    self.page.render(ctx, alpha);
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    // Help covers the page: its widgets take no clicks or hover. The shell's
    // own chrome uses no mouse.
    if self.help_open {
      ui.set_mouse_occluded();
    }
    self.page.render_ui(ui);
    self.keyboard_wanted = ui.wants_keyboard();
    let controls = self.page.controls();
    // Startup checks each page's first controls; this catches ones that
    // change later, such as with a setting
    if cfg!(debug_assertions)
      && let Some(key) = reserved_key(&controls)
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
      help_open: self.help_open,
    };
    let drawn = chrome::draw(ui, &view);
    self.panel = drawn.panel;
    self.scroll = drawn.scroll;
    SceneAction::Continue
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    self.page.render_gpu(ctx);
  }
}
