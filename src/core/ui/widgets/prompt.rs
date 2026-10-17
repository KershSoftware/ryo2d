//! Input prompts: `ui.prompt(action)` chips, and the `[prompt=action]`
//! marks in text resolved before layout

use std::borrow::Cow;
use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::input::{Binding, InputDevice};
use crate::core::render::batch::Filter;
use crate::core::text::{Icon, RichText, Span, TextLayoutOptions};
use crate::core::ui::id::UiId;
use crate::core::ui::immediate::{PromptContext, UiContext, text_seed};
use crate::core::ui::layout::{Edges, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, WidgetState, warn_once};
use crate::core::ui::tree::{DrawKind, Node, TextLeaf};
use crate::core::ui::{Color, Rect};

/// A chip's padding when its look has none
const CHIP_PADDING: Edges = Edges {
  left: 4.0,
  top: 1.0,
  right: 4.0,
  bottom: 1.0,
};

/// The binding a prompt shows for `device`: on the keyboard or the mouse
/// the first key or mouse button, on a gamepad the first pad button or
/// stick direction, otherwise the first binding
pub(crate) fn binding_for_device(bindings: &[Binding], device: InputDevice) -> Option<&Binding> {
  let fits = |binding: &&Binding| match device {
    InputDevice::Keyboard | InputDevice::Mouse => {
      matches!(binding, Binding::Key(_) | Binding::Mouse(_))
    }
    InputDevice::Gamepad => matches!(binding, Binding::Pad(_) | Binding::Stick(_)),
  };
  bindings.iter().find(fits).or_else(|| bindings.first())
}

/// The binding `action`'s prompt shows on the player's device; `None`
/// (warned once per action) when it has none
fn prompt_binding(prompts: &PromptContext, action: &str) -> Option<Binding> {
  let bindings = prompts.bindings.get(action).map_or(&[][..], Vec::as_slice);
  let binding = binding_for_device(bindings, prompts.device).copied();
  if binding.is_none() {
    warn_once(format!("prompt:{action}"), || {
      format!("the action {action:?} has no binding; its prompt shows \"?\"")
    });
  }
  binding
}

/// `text` with its prompt marks replaced: by the inline icon the game
/// registered under the binding's glyph name, else by its label in brackets
/// (`[A]`), bold in `accent`. Text without prompts comes back as it is.
pub(crate) fn resolve_prompts<'t>(
  text: &'t RichText,
  prompts: &PromptContext,
  accent: Color,
) -> Cow<'t, RichText> {
  if text.prompts.is_empty() {
    return Cow::Borrowed(text);
  }
  let mut out = text.clone();
  let mut marks = std::mem::take(&mut out.prompts);
  // Stable, so prompts at one byte keep their order
  marks.sort_by_key(|(at, _)| *at);
  // What the labels inserted so far moved the later marks by
  let mut moved = 0;
  for (at, action) in marks {
    let at = at + moved;
    let binding = prompt_binding(prompts, &action);
    if let Some(binding) = binding {
      let name = binding.glyph_name(prompts.family);
      if prompts.icons.get(&name).is_some() {
        out.insert_icon(at, Icon::Named { name, size: None });
        continue;
      }
    }
    let label = binding.map_or_else(|| "?".to_string(), |b| b.label(prompts.family));
    let before = out.text.len();
    out.insert_text(at, &format!("[{label}]"), Span::bold().with_color(accent));
    moved += out.text.len() - before;
  }
  Cow::Owned(out)
}

