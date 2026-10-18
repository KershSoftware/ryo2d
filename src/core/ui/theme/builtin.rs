//! The built-in themes: dark, light and pixel

use std::collections::BTreeMap;

use super::{
  FocusRing, Look, LookPatch, Looks, Metrics, Palette, Sizes, Skin, TextLook, TextRoles, Theme,
  ThemeFont, UiSounds, Variant, WidgetKind, WidgetLooks,
};
use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;
use crate::core::render::batch::Filter;
use crate::core::ui::{Color, Edges};

/// A disabled look's alpha, fills, borders and labels alike
const DISABLED_ALPHA: u8 = 120;

/// The dark theme's colours
pub(super) fn dark_palette() -> Palette {
  Palette {
    background: Color::rgb(20, 20, 30),
    surface: Color::rgb(40, 40, 60),
    text: Color::WHITE,
    muted: Color::GRAY,
    accent: Color::rgb(80, 150, 255),
    danger: Color::rgb(220, 70, 70),
    success: Color::rgb(80, 190, 110),
    warning: Color::rgb(230, 180, 60),
  }
}

/// The metrics every built-in theme starts from
pub(super) fn metrics() -> Metrics {
  Metrics {
    radius: 4.0,
    spacing: 8.0,
    border: 1.0,
    focus_ring: FocusRing {
      width: 2.0,
      offset: 2.0,
      color: Color::rgb(100, 140, 200),
    },
    scroll_line: 40.0,
    scroll_smoothing: 0.08,
    repeat_delay: 0.35,
    repeat_rate: 0.1,
    tooltip_delay: 0.5,
    drag_threshold: 4.0,
    double_click: 0.3,
    sizes: Sizes {
      button_padding: [12.0, 6.0],
      button_min_height: 28.0,
      panel_padding: 8.0,
      slider: [180.0, 8.0],
      slider_handle: [12.0, 16.0],
      toggle: [44.0, 24.0],
      checkbox: 20.0,
      scrollbar: 10.0,
      slot: 48.0,
      text_input: [200.0, 28.0],
    },
  }
}

/// The text roles in `palette`'s colours
pub(super) fn text_roles(palette: &Palette) -> TextRoles {
  let role = |size: f32, color: Color, font: ThemeFont, bold: bool| TextLook {
    size,
    color,
    font,
    bold,
  };
  TextRoles {
    body: role(14.0, palette.text, ThemeFont::Ui, false),
    heading: role(20.0, palette.text, ThemeFont::Ui, true),
    button: role(14.0, palette.text, ThemeFont::Ui, false),
    caption: role(12.0, palette.muted, ThemeFont::Ui, false),
    mono: role(13.0, palette.text, ThemeFont::Mono, false),
  }
}

/// The colours of a control: a fill and border that lighten under the
/// pointer, a label, and a ring colour for focus
struct Colors {
  fill: Color,
  hover: Color,
  press: Color,
  border: Color,
  border_hover: Color,
  text: Color,
  off_fill: Color,
  off_text: Color,
  ring: Color,
}

impl Colors {
  /// The looks of a control in these colours, with corners of `radius`
  fn looks(&self, radius: f32) -> Looks {
    Looks {
      normal: Look {
        fill: self.fill,
        border: self.border,
        border_width: 1.0,
        radius,
        text: self.text,
        ..Look::default()
      },
      hovered: LookPatch::default()
        .fill(self.hover)
        .border(self.border_hover),
      pressed: LookPatch::default().fill(self.press),
      focused: LookPatch::default().fill(self.hover).border(self.ring),
      disabled: LookPatch::default()
        .fill(self.off_fill)
        .border(self.border.with_alpha(DISABLED_ALPHA))
        .text(self.off_text),
    }
  }
}

/// A button's colours in the dark theme
fn dark_button_colors() -> Colors {
  Colors {
    fill: Color::rgb(40, 40, 60),
    hover: Color::rgb(50, 50, 70),
    press: Color::rgb(60, 60, 80),
    border: Color::rgb(80, 80, 100),
    border_hover: Color::rgb(100, 100, 130),
    text: Color::WHITE,
    off_fill: Color::new(40, 40, 60, DISABLED_ALPHA),
    off_text: Color::new(255, 255, 255, DISABLED_ALPHA),
    ring: Color::rgb(100, 140, 200),
  }
}

