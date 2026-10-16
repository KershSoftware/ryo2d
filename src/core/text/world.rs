//! Text on entities: `Text2d`, its laid-out bounds, and the typewriter reveal

use std::sync::Arc;

use glam::Vec2;

use crate::core::ecs::{Component, Entity, Message, MessageWriter, Query, Res, Transform};
use crate::core::render::batch::BlendMode;
use crate::core::text::RevealClock;
use crate::core::text::{RichText, TextLayout, TextLayoutOptions, TextStyle};
use crate::core::time::Time;
use crate::core::ui::Anchor;

/// Text drawn in the world, sorted with sprites by `z_order`. Sizes and
/// wrap widths are world units; the entity's transform moves, turns and
/// scales it. Unlit by default, so labels stay readable in the dark.
///
/// ```ignore
/// ctx.world.spawn((
///   Transform::from_xy(100.0, 40.0),
///   Text2d::markup("[color=#ffd700]Gold[/color] x3").with_style(TextStyle::new().size(16.0)),
/// ));
/// ```
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, TextBounds)]
#[non_exhaustive]
pub struct Text2d {
  pub text: RichText,
  pub style: TextStyle,
  /// Wrap width, alignment, line limit and the anchor that sits on the
  /// entity's position (`Center` by default)
  pub layout: TextLayoutOptions,
  /// Darkened and coloured by the scene's lights, like sprites
  pub lit: bool,
  pub z_order: i32,
  pub blend: BlendMode,
}

impl Text2d {
  /// Plain or rich text, centred on the entity
  pub fn new(text: impl Into<RichText>) -> Self {
    Self {
      text: text.into(),
      style: TextStyle::default(),
      layout: TextLayoutOptions::default().anchor(Anchor::Center),
      lit: false,
      z_order: 0,
      blend: BlendMode::Alpha,
    }
  }

  /// Text from markup (`[color=red]..[/color]`, `[b]`, `[wave]`, `[icon=name]`..)
  pub fn markup(source: &str) -> Self {
    Self::new(RichText::markup(source))
  }

  pub fn with_style(mut self, style: TextStyle) -> Self {
    self.style = style;
    self
  }

  pub fn with_layout(mut self, layout: TextLayoutOptions) -> Self {
    self.layout = layout;
    self
  }

  /// Lit text darkens and colours with the scene's lights, like sprites
  pub fn with_lighting(mut self, lit: bool) -> Self {
    self.lit = lit;
    self
  }

  pub fn with_z(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  pub fn with_blend(mut self, blend: BlendMode) -> Self {
    self.blend = blend;
    self
  }

  /// Replace the text (a reveal on the entity starts over)
  pub fn set_text(&mut self, text: impl Into<RichText>) {
    self.text = text.into();
  }
}

/// The laid-out size of a `Text2d`, in world units before the entity's
/// scale and rotation; `offset` runs from the entity's position to the
/// block's top-left. Updated when the text is drawn.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct TextBounds {
  pub size: Vec2,
  pub offset: Vec2,
}

/// Reveal a `Text2d` a character cluster at a time, on game time. It starts
/// over when the text changes; `TextRevealed` says when it is done.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TextReveal {
  /// Clusters per second
  pub per_second: f32,
  /// Seconds to hold after `. , ! ? ; :`
  pub punctuation_pause: f32,
  /// Seconds each character takes to fade in (0: appears at once)
  pub fade: f32,
  pub(crate) clock: RevealClock,
}

impl TextReveal {
  pub fn new(per_second: f32) -> Self {
    Self {
      per_second,
      punctuation_pause: 0.0,
      fade: 0.0,
      clock: RevealClock::default(),
    }
  }

  pub fn with_punctuation_pause(mut self, seconds: f32) -> Self {
    self.punctuation_pause = seconds;
    self
  }

  pub fn with_fade(mut self, seconds: f32) -> Self {
    self.fade = seconds;
    self
  }

  /// Start the reveal over
  pub fn restart(&mut self) {
    self.clock = RevealClock::default();
  }

  /// Every character has started showing (`TextRevealed` was written); with
  /// a fade, the last ones finish fading in over the next moments
  pub fn is_done(&self) -> bool {
    self.clock.done
  }

  /// Character clusters shown so far (fractional while fading)
  pub fn shown(&self) -> f32 {
    self.clock.shown
  }