/// An input prompt being set up. It is drawn by [`show`](Self::show) or when
/// it is dropped.
pub struct Prompt<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  action: String,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// What triggers `action` on the player's last device: the glyph the game
  /// registered in `TextIcons` (as `prompt.key.Space`,
  /// `prompt.pad.xbox.South` and so on), as tall as a caption line, or a
  /// chip with its label ("Space", "A", "×"). An action with no binding
  /// shows "?". Text can show the same inline with `[prompt=action]`.
  ///
  /// ```ignore
  /// ui.row().show(|ui| {
  ///     ui.prompt("jump").show();
  ///     ui.text("Jump");
  /// });
  /// ```
  #[track_caller]
  pub fn prompt<'u>(&'u mut self, action: &str) -> Prompt<'u, 'a> {
    Prompt {
      ui: Some(self),
      site: Location::caller(),
      action: action.to_string(),
      opts: WidgetOpts::default(),
    }
  }

  /// Record a prompt chip `id` for `binding` (`None` shows "?") with
  /// `opts`' layout and look options: the binding's registered glyph, else
  /// its label in the caption role on the prompt look. Its rect.
  pub(crate) fn prompt_chip(
    &mut self,
    id: UiId,
    opts: &WidgetOpts,
    binding: Option<Binding>,
  ) -> Rect {
    let theme = self.theme_arc();
    let prompts = self.prompts_arc();
    let family = prompts.family;
    let state = if opts.disabled {
      WidgetState::Disabled
    } else {
      WidgetState::Normal
    };
    let look = self.look_of(WidgetKind::Prompt, opts, state);
    let caption = theme.text.caption.style().color(look.text.with_alpha(255));
    let line = self.line_height(&caption);
    let glyph = binding.and_then(|b| prompts.icons.get(&b.glyph_name(family)));
    let record =
      (opts.tooltip.is_some() || opts.drag.is_set()).then(|| opts.record(Flags::HOVER, false));

    if let Some(source) = glyph {
      // An image a caption line high, keeping its aspect
      let sprite_size = Vec2::new(source.sprite.width() as f32, source.sprite.height() as f32);
      let size = source
        .region
        .map_or(sprite_size, |region| Vec2::new(region.width, region.height));
      let width = if size.y > 0.0 && size.x.is_finite() {
        line * size.x / size.y
      } else {
        line
      };
      let uv = match source.region {
        Some(region) if sprite_size.x > 0.0 && sprite_size.y > 0.0 => [
          region.x / sprite_size.x,
          region.y / sprite_size.y,
          (region.x + region.width) / sprite_size.x,
          (region.y + region.height) / sprite_size.y,
        ],
        _ => [0.0, 0.0, 1.0, 1.0],
      };
      let mut node = Node::new(
        id,
        opts.style.clone(),
        Measure::Fixed(Vec2::new(width, line)),
      );
      node.record = record;
      let index = self.push_node(node, opts.at);
      self.push_draw(
        index,
        DrawKind::Sprite {
          inset: Edges::ZERO,
          frac: Vec2::splat(0.5),
          size: None,
          sprite: source.sprite.clone(),
          uv,
          tint: Color::WHITE.with_alpha(look.text.a),
          filter: Filter::Default,
        },
      );
      return self.close_node(index).rect;
    }

    let label = RichText::new(binding.map_or_else(|| "?".to_string(), |b| b.label(family)));
    let options = TextLayoutOptions::default();
    let text = self.measure_rich(&label, &caption, &options);
    let pad = if look.padding == Edges::ZERO {
      CHIP_PADDING
    } else {
      look.padding
    };
    let size = Vec2::new(text.x + pad.width(), text.y.max(line) + pad.height());
    let mut style = opts.style.clone();
    style.padding = pad;
    let mut node = Node::new(id, style, Measure::Fixed(size));
    node.record = record;
    let index = self.push_node(node, opts.at);
    self.push_look_box(index, &look);
    let seed = text_seed(&label, &caption, &options);
    self.push_draw(
      index,
      DrawKind::Text {
        leaf: Box::new(TextLeaf {
          text: label,
          style: caption,
          options,
        }),
        frac: Vec2::splat(0.5),
        wrap: false,
        opacity: f32::from(look.text.a) / 255.0,
        reveal: None,
        seed,
      },
    );
    self.close_node(index).rect
  }
}

widget_builders!(Prompt<'_, '_>);

