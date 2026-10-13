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
  /// Lightens what is below without blowing out to white
  Screen,
}

/// How a texture is sampled when drawn at other than its own pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
#[expect(
  dead_code,
  reason = "trails and lines join the batch with their own shaders"
)]
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
  /// Pixels outside are not drawn (none of the world's drawables clip yet)
  pub(crate) scissor: Option<PixelRect>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Presentation;

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