/// A button's colours in the light theme
fn light_button_colors() -> Colors {
  Colors {
    fill: Color::rgb(220, 220, 230),
    hover: Color::rgb(200, 200, 210),
    press: Color::rgb(180, 180, 190),
    border: Color::rgb(180, 180, 190),
    border_hover: Color::rgb(150, 150, 160),
    text: Color::rgb(30, 30, 40),
    off_fill: Color::rgb(240, 240, 245),
    off_text: Color::rgb(150, 150, 160),
    ring: Color::rgb(100, 140, 200),
  }
}

/// The looks of a plain text field in these colours
fn field_looks(
  fill: Color,
  fill_focused: Color,
  border: Color,
  border_focused: Color,
  text: Color,
  off_text: Color,
) -> Looks {
  Looks {
    normal: Look {
      fill,
      border,
      border_width: 1.0,
      radius: 4.0,
      text,
      padding: Edges::xy(8.0, 4.0),
      ..Look::default()
    },
    hovered: LookPatch::default(),
    pressed: LookPatch::default(),
    focused: LookPatch::default()
      .fill(fill_focused)
      .border(border_focused),
    disabled: LookPatch::default()
      .fill(fill.with_alpha(DISABLED_ALPHA))
      .border(border.with_alpha(DISABLED_ALPHA))
      .text(off_text),
  }
}

/// The looks of a choice (a dropdown's option, a choice list's entry): a
/// plain row whose label sits on its list, filled when hovered, pressed or
/// focused
fn choice_looks(hover: Color, press: Color, text: Color, off_text: Color) -> Looks {
  Looks {
    normal: Look {
      radius: 4.0,
      text,
      padding: Edges::xy(10.0, 4.0),
      ..Look::default()
    },
    hovered: LookPatch::default().fill(hover),
    pressed: LookPatch::default().fill(press),
    focused: LookPatch::default().fill(hover),
    disabled: LookPatch::default().text(off_text),
  }
}

