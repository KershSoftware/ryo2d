//! The engine scene that hosts one page plus the shell

use std::rc::Rc;

use ryo2d::prelude::*;

use crate::chrome::{self, View};
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
  rows: Vec<Row>,
  left: HoldRepeat,
  right: HoldRepeat,
  /// A widget had keyboard focus on the last UI frame
  keyboard_wanted: bool,
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
      rows: Vec::new(),
      left: HoldRepeat::default(),
      right: HoldRepeat::default(),
      keyboard_wanted: false,
    }
  }

  /// A fresh scene for page `index`; the engine's scene cleanup runs between
  fn switch_to(&self, index: usize) -> SceneAction {
    SceneAction::Switch(Box::new(PageScene::new(self.def.clone(), index)))
  }

  /// Handle the shell's keys; returns the adjustment steps for the panel, or
  /// a scene action to return instead of updating the page
  fn shell_keys(&mut self, ctx: &mut SceneContext) -> Result<i32, SceneAction> {
    let dt = ctx.time.fixed_dt;
    let input = &mut *ctx.input;

    if input.key_pressed_and_consume(KeyCode::KeyH) {
      self.help_open = !self.help_open;
    }
    if input.key_pressed_and_consume(KeyCode::Escape) {
      if !self.help_open {
        return Err(SceneAction::Quit);
      }
      self.help_open = false;
    }

    let count = self.def.pages.len();
    // `|` so both aliases are consumed
    let next = input.key_pressed_and_consume(KeyCode::BracketRight)
      | input.key_pressed_and_consume(KeyCode::PageDown);
    let prev = input.key_pressed_and_consume(KeyCode::BracketLeft)
      | input.key_pressed_and_consume(KeyCode::PageUp);
    if next {
      return Err(self.switch_to((self.index + 1) % count));
    }
    if prev {
      return Err(self.switch_to((self.index + count - 1) % count));
    }
    if input.key_pressed_and_consume(KeyCode::KeyR) {
      return Err(self.switch_to(self.index));
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

    let mut settings = Settings::new(self.selected, adjust);
    self.page.settings(&mut settings);
    self.rows = settings.into_rows();
    self.selected = self.selected.min(self.rows.len().saturating_sub(1));

    self.page.update(ctx, dt);
    SceneAction::Continue
  }

  fn render(&mut self, ctx: &mut SceneContext, alpha: f32) {
    self.page.render(ctx, alpha);
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    self.page.render_ui(ui);
    self.keyboard_wanted = ui.wants_keyboard();
    let controls = self.page.controls();
    let view = View {
      title: &self.def.title,
      index: self.index,
      count: self.def.pages.len(),
      page_name: self.page.name(),
      description: self.page.description(),
      rows: &self.rows,
      selected: self.selected,
      controls: &controls,
      help_open: self.help_open,
    };
    chrome::draw(ui, &view);
    SceneAction::Continue
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    self.page.render_gpu(ctx);
  }
}
