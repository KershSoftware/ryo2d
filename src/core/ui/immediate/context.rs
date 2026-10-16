//! Immediate-mode UI drawing context
//!
//! The `UiContext` provides a simple API for drawing shapes, text, and
//! interactive elements. The engine builds one per frame for `Scene::ui`.
//! Drawing records [`UiCommand`]s in UI units; the engine's `ui` pass draws
//! them on the GPU, in the order they were recorded.
//!
//! # Clipping
//!
//! The UI context supports nested clip rects for scroll views and overflow handling.
//! Use `push_clip()` and `pop_clip()` to manage the clip stack:
//!
//! ```ignore
//! // Set up a scroll region
//! ui.push_clip(scroll_area);
//!
//! // Draw content - anything outside scroll_area is clipped
//! ui.draw_text("Item 1", 10.0, 10.0, Color::WHITE);
//! ui.draw_text("Item 2", 10.0, 30.0, Color::WHITE);
//!
//! // Restore previous clip state
//! ui.pop_clip();
//! ```

use std::cell::{RefCell, RefMut};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::Location;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::commands::{UiCommand, UiCommands};
use super::input::UiInput;

use crate::core::asset::SpriteHandle;
use crate::core::input::Direction;
use crate::core::render::UiSpace;
use crate::core::text::{
  RichText, TextAlign, TextEngine, TextLayout, TextLayoutOptions, TextOverflow, TextReveal,
  TextStyle,
};
use crate::core::ui::id::{UiId, key_hash};
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::layout::{Edges, Measure, UiStyle};
use crate::core::ui::memory::{Flags, UiMemory};
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::retained::{DropdownState, Focusable, TextInputState};
use crate::core::ui::theme::{Look, Skin, Theme, WidgetKind, WidgetState, warn_once};
use crate::core::ui::tree::{
  self, Draw, DrawKind, Node, RecordSpec, ResolveCx, Resolved, Subtree, TextLeaf, align_fraction,
};
use crate::core::ui::types::{Anchor, Color, Rect};
use crate::core::ui::widgets::common::WidgetOpts;

/// `pop_clip` on an empty stack was warned about
static EMPTY_POP_WARNED: AtomicBool = AtomicBool::new(false);
/// A container still open at the end of a scene's UI was warned about
static LEFT_OPEN_WARNED: AtomicBool = AtomicBool::new(false);
/// A container placed at a rect that is not finite was warned about
static BAD_AT_WARNED: AtomicBool = AtomicBool::new(false);

/// Pending dropdown overlay to be drawn at the end of the frame.
struct PendingDropdown {
  /// The trigger button rect
  trigger_rect: Rect,
  /// The dropdown list rect
  list_rect: Rect,
  /// Currently hovered index (for keyboard navigation)
  hover_index: usize,
  /// Options to display (stored as owned strings for lifetime management)
  options: Vec<String>,
}

/// A text's effect seed: a hash of the text, the style without its colours
/// and the layout options, so it stays the same whatever else is drawn
/// around it and when a hover changes its colour
pub(crate) fn text_seed(text: &RichText, style: &TextStyle, options: &TextLayoutOptions) -> u64 {
  let mut h = DefaultHasher::new();
  text.hash_into(&mut h);
  let mut style = style.clone();
  style.color = Color::WHITE;
  if let Some(outline) = &mut style.outline {
    outline.color = Color::WHITE;
  }
  if let Some(shadow) = &mut style.shadow {
    shadow.color = Color::WHITE;
  }
  if let Some(glow) = &mut style.glow {
    glow.color = Color::WHITE;
  }
  style.hash_into(&mut h);
  options.hash_into(&mut h);
  h.finish()
}

/// `rect` as layout can use it: a part that is not finite becomes 0 and a
/// negative size 0 (warned once)
fn clean_at(rect: Rect) -> Rect {
  let parts = [rect.x, rect.y, rect.width, rect.height];
  if parts.iter().all(|v| v.is_finite()) && rect.width >= 0.0 && rect.height >= 0.0 {
    return rect;
  }
  if !BAD_AT_WARNED.swap(true, Ordering::Relaxed) {
    crate::engine_warn!(module: "ui", "a UI container's at rect is not finite or has a negative size; using 0 there");
  }
  let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
  Rect::new(
    finite(rect.x),
    finite(rect.y),
    finite(rect.width).max(0.0),
    finite(rect.height).max(0.0),
  )
}

/// A text draw being set up with [`UiContext::text`]. `draw()` records it
/// and returns its size; a `UiText` dropped without `draw()` records it then.
///
/// Placed with [`at`](Self::at) it is drawn there. Without `at` it joins the
/// layout as a leaf: inside a container it takes its place among the
/// children and wraps to the width it gets; outside one it is placed in the
/// UI by its [`anchor`](Self::anchor).
pub struct UiText<'u, 'a> {
  ui: &'u mut UiContext<'a>,
  /// Where `ui.text(..)` was called (a leaf's id)
  site: &'static Location<'static>,
  text: RichText,
  at: Option<Vec2>,
  style: TextStyle,
  options: TextLayoutOptions,
  /// The clusters shown (`None`: no reveal) and the fade over them
  shown: Option<f32>,
  fade: f32,
  opacity: f32,
  /// A typewriter advanced by the UI's `dt` when the text is drawn
  typewriter: Option<&'u mut TextReveal>,
  /// Recorded already (by `draw`)
  recorded: bool,
}

impl<'u> UiText<'u, '_> {
  /// Draw the text here, in UI units (its top-left unless `anchor` says
  /// otherwise), outside the layout
  pub fn at(mut self, x: f32, y: f32) -> Self {
    self.at = Some(Vec2::new(x, y));
    self
  }
  /// The style (font, size, colour, effects); replaces earlier size and colour
  pub fn style(mut self, style: &TextStyle) -> Self {
    self.style = style.clone();
    self
  }
  /// The font size in UI units
  pub fn size(mut self, size: f32) -> Self {
    self.style.size = size;
    self
  }
  /// The base colour. Fade through [`opacity`](Self::opacity), not the
  /// colour's alpha: layouts are cached per colour.
  pub fn color(mut self, color: Color) -> Self {
    self.style.color = color;
    self
  }
  /// Wrap lines at `width` UI units (in a layout: at most `width`, or
  /// narrower when the text gets less room)
  pub fn wrap(mut self, width: f32) -> Self {
    self.options.wrap = Some(width);
    self
  }
  /// How each line sits in the block (needs a width from `wrap`)
  pub fn align(mut self, align: TextAlign) -> Self {
    self.options.align = align;
    self
  }
  /// Stop after this many lines
  pub fn max_lines(mut self, lines: u32) -> Self {
    self.options.max_lines = Some(lines);
    self
  }
  /// End a text cut by `max_lines` with an ellipsis
  pub fn ellipsis(mut self) -> Self {
    self.options.overflow = TextOverflow::Ellipsis;
    self
  }
  /// Which point of the block sits at `at` (default the top-left). In a
  /// layout: where the text sits in a stack or, outside any container, in
  /// the UI.
  pub fn anchor(mut self, anchor: Anchor) -> Self {
    self.options.anchor = anchor;
    self
  }
  /// Show the first `shown` character clusters (fractional while fading).
  ///
  /// With a [`fade`](Self::fade) wider than one cluster the last characters
  /// are still partly transparent when `shown` reaches the cluster count, so
  /// a typewriter must keep counting past the end to finish: pass
  /// [`TextReveal::drawn_clusters`](crate::core::text::TextReveal::drawn_clusters),
  /// or any value of at least the cluster count plus `fade`.
  /// A shown count that is not a number means no reveal.
  /// [`typewriter`](Self::typewriter) drives a `TextReveal` for you, pauses
  /// included.
  pub fn reveal(mut self, shown: f32) -> Self {
    self.shown = shown.is_finite().then_some(shown);
    self
  }
  /// Revealed characters fade in over this many clusters (a negative or
  /// non-numeric count means none; it needs a [`reveal`](Self::reveal))
  pub fn fade(mut self, clusters: f32) -> Self {
    self.fade = if clusters.is_finite() {
      clusters.max(0.0)
    } else {
      0.0
    };
    self
  }
  /// Fade the whole text, 0 (clear) to 1; clamped, and a non-numeric value
  /// is opaque
  pub fn opacity(mut self, opacity: f32) -> Self {
    self.opacity = if opacity.is_finite() {
      opacity.clamp(0.0, 1.0)
    } else {
      1.0
    };
    self
  }
  /// Reveal the text with `reveal`, the typewriter world text uses: when
  /// the text is drawn, `reveal` advances by the UI's real time
  /// ([`dt`](UiContext::dt)) over this text's layout, so `[pause]` markup
  /// and the punctuation pause hold it and a fade ends at full opacity. It
  /// replaces [`reveal`](Self::reveal) and [`fade`](Self::fade).
  ///
  /// Draw each reveal through one call a frame: every call advances it.
  /// It does not notice a new text, so call [`TextReveal::restart`] when the
  /// line changes. UI text writes no `TextRevealed` message: check
  /// [`TextReveal::is_done`] to know the line has finished.
  ///
  /// ```ignore
  /// ui.text(RichText::markup("Hello.[pause=0.5] Welcome!"))
  ///   .at(40.0, 420.0)
  ///   .wrap(400.0)
  ///   .typewriter(&mut self.reveal)
  ///   .draw();
  /// if self.reveal.is_done() { /* show the next-line prompt */ }
  /// ```
  pub fn typewriter(mut self, reveal: &'u mut TextReveal) -> Self {
    self.typewriter = Some(reveal);
    self
  }

  /// Record the text; its size in UI units (zero when it cannot be laid
  /// out). In a layout that is its laid-out size outside any container, and
  /// its size unwrapped inside one (laid out when the container closes).
  pub fn draw(mut self) -> Vec2 {
    self.record()
  }

  fn record(&mut self) -> Vec2 {
    self.recorded = true;
    let text = std::mem::take(&mut self.text);
    let style = std::mem::take(&mut self.style);
    let options = self.options;
    let typewriter = self.typewriter.take();
    let ui = &mut *self.ui;
    let scale = ui.space.px_scale;
    // Text that cannot be laid out records nothing and advances no reveal.
    // A leaf is laid out again at its width; its clusters stay the same.
    let Some(layout) = ui
      .engine()
      .and_then(|mut engine| engine.layout(&text, &style, &options, scale))
    else {
      return Vec2::ZERO;
    };
    let reveal = match typewriter {
      Some(reveal) => {
        // Checked first, as world text does: a settled reveal stays put
        if !reveal.settled() {
          reveal.advance(ui.dt, &layout);
        }
        Some(reveal.drawn())
      }
      None => self.shown.map(|shown| (shown, self.fade)),
    };
    let size = layout.size / scale;
    let seed = text_seed(&text, &style, &options);
    match self.at {
      Some(at) => {
        let origin = at - options.anchor.fraction() * size;
        ui.record_layout(layout, origin, self.opacity, reveal, seed)
      }
      None => {
        let leaf = TextLeaf {
          text,
          style,
          options,
        };
        ui.text_leaf(self.site, leaf, (self.opacity, reveal, seed), size)
      }
    }
  }
}

impl Drop for UiText<'_, '_> {
  /// A text not drawn with `draw()` is drawn when it is dropped
  fn drop(&mut self) {
    if !self.recorded {
      self.record();
    }
  }
}

// ============================================================================
// UiContext - Immediate Mode Drawing
// ============================================================================

/// Context for immediate-mode UI rendering.
///
/// Provides methods for drawing shapes, text, and interactive elements.
/// The engine builds one per frame and passes it to `Scene::ui`.
/// Coordinates are UI units: with the automatic UI scale (the default) they
/// are screen units, whatever the display's scale factor.
///
/// # Example
///
/// ```ignore
/// fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
///     // Draw a simple label
///     ui.draw_text("Score: 100", 10.0, 10.0, Color::WHITE);
///
///     // Draw a button that returns true when clicked
///     if ui.button("Start").at(Rect::new(100.0, 100.0, 120.0, 40.0)).clicked() {
///         return SceneAction::Switch(Box::new(Level::default()));
///     }
///     SceneAction::Continue
/// }
/// ```
pub struct UiContext<'a> {
  /// Where the draws are recorded
  commands: &'a mut UiCommands,
  /// Text layout and measuring (a cell, so measuring works through `&self`)
  text: RefCell<&'a mut TextEngine>,
  /// UI units, the UI image and the scale between them
  space: UiSpace,
  /// The frame's input; widgets consume what they use
  input: UiInput,
  /// The theme widgets read, taken from the world at the start of the pass
  theme: Arc<Theme>,
  /// Stack of clip rects for nested clipping
  clip_stack: Vec<Rect>,
  /// Current effective clip rect (intersection of all stacked clips)
  current_clip: Option<Rect>,
  /// The layer draws are recorded on (higher layers draw over lower ones)
  layer: u8,
  /// Pending dropdown overlays to draw at end of frame
  pending_dropdowns: Vec<PendingDropdown>,
  /// ID of the currently focused dropdown (0 = none)
  focused_dropdown_id: u32,
  /// Whether a focused text input or an open dropdown was drawn this frame
  keyboard_focus: bool,
  /// Whether the mouse is occluded by an overlay (e.g. developer console).
  /// When true, `is_hovered`, `is_held`, and `is_clicked` all return false.
  mouse_occluded: bool,
  /// Real frame delta time (seconds) for UI animations (cursor blink, scroll momentum).
  /// Set by the engine from the actual frame time so render-phase animations stay
  /// correct regardless of the fixed-timestep update rate.
  dt: f32,
  /// Records, widget state and the id scopes, kept between frames
  pub(crate) memory: &'a mut UiMemory,
  /// Lays out each top-level container when it closes
  layout: &'a mut LayoutEngine,
  /// The top-level container (or lone leaf) being recorded
  subtree: Option<Subtree>,
  /// The innermost open node of `subtree`: what a new node goes in
  open: Option<usize>,
  /// The next draw's place in the painter's order
  order: u32,
  /// The scope id of the scene whose UI is being recorded
  scene: u64,
  /// Whether that scene is the top one
  top: bool,
}

/// The draw layer a scene's page records on
pub(crate) const PAGE_LAYER: u8 = 0;
/// The top scene's open dropdown lists, above every scene's page
pub(crate) const POPUP_LAYER: u8 = 1;
/// The engine's overlays and the console, above the popups. Layers past
/// 127 draw as 127 (`render::ui::ui_z`).
pub(crate) const OVERLAY_LAYER: u8 = 2;

impl<'a> UiContext<'a> {
  /// A context recording into `commands`, laying text out with `text`,
  /// remembering in `memory` and laying containers out with `layout`, over
  /// the UI `space`, reading `input` (its mouse converted to UI units).
  pub(crate) fn new(
    commands: &'a mut UiCommands,
    text: &'a mut TextEngine,
    memory: &'a mut UiMemory,
    layout: &'a mut LayoutEngine,
    space: UiSpace,
    input: UiInput,
    theme: Arc<Theme>,
  ) -> Self {
    Self {
      commands,
      text: RefCell::new(text),
      space,
      input: input.with_mouse_scale(space.mouse_scale),
      theme,
      clip_stack: Vec::new(),
      current_clip: None,
      layer: PAGE_LAYER,
      pending_dropdowns: Vec::new(),
      focused_dropdown_id: 0,
      keyboard_focus: false,
      mouse_occluded: false,
      dt: 0.0,
      memory,
      layout,
      subtree: None,
      open: None,
      order: 0,
      scene: 0,
      top: false,
    }
  }

  /// The UI theme this pass draws with (`ctx.ui_theme()` changes it, from
  /// the next frame).
  pub fn theme(&self) -> &Theme {
    &self.theme
  }

  /// The theme, shared: for a widget that records while it reads
  pub(crate) fn theme_arc(&self) -> Arc<Theme> {
    Arc::clone(&self.theme)
  }

  /// Render pixels per UI unit: the display's scale factor with the
  /// automatic scale, or the game's `ui_scale`.
  pub fn ui_scale(&self) -> f32 {
    self.space.px_scale
  }

  /// The UI's size in UI units, rounded up.
  pub fn screen_size(&self) -> (u32, u32) {
    let size = self.space.size.ceil();
    (size.x as u32, size.y as u32)
  }

  /// Get the frame delta time used for UI animations (cursor blink, scroll momentum).
  pub fn dt(&self) -> f32 {
    self.dt
  }

  /// Set the frame delta time for UI animations.
  ///
  /// Called by the engine with the real frame dt so render-phase animations
  /// (cursor blink, scroll momentum) stay correct regardless of the
  /// fixed-timestep update rate.
  pub fn set_dt(&mut self, dt: f32) {
    self.dt = dt;
  }

  /// The text engine, for one measurement (`None` only if it is already in
  /// use, which a single call never leaves it)
  fn engine(&self) -> Option<RefMut<'_, &'a mut TextEngine>> {
    self.text.try_borrow_mut().ok()
  }

  // =========================================================================
  // Layout, ids and scenes
  // =========================================================================