  /// The clusters to draw: [`shown`](Self::shown) plus the run-on past the
  /// end that lets the last characters finish fading in. Pass this (not
  /// `shown`) to a UI text's `reveal` so a faded reveal ends at full opacity.
  pub fn drawn_clusters(&self) -> f32 {
    self.clock.drawn()
  }

  /// The fade in clusters (0 without one)
  pub(crate) fn fade_clusters(&self) -> f32 {
    let clusters = self.fade * self.per_second;
    if self.fade > 0.0 && self.per_second > 0.0 && clusters.is_finite() {
      clusters
    } else {
      0.0
    }
  }

  /// Advance by `dt` game seconds over `layout`; true on the step every
  /// character has started showing. After that it runs on until the last
  /// characters have faded in.
  pub(crate) fn advance(&mut self, dt: f32, layout: &TextLayout) -> bool {
    let (per_second, pause) = (self.per_second, self.punctuation_pause);
    if self.clock.done {
      let fade = self.fade_clusters();
      self.clock.run_on(dt, per_second, fade);
      return false;
    }
    self.clock.step(dt, per_second, pause, layout)
  }

  /// Every character shows at full opacity: nothing left to advance
  pub(crate) fn settled(&self) -> bool {
    self.clock.settled(self.fade_clusters())
  }

  /// The clusters to draw and the fade in clusters (`reveal_alpha`)
  pub(crate) fn drawn(&self) -> (f32, f32) {
    (self.clock.drawn(), self.fade_clusters())
  }
}

/// A `TextReveal` finished showing its text
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRevealed {
  pub entity: Entity,
}

/// A `Text2d`'s current layout, the hash it was made for, and the hash of
/// its text alone (a reveal starts over when that changes)
#[derive(Component, Debug, Clone)]
pub(crate) struct TextLayoutState {
  pub(crate) layout: Arc<TextLayout>,
  pub(crate) key: u64,
  pub(crate) text: u64,
}

/// `PostUpdateSet::Extract`: advance every `TextReveal` by the game delta,
/// over the layout the last frame drew; write `TextRevealed` once
pub(crate) fn advance_text_reveal(
  time: Res<Time>,
  mut reveals: Query<(Entity, &mut TextReveal, &TextLayoutState)>,
  mut finished: MessageWriter<TextRevealed>,
) {
  let dt = time.game.delta();
  for (entity, mut reveal, state) in &mut reveals {
    // Checked first, so a settled reveal is not marked changed
    if reveal.settled() {
      continue;
    }
    if reveal.advance(dt, &state.layout) {
      finished.write(TextRevealed { entity });
    }
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, World};
  use crate::core::text::{FontRole, Fonts, TextEngine};

  #[test]
  fn text_bounds_report_the_block() {
    let mut world = World::new();
    world.insert_resource(Fonts::with_builtins());
    let mut engine = TextEngine::new();
    let e = world
      .spawn((
        Transform::default(),
        Text2d::new("Hello").with_style(crate::core::text::TextStyle::new().size(20.0)),
      ))
      .id();
    crate::core::render::text::layout_world_text(&mut world, &mut engine);
    let bounds = world.get::<TextBounds>(e).unwrap();
    assert!(bounds.size.x > 30.0 && bounds.size.y > 20.0);
    assert!(
      (bounds.offset + bounds.size / 2.0).length() < 0.01,
      "centred by default"
    );
  }

  #[test]
  fn world_text_relays_out_when_its_font_arrives() {
    let mut world = World::new();
    world.insert_resource(Fonts::with_builtins());
    let mut engine = TextEngine::new();
    let e = world
      .spawn((Transform::default(), Text2d::new("iiii")))
      .id();
    crate::core::render::text::layout_world_text(&mut world, &mut engine);
    let before = world.get::<TextBounds>(e).unwrap().size.x;
    let mono = world.resource::<Fonts>().role(FontRole::Mono).unwrap();
    world.resource_mut::<Fonts>().set_role(FontRole::Ui, mono);
    crate::core::render::text::layout_world_text(&mut world, &mut engine);
    let after = world.get::<TextBounds>(e).unwrap().size.x;
    assert!(
      after > before + 5.0,
      "monospace i is wider: {before} then {after}"
    );
  }
}
