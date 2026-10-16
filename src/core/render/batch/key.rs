//! What decides whether two drawables can share a draw call.

use crate::core::graphics::{Presentation, SlotPage};
use crate::core::render::PixelRect;

/// How a drawable's colour combines with what is already drawn.
///
/// Colours are premultiplied by alpha on the way out of every shader, so
/// each mode is one fixed blend state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
  /// Ordinary transparency
  #[default]
  Alpha,
  /// The texture's colours are already multiplied by their alpha (render
  /// targets drawn back as sprites, premultiplied art)
  Premultiplied,
  /// Adds light: fire, sparks, glows and lasers
  Additive,
  /// Darkens what is below by the drawable's colour: shade and stains
  Multiply,
  /// Lightens what is below without blowing out to white. A 0..1 formula:
  /// over pixels brighter than white (HDR light, emission) it dims them, and
  /// an emissive Screen drawable over a bright pixel can go below zero and
  /// show black. Use `Additive` for light above white
  Screen,
}

/// What a sampled page holds: sRGB bytes (atlas and standalone pages) or
/// linear colour premultiplied by alpha (render targets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PageKind {
  Srgb8,
  Linear,
}

impl PageKind {
  /// The kind of `page`; untextured draws count as sRGB.
  pub(crate) fn of(page: Option<SlotPage>) -> Self {
    match page {
      Some(SlotPage::Target(_)) => Self::Linear,
      _ => Self::Srgb8,
    }
  }
}

/// How a texture is sampled when drawn at other than its own pixel size.
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Filter {
  /// Nearest for pixel-art presentation, linear otherwise
  #[default]
  Default,
  /// Hard pixel edges
  Nearest,
  /// Smooth blending between texels
  Linear,
}

/// The sampler a draw uses: a [`Filter`] resolved for the presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Sampling {
  Nearest,
  Linear,
}

impl Filter {
  /// The sampling this filter means under `presentation`.
  pub(crate) fn resolve(self, presentation: Presentation) -> Sampling {
    match (self, presentation) {
      (Filter::Nearest, _) | (Filter::Default, Presentation::PixelArt { .. }) => Sampling::Nearest,
      (Filter::Linear, _) | (Filter::Default, Presentation::ResolutionIndependent) => {
        Sampling::Linear
      }
    }
  }
}

/// The shader a batch draw uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BatchShader {
  /// Texture times tint
  Sprite,
  /// A sprite with a built-in effect
  SpriteEffect,
  /// The game shader with this source key
  Custom(u64),
  /// Untextured line geometry
  Line,
  /// Line geometry with a texture along it
  LineTextured,
  /// A light (or its shadow) into a light buffer
  Light(LightStep),
  /// A raster glyph: its coverage times the tint
  TextMask,
  /// A colour glyph times the tint
  TextColor,
  /// An MSDF glyph, with outline or as a soft shadow or glow layer
  TextMsdf,
  /// An untextured UI rectangle with rounded corners and a border
  UiRect,
}

/// One draw of a light into a light buffer. A shadowed light draws, inside
/// its quad: `ResetMask`, its `Shadow` volumes, `Masked`, and `Coloured` if
/// its shadow is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LightStep {
  /// Light added up (lights without shadow volumes)
  Add,
  /// The light buffer's alpha set to 1 in the light's quad
  ResetMask,
  /// Each shadow volume keeps 1 - strength of the alpha
  Shadow,
  /// The light added where the alpha lets it through
  Masked,
  /// The light, tinted by its shadow colour, where the alpha held it back
  Coloured,
}

impl LightStep {
  /// The entry point that draws it.
  pub(crate) fn fragment(self) -> &'static str {
    match self {
      LightStep::Add | LightStep::Masked => "fs_light",
      LightStep::ResetMask => "fs_mask_reset",
      LightStep::Shadow => "fs_shadow",
      LightStep::Coloured => "fs_light_coloured",
    }
  }

  /// The stream it draws from: shadow volumes are triangles, the rest quads.
  pub(crate) fn stream(self) -> Stream {
    match self {
      LightStep::Shadow => Stream::Triangles,
      _ => Stream::Quads,
    }
  }
}

/// Which GPU buffer a draw reads: instanced quads or indexed triangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Stream {
  Quads,
  Triangles,
}

/// Everything a draw binds, resolved: consecutive drawables with equal
/// states share a draw call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawState {
  /// The atlas or render-target page sampled; `None` for untextured geometry
  pub(crate) page: Option<SlotPage>,
  pub(crate) shader: BatchShader,
  pub(crate) blend: BlendMode,
  pub(crate) sampling: Sampling,
  /// Pixels outside are not drawn (`None`: the whole image)
  pub(crate) scissor: Option<PixelRect>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Presentation;

  #[test]
  fn render_targets_are_linear_pages() {
    use crate::core::graphics::{RenderTargets, SlotPage};
    let target = RenderTargets::default().create(4, 4);
    assert_eq!(
      PageKind::of(Some(SlotPage::Target(target))),
      PageKind::Linear
    );
    assert_eq!(PageKind::of(Some(SlotPage::Atlas(0))), PageKind::Srgb8);
    assert_eq!(PageKind::of(Some(SlotPage::Standalone(2))), PageKind::Srgb8);
    assert_eq!(PageKind::of(None), PageKind::Srgb8);
  }

  #[test]
  fn the_default_filter_follows_the_presentation() {
    let pixel_art = Presentation::PixelArt {
      width: 320,
      height: 180,
    };
    let independent = Presentation::ResolutionIndependent;
    assert_eq!(Filter::Default.resolve(pixel_art), Sampling::Nearest);
    assert_eq!(Filter::Default.resolve(independent), Sampling::Linear);
    assert_eq!(Filter::Linear.resolve(pixel_art), Sampling::Linear);
    assert_eq!(Filter::Nearest.resolve(independent), Sampling::Nearest);
  }
}