/// The dark theme's looks for every widget kind
fn dark_widgets() -> WidgetLooks {
  let button = dark_button_colors();
  let mut button_looks = button.looks(4.0);
  button_looks.normal.padding = Edges::xy(12.0, 6.0);
  let check = Colors {
    fill: Color::rgb(40, 40, 60),
    hover: Color::rgb(50, 50, 70),
    press: Color::rgb(60, 60, 80),
    border: Color::rgb(80, 80, 100),
    border_hover: Color::rgb(100, 100, 130),
    text: Color::WHITE,
    off_fill: Color::new(40, 40, 60, DISABLED_ALPHA),
    off_text: Color::new(255, 255, 255, DISABLED_ALPHA),
    ring: Color::rgb(100, 140, 200),
  };
  let toggle = Colors {
    fill: Color::rgb(60, 60, 80),
    hover: Color::rgb(70, 70, 90),
    press: Color::rgb(70, 70, 90),
    border: Color::rgb(60, 60, 80),
    border_hover: Color::rgb(70, 70, 90),
    ..dark_button_colors()
  };
  let list = Colors {
    border: Color::rgb(60, 60, 80),
    ..dark_button_colors()
  };
  let tab = Colors {
    fill: Color::rgb(30, 30, 40),
    border: Color::rgb(60, 60, 80),
    ..dark_button_colors()
  };
  WidgetLooks {
    button: button_looks,
    panel: Looks {
      normal: Look {
        fill: Color::new(20, 20, 30, 200),
        border: Color::rgb(80, 80, 100),
        border_width: 1.0,
        radius: 4.0,
        ..Look::default()
      },
      ..Looks::default()
    },
    checkbox: check.looks(3.0),
    toggle: toggle.looks(12.0),
    radio: check.looks(10.0),
    slider: Looks {
      // The track is the fill, the filled part the border, the handle the text
      normal: Look {
        fill: Color::rgb(60, 60, 80),
        border: Color::rgb(80, 150, 255),
        radius: 4.0,
        text: Color::WHITE,
        ..Look::default()
      },
      hovered: LookPatch::default().text(Color::rgb(200, 220, 255)),
      pressed: LookPatch::default(),
      focused: LookPatch::default().text(Color::rgb(200, 220, 255)),
      disabled: LookPatch::default()
        .fill(Color::new(60, 60, 80, DISABLED_ALPHA))
        .border(Color::new(80, 150, 255, DISABLED_ALPHA))
        .text(Color::new(255, 255, 255, DISABLED_ALPHA)),
    },
    text_input: field_looks(
      Color::rgb(30, 30, 45),
      Color::rgb(35, 35, 50),
      Color::rgb(70, 70, 90),
      Color::rgb(100, 140, 200),
      Color::WHITE,
      Color::new(255, 255, 255, DISABLED_ALPHA),
    ),
    dropdown: list.looks(4.0),
    tab: tab.looks(4.0),
    choice: choice_looks(
      Color::rgb(50, 50, 70),
      Color::rgb(60, 60, 80),
      Color::WHITE,
      Color::new(255, 255, 255, DISABLED_ALPHA),
    ),
    tooltip: Looks {
      normal: Look {
        fill: Color::new(30, 30, 40, 240),
        border: Color::rgb(70, 70, 90),
        border_width: 1.0,
        radius: 4.0,
        padding: Edges::all(6.0),
        ..Look::default()
      },
      ..Looks::default()
    },
    scroll: Looks {
      // The track is the fill, the thumb the border
      normal: Look {
        fill: Color::new(40, 40, 50, 180),
        border: Color::rgb(90, 90, 110),
        radius: 3.0,
        ..Look::default()
      },
      hovered: LookPatch::default().border(Color::rgb(120, 120, 140)),
      pressed: LookPatch::default().border(Color::rgb(120, 120, 140)),
      ..Looks::default()
    },
    slot: Looks {
      normal: Look {
        fill: Color::rgb(30, 30, 45),
        border: Color::rgb(70, 70, 90),
        border_width: 1.0,
        radius: 4.0,
        ..Look::default()
      },
      hovered: LookPatch::default().border(Color::rgb(100, 100, 130)),
      pressed: LookPatch::default().fill(Color::rgb(40, 40, 60)),
      focused: LookPatch::default().border(Color::rgb(100, 140, 200)),
      disabled: LookPatch::default().fill(Color::new(30, 30, 45, DISABLED_ALPHA)),
    },
    progress: Looks {
      // The track is the fill, the filled part the border
      normal: Look {
        fill: Color::DARK_GRAY,
        border: Color::rgb(80, 150, 255),
        radius: 4.0,
        ..Look::default()
      },
      ..Looks::default()
    },
    rebind: dark_button_colors().looks(4.0),
    // The dim over the UI behind a modal; its dialog has the panel look,
    // made opaque
    modal: Looks {
      normal: Look {
        fill: Color::new(0, 0, 0, 160),
        ..Look::default()
      },
      ..Looks::default()
    },
    prompt: Looks {
      normal: Look {
        fill: Color::rgb(50, 50, 70),
        border: Color::rgb(100, 100, 130),
        border_width: 1.0,
        radius: 4.0,
        ..Look::default()
      },
      ..Looks::default()
    },
  }
}

/// The variants every built-in theme carries: `danger` and `accent`
/// buttons, `tab` for a button that reads as a tab, and `selected` for the
/// selected tab of `ui.tabs`
fn standard_variants() -> BTreeMap<String, Variant> {
  let button = |normal: LookPatch, hovered: LookPatch, pressed: LookPatch| {
    Variant::of(WidgetKind::Button)
      .normal(normal)
      .hovered(hovered)
      .pressed(pressed)
  };
  let mut variants = BTreeMap::new();
  variants.insert(
    "danger".to_string(),
    button(
      LookPatch::default()
        .fill(Color::rgb(150, 50, 50))
        .border(Color::rgb(200, 80, 80))
        .text(Color::WHITE),
      LookPatch::default()
        .fill(Color::rgb(175, 60, 60))
        .border(Color::rgb(230, 100, 100)),
      LookPatch::default().fill(Color::rgb(130, 40, 40)),
    ),
  );
  variants.insert(
    "accent".to_string(),
    button(
      LookPatch::default()
        .fill(Color::rgb(50, 100, 180))
        .border(Color::rgb(80, 150, 255))
        .text(Color::WHITE),
      LookPatch::default()
        .fill(Color::rgb(60, 115, 200))
        .border(Color::rgb(120, 175, 255)),
      LookPatch::default().fill(Color::rgb(45, 90, 165)),
    ),
  );
  variants.insert(
    "tab".to_string(),
    button(
      LookPatch::default()
        .fill(Color::TRANSPARENT)
        .border(Color::TRANSPARENT),
      LookPatch::default()
        .fill(Color::new(50, 50, 70, 160))
        .border(Color::TRANSPARENT),
      LookPatch::default().fill(Color::new(60, 60, 80, 200)),
    ),
  );
  variants.insert(
    "selected".to_string(),
    Variant::of(WidgetKind::Tab)
      .normal(
        LookPatch::default()
          .fill(Color::rgb(50, 100, 180))
          .border(Color::rgb(80, 150, 255))
          .text(Color::WHITE),
      )
      .hovered(LookPatch::default().fill(Color::rgb(60, 115, 200)))
      .pressed(LookPatch::default().fill(Color::rgb(45, 90, 165)))
      .focused(LookPatch::default().fill(Color::rgb(60, 115, 200))),
  );
  variants
}