  /// Run `contents` with ids keyed by `key` at this call site: a game's own
  /// component salts its widgets' ids, so two of them (or one called in a
  /// loop over keys) never share widget state, whatever order they are
  /// called in. Adds no node, so layout is unaffected.
  ///
  /// ```ignore
  /// for item in &self.items {
  ///     ui.scope(item.id, |ui| item_row(ui, item));
  /// }
  /// ```
  #[track_caller]
  pub fn scope<R>(&mut self, key: impl Hash, contents: impl FnOnce(&mut UiContext<'a>) -> R) -> R {
    let id = self
      .memory
      .scopes
      .make(Location::caller(), Some(key_hash(&key)));
    self.memory.scopes.push(id);
    let result = contents(self);
    self.memory.scopes.pop();
    result
  }

  /// Start a scene's UI: its ids are made under its root scope, and its
  /// records name it. `top` says whether it is the top scene.
  pub(crate) fn begin_scene(&mut self, scope: u64, top: bool) {
    self.scene = scope;
    self.top = top;
    self.memory.scopes.enter_root(UiId::root(scope));
  }

  /// End a scene's UI. A container still open (its contents panicked, or a
  /// builder was leaked half way) is laid out as it is, warned once.
  pub(crate) fn end_scene(&mut self) {
    if let Some(subtree) = self.subtree.take() {
      self.open = None;
      if !LEFT_OPEN_WARNED.swap(true, Ordering::Relaxed) {
        crate::engine_warn!(module: "ui", "a UI container was still open when the scene's ui ended; laid out as it was");
      }
      self.finish_subtree(subtree);
    }
    self.memory.scopes.enter_root(UiId::root(self.scene));
  }

  /// The scene whose UI is being recorded: its scope id and whether it is
  /// the top scene
  pub(crate) fn scene(&self) -> (u64, bool) {
    (self.scene, self.top)
  }

  /// What the pointer, the keyboard and the pad did to widget `id` this
  /// frame, as resolved before the scenes' UI (`interact::resolve_pointer`,
  /// `nav::resolve_focus`), keeping only the pointer input `sense` takes.
  /// Nothing when this is not the top scene or the widget is `disabled`,
  /// and no pointer input while the pointer is occluded. Its rect is this
  /// frame's when it was laid out already, else last frame's.
  pub(crate) fn response(&mut self, id: UiId, sense: Sense, disabled: bool) -> Response {
    let rect = self
      .memory
      .current(id)
      .or_else(|| self.memory.prev(id))
      .map(|record| record.rect)
      .unwrap_or_default();
    let mut response = Response {
      id,
      rect,
      ..Response::default()
    };
    let (_, top) = self.scene();
    if !top || disabled || id == UiId::NONE {
      return response;
    }
    let i = &self.memory.interact;
    let mut states = 0;
    // Focus and Confirm, from the keyboard and pad
    for (who, state) in [
      (i.focused, Response::FOCUSED),
      (i.gained, Response::GAINED_FOCUS),
      (i.lost, Response::LOST_FOCUS),
      (i.activated, Response::CLICKED),
    ] {
      if who == id {
        states |= state;
      }
    }
    if self.mouse_occluded {
      response.states = states;
      return response;
    }
    let (hot, active) = (i.hot == id, i.active == id);
    if i.hovered == id {
      states |= Response::HOVERED;
    }
    if active {
      states |= Response::PRESSED;
    }
    if sense.click {
      for (who, state) in [
        (i.clicked, Response::CLICKED),
        (i.secondary, Response::SECONDARY),
        (i.double, Response::DOUBLE),
      ] {
        if who == id {
          states |= state;
        }
      }
    }
    if sense.drag {
      if i.drag_started == id {
        states |= Response::DRAG_STARTED;
      }
      if i.drag_released == id {
        states |= Response::DRAG_RELEASED;
      }
      if active && i.dragging {
        states |= Response::DRAGGING;
        response.drag_delta = i.pointer - i.prev_pointer;
      }
    }
    response.states = states;
    response.pointer = (hot || active).then_some(i.pointer);
    response
  }

  /// Whether the UI wants the pointer: it is over one of the top scene's
  /// widgets or panels, or a widget holds it (a press that started on it,
  /// until its release). Gameplay that reads the mouse can ignore it then.
  /// A press over the UI is hidden from gameplay already, until its release.
  pub fn wants_pointer(&self) -> bool {
    let i = &self.memory.interact;
    i.over_ui || i.active != UiId::NONE
  }

  /// Give `id` the keyboard and gamepad focus, from the next frame (when it
  /// can take it: focusable, enabled, shown, in the top scene)
  pub fn request_focus(&mut self, id: UiId) {
    self.memory.request_focus(id);
  }

  /// Whether `id` has the keyboard and gamepad focus (in the top scene)
  pub fn is_focused(&self, id: UiId) -> bool {
    self.focused() == Some(id)
  }

  /// The widget with the keyboard and gamepad focus: `None` when nothing
  /// has it, or in a scene that is not the top one
  pub fn focused(&self) -> Option<UiId> {
    let focused = self.memory.interact.focused;
    (self.top && focused != UiId::NONE).then_some(focused)
  }

  /// Whether Back (Escape, or B on a pad) was pressed this frame and no
  /// widget used it (closing a list, leaving a field). `true` takes it
  /// from gameplay, once: a press goes to the first call that gets it, so
  /// a scene brought up by the popped one's action does not pop too. Only
  /// the top scene gets it.
  ///
  /// ```ignore
  /// if ui.back() {
  ///     return SceneAction::Pop;
  /// }
  /// ```
  pub fn back(&mut self) -> bool {
    let i = &mut self.memory.interact;
    let free = self.top && i.back_pressed && !i.back_used && !i.back_taken;
    if free {
      i.back_taken = true;
    }
    free
  }

  /// This frame's tab switch, -1, 0 or 1 (`ui.tab_prev` / `ui.tab_next`
  /// while a scope is shown), for the first tabs widget of the top scene
  /// to take: a step taken (`tab_taken`) is 0 for every later call, in a
  /// rerun pass too
  #[allow(dead_code)] // Read by the tabs widget (Task 12)
  pub(crate) fn take_tab_step(&mut self) -> i8 {
    let i = &mut self.memory.interact;
    if !self.top || i.tab_taken || i.tab_step == 0 {
      return 0;
    }
    i.tab_taken = true;
    i.tab_step
  }

  /// Whether the focus ring shows: the player's last device is the keyboard
  /// or a gamepad (with the mouse, focus follows the pointer unseen)
  pub fn focus_visible(&self) -> bool {
    self.memory.interact.focus_visible
  }

  /// The direction focused widget `id` takes this frame (a slider stepping,
  /// an open list moving), when its record takes that axis
  #[allow(dead_code)] // Read by the widgets that take directions (Task 10)
  pub(crate) fn nav_step_for(&self, id: UiId) -> Option<Direction> {
    match self.memory.interact.nav_step {
      Some((who, direction)) if self.top && who == id => Some(direction),
      _ => None,
    }
  }

  /// The state a widget is drawn in: disabled, else pressed, hovered,
  /// focused with the ring showing, or normal
  pub(crate) fn widget_state(&self, response: &Response, disabled: bool) -> WidgetState {
    if disabled {
      WidgetState::Disabled
    } else if response.pressed() {
      WidgetState::Pressed
    } else if response.hovered() {
      WidgetState::Hovered
    } else if response.focused() && self.focus_visible() {
      WidgetState::Focused
    } else {
      WidgetState::Normal
    }
  }

  /// The look of a `kind` widget in `state`, as its options (`.variant`,
  /// `.style`) change it
  pub(crate) fn look_of(&self, kind: WidgetKind, opts: &WidgetOpts, state: WidgetState) -> Look {
    let mut look = self.theme.look(kind, opts.variant.as_deref(), state);
    if let Some(patch) = &opts.look_patch {
      patch.apply(&mut look);
    }
    look
  }

  /// Draw `look`'s fill and border on `node`: its skin as a nine-slice,
  /// tinted by the fill, when it names one the theme has a sprite for, else
  /// a rounded rect
  pub(crate) fn push_look_box(&mut self, node: usize, look: &Look) {
    if let Some(name) = &look.skin {
      match self.theme.skins.get(name) {
        Some(Skin {
          sprite: Some(sprite),
          insets,
          ..
        }) => {
          let kind = DrawKind::NineSlice {
            inset: Edges::ZERO,
            sprite: sprite.clone(),
            insets: *insets,
            tint: look.fill,
          };
          self.push_draw(node, kind);
          return;
        }
        Some(_) => warn_once(format!("skin:{name}"), || {
          format!(
            "UI skin {name:?} has no sprite (give it a loadable path or a sprite); drawing the plain look"
          )
        }),
        None => warn_once(format!("skin:{name}"), || {
          format!("no UI skin named {name:?}; drawing the plain look")
        }),
      }
    }
    let border =
      (look.border_width > 0.0 && look.border.a > 0).then_some((look.border_width, look.border));
    if look.fill.a > 0 || border.is_some() {
      self.push_draw(
        node,
        DrawKind::Fill {
          inset: Edges::ZERO,
          fill: look.fill,
          radius: look.radius,
          border,
        },
      );
    }
  }

  /// Draw the theme's focus ring around `node`, a widget whose corners have
  /// `radius`
  pub(crate) fn focus_ring(&mut self, node: usize, radius: f32) {
    let ring = self.theme.metrics.focus_ring.clone();
    self.push_draw(
      node,
      DrawKind::Fill {
        inset: Edges::all(-(ring.offset + ring.width)),
        fill: Color::TRANSPARENT,
        radius: radius + ring.offset + ring.width,
        border: Some((ring.width, ring.color)),
      },
    );
  }

  /// The next draw's place in the painter's order
  pub(crate) fn next_order(&mut self) -> u32 {
    let order = self.order;
    self.order = self.order.saturating_add(1);
    order
  }

  /// The memory, for tests to read records
  #[cfg(test)]
  pub(crate) fn memory_for_test(&self) -> &UiMemory {
    self.memory
  }

  /// Open a container node recording `flags`: inside the open container,
  /// or as the root of a new subtree placed at `at` (when given). Its index
  /// in the subtree.
  pub(crate) fn open_node(
    &mut self,
    id: UiId,
    style: UiStyle,
    at: Option<Rect>,
    flags: Flags,
  ) -> usize {
    let mut node = Node::new(id, style, Measure::None);
    node.record = Some(RecordSpec {
      flags,
      nav: [UiId::NONE; 4],
    });
    self.push_node(node, at)
  }

  /// Close `node` (which must be the innermost open one). Closing a
  /// subtree's root lays it out and writes its records and draws.
  ///
  /// Its `Response` carries this frame's rect when the subtree was laid
  /// out, otherwise last frame's.
  pub(crate) fn close_node(&mut self, node: usize) -> Response {
    let id = self
      .subtree
      .as_ref()
      .and_then(|subtree| subtree.nodes.get(node))
      .map_or(UiId::NONE, |node| node.id);
    let rect = match self.close(node) {
      Some(rect) => rect,
      None => self.memory.prev(id).map(|r| r.rect).unwrap_or_default(),
    };
    Response {
      id,
      rect,
      ..Response::default()
    }
  }

  /// Draw `kind` on `node` of the open subtree, in the painter's order of
  /// this call
  pub(crate) fn push_draw(&mut self, node: usize, kind: DrawKind) {
    let order = self.next_order();
    let layer = self.layer;
    if let Some(subtree) = &mut self.subtree {
      subtree.draws.push(Draw {
        order,
        layer,
        node,
        kind,
      });
    }
  }

  /// Push `node` into the open container, or start a subtree with it
  /// (placed at `at`, under the current clip). It becomes the innermost
  /// open node: close it with [`close_node`](Self::close_node).
  pub(crate) fn push_node(&mut self, mut node: Node, at: Option<Rect>) -> usize {
    node.layer = self.layer;
    node.order = self.next_order();
    let parent = self.open;
    let at = at.map(clean_at);
    let clip = self.current_clip;
    // The nearest open scope, or the scope the subtree started in
    node.scope = self.focus_scope_here();
    let subtree = self.subtree.get_or_insert_with(|| Subtree {
      nodes: Vec::new(),
      draws: Vec::new(),
      at,
      clip,
      scope: node.scope,
    });
    if let Some(p) = parent.and_then(|p| subtree.nodes.get(p)) {
      let flags = p.record.map(|r| r.flags).unwrap_or_default();
      node.parent = parent;
      node.depth = p.depth.saturating_add(1);
      node.scroll = if flags.contains(Flags::SCROLLABLE) {
        p.id
      } else {
        p.scroll
      };
    }
    let index = subtree.nodes.len();
    subtree.nodes.push(node);
    if let Some(p) = parent.and_then(|p| subtree.nodes.get_mut(p)) {
      p.children.push(index);
    }
    self.open = Some(index);
    index
  }

  /// The focus scope a node opened now sits in: the innermost open node
  /// that is a scope, or that node's scope (`NONE` outside any)
  fn focus_scope_here(&self) -> UiId {
    let Some(subtree) = &self.subtree else {
      return UiId::NONE;
    };
    let Some(open) = self.open.and_then(|open| subtree.nodes.get(open)) else {
      return subtree.scope;
    };
    let is_scope = open
      .record
      .is_some_and(|r| r.flags.contains(Flags::FOCUS_SCOPE));
    if is_scope { open.id } else { open.scope }
  }

  /// Close `node`: its parent is the innermost open node again. Closing the
  /// root lays the subtree out; the root's rect this frame then.
  fn close(&mut self, node: usize) -> Option<Rect> {
    let parent = self
      .subtree
      .as_ref()
      .and_then(|subtree| subtree.nodes.get(node))
      .and_then(|node| node.parent);
    self.open = parent;
    if parent.is_some() {
      return None;
    }
    let subtree = self.subtree.take()?;
    self.finish_subtree(subtree).rects.first().copied()
  }

  /// Lay `subtree` out, write its records and turn its draws into commands
  fn finish_subtree(&mut self, subtree: Subtree) -> Resolved {
    tree::resolve(
      subtree,
      ResolveCx {
        layout: &mut *self.layout,
        text: &self.text,
        memory: &mut *self.memory,
        commands: &mut *self.commands,
        space: self.space,
        scene: self.scene,
      },
    )
  }

  /// Record `leaf` as a text node: on its own when no container is open
  /// (laid out at once; its laid-out size), else in the open container
  /// (its `measured` size). `draw` is its opacity, reveal and seed.
  fn text_leaf(
    &mut self,
    site: &'static Location<'static>,
    leaf: TextLeaf,
    draw: (f32, Option<(f32, f32)>, u64),
    measured: Vec2,
  ) -> Vec2 {
    let (opacity, reveal, seed) = draw;
    let id = self.memory.scopes.make(site, None);
    let style = UiStyle {
      anchor: leaf.options.anchor,
      ..UiStyle::default()
    };
    let frac = Vec2::new(align_fraction(leaf.options.align), 0.0);
    let mut node = Node::new(id, style, Measure::Text);
    // Measured again when the text, its look, the scale or the fonts change
    let mut key = DefaultHasher::new();
    seed.hash(&mut key);
    self.space.px_scale.to_bits().hash(&mut key);
    self
      .engine()
      .map_or(0, |engine| engine.generation())
      .hash(&mut key);
    node.content_key = key.finish();
    node.text = Some(leaf.clone());
    let index = self.push_node(node, None);
    self.push_draw(
      index,
      DrawKind::Text {
        leaf,
        frac,
        wrap: true,
        opacity,
        reveal,
        seed,
      },
    );
    match self.close(index) {
      Some(rect) => Vec2::new(rect.width, rect.height),
      None => measured,
    }
  }

  // =========================================================================
  // Input Consumption
  // =========================================================================

  /// Check if the Escape key was consumed by a widget this frame.
  ///
  /// The UI pass runs before gameplay and what it consumes is hidden from
  /// `Input`, so `update` never sees an Escape a widget took. Use this in
  /// `Scene::ui`, after the widgets, to tell the cases apart there.
  ///
  /// # Example
  /// ```ignore
  /// // In Scene::ui:
  /// ui.dropdown(&mut self.dropdown, rect, &options);
  /// if !ui.escape_consumed() && ui.input().key_pressed(KeyCode::Escape) {
  ///     return SceneAction::Pop;
  /// }
  ///
  /// // Or in update(): an Escape the dropdown took reads as not pressed
  /// if ctx.input.key_pressed(KeyCode::Escape) {
  ///     return SceneAction::Pop;
  /// }
  /// ```
  pub fn escape_consumed(&self) -> bool {
    self.input.is_key_consumed(KeyCode::Escape)
  }

  /// Check if a mouse click was consumed by a widget this frame.
  ///
  /// Use this to avoid processing clicks on game elements when UI
  /// handled the click.
  pub fn click_consumed(&self) -> bool {
    self.input.is_mouse_consumed(MouseButton::Left)
  }

  /// Consume the mouse click for this frame, from other widgets and from
  /// gameplay.
  pub fn consume_click(&mut self) {
    self.input.consume_mouse(MouseButton::Left);
  }

  /// Consume the mouse wheel for this frame, from other widgets and from
  /// gameplay (scroll views that scroll take it themselves).
  pub fn consume_scroll(&mut self) {
    self.input.consume_scroll();
  }

  /// Consume the escape key for this frame, from other widgets and from
  /// gameplay.
  pub fn consume_escape(&mut self) {
    self.input.consume_key(KeyCode::Escape);
  }

  /// Consume `key` for this frame, from other widgets and from gameplay.
  pub fn consume_key(&mut self, key: KeyCode) {
    self.input.consume_key(key);
  }

  /// The frame's input as the UI sees it.
  pub fn input(&self) -> &UiInput {
    &self.input
  }

  /// End the pass, handing back the input with what was consumed.
  pub fn into_input(self) -> UiInput {
    self.input
  }

  /// Whether the pointer is occluded for the widgets drawn from here on.
  pub fn is_mouse_occluded(&self) -> bool {
    self.mouse_occluded
  }

  /// Restore occlusion saved with [`is_mouse_occluded`](Self::is_mouse_occluded).
  pub(crate) fn restore_mouse_occluded(&mut self, occluded: bool) {
    self.mouse_occluded = occluded;
  }

  /// Mark the mouse as occluded by an overlay (e.g. developer console).
  ///
  /// When set, `is_hovered`, `is_held`, and `is_clicked` all return false,
  /// preventing widgets underneath the overlay from responding to the mouse.
  /// Scenes under the top scene run their `ui` with the mouse occluded.
  pub fn set_mouse_occluded(&mut self) {
    self.mouse_occluded = true;
  }

  /// Check if any dropdown is currently open.
  ///
  /// Useful for knowing whether to handle scene-level input.
  pub fn any_dropdown_open(&self) -> bool {
    !self.pending_dropdowns.is_empty()
  }

  /// Get the currently focused dropdown ID (0 = none focused).
  pub fn focused_dropdown(&self) -> u32 {
    self.focused_dropdown_id
  }

  /// Whether a widget drawn this frame wants the keyboard: a focused text
  /// input, or an open dropdown. A dropdown that is merely focused does not
  /// count, since its focus is only cleared when another dropdown takes it.
  ///
  /// Scenes with their own key bindings can check it after drawing their UI
  /// and ignore those keys on the next update, so typing into a field never
  /// triggers them.
  pub fn wants_keyboard(&self) -> bool {
    self.keyboard_focus
  }

  /// Set the currently focused dropdown ID.
  pub fn set_focused_dropdown(&mut self, id: u32) {
    self.focused_dropdown_id = id;
  }

  // =========================================================================
  // Clipping
  // =========================================================================

  /// Push a clip rect onto the stack.
  ///
  /// All subsequent drawing will be clipped to the intersection of the
  /// current clip and this new rect. Use `pop_clip()` to restore the
  /// previous clip state. Clips nest without a limit.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Create a scroll region
  /// let scroll_area = Rect::new(10.0, 10.0, 200.0, 150.0);
  /// ui.push_clip(scroll_area);
  ///
  /// // Draw clipped content
  /// for (i, item) in items.iter().enumerate() {
  ///     let y = 10.0 + i as f32 * 20.0 - scroll_offset;
  ///     ui.draw_text(item, 15.0, y, Color::WHITE);
  /// }
  ///
  /// ui.pop_clip();
  /// ```
  pub fn push_clip(&mut self, rect: Rect) {
    self.clip_stack.push(rect);
    self.update_current_clip();
  }

  /// Pop the most recent clip rect from the stack.
  ///
  /// Restores the clip state to what it was before the corresponding
  /// `push_clip()` call. Popping an empty stack does nothing (warned once).
  pub fn pop_clip(&mut self) {
    if self.clip_stack.pop().is_none() {
      if !EMPTY_POP_WARNED.swap(true, Ordering::Relaxed) {
        crate::engine_warn!(module: "ui", "pop_clip with no clip to pop; ignored");
      }
      return;
    }
    self.update_current_clip();
  }

  /// Get the current effective clip rect, if any.
  ///
  /// Returns `None` if no clip is active (full screen is visible).
  pub fn current_clip(&self) -> Option<Rect> {
    self.current_clip
  }

  /// Check if a clip is currently active.
  pub fn has_clip(&self) -> bool {
    self.current_clip.is_some()
  }

  /// Get the current clip stack depth.
  pub fn clip_depth(&self) -> usize {
    self.clip_stack.len()
  }

  /// Clear all clips from the stack.
  ///
  /// Useful for drawing modal overlays that should ignore parent clips.
  pub fn clear_clips(&mut self) {
    self.clip_stack.clear();
    self.current_clip = None;
  }

  /// Temporarily clear clips, run a closure, then restore.
  ///
  /// Useful for drawing modal backgrounds or overlays.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Draw a dimmed background over everything
  /// ui.with_no_clip(|ui| {
  ///     let (w, h) = ui.screen_size();
  ///     ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32),
  ///                  Color::rgba(0, 0, 0, 128));
  /// });
  /// ```
  pub fn with_no_clip<F, R>(&mut self, f: F) -> R
  where
    F: FnOnce(&mut Self) -> R,
  {
    let saved_stack = std::mem::take(&mut self.clip_stack);
    let saved_clip = self.current_clip.take();

    let result = f(self);

    self.clip_stack = saved_stack;
    self.current_clip = saved_clip;

    result
  }

  /// Recalculate the current effective clip rect.
  fn update_current_clip(&mut self) {
    let mut clips = self.clip_stack.iter();
    self.current_clip = clips.next().map(|first| {
      let result = clips.fold(*first, |result, clip| Self::intersect_rects(result, *clip));
      if result.width > 0.0 && result.height > 0.0 {
        result
      } else {
        // Empty intersection - nothing visible
        Rect::new(0.0, 0.0, 0.0, 0.0)
      }
    });
  }

  /// Calculate the intersection of two rectangles.
  fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());