impl Prompt<'_, '_> {
  /// Draw the prompt; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the prompt, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let mut response = ui.widget_response(id, Sense::hover(), &self.opts);
    let binding = prompt_binding(ui.prompts(), &self.action);
    response.rect = ui.prompt_chip(id, &self.opts, binding);
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Prompt<'_, '_> {
  /// A prompt not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::input::{Binding, GamepadButton, InputDevice};
  use crate::core::ui::UiContext;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::{UiDriver, prompts_with};
  use winit::keyboard::KeyCode;

  fn shown(d: &UiDriver) -> Vec<String> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_prompt_shows_the_binding_for_the_last_device() {
    let confirm = (
      "ui.confirm",
      vec![
        Binding::Key(KeyCode::Enter),
        Binding::Pad(GamepadButton::South),
      ],
    );
    let mut d = UiDriver::new((200.0, 100.0)).prompts(prompts_with(
      InputDevice::Keyboard,
      std::slice::from_ref(&confirm),
    ));
    d.frame(|ui| ui.prompt("ui.confirm").show());
    assert!(shown(&d).contains(&"Enter".to_string()));
    let mut d =
      UiDriver::new((200.0, 100.0)).prompts(prompts_with(InputDevice::Gamepad, &[confirm]));
    d.frame(|ui| ui.prompt("ui.confirm").show());
    assert!(shown(&d).contains(&"A".to_string()));
  }

  #[test]
  fn markup_prompts_become_labels_or_registered_glyphs() {
    let jump = (
      "jump",
      vec![
        Binding::Key(KeyCode::Space),
        Binding::Pad(GamepadButton::South),
      ],
    );
    let mut context = prompts_with(InputDevice::Gamepad, &[jump]);
    let text = RichText::markup("Press [prompt=jump] to jump");
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(resolved.as_str(), "Press [A] to jump");
    let sprite = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(4, 4, 255, 255, 255, 255),
      "a-glyph",
    );
    context.icons.insert("prompt.pad.xbox.South", sprite);
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(
      resolved.as_str(),
      "Press  to jump",
      "the glyph replaces the label"
    );
    assert_eq!(resolved.icons.len(), 1);
  }