/// The dark theme
pub(super) fn dark() -> Theme {
  let palette = dark_palette();
  Theme {
    name: "dark".to_string(),
    text: text_roles(&palette),
    palette,
    metrics: metrics(),
    widgets: dark_widgets(),
    variants: standard_variants(),
    skins: BTreeMap::new(),
    sounds: UiSounds::default(),
    source: None,
  }
}

/// The light theme
pub(super) fn light() -> Theme {
  let palette = Palette {
    background: Color::rgb(235, 235, 240),
    surface: Color::rgb(245, 245, 250),
    text: Color::rgb(30, 30, 40),
    muted: Color::rgb(120, 120, 130),
    accent: Color::rgb(40, 100, 210),
    danger: Color::rgb(200, 50, 50),
    success: Color::rgb(50, 160, 80),
    warning: Color::rgb(210, 150, 30),
  };
  let button = light_button_colors();
  let mut widgets = dark_widgets();
  widgets.button = button.looks(4.0);
  widgets.button.normal.padding = Edges::xy(12.0, 6.0);
  widgets.checkbox = button.looks(3.0);
  widgets.radio = button.looks(10.0);
  widgets.dropdown = button.looks(4.0);
  widgets.choice = choice_looks(
    Color::rgb(215, 215, 225),
    Color::rgb(200, 200, 210),
    Color::rgb(30, 30, 40),
    Color::rgb(150, 150, 160),
  );
  widgets.rebind = button.looks(4.0);
  widgets.tab = Colors {
    fill: Color::rgb(235, 235, 240),
    ..light_button_colors()
  }
  .looks(4.0);
  widgets.toggle = Colors {
    fill: Color::rgb(200, 200, 210),
    hover: Color::rgb(190, 190, 200),
    press: Color::rgb(180, 180, 190),
    text: Color::WHITE,
    ..light_button_colors()
  }
  .looks(12.0);
  widgets.panel.normal.fill = Color::new(245, 245, 250, 230);
  widgets.panel.normal.border = Color::rgb(200, 200, 210);
  widgets.text_input = field_looks(
    Color::rgb(255, 255, 255),
    Color::rgb(250, 250, 255),
    Color::rgb(180, 180, 190),
    Color::rgb(100, 140, 200),
    Color::rgb(30, 30, 40),
    Color::rgb(150, 150, 160),
  );
  widgets.slider.normal.fill = Color::rgb(200, 200, 210);
  widgets.slider.normal.border = palette.accent;
  widgets.slider.normal.text = Color::WHITE;
  widgets.slider.hovered = LookPatch::default().text(Color::rgb(235, 240, 255));
  widgets.slot.normal.fill = Color::rgb(255, 255, 255);
  widgets.slot.normal.border = Color::rgb(180, 180, 190);
  widgets.scroll.normal.fill = Color::new(200, 200, 210, 180);
  widgets.scroll.normal.border = Color::rgb(160, 160, 175);
  widgets.scroll.hovered = LookPatch::default().border(Color::rgb(130, 130, 150));
  widgets.scroll.pressed = LookPatch::default().border(Color::rgb(110, 110, 130));
  widgets.progress.normal.fill = Color::rgb(200, 200, 210);
  widgets.progress.normal.border = palette.accent;
  widgets.prompt.normal.fill = Color::rgb(225, 225, 235);
  widgets.prompt.normal.border = Color::rgb(170, 170, 185);
  widgets.prompt.normal.text = Color::rgb(30, 30, 40);
  widgets.tooltip.normal.fill = Color::new(50, 50, 60, 240);
  widgets.tooltip.normal.border = Color::rgb(70, 70, 90);
  widgets.tooltip.normal.text = Color::WHITE;
  Theme {
    name: "light".to_string(),
    text: text_roles(&palette),
    palette,
    widgets,
    ..dark()
  }
}

