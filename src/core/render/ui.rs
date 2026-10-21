//! The UI camera: the UI space (UI units, the UI image and the scale
//! between them) and the frame's UI commands laid into the 2D batch
//!
//! `Scene::ui` records [`UiCommand`]s in UI units. Each frame [`emit_ui`]
//! lays them into the draw list as unlit items for the UI camera alone,
//! ordered by their layer, then by the order they were recorded in (one z
//! per command, so the batch's sort keeps the painter's order). The `ui` pass draws the UI
//! camera into the UI image, which the composite blends over the frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec2;

use crate::core::ecs::Entity;
use crate::core::graphics::{
  Camera2d, CameraClear, CameraView, Presentation, RenderLayers, ScreenMapping, SpriteInstance,
};
use crate::core::render::batch::{
  BlendMode, Filter, LineLook, PolyPoint, PolyStyle, Triangles, line_color, push_polyline,
};
use crate::core::render::text::{TextDraw, TextPlacement, TextSources, TextTargets, UI_GLYPH_PX};
use crate::core::render::ui_retain::{UiRenderEnv, UiTextCache};
use crate::core::render::{CameraDraw, DrawKind, DrawShape, PixelRect, SpriteLook, SpriteShading};
use crate::core::text::TextLayout;
use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::immediate::{UiCommand, UiCommands};
use crate::core::ui::{Color, Rect};

/// The UI camera's entity: no entity in the world, only a key for its
/// uniforms, bind group and draws
pub(crate) const UI_CAMERA: Entity = Entity::PLACEHOLDER;

/// What resolution the UI image has.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiResolution {
  /// The render size: the window's physical pixels by default, the
  /// low-res target in pixel-art mode (the UI is pixel art too)
  #[default]
  Render,
  /// The window's physical pixels, also in pixel-art mode (smooth text and
  /// shapes over pixel art)
  Window,
}

/// UI units, the UI image and the scale between them
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UiSpace {
  /// The UI's size in UI units
  pub(crate) size: Vec2,
  /// Render pixels per UI unit
  pub(crate) px_scale: f32,
  /// The UI image's size in render pixels
  pub(crate) image: (u32, u32),
  /// UI units per screen unit (mouse positions are converted by it)
  pub(crate) mouse_scale: f32,
}

/// An invalid UI scale was warned about
static BAD_SCALE_WARNED: AtomicBool = AtomicBool::new(false);

impl UiSpace {
  /// The UI space of `mapping` at `ui_scale` render pixels per UI unit
  /// (`None`: automatic, UI units equal to screen units), drawn at
  /// `resolution`. An invalid scale is warned once and treated as automatic.
  pub(crate) fn of(
    mapping: &ScreenMapping,
    ui_scale: Option<f32>,
    resolution: UiResolution,
  ) -> UiSpace {
    let screen = mapping.screen_size();
    let image = match (resolution, mapping.presentation()) {
      (UiResolution::Window, Presentation::PixelArt { .. }) => {
        let size = (screen * mapping.scale()).round();
        ((size.x as u32).max(1), (size.y as u32).max(1))
      }
      _ => mapping.render_size(),
    };
    let automatic = image.0 as f32 / screen.x;
    let automatic = if automatic.is_finite() && automatic > 0.0 {
      automatic
    } else {
      1.0
    };
    let px_scale = match ui_scale {
      Some(scale) if scale.is_finite() && scale > 0.0 => scale,
      Some(scale) => {
        if !BAD_SCALE_WARNED.swap(true, Ordering::Relaxed) {
          crate::engine_warn!(module: "ui", "UI scale {scale} is not a positive number; using the automatic scale");
        }
        automatic
      }
      None => automatic,
    };
    UiSpace {
      size: Vec2::new(image.0 as f32, image.1 as f32) / px_scale,
      px_scale,
      image,
      mouse_scale: automatic / px_scale,
    }
  }
}

/// A command's place in the painter's order: its layer (0..=127, so the
/// sign bit stays clear), then the order it was recorded in (saturating
/// below 2^24, so it never reaches the layer bits)
pub(crate) fn ui_z(layer: u8, order: u32) -> i32 {
  const ORDER_BITS: u32 = 24;
  let layer = i32::from(layer.min(127));
  let order = order.min((1 << ORDER_BITS) - 1) as i32;
  (layer << ORDER_BITS) | order
}