  fn sprite(name: &str) -> crate::core::asset::SpriteHandle {
    crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(8, 4, 255, 255, 255, 255),
      name,
    )
  }

  #[test]
  fn prompts_at_one_byte_keep_their_order_after_its_icons() {
    let actions = [
      ("a", vec![Binding::Key(KeyCode::KeyA)]),
      ("b", vec![Binding::Key(KeyCode::KeyB)]),
      ("c", vec![Binding::Key(KeyCode::KeyC)]),
    ];
    let mut context = prompts_with(InputDevice::Keyboard, &actions);
    let text = RichText::markup("[icon=star][prompt=a][prompt=b][prompt=c]!");
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(resolved.as_str(), "[A][B][C]!");
    assert_eq!(resolved.icons[0].at, 0, "the icon stays first");
    assert!(resolved.prompts.is_empty());
    // A and C as glyphs: A, then [B], then C
    context.icons.insert("prompt.key.KeyA", sprite("ga"));
    context.icons.insert("prompt.key.KeyC", sprite("gc"));
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(resolved.as_str(), "[B]!");
    let icons: Vec<(usize, String)> = resolved
      .icons
      .iter()
      .map(|mark| match &mark.icon {
        Icon::Named { name, .. } => (mark.at, name.clone()),
        Icon::Sprite { .. } => (mark.at, String::new()),
      })
      .collect();
    assert_eq!(
      icons,
      vec![
        (0, "star".to_string()),
        (0, "prompt.key.KeyA".to_string()),
        (3, "prompt.key.KeyC".to_string()),
      ]
    );
  }

  #[test]
  fn a_label_is_bold_in_the_accent_inside_the_span_around_it() {
    let jump = ("jump", vec![Binding::Key(KeyCode::Space)]);
    let context = prompts_with(InputDevice::Keyboard, &[jump]);
    let text = RichText::markup("[size=20]Press [prompt=jump] now[/size]");
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(resolved.as_str(), "Press [Space] now");
    let spans: Vec<_> = resolved
      .spans
      .iter()
      .map(|mark| (mark.range.clone(), mark.span.clone()))
      .collect();
    assert_eq!(spans.len(), 3, "{spans:?}");
    assert_eq!(spans[0].0, 0..6);
    assert_eq!(spans[1].0, 6..13);
    assert_eq!(spans[1].1.color, Some(Color::YELLOW));
    assert_eq!(spans[1].1.weight, Some(700.0));
    assert_eq!(spans[1].1.size, Some(20.0), "the size around it stays");
    assert_eq!(spans[2].0, 13..17);
    assert_eq!(spans[2].1.size, Some(20.0));
  }

  #[test]
  fn an_unbound_prompt_shows_a_question_mark_and_warns_once() {
    let context = prompts_with(InputDevice::Gamepad, &[]);
    let text = RichText::new("Use ").prompt("t14_unbound_action");
    for _ in 0..2 {
      let resolved = resolve_prompts(&text, &context, Color::YELLOW);
      assert_eq!(resolved.as_str(), "Use [?]");
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("t14_unbound_action"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn ui_text_and_labels_are_laid_out_with_their_prompts_resolved() {
    let jump = (
      "jump",
      vec![
        Binding::Key(KeyCode::Space),
        Binding::Pad(GamepadButton::South),
      ],
    );
    for (device, label) in [
      (InputDevice::Keyboard, "Space"),
      (InputDevice::Gamepad, "A"),
    ] {
      let mut d =
        UiDriver::new((300.0, 100.0)).prompts(prompts_with(device, std::slice::from_ref(&jump)));
      // Rects in a container are last frame's: the second frame reads them
      let page = |ui: &mut UiContext<'_>| {
        let mut sizes = (Vec2::ZERO, Vec2::ZERO);
        ui.column().show(|ui| {
          sizes.0 = ui.text(RichText::markup("Press [prompt=jump]")).draw();
          let rect = ui.button(RichText::new("Go ").prompt("jump")).show().rect;
          sizes.1 = Vec2::new(rect.width, rect.height);
        });
        sizes
      };
      d.frame(page);
      let (leaf, button) = d.frame(page);
      let shown = shown(&d);
      assert!(shown.contains(&format!("Press [{label}]")), "{shown:?}");
      assert!(shown.contains(&format!("Go [{label}]")), "{shown:?}");
      let measured = d.frame(|ui| {
        let style = crate::core::text::TextStyle::default();
        ui.measure_text(format!("Press [{label}]"), &style)
      });
      assert!(leaf.x >= measured.x - 0.5, "measured as drawn: {leaf:?}");
      assert!(button.x > measured.x / 2.0, "{button:?}");
    }
  }

  #[test]
  fn a_registered_glyph_draws_a_caption_line_high() {
    let jump = ("jump", vec![Binding::Pad(GamepadButton::South)]);
    let mut context = prompts_with(InputDevice::Gamepad, &[jump]);
    context
      .icons
      .insert("prompt.pad.xbox.South", sprite("south"));
    let mut d = UiDriver::new((200.0, 100.0)).prompts(context);
    let line = d.frame(|ui| {
      ui.prompt("jump").show();
      let caption = ui.theme().text.caption.style();
      ui.line_height(&caption)
    });
    let rect = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Sprite { rect, .. } => Some(*rect),
      _ => None,
    });
    let rect = rect.expect("the glyph draws");
    assert!((rect.height - line).abs() < 1.0, "{rect:?} for {line}");
    assert!(
      (rect.width - 2.0 * line).abs() < 1.0,
      "its aspect: {rect:?}"
    );
    assert!(shown(&d).is_empty(), "no label");
  }
}