/// A 12 by 12 skin sprite: a one-pixel `border`, a lighter bevel on the top
/// and left, a darker one on the bottom and right, and `fill` inside
fn skin_sprite(name: &str, border: Color, fill: Color, light: Color, dark: Color) -> SpriteHandle {
  const SIDE: u32 = 12;
  let last = SIDE - 1;
  let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
  for y in 0..SIDE {
    for x in 0..SIDE {
      let color = if x == 0 || y == 0 || x == last || y == last {
        border
      } else if x == 1 || y == 1 {
        light
      } else if x == last - 1 || y == last - 1 {
        dark
      } else {
        fill
      };
      data.extend_from_slice(&[color.r, color.g, color.b, color.a]);
    }
  }
  SpriteHandle::from_sprite(Sprite::from_rgba(data, SIDE, SIDE), name)
}

/// The pixel theme
pub(super) fn pixel() -> Theme {
  let mut theme = dark();
  theme.name = "pixel".to_string();
  for look in [
    &mut theme.text.body,
    &mut theme.text.heading,
    &mut theme.text.button,
    &mut theme.text.caption,
    &mut theme.text.mono,
  ] {
    look.font = ThemeFont::Pixel;
    look.size = 16.0;
    look.bold = false;
  }
  theme.metrics.radius = 0.0;
  theme.metrics.focus_ring.color = Color::rgb(255, 220, 80);
  theme.skins.insert(
    "button".to_string(),
    Skin::from_sprite(
      skin_sprite(
        "ryo2d-pixel-button",
        Color::rgb(16, 16, 28),
        Color::rgb(104, 104, 144),
        Color::rgb(152, 152, 196),
        Color::rgb(76, 76, 108),
      ),
      [4.0; 4],
      Filter::Nearest,
    ),
  );
  theme.skins.insert(
    "panel".to_string(),
    Skin::from_sprite(
      skin_sprite(
        "ryo2d-pixel-panel",
        Color::rgb(16, 16, 28),
        Color::new(34, 34, 54, 235),
        Color::new(62, 62, 92, 235),
        Color::new(26, 26, 42, 235),
      ),
      [4.0; 4],
      Filter::Nearest,
    ),
  );

  let widgets = &mut theme.widgets;
  for looks in [
    &mut widgets.button,
    &mut widgets.panel,
    &mut widgets.checkbox,
    &mut widgets.toggle,
    &mut widgets.radio,
    &mut widgets.slider,
    &mut widgets.text_input,
    &mut widgets.dropdown,
    &mut widgets.tab,
    &mut widgets.choice,
    &mut widgets.tooltip,
    &mut widgets.scroll,
    &mut widgets.slot,
    &mut widgets.progress,
    &mut widgets.rebind,
    &mut widgets.modal,
    &mut widgets.prompt,
  ] {
    looks.normal.radius = 0.0;
  }
  // Buttons and panels draw their skin, tinted by the fill
  let button = &mut widgets.button;
  button.normal.fill = Color::WHITE;
  button.normal.border_width = 0.0;
  button.normal.skin = Some("button".into());
  button.hovered = LookPatch::default().fill(Color::rgb(225, 225, 255));
  button.pressed = LookPatch::default().fill(Color::rgb(170, 170, 200));
  button.focused = LookPatch::default().fill(Color::rgb(225, 225, 255));
  button.disabled = LookPatch::default()
    .fill(Color::new(255, 255, 255, DISABLED_ALPHA))
    .text(Color::new(255, 255, 255, DISABLED_ALPHA));
  let panel = &mut widgets.panel;
  panel.normal.fill = Color::WHITE;
  panel.normal.border_width = 0.0;
  panel.normal.skin = Some("panel".into());
  theme
}