/// `clip` (UI units) as a scissor on the UI image: `None` to skip the
/// command (an empty or invalid clip), `Some(None)` for no scissor
pub(crate) fn scissor_of(clip: Option<Rect>, space: &UiSpace) -> Option<Option<PixelRect>> {
  let Some(clip) = clip else {
    return Some(None);
  };
  let s = space.px_scale;
  let (w, h) = (space.image.0 as f32, space.image.1 as f32);
  let x0 = (clip.x * s).floor().clamp(0.0, w);
  let y0 = (clip.y * s).floor().clamp(0.0, h);
  let x1 = (clip.right() * s).ceil().clamp(0.0, w);
  let y1 = (clip.bottom() * s).ceil().clamp(0.0, h);
  // NaN fails these too
  if !(x1 > x0 && y1 > y0) {
    return None;
  }
  Some(Some(PixelRect {
    x: x0 as u32,
    y: y0 as u32,
    width: (x1 - x0) as u32,
    height: (y1 - y0) as u32,
  }))
}

/// `rect` with its edges on whole pixels of a `px_scale` grid (unchanged
/// when the scale is not a positive number)
pub(crate) fn snap_rect(rect: Rect, px_scale: f32) -> Rect {
  if !(px_scale.is_finite() && px_scale > 0.0) {
    return rect;
  }
  let edge = |at: f32| (at * px_scale).round() / px_scale;
  let (x0, y0) = (edge(rect.x), edge(rect.y));
  Rect::new(x0, y0, edge(rect.right()) - x0, edge(rect.bottom()) - y0)
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

/// A colour's channels as 0..1 (sRGB, straight alpha)
fn unit(color: Color) -> [f32; 4] {
  [color.r, color.g, color.b, color.a].map(|c| f32::from(c) / 255.0)
}

/// The UI camera's view: the UI space, top-left at (0, 0)
pub(crate) fn ui_view(space: &UiSpace) -> CameraView {
  CameraView::compute(
    space.size / 2.0,
    0.0,
    &Camera2d::default(),
    Vec2::ZERO,
    space.size,
  )
}

/// A rect command's quad (`UiRect`'s lanes) and the box it covers, or
/// `None` when it draws nothing. With `pixel_art` its edges land on whole
/// image pixels.
pub(crate) fn rect_instance(
  rect: Rect,
  fill: Color,
  radius: f32,
  border: Option<(f32, Color)>,
  space: &UiSpace,
  pixel_art: bool,
) -> Option<(SpriteInstance, Vec2, Vec2)> {
  let (border_width, border_color) = match border {
    Some((width, color)) if width.is_finite() && width > 0.0 && color.a > 0 => (width, color),
    _ => (0.0, Color::TRANSPARENT),
  };
  if fill.a == 0 && border_width == 0.0 {
    return None;
  }
  let rect = if pixel_art {
    snap_rect(rect, space.px_scale)
  } else {
    rect
  };
  if !drawable(rect) {
    return None;
  }
  let (w, h) = (rect.width, rect.height);
  let radius = if radius.is_finite() {
    radius.max(0.0)
  } else {
    0.0
  };
  // The quad reaches one image pixel past the rect on every side,
  // and its UVs as far past [0, 1], so the shader anti-aliases the
  // outer edge there
  let p = 1.0 / space.px_scale;
  let centre = Vec2::new(rect.x + w / 2.0, rect.y + h / 2.0);
  let mut instance = SpriteInstance::new()
    .with_transform(centre.x, centre.y, 0.0, w + 2.0 * p, h + 2.0 * p)
    .with_tint(fill, 1.0);
  instance.uv_rect = [-p / w, -p / h, 1.0 + p / w, 1.0 + p / h];
  instance.uv_bounds = instance.uv_rect;
  instance.effect_params = [radius, border_width, w, h];
  instance.effect_extra = unit(border_color);
  let half = Vec2::new(w / 2.0 + p, h / 2.0 + p);
  Some((instance, centre - half, centre + half))
}

/// A sprite command's quad (before it is placed in its texture's slot), or
/// `None` when it draws nothing
pub(crate) fn sprite_instance(rect: Rect, uv: [f32; 4], tint: Color) -> Option<SpriteInstance> {
  if tint.a == 0 || !drawable(rect) {
    return None;
  }
  let (cx, cy) = rect.center();
  Some(
    SpriteInstance::new()
      .with_transform(cx, cy, 0.0, rect.width, rect.height)
      .with_uv(uv[0], uv[1], uv[2], uv[3])
      .with_tint(tint, 1.0),
  )
}

/// A line command's points (into `points`, cleared first) and how it is
/// drawn at `z`, or `None` for another command
pub(crate) fn line_of(
  command: &UiCommand,
  z: i32,
  points: &mut Vec<PolyPoint>,
) -> Option<(PolyStyle, LineLook)> {
  let UiCommand::Line {
    points: line,
    at,
    width,
    color,
    closed,
    ..
  } = command
  else {
    return None;
  };
  let last = line.len().saturating_sub(1).max(1) as f32;
  points.clear();
  points.extend(line.iter().enumerate().map(|(i, pos)| PolyPoint {
    pos: *pos + *at,
    half_width: width / 2.0,
    color: line_color(*color, 1.0),
    u: i as f32 / last,
  }));
  let style = PolyStyle {
    join: LineJoin::Miter,
    cap: LineCap::Butt,
    closed: *closed,
    params: [0.0; 4],
  };
  let look = LineLook {
    lit: false,
    z,
    texture: None,
    blend: BlendMode::Alpha,
    filter: Filter::Default,
    layers: RenderLayers::all(),
    only_camera: Some(UI_CAMERA),
  };
  Some((style, look))
}

/// A text command's layout and where it draws at `z` under `scissor`, or
/// `None` for another command or a layout without a usable scale
pub(crate) fn text_of<'c>(
  command: &'c UiCommand,
  space: &UiSpace,
  z: i32,
  scissor: Option<PixelRect>,
  time: f32,
) -> Option<(&'c Arc<TextLayout>, TextPlacement)> {
  let UiCommand::Text {
    layout,
    origin,
    reveal,
    opacity,
    seed,
    ..
  } = command
  else {
    return None;
  };
  if !(layout.scale.is_finite() && layout.scale > 0.0) {
    return None;
  }
  // Layout pixels to UI units (the layout was made at the UI scale)
  let k = 1.0 / layout.scale;
  let place = TextPlacement {
    origin: *origin,
    axis_x: Vec2::new(k, 0.0),
    axis_y: Vec2::new(0.0, k),
    px_per_layout: space.px_scale * k,
    size_steps: false,
    max_glyph_px: UI_GLYPH_PX,
    pixel_grid: Some((Vec2::ZERO, space.px_scale)),
    draw: TextDraw {
      kind: DrawKind::Sprite,
      z,
      lit: false,
      only_camera: Some(UI_CAMERA),
      layers: RenderLayers::all(),
      scissor,
      blend: BlendMode::Alpha,
    },
    time,
    reveal: *reveal,
    seed: *seed,
    opacity: *opacity,
  };
  Some((layout, place))
}