    let width = (right - x).max(0.0);
    let height = (bottom - y).max(0.0);

    Rect::new(x, y, width, height)
  }

  /// Whether `rect` lies wholly outside the current clip (nothing of it
  /// would show).
  fn outside_clip(&self, rect: Rect) -> bool {
    self.current_clip.is_some_and(|clip| {
      clip.width <= 0.0
        || clip.height <= 0.0
        || rect.right() <= clip.x
        || rect.x >= clip.right()
        || rect.bottom() <= clip.y
        || rect.y >= clip.bottom()
    })
  }

  /// Whether `rect` has a finite position and a positive finite size
  fn drawable(rect: Rect) -> bool {
    rect.x.is_finite()
      && rect.y.is_finite()
      && rect.width.is_finite()
      && rect.height.is_finite()
      && rect.width > 0.0
      && rect.height > 0.0
  }

  // =========================================================================
  // Shape Drawing
  // =========================================================================

  /// Record a rectangle: skipped when degenerate or outside the clip
  fn record_rect(&mut self, rect: Rect, fill: Color, radius: f32, border: Option<(f32, Color)>) {
    if !Self::drawable(rect) || self.outside_clip(rect) {
      return;
    }
    let order = self.next_order();
    self.commands.push(
      order,
      UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip: self.current_clip,
        layer: self.layer,
      },
    );
  }

  /// Draw a filled rectangle.
  ///
  /// The rectangle is clipped to the current clip rect if one is active.
  pub fn draw_rect(&mut self, rect: Rect, color: Color) {
    self.record_rect(rect, color, 0.0, None);
  }

  /// Draw a rectangle outline (border only), `thickness` inside the rect.
  pub fn draw_rect_outline(&mut self, rect: Rect, color: Color, thickness: f32) {
    self.record_rect(rect, Color::TRANSPARENT, 0.0, Some((thickness, color)));
  }

  /// Draw a filled rectangle with corners rounded by `radius`.
  pub fn draw_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
    self.record_rect(rect, color, radius, None);
  }

  /// Draw a filled rectangle, its corners rounded by `radius`, with a
  /// `border_width` border inside it.
  pub fn draw_rect_bordered(
    &mut self,
    rect: Rect,
    fill: Color,
    radius: f32,
    border_width: f32,
    border_color: Color,
  ) {
    self.record_rect(rect, fill, radius, Some((border_width, border_color)));
  }

  /// Draw a panel with background and optional border.
  pub fn draw_panel(&mut self, rect: Rect, bg_color: Color, border_color: Option<Color>) {
    self.record_rect(rect, bg_color, 0.0, border_color.map(|c| (1.0, c)));
  }

  /// Draw a horizontal progress bar.
  ///
  /// # Arguments
  /// * `rect` - Bounding rectangle for the progress bar
  /// * `progress` - Fill amount (0.0 to 1.0)
  /// * `fill_color` - Color of the filled portion
  /// * `bg_color` - Color of the background
  pub fn draw_progress_bar(
    &mut self,
    rect: Rect,
    progress: f32,
    fill_color: Color,
    bg_color: Color,
  ) {
    // Background
    self.draw_rect(rect, bg_color);

    // Fill
    let progress = progress.clamp(0.0, 1.0);
    if progress > 0.0 {
      let fill_width = rect.width * progress;
      self.draw_rect(
        Rect::new(rect.x, rect.y, fill_width, rect.height),
        fill_color,
      );
    }
  }

  /// Draw a progress bar with border.
  pub fn draw_progress_bar_with_border(
    &mut self,
    rect: Rect,
    progress: f32,
    fill_color: Color,
    bg_color: Color,
    border_color: Color,
  ) {
    self.draw_progress_bar(rect, progress, fill_color, bg_color);
    self.draw_rect_outline(rect, border_color, 1.0);
  }

  // =========================================================================
  // Text Drawing
  // =========================================================================

  /// The default UI style in `color`, laid out opaque, and the opacity that
  /// carries the colour's alpha (so a fading colour reuses one cached layout)
  fn plain_style(color: Color) -> (TextStyle, f32) {
    (
      TextStyle::default().color(color.with_alpha(255)),
      f32::from(color.a) / 255.0,
    )
  }

  /// Lay `text` out and record it with its top-left at `origin`; its size in
  /// UI units, or `None` when it cannot be laid out (warned once by the
  /// engine). Text wholly outside the clip is not recorded.
  fn record_text(
    &mut self,
    text: &RichText,
    style: &TextStyle,
    options: &TextLayoutOptions,
    origin: Vec2,
    opacity: f32,
    reveal: Option<(f32, f32)>,
  ) -> Option<Vec2> {
    let scale = self.space.px_scale;
    let layout = self.engine()?.layout(text, style, options, scale)?;
    let seed = text_seed(text, style, options);
    Some(self.record_layout(layout, origin, opacity, reveal, seed))
  }

  /// Record laid-out text with its top-left at `origin`; its size in UI
  /// units. Text wholly outside the clip is not recorded.
  fn record_layout(
    &mut self,
    layout: Arc<TextLayout>,
    origin: Vec2,
    opacity: f32,
    reveal: Option<(f32, f32)>,
    seed: u64,
  ) -> Vec2 {
    let size = layout.size / self.space.px_scale;
    if !origin.is_finite() || self.outside_clip(Rect::new(origin.x, origin.y, size.x, size.y)) {
      return size;
    }
    let order = self.next_order();
    self.commands.push(
      order,
      UiCommand::Text {
        layout,
        origin,
        reveal,
        opacity,
        seed,
        clip: self.current_clip,
        layer: self.layer,
      },
    );
    size
  }

  /// Draw text with its top-left at (x, y), in the default UI style
  /// (`FontRole::Ui`, 14 px) and `color`. Newlines break lines.
  pub fn draw_text(&mut self, text: &str, x: f32, y: f32, color: Color) {
    let (style, opacity) = Self::plain_style(color);
    let options = TextLayoutOptions::default();
    self.record_text(
      &RichText::new(text),
      &style,
      &options,
      Vec2::new(x, y),
      opacity,
      None,
    );
  }

  /// Draw text in `style` with its top-left at (x, y).
  pub fn draw_text_styled(&mut self, text: impl Into<RichText>, x: f32, y: f32, style: &TextStyle) {
    let options = TextLayoutOptions::default();
    self.record_text(&text.into(), style, &options, Vec2::new(x, y), 1.0, None);
  }

  /// The size of `text` in `style`, in UI units.
  pub fn measure_text(&self, text: impl Into<RichText>, style: &TextStyle) -> Vec2 {
    let options = TextLayoutOptions::default();
    let scale = self.space.px_scale;
    self.engine().map_or(Vec2::ZERO, |mut engine| {
      engine.measure(&text.into(), style, &options, scale)
    })
  }

  /// The size of `text` laid out with `options` in `style`, in UI units
  pub(crate) fn measure_rich(
    &self,
    text: &RichText,
    style: &TextStyle,
    options: &TextLayoutOptions,
  ) -> Vec2 {
    let scale = self.space.px_scale;
    self.engine().map_or(Vec2::ZERO, |mut engine| {
      engine.measure(text, style, options, scale)
    })
  }

  /// The text engine, for a widget's painter
  pub(crate) fn text_engine(&mut self) -> &mut TextEngine {
    self.text.get_mut()
  }

  /// The height of one line of `style`, in UI units.
  pub fn line_height(&self, style: &TextStyle) -> f32 {
    let scale = self.space.px_scale;
    self
      .engine()
      .map_or(0.0, |engine| engine.line_height(style, scale))
  }

  /// Draw text with anchor-based positioning.
  pub fn draw_text_anchored(
    &mut self,
    text: &str,
    anchor: Anchor,
    offset_x: f32,
    offset_y: f32,
    color: Color,
  ) {
    let size = self.measure_text(text, &TextStyle::default());
    let (x, y) = anchor.calculate_position(
      offset_x,
      offset_y,
      size.x,
      size.y,
      self.space.size.x,
      self.space.size.y,
    );
    self.draw_text(text, x, y, color);
  }

  /// Draw centered text within a rectangle (wrapped when it is wider).
  pub fn draw_text_centered(&mut self, text: &str, rect: Rect, color: Color) {
    // Measured in the style it draws in, so the draw is a cache hit
    let size = self.measure_text(text, &Self::plain_style(color).0);
    if size.x > rect.width {
      self.draw_text_wrapped_centered(text, rect, color);
      return;
    }
    let (cx, cy) = rect.center();
    self.draw_text(text, cx - size.x / 2.0, cy - size.y / 2.0, color);
  }

  /// The size of text in the default UI style, in UI units rounded up.
  pub fn text_size(&self, text: &str) -> (u32, u32) {
    let size = self.measure_text(text, &TextStyle::default()).ceil();
    (size.x as u32, size.y as u32)
  }

  /// Wrap text to fit within a maximum width.
  ///
  /// Returns the lines (trailing spaces trimmed) that fit within `max_width`
  /// UI units in the default UI style.
  pub fn wrap_text(&self, text: &str, max_width: u32) -> Vec<String> {
    let scale = self.space.px_scale;
    let Some(mut engine) = self.engine() else {
      return vec![text.to_string()];
    };
    engine
      .wrap_lines(text, &TextStyle::default(), max_width as f32, scale)
      .into_iter()
      .map(|range| text.get(range).unwrap_or_default().trim_end().to_string())
      .collect()
  }

  /// Wrap text to fit within `width` UI units in `style`, like
  /// [`wrap_text`](Self::wrap_text) (the console wraps in its own font).
  pub fn wrap_text_styled(&self, text: &str, width: f32, style: &TextStyle) -> Vec<String> {
    let scale = self.space.px_scale;
    let Some(mut engine) = self.engine() else {
      return vec![text.to_string()];
    };
    engine
      .wrap_lines(text, style, width, scale)
      .into_iter()
      .map(|range| text.get(range).unwrap_or_default().trim_end().to_string())
      .collect()
  }

  /// Draw text with every option: markup, wrapping, anchor, reveal, fade and
  /// opacity. `ui.text("Hi").at(10.0, 10.0).size(24.0).draw()` draws it at a
  /// point; without `at` it joins the layout (`ui.text("Hi");` in a column).
  /// The `draw_text*` family is the shorthand with the default style.
  #[track_caller]
  pub fn text<'u>(&'u mut self, text: impl Into<RichText>) -> UiText<'u, 'a> {
    UiText {
      ui: self,
      site: Location::caller(),
      text: text.into(),
      at: None,
      style: TextStyle::default(),
      options: TextLayoutOptions::default(),
      shown: None,
      fade: 0.0,
      opacity: 1.0,
      typewriter: None,
      recorded: false,
    }
  }

  /// The size of text wrapped to `max_width`, in UI units rounded up.
  pub fn wrapped_text_size(&self, text: &str, max_width: u32) -> (u32, u32) {
    let options = TextLayoutOptions::default().wrap(max_width as f32);
    let scale = self.space.px_scale;
    let size = self.engine().map_or(Vec2::ZERO, |mut engine| {
      engine.measure(&RichText::new(text), &TextStyle::default(), &options, scale)
    });
    let size = size.ceil();
    (size.x as u32, size.y as u32)
  }

  /// Draw text with word wrapping within a maximum width.
  ///
  /// Text is wrapped at word boundaries to fit within `max_width` UI units.
  /// Long words that don't fit are broken across lines.
  pub fn draw_text_wrapped(&mut self, text: &str, x: f32, y: f32, max_width: u32, color: Color) {
    let (style, opacity) = Self::plain_style(color);
    let options = TextLayoutOptions::default().wrap(max_width as f32);
    self.record_text(
      &RichText::new(text),
      &style,
      &options,
      Vec2::new(x, y),
      opacity,
      None,
    );
  }

  /// Draw wrapped text centered within a rectangle.
  ///
  /// Text is word-wrapped to fit the rectangle width, then centered both
  /// horizontally and vertically within the bounds.
  pub fn draw_text_wrapped_centered(&mut self, text: &str, rect: Rect, color: Color) {
    let (style, opacity) = Self::plain_style(color);
    let options = TextLayoutOptions::default()
      .wrap(rect.width)
      .align(TextAlign::Center);
    let text = RichText::new(text);
    let scale = self.space.px_scale;
    let size = self.engine().map_or(Vec2::ZERO, |mut engine| {
      engine.measure(&text, &style, &options, scale)
    });
    // A centred block is as wide as its wrap width
    let origin = Vec2::new(rect.x, rect.y + (rect.height - size.y) / 2.0);
    self.record_text(&text, &style, &options, origin, opacity, None);
  }

  // =========================================================================
  // Interactive Widgets
  // =========================================================================

  /// Check if a rectangle is being clicked this frame.
  ///
  /// Returns `true` if the mouse button was just pressed while over the rectangle.
  /// Returns `false` if the mouse is occluded by an overlay.
  pub fn is_clicked(&self, rect: Rect) -> bool {
    if self.mouse_occluded || !self.input.mouse_pressed(MouseButton::Left) {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Check if a rectangle is being hovered.
  ///
  /// Returns `false` if the mouse is occluded by an overlay.
  pub fn is_hovered(&self, rect: Rect) -> bool {
    if self.mouse_occluded {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Check if a rectangle is being held (mouse down while hovering).
  ///
  /// Returns `false` if the mouse is occluded by an overlay.
  pub fn is_held(&self, rect: Rect) -> bool {
    if self.mouse_occluded || !self.input.mouse_down(MouseButton::Left) {
      return false;
    }
    let (mx, my) = self.input.mouse_position();
    rect.contains(mx, my)
  }

  /// Draw a label with optional background panel.
  pub fn label(&mut self, text: &str, x: f32, y: f32, text_color: Color, bg_color: Option<Color>) {
    if let Some(bg) = bg_color {
      let (w, h) = self.text_size(text);
      let padding = 4.0;
      self.draw_rect(
        Rect::new(
          x - padding,
          y - padding,
          w as f32 + padding * 2.0,
          h as f32 + padding * 2.0,
        ),
        bg,
      );
    }
    self.draw_text(text, x, y, text_color);
  }

  // =========================================================================
  // Sprite and Line Drawing
  // =========================================================================

  /// Record part of a sprite (`uv`, fractions of it) stretched over `rect`
  fn record_sprite(&mut self, sprite: &SpriteHandle, rect: Rect, uv: [f32; 4], tint: Color) {
    if !Self::drawable(rect) || self.outside_clip(rect) {
      return;
    }
    let order = self.next_order();
    self.commands.push(
      order,
      UiCommand::Sprite {
        sprite: sprite.clone(),
        rect,
        uv,
        tint,
        clip: self.current_clip,
        layer: self.layer,
      },
    );
  }

  /// Draw a sprite with its top-left at (x, y), one UI unit per texel.
  pub fn draw_sprite(&mut self, sprite: &SpriteHandle, x: f32, y: f32) {
    self.draw_sprite_scaled(sprite, x, y, 1.0);
  }

  /// Draw a sprite with its top-left at (x, y), scaled uniformly.
  pub fn draw_sprite_scaled(&mut self, sprite: &SpriteHandle, x: f32, y: f32, scale: f32) {
    let (w, h) = (sprite.width() as f32, sprite.height() as f32);
    let rect = Rect::new(x, y, w * scale, h * scale);
    self.record_sprite(sprite, rect, [0.0, 0.0, 1.0, 1.0], Color::WHITE);
  }

  /// Draw a sprite stretched over `rect`, tinted.
  pub fn draw_sprite_rect(&mut self, sprite: &SpriteHandle, rect: Rect, tint: Color) {
    self.record_sprite(sprite, rect, [0.0, 0.0, 1.0, 1.0], tint);
  }

  /// Draw a sprite as a nine-slice over `rect`: the corners at their texel
  /// size, the edges stretched along one axis and the centre both ways.
  /// `insets` are `[left, top, right, bottom]` in texels.
  pub fn draw_nine_slice(
    &mut self,
    sprite: &SpriteHandle,
    rect: Rect,
    insets: [f32; 4],
    tint: Color,
  ) {
    let size = Vec2::new(sprite.width() as f32, sprite.height() as f32);
    if size.x <= 0.0 || size.y <= 0.0 || !Self::drawable(rect) {
      return;
    }
    // Insets are texels: finite, not negative, and together no more than
    // the texture (scaled down proportionally when they are)
    let clean = |inset: f32| {
      if inset.is_finite() {
        inset.max(0.0)
      } else {
        0.0
      }
    };
    let fit = |near: f32, far: f32, limit: f32| {
      let (near, far) = (near.min(limit), far.min(limit));
      let sum = near + far;
      if sum > limit {
        (near * limit / sum, far * limit / sum)
      } else {
        (near, far)
      }
    };
    let [left, top, right, bottom] = insets.map(clean);
    let (left, right) = fit(left, right, size.x);
    let (top, bottom) = fit(top, bottom, size.y);
    // Columns and rows: (start, length) in the rect, (start, end) in UVs.
    // The corners shrink with the rect when it is smaller than their sum.
    let span = |start: f32, length: f32, near: f32, far: f32, texels: f32| {
      let shrink = if near + far > length {
        length.max(0.0) / (near + far)
      } else {
        1.0
      };
      let (rect_near, rect_far) = (near * shrink, far * shrink);
      let middle = (length - rect_near - rect_far).max(0.0);
      [
        (start, rect_near, 0.0, near / texels),
        (start + rect_near, middle, near / texels, 1.0 - far / texels),
        (
          start + rect_near + middle,
          rect_far,
          1.0 - far / texels,
          1.0,
        ),
      ]
    };
    let columns = span(rect.x, rect.width, left, right, size.x);
    let rows = span(rect.y, rect.height, top, bottom, size.y);
    for (y, height, v0, v1) in rows {
      for (x, width, u0, u1) in columns {
        self.record_sprite(
          sprite,
          Rect::new(x, y, width, height),
          [u0, v0, u1, v1],
          tint,
        );
      }
    }
  }

  /// Draw a polyline through `points`, `width` UI units wide.
  pub fn draw_line(&mut self, points: &[Vec2], width: f32, color: Color) {
    if points.len() < 2 || !width.is_finite() || width <= 0.0 {
      return;
    }
    let order = self.next_order();
    self.commands.push(
      order,
      UiCommand::Line {
        points: points.to_vec(),
        width,
        color,
        closed: false,
        clip: self.current_clip,
        layer: self.layer,
      },
    );
  }

  // =========================================================================
  // Slider Widget
  // =========================================================================

  /// Draw an interactive horizontal slider.
  ///
  /// Returns true if the value was changed this frame.
  ///
  /// # Arguments
  /// * `rect` - The bounding box for the slider track
  /// * `value` - Mutable reference to the value being controlled (will be updated if dragged)
  /// * `min` - Minimum value
  /// * `max` - Maximum value
  pub fn slider(&mut self, rect: Rect, value: &mut f32, min: f32, max: f32) -> bool {
    let (track_color, fill_color, handle_color, hover_color) = (
      Color::rgb(60, 60, 80),
      Color::rgb(80, 150, 255),
      Color::WHITE,
      Color::rgb(200, 220, 255),
    );

    let mouse_pos = self.input.mouse_position();

    // Interaction area - must click here to start dragging, and stay roughly here to continue
    // Horizontal is generous (allow overshooting ends), vertical is tight (avoid other widgets)
    let interaction_rect = Rect::new(
      rect.x - 20.0,
      rect.y - 15.0,
      rect.width + 40.0,
      rect.height + 30.0,
    );
    let mouse_in_interaction = interaction_rect.contains(mouse_pos.0, mouse_pos.1);

    // Calculate handle position
    let normalized = (*value - min) / (max - min).max(0.0001);
    let handle_x = rect.x + normalized * rect.width;
    let handle_width = 12.0;
    let handle_height = rect.height + 8.0;
    let handle_rect = Rect::new(
      handle_x - handle_width / 2.0,
      rect.y - 4.0,
      handle_width,
      handle_height,
    );

    let mut changed = false;

    // Handle dragging - mouse must be within the interaction area
    // (In pure immediate mode, we can't track "which widget started the drag",
    // so we require staying near the slider to avoid affecting other widgets)
    let is_dragging = self.input.mouse_down(MouseButton::Left) && mouse_in_interaction;
    // The drag is the slider's, not the game's
    if is_dragging {
      self.input.consume_mouse(MouseButton::Left);
    }

    if is_dragging {
      // Calculate new value based on mouse position (clamped to slider bounds)
      let mouse_x = mouse_pos.0.clamp(rect.x, rect.right());
      let new_normalized = (mouse_x - rect.x) / rect.width;
      let new_value = min + new_normalized * (max - min);
      let clamped_value = new_value.clamp(min, max);

      if (*value - clamped_value).abs() > 0.001 {
        *value = clamped_value;
        changed = true;
      }
    }

    // Draw track background
    self.draw_rect(rect, track_color);

    // Draw filled portion
    let fill_rect = Rect::new(rect.x, rect.y, normalized * rect.width, rect.height);
    self.draw_rect(fill_rect, fill_color);

    // Draw handle
    let handle_visual_color = if mouse_in_interaction || is_dragging {
      hover_color
    } else {
      handle_color
    };
    self.draw_rect(handle_rect, handle_visual_color);

    changed
  }

  // =========================================================================
  // Checkbox Widget
  // =========================================================================

  /// Draw an interactive checkbox.
  ///
  /// Returns true if the checkbox was toggled this frame.
  ///
  /// # Arguments
  /// * `rect` - The bounding box for the checkbox (typically square)
  /// * `checked` - Mutable reference to the checked state
  /// * `label` - Optional label text to display next to the checkbox
  pub fn checkbox(&mut self, rect: Rect, checked: &mut bool, label: Option<&str>) -> bool {
    let hovered = self.is_hovered(rect);
    let clicked = self.is_clicked(rect);
    if clicked {
      self.input.consume_mouse(MouseButton::Left);
    }

    let bg_color = if *checked {
      Color::rgb(60, 120, 200)
    } else if hovered {
      Color::rgb(50, 50, 70)
    } else {
      Color::rgb(40, 40, 60)
    };
    let (border_color, checkmark_color) = (Color::rgb(80, 80, 100), Color::WHITE);

    // Toggle on click
    if clicked {
      *checked = !*checked;
    }

    // Draw checkbox background
    self.draw_panel(rect, bg_color, Some(border_color));

    // Draw checkmark if checked - simple filled inner square
    if *checked {
      let padding = (rect.width.min(rect.height) * 0.25).max(3.0);
      let inner_rect = rect.shrink(padding);
      self.draw_rect(inner_rect, checkmark_color);
    }

    // Draw label if provided
    if let Some(label_text) = label {
      let text_color = Color::WHITE;
      let text_x = rect.right() + 8.0;
      let (_, text_h) = self.text_size(label_text);
      let text_y = rect.y + (rect.height - text_h as f32) / 2.0;
      self.draw_text(label_text, text_x, text_y, text_color);
    }

    clicked
  }

  // =========================================================================
  // Toggle Widget
  // =========================================================================

  /// Draw an interactive toggle switch.
  ///
  /// Returns true if the toggle was changed this frame.
  ///
  /// # Arguments
  /// * `rect` - The bounding box for the toggle (width should be ~2x height)
  /// * `on` - Mutable reference to the toggle state
  pub fn toggle(&mut self, rect: Rect, on: &mut bool) -> bool {
    let hovered = self.is_hovered(rect);
    let clicked = self.is_clicked(rect);
    if clicked {
      self.input.consume_mouse(MouseButton::Left);
    }

    let track_color = if *on {
      Color::rgb(60, 150, 80)
    } else if hovered {
      Color::rgb(70, 70, 90)
    } else {
      Color::rgb(60, 60, 80)
    };
    let knob_color = Color::WHITE;

    // Toggle on click
    if clicked {
      *on = !*on;
    }

    // Draw track (rounded appearance via rect)
    self.draw_rect(rect, track_color);

    // Draw knob
    let knob_size = rect.height - 4.0;
    let knob_x = if *on {
      rect.right() - knob_size - 2.0
    } else {
      rect.x + 2.0
    };
    let knob_rect = Rect::new(knob_x, rect.y + 2.0, knob_size, knob_size);
    self.draw_rect(knob_rect, knob_color);

    clicked
  }

  // =========================================================================
  // Text Input Widget
  // =========================================================================

  /// Draw an interactive text input field.
  ///
  /// This is a retained-mode widget - the `state` persists across frames and
  /// holds the text buffer, cursor position, and selection state.
  ///
  /// Returns `true` if the text was modified this frame.
  ///
  /// # Arguments
  /// * `state` - Mutable reference to the text input state
  /// * `rect` - The bounding box for the text field
  /// * `placeholder` - Text to show when the field is empty
  /// * `dt` - Delta time for cursor blinking
  ///
  /// # Example
  ///
  /// ```ignore
  /// // In your scene struct:
  /// struct MyScene {
  ///     username: TextInputState,
  /// }
  ///
  /// // Each frame:
  /// let rect = Rect::new(100.0, 100.0, 200.0, 30.0);
  /// if ui.text_input(&mut self.username, rect, "Enter name...", dt) {
  ///     println!("Text changed: {}", self.username.text());
  /// }
  /// ```
  pub fn text_input(
    &mut self,
    state: &mut TextInputState,
    rect: Rect,
    placeholder: &str,
    #[allow(unused_variables)] dt: f32,
  ) -> bool {
    // Update blink timer using the real frame dt (not the user-passed dt which
    // may be the fixed-timestep dt, causing too-fast blink at high render FPS)
    state.update_blink(self.dt);
    // The field's text is one line of the default UI style; every position in
    // it comes from the text engine
    let style = TextStyle::default();
    let (pad_x, pad_y) = (8.0, 4.0);
    let text_x = rect.x + pad_x;

    // Handle focus on click
    let clicked = self.is_clicked(rect);
    let hovered = self.is_hovered(rect);

    if clicked {
      self.input.consume_mouse(MouseButton::Left);
      state.set_focused(true);
      // The character boundary nearest the click, scroll included
      let x = self.input.mouse_x() - text_x + state.scroll_offset();
      let byte = self.byte_at(state.text(), &style, x);
      state.set_cursor(byte, false);
    } else if state.is_focused()
      && self.input.mouse_down(MouseButton::Left)
      && !self.input.mouse_pressed(MouseButton::Left)
    {
      // Drag-to-select: mouse held after initial click, extend selection
      let x = self.input.mouse_x() - text_x + state.scroll_offset();
      let byte = self.byte_at(state.text(), &style, x);
      state.set_cursor(byte, true); // true = extend selection from anchor
    } else if self.input.mouse_went_down(MouseButton::Left) && !hovered {
      // Click outside always unfocuses, even if click was consumed by another widget.
      // This prevents dual-focus when clicking between text inputs rendered in any order.
      state.set_focused(false);
    }

    // Escape leaves the field, used up so the scene does not also act on it
    if state.is_focused() && self.input.key_pressed(KeyCode::Escape) {
      state.set_focused(false);
      self.input.consume_key(KeyCode::Escape);
    }

    if state.is_focused() {
      self.keyboard_focus = true;
    }

    // Handle keyboard input
    let modified = state.handle_input(&self.input);
    // A focused field owns the keyboard: what is typed into it is not also
    // gameplay input
    if state.is_focused() {
      self.input.consume_keyboard();
    }

    let bg_color = if state.is_focused() {
      Color::rgb(35, 35, 50)
    } else {
      Color::rgb(30, 30, 45)
    };
    let border_color = if state.is_focused() {
      Color::rgb(100, 140, 200)
    } else {
      Color::rgb(70, 70, 90)
    };
    let (text_color, placeholder_color, cursor_color, selection_color) = (
      Color::WHITE,
      Color::GRAY,
      Color::WHITE,
      Color::new(100, 140, 200, 100),
    );
    let border_thickness = 1.0;

    // Background and border
    self.draw_rect_bordered(rect, bg_color, 0.0, border_thickness, border_color);

    // The text line, centred vertically
    let line_height = self.line_height(&style);
    let text_y = rect.y + (rect.height - line_height) / 2.0;
    let text_width = rect.width - pad_x * 2.0;

    // Clip to text area for overflow handling
    let text_area = Rect::new(
      text_x,
      rect.y + pad_y,
      text_width,
      rect.height - pad_y * 2.0,
    );
    self.push_clip(text_area);

    // The caret, after any IME preedit being composed at it, kept in view
    let preedit = self.input.ime_preedit().to_owned();
    let preedit_width = if preedit.is_empty() {
      0.0
    } else {
      self.measure_text(preedit.as_str(), &style).x
    };
    let caret_x = self.caret_x(state.text(), &style, state.cursor());
    state.ensure_caret_visible(caret_x + preedit_width, text_width);
    let scroll = state.scroll_offset();
    let text = state.text();

    // Draw selection highlight if any
    if let Some((sel_start, sel_end)) = state.selection_range() {
      let (left, right) = self.selection_x(text, &style, sel_start..sel_end);
      let sel_rect = Rect::new(text_x + left - scroll, text_y, right - left, line_height);
      self.draw_rect(sel_rect, selection_color);
    }

    // Draw text or placeholder
    if text.is_empty() {
      self.draw_text(placeholder, text_x, text_y, placeholder_color);
    } else {
      // Draw the text content, shifted by scroll offset
      self.draw_text(text, text_x - scroll, text_y, text_color);

      // Draw IME preedit text at the caret, underlined
      if !preedit.is_empty() {
        let preedit_x = text_x + caret_x - scroll;
        self.draw_text(&preedit, preedit_x, text_y, text_color);
        self.draw_rect(
          Rect::new(preedit_x, text_y + line_height - 1.0, preedit_width, 1.0),
          cursor_color,
        );
      }
    }

    // Draw cursor if focused and visible (blinking)
    if state.should_draw_cursor() && !state.has_selection() {
      let cursor_x = text_x + caret_x + preedit_width - scroll;
      let cursor_rect = Rect::new(cursor_x, text_y, 2.0, line_height);
      self.draw_rect(cursor_rect, cursor_color);
    }

    self.pop_clip();

    modified
  }

  /// The caret's x before `byte` of one line of `text`, in UI units
  fn caret_x(&self, text: &str, style: &TextStyle, byte: usize) -> f32 {
    let scale = self.space.px_scale;
    self
      .engine()
      .map_or(0.0, |mut engine| engine.caret_x(text, style, byte, scale))
  }

  /// The character boundary nearest `x` (UI units) on one line of `text`
  fn byte_at(&self, text: &str, style: &TextStyle, x: f32) -> usize {
    let scale = self.space.px_scale;
    self
      .engine()
      .map_or(0, |mut engine| engine.byte_at(text, style, x, scale))
  }

  /// The left and right x (UI units) of a selection on one line of `text`
  fn selection_x(
    &self,
    text: &str,
    style: &TextStyle,
    range: std::ops::Range<usize>,
  ) -> (f32, f32) {
    let scale = self.space.px_scale;
    self.engine().map_or((0.0, 0.0), |mut engine| {
      engine.selection_x(text, style, range, scale)
    })
  }

  // =========================================================================
  // Dropdown Widget
  // =========================================================================

  /// A dropdown selector widget for choosing from a list of options.
  ///
  /// This is a retained-mode widget - the state persists between frames.
  /// When open, the dropdown draws above other content using the modal overlay.
  ///
  /// # Arguments
  /// * `state` - The retained dropdown state (must be stored between frames)
  /// * `rect` - The trigger button rectangle (closed dropdown size)
  /// * `options` - Slice of option labels to display
  ///
  /// # Returns
  /// `true` if the selection changed this frame
  ///
  /// # Input Consumption
  /// When a dropdown is open:
  /// - Escape key is consumed (check `ui.escape_consumed()` afterwards)
  /// - Clicks on the dropdown list are consumed (check `ui.click_consumed()`)
  ///
  /// # Tab Navigation
  /// If the dropdown has an ID > 0, Tab/Shift+Tab can navigate between dropdowns.
  /// Space or Enter opens a focused dropdown.
  ///
  /// # Note
  /// Open dropdown lists draw on the popup layer, above everything the scene
  /// drew; the engine flushes them after each scene's `ui`.
  ///
  /// # Example
  /// ```ignore
  /// let options = ["Easy", "Medium", "Hard"];
  /// if ui.dropdown(&mut self.difficulty, rect, &options) {
  ///     println!("Selected: {}", options[self.difficulty.selected()]);
  /// }
  /// ```
  pub fn dropdown(&mut self, state: &mut DropdownState, rect: Rect, options: &[&str]) -> bool {
    if options.is_empty() {
      return false;
    }

    let old_selected = state.selected();

    let (bg_color, bg_hover, text_color, border_color, focus_color) = (
      Color::rgb(50, 50, 65),
      Color::rgb(65, 65, 85),
      Color::WHITE,
      Color::rgb(80, 80, 100),
      Color::rgb(100, 150, 255),
    );

    let trigger_hovered = self.is_hovered(rect);
    let mouse_pressed = self.input.mouse_pressed(MouseButton::Left);
    let mouse_x = self.input.mouse_x();
    let mouse_y = self.input.mouse_y();

    // Handle focus via click on trigger - this dropdown becomes focused
    if trigger_hovered && mouse_pressed {
      state.set_focused(true);
      // Track this as the focused dropdown so we can unfocus others
      if state.id() > 0 {
        self.focused_dropdown_id = state.id();
      }
    }

    // Sync focus: if another dropdown was clicked this frame, unfocus this one
    // This ensures only one dropdown has focus at a time
    if state.id() > 0 && self.focused_dropdown_id > 0 && self.focused_dropdown_id != state.id() {
      state.set_focused(false);
    }

    if state.is_open() {
      self.keyboard_focus = true;
    }

    // Determine if we should show focus ring
    let show_focus = state.is_focused();

    // Draw the trigger button
    let trigger_bg = if trigger_hovered || state.is_open() {
      bg_hover
    } else {
      bg_color
    };
    self.draw_rect(rect, trigger_bg);

    // Draw focus ring if focused
    if show_focus {
      self.draw_rect_outline(rect, focus_color, 2.0);
    } else {
      self.draw_rect_outline(rect, border_color, 1.0);
    }

    // Draw selected option text
    let selected_text = if state.selected() < options.len() {
      options[state.selected()]
    } else {
      ""
    };
    let text_x = rect.x + 8.0;
    let line = self.line_height(&TextStyle::default());
    let text_y = rect.y + (rect.height - line) / 2.0;
    self.draw_text(selected_text, text_x, text_y, text_color);

    // Draw dropdown arrow
    let arrow_size = 6.0;
    let arrow_x = rect.x + rect.width - 16.0;
    let arrow_y = rect.y + rect.height / 2.0;
    self.draw_dropdown_arrow(arrow_x, arrow_y, arrow_size, text_color, state.is_open());

    // Handle trigger click to toggle dropdown
    if trigger_hovered && mouse_pressed {
      state.toggle();
      self.input.consume_mouse(MouseButton::Left);
    }

    // Note: Space/Enter to open focused dropdown is handled in handle_dropdown_tab_navigation()
    // which runs after all dropdowns are processed, ensuring focus state is correct.

    // Handle keyboard when open
    if state.is_open() {
      let max_index = options.len().saturating_sub(1);

      if self.input.key_pressed(winit::keyboard::KeyCode::ArrowUp) {
        state.hover_up();
      }
      if self.input.key_pressed(winit::keyboard::KeyCode::ArrowDown) {
        state.hover_down(max_index);
      }
      // Both Enter and Space select the hovered item
      if self.input.key_pressed(winit::keyboard::KeyCode::Enter)
        || self.input.key_pressed(winit::keyboard::KeyCode::Space)
      {
        state.select_hovered();
      }
      if self.input.key_pressed(winit::keyboard::KeyCode::Escape) {
        state.close();
        self.input.consume_key(KeyCode::Escape);
      }

      // Calculate list rect for click-outside detection
      let item_height = rect.height;
      let list_height = options.len() as f32 * item_height;
      let list_rect = Rect::new(rect.x, rect.y + rect.height, rect.width, list_height);

      // Check if click is outside the dropdown entirely
      let full_dropdown = Rect::new(rect.x, rect.y, rect.width, rect.height + list_height);
      if mouse_pressed && !full_dropdown.contains(mouse_x, mouse_y) {
        state.close();
      }

      // Handle clicks on options (must be done here for state mutation)
      for (i, _) in options.iter().enumerate() {
        let option_rect = Rect::new(
          rect.x,
          rect.y + rect.height + i as f32 * item_height,
          rect.width,
          item_height,
        );
        let option_hovered = option_rect.contains(mouse_x, mouse_y);

        if option_hovered && mouse_pressed {
          state.set_selected(i);
          state.close();
          self.input.consume_mouse(MouseButton::Left);
        }
      }

      // Consume click if anywhere on the dropdown to prevent passthrough
      if mouse_pressed && full_dropdown.contains(mouse_x, mouse_y) {
        self.input.consume_mouse(MouseButton::Left);
      }

      // Queue the dropdown list to be drawn later (on top of everything)
      // Only add if still open after handling clicks
      if state.is_open() {
        self.pending_dropdowns.push(PendingDropdown {
          trigger_rect: rect,
          list_rect,
          hover_index: state.hover_index(),
          options: options.iter().map(|s| s.to_string()).collect(),
        });
      }
    }

    state.selected() != old_selected
  }

  /// Handle Tab navigation between multiple dropdowns.
  ///
  /// Call this once per frame after all dropdowns to enable Tab/Shift+Tab
  /// navigation. Provide all dropdown states in tab order.
  ///
  /// # Example
  /// ```ignore
  /// ui.dropdown(&mut self.dropdown1, rect1, &options1);
  /// ui.dropdown(&mut self.dropdown2, rect2, &options2);
  /// ui.dropdown(&mut self.dropdown3, rect3, &options3);
  ///
  /// // Enable tab navigation
  /// ui.handle_dropdown_tab_navigation(&mut [
  ///     &mut self.dropdown1,
  ///     &mut self.dropdown2,
  ///     &mut self.dropdown3,
  /// ]);
  /// ```
  pub fn handle_dropdown_tab_navigation(&mut self, dropdowns: &mut [&mut DropdownState]) {
    if dropdowns.is_empty() {
      return;
    }

    // Clear just_closed flags at the start of each frame's handling
    // This must happen BEFORE we check for open key, so a dropdown closed
    // on a previous frame doesn't block opening on this frame
    for dropdown in dropdowns.iter_mut() {
      if dropdown.was_just_closed() {
        // Don't clear yet - we'll use this to prevent re-open this frame
      }
    }

    // If a dropdown was clicked this frame (focused_dropdown_id was set),
    // ensure only that one has focus
    if self.focused_dropdown_id > 0 {
      for dropdown in dropdowns.iter_mut() {
        let should_focus = dropdown.id() == self.focused_dropdown_id;
        dropdown.set_focused(should_focus);
      }
    }

    // Don't do keyboard navigation if any dropdown is open
    if dropdowns.iter().any(|d| d.is_open()) {
      return;
    }

    // Handle Space/Enter to open the focused dropdown
    // This must be done HERE because focus may have been set by Tab on a previous frame
    let open_key = self.input.key_pressed(winit::keyboard::KeyCode::Space)
      || self.input.key_pressed(winit::keyboard::KeyCode::Enter);
    if open_key {
      for dropdown in dropdowns.iter_mut() {
        // Don't re-open if this dropdown was just closed this frame (by selecting an item)
        if dropdown.is_focused() && !dropdown.was_just_closed() {
          dropdown.open();
          return; // Only open one
        }
      }
    }

    // Clear just_closed flags at the END so they persist for one frame
    for dropdown in dropdowns.iter_mut() {
      dropdown.clear_just_closed();
    }

    let tab_pressed = self.input.key_pressed(winit::keyboard::KeyCode::Tab);
    if !tab_pressed {
      return;
    }

    let shift_held = self.input.key_down(winit::keyboard::KeyCode::ShiftLeft)
      || self.input.key_down(winit::keyboard::KeyCode::ShiftRight);

    // Find currently focused dropdown index
    let current_idx = dropdowns.iter().position(|d| d.is_focused());

    let next_idx = match current_idx {
      Some(idx) => {
        if shift_held {
          // Shift+Tab: go backwards
          if idx == 0 {
            dropdowns.len() - 1
          } else {
            idx - 1
          }
        } else {
          // Tab: go forwards
          (idx + 1) % dropdowns.len()
        }
      }
      None => {
        // Nothing focused, start at first (or last if shift)
        if shift_held { dropdowns.len() - 1 } else { 0 }
      }
    };

    // Update focus
    for (i, dropdown) in dropdowns.iter_mut().enumerate() {
      dropdown.set_focused(i == next_idx);
    }
  }

  /// Draw all pending dropdown lists on the popup layer, above every
  /// scene's page. The engine calls this after each scene's `ui`.
  pub(crate) fn finish_dropdowns(&mut self) {
    self.finish_dropdowns_on(POPUP_LAYER);
  }

  /// Record what follows on draw `layer` (the engine draws its overlays and
  /// the console on `OVERLAY_LAYER`)
  pub(crate) fn set_layer(&mut self, layer: u8) {
    self.layer = layer;
  }

  /// `finish_dropdowns` onto draw layer `layer`: a scene under others keeps
  /// its lists on layer 0 so the scenes above still cover them
  pub(crate) fn finish_dropdowns_on(&mut self, layer: u8) {
    // Take pending dropdowns to avoid borrow issues
    let dropdowns = std::mem::take(&mut self.pending_dropdowns);

    if dropdowns.is_empty() {
      return;
    }

    let (text_color, border_color, option_bg, option_hover) = (
      Color::WHITE,
      Color::rgb(80, 80, 100),
      Color::rgb(40, 40, 55),
      Color::rgb(60, 60, 80),
    );

    let mouse_x = self.input.mouse_x();
    let mouse_y = self.input.mouse_y();
    let line = self.line_height(&TextStyle::default());
    let page_layer = std::mem::replace(&mut self.layer, layer);

    // Draw each pending dropdown list
    for dropdown in dropdowns {
      let item_height = dropdown.trigger_rect.height;

      // Draw list background
      self.draw_rect(dropdown.list_rect, option_bg);
      self.draw_rect_outline(dropdown.list_rect, border_color, 1.0);

      // Draw options
      for (i, option) in dropdown.options.iter().enumerate() {
        let option_rect = Rect::new(
          dropdown.trigger_rect.x,
          dropdown.trigger_rect.y + dropdown.trigger_rect.height + i as f32 * item_height,
          dropdown.trigger_rect.width,
          item_height,
        );

        let option_hovered = option_rect.contains(mouse_x, mouse_y);
        let is_keyboard_hover = i == dropdown.hover_index;

        // Highlight hovered or keyboard-selected option
        if option_hovered || is_keyboard_hover {
          self.draw_rect(option_rect, option_hover);
        }

        // Draw option text
        let opt_text_y = option_rect.y + (option_rect.height - line) / 2.0;
        self.draw_text(option, option_rect.x + 8.0, opt_text_y, text_color);
      }
    }
    self.layer = page_layer;
  }

  /// Draw a dropdown arrow indicator.
  fn draw_dropdown_arrow(&mut self, x: f32, y: f32, size: f32, color: Color, pointing_up: bool) {
    // Draw a simple triangle arrow
    // For now, we'll draw it using small rectangles (since we don't have triangle primitives)
    let half = size / 2.0;
    if pointing_up {
      // Arrow pointing up (dropdown is open)
      self.draw_rect(Rect::new(x - half, y, size, 2.0), color);
      self.draw_rect(Rect::new(x - half + 1.0, y - 2.0, size - 2.0, 2.0), color);
      self.draw_rect(Rect::new(x - half + 2.0, y - 4.0, size - 4.0, 2.0), color);
    } else {
      // Arrow pointing down (dropdown is closed)
      self.draw_rect(Rect::new(x - half, y - 2.0, size, 2.0), color);
      self.draw_rect(Rect::new(x - half + 1.0, y, size - 2.0, 2.0), color);
      self.draw_rect(Rect::new(x - half + 2.0, y + 2.0, size - 4.0, 2.0), color);
    }
  }

  // =========================================================================
  // Modal & Overlay Support
  // =========================================================================

  /// Draw a modal overlay that dims the background.
  ///
  /// This clears all current clips and draws a semi-transparent rectangle
  /// over the entire screen. Useful as a backdrop for modal dialogs.
  ///
  /// # Arguments
  /// * `dim_color` - The color to overlay (typically semi-transparent black)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Dim the background
  /// ui.draw_modal_overlay(Color::rgba(0, 0, 0, 128));
  ///
  /// // Draw the modal dialog on top
  /// let dialog = Rect::centered_at(center_x, center_y, 300.0, 200.0);
  /// ui.draw_panel(dialog, Color::rgb(40, 40, 50), Some(Color::WHITE));
  /// ui.draw_text_centered("Confirm?", dialog, Color::WHITE);
  /// ```
  pub fn draw_modal_overlay(&mut self, dim_color: Color) {
    self.with_no_clip(|ui| {
      let size = ui.space.size;
      ui.draw_rect(Rect::new(0.0, 0.0, size.x, size.y), dim_color);
    });
  }

  /// Check if a point is within the current clip region.
  ///
  /// Returns `true` if no clip is active or if the point is inside the clip.
  pub fn point_in_clip(&self, x: f32, y: f32) -> bool {
    match self.current_clip {
      Some(clip) => clip.contains(x, y),
      None => {
        // Check screen bounds
        x >= 0.0 && y >= 0.0 && x < self.space.size.x && y < self.space.size.y
      }
    }
  }

  // =========================================================================
  // Focus Management
  // =========================================================================

  /// Handle Tab/Shift+Tab navigation between focusable widgets.
  ///
  /// Call this once per frame after all widgets to enable Tab navigation
  /// across different widget types (text inputs, dropdowns, etc.).
  ///
  /// # Arguments
  /// * `widgets` - Slice of focusable widgets in tab order
  ///
  /// # Example
  /// ```ignore
  /// ui.text_input(&mut self.username, rect1, "Username", dt);
  /// ui.text_input(&mut self.password, rect2, "Password", dt);
  /// ui.dropdown(&mut self.role, rect3, &roles);
  ///
  /// // Enable tab navigation across all widgets
  /// ui.handle_tab_navigation(&mut [
  ///     &mut self.username,
  ///     &mut self.password,
  ///     &mut self.role,
  /// ]);
  /// ```
  pub fn handle_tab_navigation(&mut self, widgets: &mut [&mut dyn Focusable]) {
    if widgets.is_empty() {
      return;
    }

    // Don't do keyboard navigation if any widget is capturing input
    // (e.g., open dropdown handling arrow keys)
    if widgets.iter().any(|w| w.is_capturing()) {
      return;
    }

    let tab_pressed = self.input.key_pressed(winit::keyboard::KeyCode::Tab);
    if !tab_pressed {
      return;
    }

    let shift_held = self.input.key_down(winit::keyboard::KeyCode::ShiftLeft)
      || self.input.key_down(winit::keyboard::KeyCode::ShiftRight);

    // Find currently focused widget index
    let current_idx = widgets.iter().position(|w| w.is_focused());

    let next_idx = match current_idx {
      Some(idx) => {
        if shift_held {
          // Shift+Tab: go backwards (wrap around)
          if idx == 0 { widgets.len() - 1 } else { idx - 1 }
        } else {
          // Tab: go forwards (wrap around)
          (idx + 1) % widgets.len()
        }
      }
      None => {
        // Nothing focused, start at first (or last if shift)
        if shift_held { widgets.len() - 1 } else { 0 }
      }
    };

    // Blur current widget
    if let Some(idx) = current_idx {
      widgets[idx].set_focused(false);
      widgets[idx].on_blur();
    }

    // Focus next widget
    widgets[next_idx].set_focused(true);
    widgets[next_idx].on_focus();
  }

  /// Draw a focus ring (outline) around a rectangle.
  ///
  /// Use this to indicate which widget currently has keyboard focus.
  /// The ring is drawn outside the widget bounds.
  ///
  /// # Arguments
  /// * `rect` - The widget bounds to draw around
  /// * `color` - The focus ring color (typically a bright accent color)
  /// * `thickness` - Ring thickness in pixels (typically 2.0)
  /// * `offset` - Gap between widget and ring (typically 1.0-2.0)
  ///
  /// # Example
  /// ```ignore
  /// if self.button_focused {
  ///     ui.draw_focus_ring(button_rect, Color::rgb(100, 150, 255), 2.0, 2.0);
  /// }
  /// ui.button("Submit").at(button_rect);
  /// ```
  pub fn draw_focus_ring(&mut self, rect: Rect, color: Color, thickness: f32, offset: f32) {
    let expanded = rect.expand(offset + thickness);
    let inner = rect.expand(offset);

    // Draw the four sides of the ring
    // Top
    self.draw_rect(
      Rect::new(expanded.x, expanded.y, expanded.width, thickness),
      color,
    );
    // Bottom
    self.draw_rect(
      Rect::new(
        expanded.x,
        inner.y + inner.height,
        expanded.width,
        thickness,
      ),
      color,
    );
    // Left
    self.draw_rect(
      Rect::new(expanded.x, inner.y, thickness, inner.height),
      color,
    );
    // Right
    self.draw_rect(
      Rect::new(inner.x + inner.width, inner.y, thickness, inner.height),
      color,
    );
  }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use glam::Vec2;
  use winit::event::ElementState;

  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::input::Input;
  use crate::core::render::UiSpace;
  use crate::core::text::{TextEngine, TextLayout, TextStyle};
  use crate::core::ui::immediate::{UiCommand, UiCommands};
  use crate::core::ui::test_util::{UiDriver, context as ui_context, text_engine};

  fn context<'a>(c: &'a mut UiCommands, t: &'a mut TextEngine) -> UiContext<'a> {
    ui_context(c, t, (320.0, 180.0))
  }

  /// A context `size` UI units at `px_scale`, reading `input`
  fn context_with<'a>(
    commands: &'a mut UiCommands,
    text: &'a mut TextEngine,
    size: (f32, f32),
    px_scale: f32,
    input: UiInput,
  ) -> UiContext<'a> {
    let space = UiSpace {
      size: Vec2::new(size.0, size.1),
      px_scale,
      image: (
        (size.0 * px_scale).ceil() as u32,
        (size.1 * px_scale).ceil() as u32,
      ),
      mouse_scale: 1.0,
    };
    crate::core::ui::test_util::context_in(commands, text, space, input)
  }

  /// Input with the mouse at (x, y) and, if `press`, the left button just
  /// pressed
  fn mouse_at(x: f32, y: f32, press: bool) -> UiInput {
    let mut input = Input::new();
    input.handle_mouse_move(x as f64, y as f64);
    if press {
      input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
    }
    UiInput::snapshot(&input)
  }

  /// The rect commands' rects, in order
  fn rects(commands: &UiCommands) -> Vec<Rect> {
    commands
      .commands()
      .filter_map(|command| match command {
        UiCommand::Rect { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect()
  }

  // ==========================================================================
  // Recording
  // ==========================================================================

  #[test]
  fn drawing_records_commands_in_order() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_rect(Rect::new(1.0, 2.0, 3.0, 4.0), Color::RED);
    ui.draw_text("hi", 5.0, 6.0, Color::WHITE);
    drop(ui);
    let kinds: Vec<&str> = commands.commands().map(UiCommand::kind_name).collect();
    assert_eq!(kinds, vec!["rect", "text"]);
  }

  #[test]
  fn clips_nest_without_a_limit_and_an_empty_pop_warns_once() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    for i in 0..40 {
      ui.push_clip(Rect::new(i as f32, 0.0, 200.0, 100.0));
    }
    // Inside the nested clip (a rect wholly outside it is skipped)
    ui.draw_rect(Rect::new(50.0, 0.0, 10.0, 10.0), Color::RED);
    for _ in 0..42 {
      ui.pop_clip();
    }
    drop(ui);
    assert_eq!(
      commands.commands().next().and_then(UiCommand::clip),
      Some(Rect::new(39.0, 0.0, 161.0, 100.0))
    );
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("pop_clip with no clip"))
      .count();
    assert_eq!(warned, 1);
  }

  #[test]
  fn measured_and_drawn_text_agree() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let (w, h) = ui.text_size("Measure me");
    let measured = ui.measure_text("Measure me", &TextStyle::default());
    assert_eq!((w, h), (measured.x.ceil() as u32, measured.y.ceil() as u32));
    ui.draw_text_anchored("Measure me", Anchor::Center, 0.0, 0.0, Color::WHITE);
    drop(ui);
    let origin = commands
      .commands()
      .find_map(UiCommand::text_origin)
      .unwrap();
    assert!((origin.x - (160.0 - measured.x / 2.0)).abs() < 0.01);
  }

  #[test]
  fn draw_rect_outline_is_one_bordered_command() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_rect_outline(Rect::new(0.0, 0.0, 20.0, 10.0), Color::WHITE, 2.0);
    drop(ui);
    assert_eq!(commands.commands().count(), 1);
  }

  #[test]
  fn degenerate_rects_and_rects_outside_the_clip_are_skipped() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_rect(Rect::new(0.0, 0.0, 0.0, 10.0), Color::RED);
    ui.draw_rect(Rect::new(0.0, 0.0, -5.0, 10.0), Color::RED);
    ui.draw_rect(Rect::new(f32::NAN, 0.0, 5.0, 10.0), Color::RED);
    ui.draw_rect(Rect::new(0.0, 0.0, f32::INFINITY, 10.0), Color::RED);
    ui.push_clip(Rect::new(100.0, 100.0, 50.0, 50.0));
    ui.draw_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::RED);
    ui.draw_rect(Rect::new(140.0, 140.0, 20.0, 20.0), Color::GREEN);
    drop(ui);
    assert_eq!(rects(&commands), [Rect::new(140.0, 140.0, 20.0, 20.0)]);
  }

  #[test]
  fn panels_and_rounded_rects_record_their_radius_and_border() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let rect = Rect::new(10.0, 10.0, 40.0, 20.0);
    ui.draw_panel(rect, Color::BLUE, Some(Color::WHITE));
    ui.draw_rounded_rect(rect, 6.0, Color::RED);
    ui.draw_rect_bordered(rect, Color::GREEN, 4.0, 2.0, Color::BLACK);
    drop(ui);
    type Look = (Color, f32, Option<(f32, Color)>);
    let looks: Vec<Look> = commands
      .commands()
      .filter_map(|command| match command {
        UiCommand::Rect {
          fill,
          radius,
          border,
          ..
        } => Some((*fill, *radius, *border)),
        _ => None,
      })
      .collect();
    assert_eq!(
      looks,
      [
        (Color::BLUE, 0.0, Some((1.0, Color::WHITE))),
        (Color::RED, 6.0, None),
        (Color::GREEN, 4.0, Some((2.0, Color::BLACK))),
      ]
    );
  }

  #[test]
  fn the_modal_overlay_covers_the_whole_ui_whatever_the_clip() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.push_clip(Rect::new(10.0, 10.0, 20.0, 20.0));
    ui.draw_modal_overlay(Color::new(0, 0, 0, 128));
    assert_eq!(ui.clip_depth(), 1, "the clip comes back");
    drop(ui);
    let overlay = commands.commands().next().unwrap();
    assert_eq!(overlay.clip(), None);
    assert_eq!(rects(&commands), [Rect::new(0.0, 0.0, 320.0, 180.0)]);
  }

  #[test]
  fn sprites_record_their_rect_and_a_nine_slice_records_nine() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(16, 8, 255, 255, 255, 255), "s");
    ui.draw_sprite(&sprite, 1.0, 2.0);
    ui.draw_sprite_scaled(&sprite, 1.0, 2.0, 2.0);
    ui.draw_sprite_rect(&sprite, Rect::new(0.0, 0.0, 5.0, 5.0), Color::RED);
    drop(ui);
    let sprites: Vec<(Rect, [f32; 4], Color)> = commands
      .commands()
      .filter_map(|command| match command {
        UiCommand::Sprite { rect, uv, tint, .. } => Some((*rect, *uv, *tint)),
        _ => None,
      })
      .collect();
    let full = [0.0, 0.0, 1.0, 1.0];
    assert_eq!(
      sprites,
      [
        (Rect::new(1.0, 2.0, 16.0, 8.0), full, Color::WHITE),
        (Rect::new(1.0, 2.0, 32.0, 16.0), full, Color::WHITE),
        (Rect::new(0.0, 0.0, 5.0, 5.0), full, Color::RED),
      ]
    );

    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_nine_slice(
      &sprite,
      Rect::new(0.0, 0.0, 100.0, 50.0),
      [4.0, 2.0, 4.0, 2.0],
      Color::WHITE,
    );
    drop(ui);
    let pieces: Vec<(Rect, [f32; 4])> = commands
      .commands()
      .filter_map(|command| match command {
        UiCommand::Sprite { rect, uv, .. } => Some((*rect, *uv)),
        _ => None,
      })
      .collect();
    assert_eq!(pieces.len(), 9);
    // Corners at their pixel size, the centre stretched both ways
    assert_eq!(
      pieces[0],
      (Rect::new(0.0, 0.0, 4.0, 2.0), [0.0, 0.0, 0.25, 0.25])
    );
    assert_eq!(
      pieces[4],
      (Rect::new(4.0, 2.0, 92.0, 46.0), [0.25, 0.25, 0.75, 0.75])
    );
    assert_eq!(
      pieces[8],
      (Rect::new(96.0, 48.0, 4.0, 2.0), [0.75, 0.75, 1.0, 1.0])
    );
    let area: f32 = pieces.iter().map(|(r, _)| r.width * r.height).sum();
    assert_eq!(area, 100.0 * 50.0, "the pieces tile the rect");
  }

  #[test]
  fn lines_need_two_points_and_a_positive_width() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let two = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
    ui.draw_line(&two[..1], 2.0, Color::WHITE);
    ui.draw_line(&two, 0.0, Color::WHITE);
    ui.draw_line(&two, f32::NAN, Color::WHITE);
    ui.draw_line(&two, 2.0, Color::WHITE);
    drop(ui);
    let kinds: Vec<&str> = commands.commands().map(UiCommand::kind_name).collect();
    assert_eq!(kinds, ["line"]);
  }

  #[test]
  fn translucent_shorthand_text_fades_through_opacity() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_text("fading", 0.0, 0.0, Color::new(255, 255, 255, 51));
    ui.draw_text("fading", 0.0, 0.0, Color::new(255, 255, 255, 204));
    drop(ui);
    let drawn: Vec<(f32, Color, *const TextLayout)> = commands
      .commands()
      .filter_map(|command| match command {
        UiCommand::Text {
          layout, opacity, ..
        } => Some((*opacity, layout.styles[0].color, Arc::as_ptr(layout))),
        _ => None,
      })
      .collect();
    assert!((drawn[0].0 - 0.2).abs() < 1e-6 && (drawn[1].0 - 0.8).abs() < 1e-6);
    assert_eq!(drawn[0].1, Color::WHITE, "laid out opaque");
    assert_eq!(drawn[0].2, drawn[1].2, "one cached layout for every alpha");
  }

  #[test]
  fn text_is_laid_out_at_the_ui_scale_and_measured_in_ui_units() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let at_one = {
      let ui = context(&mut commands, &mut text);
      ui.measure_text("Scale", &TextStyle::default())
    };
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (320.0, 180.0),
      2.0,
      UiInput::default(),
    );
    assert_eq!(ui.ui_scale(), 2.0);
    let at_two = ui.measure_text("Scale", &TextStyle::default());
    assert!(
      (at_one - at_two).abs().max_element() < 1.0,
      "{at_one} vs {at_two}"
    );
    ui.draw_text("Scale", 0.0, 0.0, Color::WHITE);
    drop(ui);
    let scale = commands.commands().find_map(|command| match command {
      UiCommand::Text { layout, .. } => Some(layout.scale),
      _ => None,
    });
    assert_eq!(scale, Some(2.0), "glyphs at render pixels");
  }

  #[test]
  fn newlines_break_lines() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = context(&mut commands, &mut text);
    let one = ui.text_size("a");
    let two = ui.text_size("a\nb");
    assert!(two.1 + 1 >= one.1 * 2, "{one:?} {two:?}");
  }

  // ==========================================================================
  // Clipping
  // ==========================================================================

  #[test]
  fn clip_stack_push_pop() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (100.0, 100.0));

    assert_eq!(ui.clip_depth(), 0);
    assert!(!ui.has_clip());

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    assert_eq!(ui.clip_depth(), 1);
    assert!(ui.has_clip());
    assert_eq!(ui.current_clip(), Some(Rect::new(10.0, 10.0, 50.0, 50.0)));

    ui.pop_clip();
    assert_eq!(ui.clip_depth(), 0);
    assert!(!ui.has_clip());
  }

  #[test]
  fn nested_clips_intersect() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (100.0, 100.0));

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    ui.push_clip(Rect::new(30.0, 30.0, 50.0, 50.0));

    assert_eq!(ui.current_clip(), Some(Rect::new(30.0, 30.0, 30.0, 30.0)));
  }

  #[test]
  fn clear_clips_resets_stack() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (100.0, 100.0));

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    ui.push_clip(Rect::new(20.0, 20.0, 30.0, 30.0));
    assert_eq!(ui.clip_depth(), 2);

    ui.clear_clips();
    assert_eq!(ui.clip_depth(), 0);
    assert!(!ui.has_clip());
  }

  #[test]
  fn with_no_clip_restores_state() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (100.0, 100.0));

    ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
    let original_clip = ui.current_clip();

    let inside_clip = ui.with_no_clip(|ui| {
      assert!(!ui.has_clip());
      42
    });

    assert_eq!(inside_clip, 42);
    assert_eq!(ui.current_clip(), original_clip);
    assert_eq!(ui.clip_depth(), 1);
  }

  #[test]
  fn point_in_clip_checks_bounds() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (100.0, 100.0));

    // No clip - check screen bounds
    assert!(ui.point_in_clip(50.0, 50.0));
    assert!(!ui.point_in_clip(-1.0, 50.0));
    assert!(!ui.point_in_clip(100.0, 50.0));

    // With clip
    ui.push_clip(Rect::new(20.0, 20.0, 40.0, 40.0));
    assert!(ui.point_in_clip(30.0, 30.0));
    assert!(!ui.point_in_clip(10.0, 30.0));
    assert!(!ui.point_in_clip(70.0, 30.0));
  }

  #[test]
  fn empty_clip_intersection() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (100.0, 100.0));

    ui.push_clip(Rect::new(10.0, 10.0, 20.0, 20.0));
    ui.push_clip(Rect::new(50.0, 50.0, 20.0, 20.0));

    let clip = ui.current_clip().unwrap();
    assert!(clip.width == 0.0 || clip.height == 0.0);
  }

  #[test]
  fn rect_intersection_calculation() {
    let a = Rect::new(10.0, 10.0, 50.0, 50.0);
    let b = Rect::new(30.0, 30.0, 50.0, 50.0);
    assert_eq!(
      UiContext::intersect_rects(a, b),
      Rect::new(30.0, 30.0, 30.0, 30.0)
    );

    let c = Rect::new(0.0, 0.0, 10.0, 10.0);
    let d = Rect::new(20.0, 20.0, 10.0, 10.0);
    let result = UiContext::intersect_rects(c, d);
    assert!(result.width == 0.0 || result.height == 0.0);
  }

  // ==========================================================================
  // Measuring and wrapping
  // ==========================================================================

  #[test]
  fn wrap_text_breaks_between_words() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = ui_context(&mut commands, &mut text, (100.0, 100.0));
    assert_eq!(ui.wrap_text("Hello World", 50), ["Hello", "World"]);
  }

  #[test]
  fn wrapped_text_size_is_narrow_and_two_lines_tall() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = ui_context(&mut commands, &mut text, (100.0, 100.0));
    let (w, h) = ui.wrapped_text_size("Hello World", 50);
    let line = ui.line_height(&TextStyle::default());
    assert!(w > 0 && w <= 50, "{w}");
    assert!((h as f32 - 2.0 * line).abs() <= 1.0, "{h} vs {line}");
  }

  #[test]
  fn text_size_is_the_measured_size_rounded_up() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = ui_context(&mut commands, &mut text, (100.0, 100.0));
    let (w, h) = ui.text_size("Test");
    let size = ui.measure_text("Test", &TextStyle::default());
    assert_eq!((w, h), (size.x.ceil() as u32, size.y.ceil() as u32));
    assert!(w > 0);
    assert_eq!(h, ui.line_height(&TextStyle::default()).ceil() as u32);
  }

  #[test]
  fn ui_context_default_scale_is_1() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = ui_context(&mut commands, &mut text, (100.0, 100.0));
    assert_eq!(ui.ui_scale(), 1.0);
    assert_eq!(ui.screen_size(), (100, 100));
  }

  #[test]
  fn the_mouse_is_read_in_ui_units() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let space = UiSpace {
      size: Vec2::new(160.0, 90.0),
      px_scale: 2.0,
      image: (320, 180),
      mouse_scale: 0.5,
    };
    let ui = crate::core::ui::test_util::context_in(
      &mut commands,
      &mut text,
      space,
      mouse_at(100.0, 60.0, false),
    );
    assert_eq!(ui.input().mouse_position(), (50.0, 30.0));
    assert!(ui.is_hovered(Rect::new(45.0, 25.0, 10.0, 10.0)));
  }

  // ==========================================================================
  // Click consumption
  // ==========================================================================

  #[test]
  fn the_pointer_over_or_held_by_a_button_is_the_uis() {
    let mut d = UiDriver::new((200.0, 200.0));
    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let draw = |ui: &mut UiContext<'_>| {
      ui.button("Test").at(rect);
      ui.wants_pointer()
    };
    d.move_to(150.0, 150.0);
    d.frame(draw);
    assert!(!d.frame(draw), "beside the button");
    d.move_to(50.0, 50.0);
    assert!(d.frame(draw), "over it");
    d.press();
    d.frame(draw);
    d.move_to(150.0, 150.0);
    assert!(d.frame(draw), "held by it, though the pointer left");
    d.release();
    d.frame(draw);
    assert!(!d.frame(draw), "let go");
  }

  #[test]
  fn only_the_topmost_of_two_overlapping_buttons_clicks() {
    let mut d = UiDriver::new((200.0, 200.0));
    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let draw = |ui: &mut UiContext<'_>| {
      let first = ui.button("First").at(rect).clicked();
      let second = ui.button("Second").at(rect).clicked();
      (first, second)
    };
    d.move_to(50.0, 50.0);
    d.frame(draw);
    d.press();
    d.frame(draw);
    d.release();
    assert_eq!(d.frame(draw), (false, true), "the one drawn last is on top");
  }

  #[test]
  fn checkbox_consumes_click() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, true),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut checked = false;
    let toggled = ui.checkbox(rect, &mut checked, None);

    assert!(toggled, "Checkbox should register click");
    assert!(checked, "Checkbox should be checked");
    assert!(ui.click_consumed(), "Click should be consumed");
  }

  #[test]
  fn toggle_consumes_click() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, true),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut on = false;
    let toggled = ui.toggle(rect, &mut on);

    assert!(toggled, "Toggle should register click");
    assert!(on, "Toggle should be on");
    assert!(ui.click_consumed(), "Click should be consumed");
  }

  #[test]
  fn text_input_consumes_click() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, true),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut state = TextInputState::new();
    ui.text_input(&mut state, rect, "placeholder", 0.016);

    assert!(state.is_focused(), "Text input should be focused");
    assert!(ui.click_consumed(), "Click should be consumed");
  }

  #[test]
  fn consume_escape_sets_flag() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (200.0, 200.0));

    assert!(!ui.escape_consumed());
    ui.consume_escape();
    assert!(ui.escape_consumed());
  }

  #[test]
  fn checkbox_not_toggled_when_click_consumed() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, true),
    );

    ui.consume_click();
    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let mut checked = false;
    let toggled = ui.checkbox(rect, &mut checked, None);

    assert!(!toggled, "Checkbox should not toggle when click consumed");
    assert!(!checked, "Checkbox state should be unchanged");
  }

  #[test]
  fn text_input_unfocuses_even_when_click_consumed() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    // Click outside the text input rect
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(180.0, 180.0, true),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 30.0);
    let mut state = TextInputState::new();
    state.set_focused(true);

    // Another widget handled the click first
    ui.consume_click();

    // Clicking outside always unfocuses, consumed or not, so two fields
    // drawn in any order are never both focused
    ui.text_input(&mut state, rect, "placeholder", 0.016);
    assert!(
      !state.is_focused(),
      "Text input should unfocus when clicked outside, even if click was consumed"
    );
  }

  #[test]
  fn clicking_second_text_input_unfocuses_first() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    // A click inside field B, which is drawn before field A
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (400.0, 200.0),
      1.0,
      mouse_at(250.0, 20.0, true),
    );

    let rect_a = Rect::new(10.0, 10.0, 150.0, 30.0);
    let rect_b = Rect::new(200.0, 10.0, 150.0, 30.0);

    let mut state_a = TextInputState::new();
    let mut state_b = TextInputState::new();
    state_a.set_focused(true);

    ui.text_input(&mut state_b, rect_b, "Field B", 0.016);
    ui.text_input(&mut state_a, rect_a, "Field A", 0.016);

    assert!(state_b.is_focused(), "Field B should be focused");
    assert!(!state_a.is_focused(), "Field A should be unfocused");
  }

  // ==========================================================================
  // The text input's geometry
  // ==========================================================================

  /// Where a text input's text starts: the theme-less padding
  const PAD_X: f32 = 8.0;

  #[test]
  fn a_click_puts_the_caret_at_the_nearest_character_boundary() {
    let mut text = text_engine();
    let style = TextStyle::default();
    let field = Rect::new(10.0, 10.0, 200.0, 30.0);
    for content in ["hello world", "héllo wörld"] {
      // The caret x before the 5th character
      let byte = content.char_indices().nth(5).unwrap().0;
      let x = text.caret_x(content, &style, byte, 1.0);
      let mut commands = UiCommands::default();
      let mut ui = context_with(
        &mut commands,
        &mut text,
        (320.0, 180.0),
        1.0,
        mouse_at(field.x + PAD_X + x + 0.4, 20.0, true),
      );
      let mut state = TextInputState::with_text(content);
      ui.text_input(&mut state, field, "", 0.0);
      assert_eq!(state.cursor(), byte, "{content}");
      assert!(content.is_char_boundary(state.cursor()));
    }
  }

  #[test]
  fn the_caret_is_drawn_at_the_text_engines_caret_x() {
    let mut text = text_engine();
    let style = TextStyle::default();
    let field = Rect::new(10.0, 10.0, 200.0, 30.0);
    let content = "Wide caret";
    let expected = text.caret_x(content, &style, 4, 1.0);
    let line = text.line_height(&style, 1.0);
    let mut commands = UiCommands::default();
    let mut ui = context(&mut commands, &mut text);
    let mut state = TextInputState::with_text(content);
    state.set_cursor(4, false);
    state.set_focused(true);
    ui.text_input(&mut state, field, "", 0.0);
    drop(ui);
    // The caret: the last rect, 2 wide, a line tall
    let caret = *rects(&commands).last().unwrap();
    assert!(
      (caret.x - (field.x + PAD_X + expected)).abs() < 0.01,
      "{caret:?}"
    );
    assert_eq!((caret.width, caret.height), (2.0, line));
    assert!((caret.y - (field.y + (field.height - line) / 2.0)).abs() < 0.01);
  }

  #[test]
  fn a_selection_spans_the_text_engines_selection_x() {
    let mut text = text_engine();
    let style = TextStyle::default();
    let field = Rect::new(0.0, 0.0, 300.0, 30.0);
    let content = "select these words";
    let (left, right) = text.selection_x(content, &style, 7..12, 1.0);
    let mut commands = UiCommands::default();
    let mut ui = context(&mut commands, &mut text);
    let mut state = TextInputState::with_text(content);
    state.set_cursor(7, false);
    state.set_cursor(12, true);
    ui.text_input(&mut state, field, "", 0.0);
    drop(ui);
    let selection = rects(&commands)
      .into_iter()
      .find(|r| (r.width - (right - left)).abs() < 0.01)
      .expect("a selection rect");
    assert!((selection.x - (field.x + PAD_X + left)).abs() < 0.01);
  }

  // ==========================================================================
  // Mouse occlusion
  // ==========================================================================

  #[test]
  fn mouse_occluded_blocks_hover() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, false),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(ui.is_hovered(rect), "Should be hovered before occlusion");

    ui.set_mouse_occluded();
    assert!(
      !ui.is_hovered(rect),
      "Should not be hovered after occlusion"
    );
  }

  #[test]
  fn mouse_occluded_blocks_click() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, true),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(ui.is_clicked(rect), "Should be clicked before occlusion");

    ui.set_mouse_occluded();
    assert!(
      !ui.is_clicked(rect),
      "Should not be clicked after occlusion"
    );
  }

  #[test]
  fn mouse_occluded_blocks_held() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    // A press is also a hold
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, true),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(ui.is_held(rect), "Should be held before occlusion");

    ui.set_mouse_occluded();
    assert!(!ui.is_held(rect), "Should not be held after occlusion");
  }

  #[test]
  fn a_widget_outside_the_top_scene_reports_nothing() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let draw = |ui: &mut UiContext<'_>| ui.button("Test").at(rect).show();
    ui.memory.begin_frame();
    ui.begin_scene(2, true);
    let id = draw(&mut ui).id;
    for top in [true, false] {
      // As if the pointer had clicked it, with nothing occluding it
      ui.memory.begin_frame();
      ui.memory.interact.hot = id;
      ui.memory.interact.hovered = id;
      ui.memory.interact.clicked = id;
      ui.begin_scene(2, top);
      let response = draw(&mut ui);
      assert_eq!(response.id, id);
      assert_eq!(
        (response.hovered(), response.clicked()),
        (top, top),
        "top scene: {top}"
      );
    }
  }

  #[test]
  fn back_and_the_tab_step_go_to_their_first_taker() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.memory.interact.back_pressed = true;
    ui.memory.interact.tab_step = 1;
    ui.begin_scene(2, false);
    assert!(!ui.back(), "not below the top scene");
    assert_eq!(ui.take_tab_step(), 0);
    ui.begin_scene(1, true);
    assert!(ui.back());
    // A scene the first one's action brought up, drawn in a rerun pass
    ui.begin_scene(3, true);
    assert!(!ui.back(), "one press, one taker");
    assert_eq!(ui.take_tab_step(), 1);
    assert_eq!(ui.take_tab_step(), 0, "taken");
  }

  #[test]
  fn mouse_occluded_prevents_button_click() {
    let mut d = UiDriver::new((200.0, 200.0));
    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    let draw = |ui: &mut UiContext<'_>| {
      ui.set_mouse_occluded();
      let response = ui.button("Test").at(rect).show();
      response.hovered() || response.pressed() || response.clicked()
    };
    d.move_to(50.0, 50.0);
    d.frame(draw);
    d.press();
    assert!(!d.frame(draw), "no hover or press when occluded");
    d.release();
    assert!(!d.frame(draw), "no click when occluded");
  }

  #[test]
  fn mouse_not_occluded_by_default() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, false),
    );

    let rect = Rect::new(10.0, 10.0, 100.0, 100.0);
    assert!(
      ui.is_hovered(rect),
      "Mouse should not be occluded by default"
    );
  }

  #[test]
  fn mouse_occluded_does_not_affect_rects_outside_mouse() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 200.0),
      1.0,
      mouse_at(50.0, 50.0, false),
    );

    ui.set_mouse_occluded();
    let far_rect = Rect::new(150.0, 150.0, 30.0, 30.0);
    assert!(!ui.is_hovered(far_rect));
  }

  // ==========================================================================
  // Keyboard focus and scrolling
  // ==========================================================================

  #[test]
  fn wants_keyboard_is_false_without_widgets() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = ui_context(&mut commands, &mut text, (200.0, 100.0));
    assert!(!ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_while_a_focused_text_input_is_drawn() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (200.0, 100.0));
    let mut unfocused = TextInputState::new();
    ui.text_input(&mut unfocused, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);
    assert!(!ui.wants_keyboard());

    let mut focused = TextInputState::new();
    focused.set_focused(true);
    ui.text_input(&mut focused, Rect::new(0.0, 30.0, 100.0, 20.0), "", 0.0);
    assert!(ui.wants_keyboard());
  }

  #[test]
  fn escape_leaves_a_focused_text_input_and_is_consumed() {
    let mut input = Input::new();
    input.handle_keyboard(
      winit::keyboard::KeyCode::Escape,
      winit::event::ElementState::Pressed,
    );
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context_with(
      &mut commands,
      &mut text,
      (200.0, 100.0),
      1.0,
      UiInput::snapshot(&input),
    );
    let mut field = TextInputState::new();
    field.set_focused(true);

    ui.text_input(&mut field, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);

    assert!(!field.is_focused());
    assert!(
      ui.escape_consumed(),
      "the scene must not also act on this Esc"
    );
    assert!(!ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_while_a_dropdown_is_open() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (200.0, 200.0));
    let mut closed = DropdownState::with_id(1);
    ui.dropdown(&mut closed, Rect::new(0.0, 0.0, 100.0, 20.0), &["a", "b"]);
    assert!(!ui.wants_keyboard());

    let mut open = DropdownState::with_id(2);
    open.open();
    ui.dropdown(&mut open, Rect::new(0.0, 40.0, 100.0, 20.0), &["a", "b"]);
    assert!(ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_ignores_a_focused_but_closed_dropdown() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = ui_context(&mut commands, &mut text, (200.0, 200.0));
    let mut focused = DropdownState::with_id(1);
    focused.set_focused(true);
    ui.dropdown(&mut focused, Rect::new(0.0, 0.0, 100.0, 20.0), &["a", "b"]);
    assert!(!ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_resets_with_each_frame() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut focused = TextInputState::new();
    focused.set_focused(true);
    {
      let mut ui = ui_context(&mut commands, &mut text, (200.0, 100.0));
      ui.text_input(&mut focused, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);
      assert!(ui.wants_keyboard());
    }
    // Focus lost before the next frame: the next context starts clear
    focused.set_focused(false);
    let mut ui = ui_context(&mut commands, &mut text, (200.0, 100.0));
    ui.text_input(&mut focused, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);
    assert!(!ui.wants_keyboard());
  }
  #[test]
  fn the_text_builder_records_its_options() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let size = ui
      .text(RichText::markup("[b]Hi[/b] there"))
      .at(100.0, 50.0)
      .anchor(Anchor::Center)
      .reveal(3.0)
      .opacity(0.5)
      .draw();
    drop(ui);
    let UiCommand::Text {
      origin,
      reveal,
      opacity,
      ..
    } = commands.commands().next().unwrap().clone()
    else {
      panic!()
    };
    assert!((origin - (Vec2::new(100.0, 50.0) - size / 2.0)).length() < 0.01);
    assert_eq!(reveal, Some((3.0, 0.0)));
    assert_eq!(opacity, 0.5);
  }

  #[test]
  fn wrapped_text_through_the_builder_fits_its_width() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let size = ui
      .text("a few words that need to wrap")
      .at(0.0, 0.0)
      .wrap(60.0)
      .draw();
    assert!(size.x <= 60.5);
    assert!(size.y > 30.0, "more than one line");
  }

  #[test]
  fn wrap_text_styled_wraps_in_the_given_style() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let ui = context(&mut commands, &mut text);
    let small = ui.wrap_text_styled(
      "a few words that need to wrap",
      80.0,
      &TextStyle::new().size(10.0),
    );
    let big = ui.wrap_text_styled(
      "a few words that need to wrap",
      80.0,
      &TextStyle::new().size(24.0),
    );
    assert!(small.len() < big.len(), "{small:?} vs {big:?}");
    assert_eq!(big.join(" "), "a few words that need to wrap");
  }

  #[test]
  fn open_dropdown_lists_draw_on_the_popup_layer_after_the_page() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let mut state = DropdownState::new();
    state.open();
    ui.dropdown(
      &mut state,
      Rect::new(10.0, 10.0, 100.0, 20.0),
      &["one", "two"],
    );
    ui.draw_rect(Rect::new(0.0, 0.0, 300.0, 300.0), Color::RED); // the page draws on after
    ui.finish_dropdowns();
    drop(ui);
    let layers: Vec<u8> = commands.commands().map(UiCommand::draw_layer).collect();
    let first_popup = layers
      .iter()
      .position(|&l| l == 1)
      .expect("a popup command");
    assert!(layers[first_popup..].iter().all(|&l| l == 1));
    assert!(layers[..first_popup].contains(&0));
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn a_centred_label_lays_out_once() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let before = text.stats().misses;
    let mut ui = context(&mut commands, &mut text);
    // A button's label: measured to centre it, then drawn in its colour
    ui.draw_text_centered(
      "Play",
      Rect::new(10.0, 10.0, 120.0, 30.0),
      Color::rgb(200, 180, 90),
    );
    drop(ui);
    assert_eq!(
      text.stats().misses - before,
      1,
      "the draw reuses the measure"
    );
  }

  #[test]
  fn dropdown_text_is_centred_by_the_real_line_height() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let mut state = DropdownState::new();
    let line = ui.line_height(&TextStyle::default());
    ui.dropdown(&mut state, Rect::new(10.0, 10.0, 100.0, 30.0), &["one"]);
    drop(ui);
    let origin = commands
      .commands()
      .find_map(UiCommand::text_origin)
      .unwrap();
    assert!((origin.y - (10.0 + (30.0 - line) / 2.0)).abs() < 0.01);
  }

  #[test]
  fn a_ui_reveal_driven_by_the_clock_finishes_its_fade() {
    use crate::core::text::{RevealClock, TextLayoutOptions, reveal_alpha};
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let rich = RichText::new("Hello there");
    let layout = text
      .layout(
        &rich,
        &TextStyle::default(),
        &TextLayoutOptions::default(),
        1.0,
      )
      .unwrap();
    let fade = 3.0;
    let mut clock = RevealClock::default();
    for _ in 0..1000 {
      if clock.settled(fade) {
        break;
      }
      clock.step(0.05, 20.0, 0.0, &layout);
      clock.run_on(0.05, 20.0, fade);
    }
    assert!(clock.settled(fade));
    let mut ui = context(&mut commands, &mut text);
    ui.text(rich.clone())
      .reveal(clock.drawn())
      .fade(fade)
      .draw();
    ui.text(rich).reveal(clock.shown).fade(fade).draw();
    drop(ui);
    let revealed: Vec<(f32, f32)> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { reveal, .. } => *reveal,
        _ => None,
      })
      .collect();
    let (shown, fade) = revealed[0];
    assert!((0..layout.clusters).all(|c| reveal_alpha(c, shown, fade) == 1.0));
    // Driven by `shown` alone the last clusters stay partial: the drawn
    // value is the one to pass
    let (shown, fade) = revealed[1];
    assert!(reveal_alpha(layout.clusters - 1, shown, fade) < 1.0);
  }

  #[test]
  fn a_texts_effect_seed_ignores_what_was_drawn_before_it() {
    let seed_of = |before: bool| {
      let (mut commands, mut text) = (UiCommands::default(), text_engine());
      let mut ui = context(&mut commands, &mut text);
      if before {
        ui.draw_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::RED);
        ui.draw_text("other", 0.0, 0.0, Color::WHITE);
      }
      ui.text(RichText::markup("[shake]label[/shake]"))
        .at(5.0, 5.0)
        .draw();
      drop(ui);
      commands
        .commands()
        .filter_map(|c| match c {
          UiCommand::Text { seed, .. } => Some(*seed),
          _ => None,
        })
        .last()
        .unwrap()
    };
    assert_eq!(seed_of(false), seed_of(true));
  }

  /// The sprite pieces of a nine-slice: (rect, uv)
  fn slice_pieces(sprite: &SpriteHandle, rect: Rect, insets: [f32; 4]) -> Vec<(Rect, [f32; 4])> {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_nine_slice(sprite, rect, insets, Color::WHITE);
    drop(ui);
    commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Sprite { rect, uv, .. } => Some((*rect, *uv)),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn nine_slice_insets_clamp_to_the_texture() {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(16, 8, 255, 255, 255, 255), "s");
    let pieces = slice_pieces(
      &sprite,
      Rect::new(0.0, 0.0, 100.0, 50.0),
      [20.0, 0.0, 20.0, 0.0],
    );
    assert!(!pieces.is_empty());
    for (rect, uv) in &pieces {
      assert!(uv[0] <= uv[2] && uv[1] <= uv[3], "no inverted UVs: {uv:?}");
      assert!(rect.width >= 0.0);
    }
    let left = pieces.iter().find(|(r, _)| r.x == 0.0).unwrap();
    assert!(left.0.width <= 8.0 + 1e-4, "{left:?}");
  }

  #[test]
  fn nine_slice_corners_shrink_to_fit_a_small_rect() {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 255, 255, 255, 255), "s");
    let rect = Rect::new(10.0, 10.0, 6.0, 20.0);
    let pieces = slice_pieces(&sprite, rect, [4.0, 2.0, 4.0, 2.0]);
    for (piece, _) in &pieces {
      assert!(
        piece.x >= rect.x - 1e-4 && piece.right() <= rect.right() + 1e-4,
        "{piece:?}"
      );
      assert!(
        piece.y >= rect.y - 1e-4 && piece.bottom() <= rect.bottom() + 1e-4,
        "{piece:?}"
      );
    }
    let area: f32 = pieces.iter().map(|(r, _)| r.width * r.height).sum();
    assert!((area - rect.width * rect.height).abs() < 1e-3);
    let left = pieces.iter().find(|(r, _)| r.x == rect.x).unwrap();
    assert!((left.0.width - 3.0).abs() < 1e-4, "{left:?}");
  }
  #[test]
  fn a_texts_effect_seed_ignores_its_colour() {
    let seed_of = |color: Color| {
      let (mut commands, mut text) = (UiCommands::default(), text_engine());
      let mut ui = context(&mut commands, &mut text);
      let style = TextStyle::new()
        .color(color)
        .outline(1.0, color)
        .shadow(Vec2::new(1.0, 1.0), color, 0.0)
        .glow(2.0, color);
      ui.text(RichText::markup("[shake]label[/shake]"))
        .style(&style)
        .draw();
      drop(ui);
      match commands.commands().next().unwrap() {
        UiCommand::Text { seed, .. } => *seed,
        _ => panic!(),
      }
    };
    assert_eq!(seed_of(Color::WHITE), seed_of(Color::RED));
  }

  /// The one text command a builder call records
  fn built(build: impl for<'u, 'a> FnOnce(UiText<'u, 'a>) -> UiText<'u, 'a>) -> UiCommand {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    build(ui.text("Hello")).draw();
    drop(ui);
    commands.commands().next().unwrap().clone()
  }

  #[test]
  fn text_opacity_is_clamped_and_a_nan_is_opaque() {
    let opacity = |value: f32| match built(|t| t.opacity(value)) {
      UiCommand::Text { opacity, .. } => opacity,
      _ => panic!(),
    };
    assert_eq!(opacity(f32::NAN), 1.0);
    assert_eq!(opacity(f32::INFINITY), 1.0);
    assert_eq!(opacity(5.0), 1.0);
    assert_eq!(opacity(-2.0), 0.0);
    assert_eq!(opacity(0.25), 0.25);
  }

  #[test]
  fn text_reveal_and_fade_ignore_values_that_are_not_numbers() {
    fn reveal(
      build: impl for<'u, 'a> FnOnce(UiText<'u, 'a>) -> UiText<'u, 'a>,
    ) -> Option<(f32, f32)> {
      match built(build) {
        UiCommand::Text { reveal, .. } => reveal,
        _ => panic!(),
      }
    }
    assert_eq!(reveal(|t| t.reveal(f32::NAN)), None);
    assert_eq!(reveal(|t| t.reveal(f32::NAN).fade(2.0)), None);
    assert_eq!(reveal(|t| t.reveal(f32::INFINITY)), None);
    assert_eq!(reveal(|t| t.reveal(2.0).fade(f32::NAN)), Some((2.0, 0.0)));
    assert_eq!(reveal(|t| t.fade(-3.0).reveal(2.0)), Some((2.0, 0.0)));
    assert_eq!(reveal(|t| t.fade(2.0).reveal(3.0)), Some((3.0, 2.0)));
  }

  #[test]
  fn a_public_text_reveal_settles_with_its_fade_at_full_alpha() {
    use crate::core::text::{TextLayoutOptions, TextReveal, reveal_alpha};
    let mut text = text_engine();
    let layout = text
      .layout(
        &RichText::new("Hello there"),
        &TextStyle::default(),
        &TextLayoutOptions::default(),
        1.0,
      )
      .unwrap();
    // 20 clusters a second with a 0.15 s fade is a 3 cluster fade
    let mut reveal = TextReveal::new(20.0).with_fade(0.15);
    for _ in 0..1000 {
      if reveal.settled() {
        break;
      }
      reveal.advance(0.05, &layout);
    }
    assert!(reveal.settled());
    let (drawn, fade) = reveal.drawn();
    assert!((fade - 3.0).abs() < 1e-4);
    assert_eq!(drawn, reveal.drawn_clusters());
    assert!(drawn >= layout.clusters as f32 + fade - 1.0);
    assert!((0..layout.clusters).all(|c| reveal_alpha(c, drawn, fade) == 1.0));
  }

  /// One UI frame of `dt` seconds drawing `markup` through `reveal`; the
  /// reveal its text command recorded
  fn typewriter_frame(
    text: &mut TextEngine,
    markup: &str,
    reveal: &mut TextReveal,
    dt: f32,
  ) -> Option<(f32, f32)> {
    let mut commands = UiCommands::default();
    let mut ui = context(&mut commands, text);
    ui.set_dt(dt);
    ui.text(RichText::markup(markup))
      .at(10.0, 10.0)
      .typewriter(reveal)
      .draw();
    drop(ui);
    commands.commands().find_map(|c| match c {
      UiCommand::Text { reveal, .. } => *reveal,
      _ => None,
    })
  }

  #[test]
  fn a_ui_typewriter_holds_at_an_inline_pause() {
    let mut text = text_engine();
    let mut reveal = TextReveal::new(10.0);
    let line = "ab[pause=0.5]cd";
    let recorded = typewriter_frame(&mut text, line, &mut reveal, 0.2);
    assert_eq!(recorded, Some((2.0, 0.0)), "a and b, then the pause");
    typewriter_frame(&mut text, line, &mut reveal, 0.4);
    assert!((reveal.shown() - 2.0).abs() < 1e-5, "still paused");
    let recorded = typewriter_frame(&mut text, line, &mut reveal, 0.2);
    let (shown, _) = recorded.unwrap();
    assert!(
      (shown - 3.0).abs() < 1e-4,
      "0.1 s of pause left, then c: {shown}"
    );
    assert!(!reveal.is_done());
  }

  #[test]
  fn a_ui_typewriter_pauses_after_punctuation() {
    let mut text = text_engine();
    let mut paused = TextReveal::new(10.0).with_punctuation_pause(0.5);
    let mut plain = TextReveal::new(10.0);
    typewriter_frame(&mut text, "a, b", &mut paused, 0.25);
    typewriter_frame(&mut text, "a, b", &mut plain, 0.25);
    assert!(
      (paused.shown() - 2.0).abs() < 1e-5,
      "a and the comma, then the hold: {}",
      paused.shown()
    );
    assert!((plain.shown() - 2.5).abs() < 1e-4, "{}", plain.shown());
    typewriter_frame(&mut text, "a, b", &mut paused, 0.3);
    assert!((paused.shown() - 2.0).abs() < 1e-5, "0.35 s of 0.5 held");
  }

  #[test]
  fn a_ui_typewriter_with_a_fade_settles_at_full_alpha() {
    use crate::core::text::reveal_alpha;
    let mut text = text_engine();
    let line = "Hello there";
    // 20 clusters a second with a 0.15 s fade is a 3 cluster fade
    let mut reveal = TextReveal::new(20.0).with_fade(0.15);
    let mut recorded = None;
    for _ in 0..200 {
      recorded = typewriter_frame(&mut text, line, &mut reveal, 0.05);
    }
    assert!(reveal.is_done());
    let (shown, fade) = recorded.expect("a reveal is recorded");
    assert!((fade - 3.0).abs() < 1e-4, "the reveal's fade: {fade}");
    assert_eq!(shown, reveal.drawn_clusters());
    let clusters = text
      .layout(
        &RichText::new(line),
        &TextStyle::default(),
        &TextLayoutOptions::default(),
        1.0,
      )
      .unwrap()
      .clusters;
    assert!((0..clusters).all(|c| reveal_alpha(c, shown, fade) == 1.0));
  }

  #[test]
  fn a_ui_typewriter_advances_by_each_frames_dt() {
    let mut text = text_engine();
    let mut reveal = TextReveal::new(10.0);
    typewriter_frame(&mut text, "Hello there", &mut reveal, 0.1);
    assert!((reveal.shown() - 1.0).abs() < 1e-4, "{}", reveal.shown());
    let recorded = typewriter_frame(&mut text, "Hello there", &mut reveal, 0.15);
    assert!((reveal.shown() - 2.5).abs() < 1e-4, "{}", reveal.shown());
    assert_eq!(recorded, Some((reveal.shown(), 0.0)));
  }

  #[test]
  fn a_ui_typewriter_wins_over_reveal_and_fade() {
    let mut text = text_engine();
    let mut reveal = TextReveal::new(10.0);
    let mut commands = UiCommands::default();
    let mut ui = context(&mut commands, &mut text);
    ui.set_dt(0.2);
    ui.text("Hello")
      .reveal(4.0)
      .fade(3.0)
      .typewriter(&mut reveal)
      .draw();
    ui.text("Hello")
      .typewriter(&mut reveal)
      .reveal(4.0)
      .fade(3.0)
      .draw();
    drop(ui);
    let recorded: Vec<Option<(f32, f32)>> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { reveal, .. } => Some(*reveal),
        _ => None,
      })
      .collect();
    // Two 0.2 s draws of the same reveal: 2 then 4 clusters, no fade
    assert_eq!(recorded, vec![Some((2.0, 0.0)), Some((4.0, 0.0))]);
  }

  #[test]
  fn a_settled_ui_typewriter_stays_put() {
    let mut text = text_engine();
    let mut reveal = TextReveal::new(20.0).with_fade(0.15);
    for _ in 0..100 {
      typewriter_frame(&mut text, "Hi there", &mut reveal, 0.05);
    }
    let settled = (reveal.shown(), reveal.drawn_clusters());
    for _ in 0..10 {
      let recorded = typewriter_frame(&mut text, "Hi there", &mut reveal, 0.5);
      assert_eq!(recorded, Some((settled.1, 3.0)));
    }
    assert_eq!((reveal.shown(), reveal.drawn_clusters()), settled);
  }

  #[test]
  fn a_ui_typewriter_waits_out_a_leading_pause() {
    let mut text = text_engine();
    let mut reveal = TextReveal::new(10.0);
    let line = "[pause=0.5]ab";
    assert_eq!(
      typewriter_frame(&mut text, line, &mut reveal, 0.3),
      Some((0.0, 0.0))
    );
    let (shown, _) = typewriter_frame(&mut text, line, &mut reveal, 0.3).unwrap();
    assert!(
      (shown - 1.0).abs() < 1e-4,
      "0.2 s of pause, then a: {shown}"
    );
  }

  #[test]
  fn a_ui_typewriter_on_text_that_cannot_be_laid_out_records_nothing() {
    let mut text = text_engine();
    let mut reveal = TextReveal::new(10.0);
    let mut commands = UiCommands::default();
    let mut ui = context(&mut commands, &mut text);
    ui.set_dt(0.5);
    let size = ui.text("Hello").size(0.0).typewriter(&mut reveal).draw();
    drop(ui);
    assert_eq!(size, Vec2::ZERO);
    assert_eq!(commands.commands().count(), 0, "nothing recorded");
    assert_eq!(reveal.shown(), 0.0, "and the reveal is left alone");
  }
}
