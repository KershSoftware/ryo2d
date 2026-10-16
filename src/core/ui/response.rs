//! What a widget or container reports back when it is shown, and what a
//! widget senses of the pointer

use glam::Vec2;

use super::{Rect, UiId};

/// What a shown widget or container reports: its id, where it is and what
/// the pointer did to it this frame.
///
/// A top-level container or widget is laid out when it is shown, so its
/// `rect` is this frame's. Inside another container the layout is not done
/// yet, so `rect` is where it was last frame (empty in its first frame).
///
/// The pointer is resolved against what was drawn last frame, and only the
/// top scene's widgets report it: a click lands on the widget the player saw
/// under the pointer.
///
/// ```ignore
/// if ui.button("Play").show().clicked() {
///     return SceneAction::Switch(Box::new(Level::default()));
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct Response {
  /// Its id, made from where it was called (or its `.id(key)`)
  pub id: UiId,
  /// Its rect in UI units
  pub rect: Rect,
  pub(crate) states: u16,
  pub(crate) drag_delta: Vec2,
  pub(crate) pointer: Option<Vec2>,
}

impl Response {
  pub(crate) const HOVERED: u16 = 1 << 0;
  pub(crate) const PRESSED: u16 = 1 << 1;
  pub(crate) const CLICKED: u16 = 1 << 2;
  pub(crate) const SECONDARY: u16 = 1 << 3;
  pub(crate) const DOUBLE: u16 = 1 << 4;
  pub(crate) const DRAG_STARTED: u16 = 1 << 5;
  pub(crate) const DRAGGING: u16 = 1 << 6;
  pub(crate) const DRAG_RELEASED: u16 = 1 << 7;
  pub(crate) const CHANGED: u16 = 1 << 8;
  pub(crate) const FOCUSED: u16 = 1 << 9;
  pub(crate) const GAINED_FOCUS: u16 = 1 << 10;
  pub(crate) const LOST_FOCUS: u16 = 1 << 11;
  pub(crate) const SUBMITTED: u16 = 1 << 12;

  fn is(&self, state: u16) -> bool {
    self.states & state != 0
  }

  /// The pointer is over it, and nothing drawn over it takes the pointer
  pub fn hovered(&self) -> bool {
    self.is(Self::HOVERED)
  }

  /// A press that started on it is still held
  pub fn pressed(&self) -> bool {
    self.is(Self::PRESSED)
  }

  /// The left button was pressed on it and released over it this frame
  pub fn clicked(&self) -> bool {
    self.is(Self::CLICKED)
  }

  /// The right button was pressed on it and released over it this frame
  pub fn secondary_clicked(&self) -> bool {
    self.is(Self::SECONDARY)
  }

  /// This frame's click was its second within 0.3 s (`clicked` is true too)
  pub fn double_clicked(&self) -> bool {
    self.is(Self::DOUBLE)
  }

  /// A press on it moved past the drag threshold this frame
  pub fn drag_started(&self) -> bool {
    self.is(Self::DRAG_STARTED)
  }

  /// It is being dragged (its press moved past the drag threshold and is
  /// still held)
  pub fn dragging(&self) -> bool {
    self.is(Self::DRAGGING)
  }

  /// Its drag ended this frame
  pub fn drag_released(&self) -> bool {
    self.is(Self::DRAG_RELEASED)
  }

  /// How far the pointer moved this frame while dragging it, in UI units
  pub fn drag_delta(&self) -> Vec2 {
    self.drag_delta
  }

  /// Its value changed this frame (value widgets: sliders, checkboxes, ...)
  pub fn changed(&self) -> bool {
    self.is(Self::CHANGED)
  }

  /// It has the keyboard and gamepad focus
  pub fn focused(&self) -> bool {
    self.is(Self::FOCUSED)
  }

  /// It took the focus this frame
  pub fn gained_focus(&self) -> bool {
    self.is(Self::GAINED_FOCUS)
  }

  /// It lost the focus this frame
  pub fn lost_focus(&self) -> bool {
    self.is(Self::LOST_FOCUS)
  }

  /// It was submitted this frame (Enter in a text field)
  pub fn submitted(&self) -> bool {
    self.is(Self::SUBMITTED)
  }

  /// The pointer in UI units while this widget is hot or active
  pub fn pointer(&self) -> Option<Vec2> {
    self.pointer
  }

  /// Its value changed this frame
  #[allow(dead_code)] // Used from Task 10 (value widgets)
  pub(crate) fn mark_changed(&mut self) {
    self.states |= Self::CHANGED;
  }

  /// It was submitted this frame
  #[allow(dead_code)] // Used from Task 11 (the text field)
  pub(crate) fn mark_submitted(&mut self) {
    self.states |= Self::SUBMITTED;
  }
}

/// What a widget takes from the pointer: hovering only, clicks, drags or
/// both. What it does not sense, its [`Response`] never reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sense {
  pub(crate) click: bool,
  pub(crate) drag: bool,
}

impl Sense {
  /// Neither clicks nor drags: the pointer passes through to what is under it
  pub fn hover() -> Sense {
    Sense {
      click: false,
      drag: false,
    }
  }

  /// Clicks (left, right and double)
  pub fn click() -> Sense {
    Sense {
      click: true,
      drag: false,
    }
  }

  /// Drags
  pub fn drag() -> Sense {
    Sense {
      click: false,
      drag: true,
    }
  }

  /// Clicks and drags
  pub fn click_and_drag() -> Sense {
    Sense {
      click: true,
      drag: true,
    }
  }
}