/// Lay the frame's UI commands into the draw list for the UI camera and
/// return that camera, or `None` when nothing was drawn.
///
/// With `env.on_target` the UI image is the pixel-art target: rects land on
/// whole target pixels, like the sprites the camera snaps. Without `sources`
/// (a game removed the text resources) text is skipped and the rest drawn.
/// Texts are drawn through `texts`, which reuses the glyphs of those drawn
/// unchanged before.
pub(crate) fn emit_ui(
  commands: &UiCommands,
  env: &UiRenderEnv,
  sources: Option<&TextSources<'_>>,
  targets: &mut TextTargets<'_>,
  triangles: &mut Triangles,
  real_time: f32,
  texts: &mut UiTextCache,
) -> Option<CameraDraw> {
  if commands.is_empty() {
    return None;
  }
  let (space, pixel_art) = (&env.space, env.on_target);
  let view = ui_view(space);
  let only = Some(UI_CAMERA);
  let layers = RenderLayers::all();
  let mut points: Vec<PolyPoint> = Vec::new();
  for (order, command) in commands.iter() {
    // Painter order: z orders the layer, then the recorded order
    let z = ui_z(command.draw_layer(), order);
    let Some(scissor) = scissor_of(command.clip_rect(), space) else {
      continue;
    };
    match command {
      UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        ..
      } => {
        let Some((instance, min, max)) =
          rect_instance(*rect, *fill, *radius, *border, space, pixel_art)
        else {
          continue;
        };
        let shape = DrawShape {
          instance,
          blend: BlendMode::Alpha,
          layers,
          min,
          max,
          scissor,
        };
        targets
          .list
          .push_shape(DrawKind::Sprite, z, false, only, shape);
      }
      UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        ..
      } => {
        let Some(instance) = sprite_instance(*rect, *uv, *tint) else {
          continue;
        };
        let texture = targets.sprites.texture_of(sprite);
        let look = SpriteLook {
          shading: SpriteShading::Plain,
          blend: BlendMode::Alpha,
          filter: *filter,
          lit: false,
        };
        let list = &mut *targets.list;
        list.push_sprite_in(
          DrawKind::Sprite,
          z,
          texture,
          look,
          instance,
          (layers, None, only),
        );
        list.set_last_sprite_scissor(scissor);
      }
      UiCommand::Line { .. } => {
        let Some((style, look)) = line_of(command, z, &mut points) else {
          continue;
        };
        let list = &mut *targets.list;
        let before = list.triangles_count();
        push_polyline(list, triangles, &points, &style, look);
        // Only a line that made triangles is clipped (never an earlier one)
        if list.triangles_count() > before {
          list.set_last_triangles_scissor(scissor);
        }
      }
      UiCommand::Text { .. } => {
        let Some(sources) = sources else {
          continue;
        };
        let Some((layout, place)) = text_of(command, space, z, scissor, real_time) else {
          continue;
        };
        texts.emit(sources, targets, layout, &place);
      }
    }
  }
  Some(CameraDraw {
    entity: UI_CAMERA,
    view,
    layers,
    clear: CameraClear::None,
    target: None,
    direct: false,
    own_image: false,
    rect: PixelRect {
      x: 0,
      y: 0,
      width: space.image.0,
      height: space.image.1,
    },
    lit: false,
    ui: true,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{Presentation, ScreenMapping};

  #[test]
  fn the_automatic_scale_keeps_ui_units_equal_to_screen_units() {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (2560, 1440), 2.0);
    let space = UiSpace::of(&mapping, None, UiResolution::Render);
    assert_eq!(space.image, (2560, 1440));
    assert_eq!(space.px_scale, 2.0);
    assert_eq!(space.size, Vec2::new(1280.0, 720.0));
    assert_eq!(space.mouse_scale, 1.0);
  }

  #[test]
  fn a_ui_scale_makes_the_ui_bigger_and_converts_the_mouse() {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (1280, 720), 1.0);
    let space = UiSpace::of(&mapping, Some(2.0), UiResolution::Render);
    assert_eq!(space.size, Vec2::new(640.0, 360.0));
    assert_eq!(space.mouse_scale, 0.5);
  }

  #[test]
  fn pixel_art_draws_on_the_target_or_the_window() {
    let mapping = ScreenMapping::new(
      Presentation::PixelArt {
        width: 320,
        height: 180,
      },
      (1280, 720),
      1.0,
    );
    let target = UiSpace::of(&mapping, None, UiResolution::Render);
    assert_eq!(
      (target.image, target.px_scale, target.size),
      ((320, 180), 1.0, Vec2::new(320.0, 180.0))
    );
    let window = UiSpace::of(&mapping, None, UiResolution::Window);
    assert_eq!(
      (window.image, window.px_scale, window.size),
      ((1280, 720), 4.0, Vec2::new(320.0, 180.0))
    );
    assert_eq!(window.mouse_scale, 1.0);
  }

  #[test]
  fn a_bad_ui_scale_falls_back_to_automatic() {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (1280, 720), 1.0);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
      let space = UiSpace::of(&mapping, Some(bad), UiResolution::Render);
      assert_eq!(space.px_scale, 1.0, "{bad}");
    }
  }

  #[test]
  fn ui_z_orders_layers_then_submission_without_overflowing() {
    // Layers above 127 would reach the sign bit
    assert!(ui_z(255, 0) > 0);
    assert_eq!(ui_z(255, 0), ui_z(127, 0));
    // An order past 2^24 would spill into the layer bits
    assert_eq!(ui_z(0, u32::MAX), (1 << 24) - 1);
    assert!(ui_z(0, u32::MAX) < ui_z(1, 0));
    assert!(ui_z(0, 3) < ui_z(0, 4));
    assert!(ui_z(1, 0) < ui_z(1, 1));
  }

  #[test]
  fn clips_become_scissors_on_whole_image_pixels() {
    let space = UiSpace {
      size: Vec2::new(100.0, 50.0),
      px_scale: 2.0,
      image: (200, 100),
      mouse_scale: 1.0,
    };
    assert_eq!(scissor_of(None, &space), Some(None));
    // Outward to whole pixels
    let clip = Rect::new(10.3, 5.0, 20.0, 10.2);
    assert_eq!(
      scissor_of(Some(clip), &space),
      Some(Some(PixelRect {
        x: 20,
        y: 10,
        width: 41,
        height: 21
      }))
    );
    // Clamped to the image
    let wide = Rect::new(-10.0, -10.0, 500.0, 500.0);
    assert_eq!(
      scissor_of(Some(wide), &space),
      Some(Some(PixelRect {
        x: 0,
        y: 0,
        width: 200,
        height: 100
      }))
    );
    // Empty, off the image or not a number: the command is skipped
    for gone in [
      Rect::new(10.0, 10.0, 0.0, 10.0),
      Rect::new(300.0, 10.0, 10.0, 10.0),
      Rect::new(f32::NAN, 10.0, 10.0, 10.0),
    ] {
      assert_eq!(scissor_of(Some(gone), &space), None, "{gone:?}");
    }
  }

  #[test]
  fn pixel_art_rects_snap_to_whole_pixels() {
    let rect = snap_rect(Rect::new(10.3, 10.6, 5.3, 4.0), 1.0);
    assert_eq!(rect, Rect::new(10.0, 11.0, 6.0, 4.0));
    let rect = snap_rect(Rect::new(10.3, 0.0, 5.0, 1.0), 2.0);
    assert_eq!(rect, Rect::new(10.5, 0.0, 5.0, 1.0));
  }
}
