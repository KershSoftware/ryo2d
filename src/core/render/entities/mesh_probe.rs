//! A mesh of a test's own making, drawn where its entity's sprite would
//! be. Nothing public draws a bare mesh (a skeleton's mesh attachments
//! are the engine's own), so the tests that compare a mesh with a sprite
//! of its image put one on an entity through this.

use super::{Drawable, Emitter, EntityFrame, MeshLook, RunLook};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{Component, SpriteShader, Vec2, World};
use crate::core::graphics::SpriteInstance;
use crate::core::graphics::sprite_gpu::effective_shader;

/// A mesh a test draws where its entity's sprite would be: its entity's
/// `Renderable` gives the image, tint, alpha, blend, emission, z, filter
/// and lit flag, and its sprite shader (its `Material`'s first), as a
/// sprite's does
#[derive(Component, Clone, Debug)]
pub(crate) struct MeshProbe {
  /// The rectangle of the image the mesh's UVs run over, in texels: x, y,
  /// width and height
  pub(crate) rect: [u32; 4],
  /// Each vertex: where it is from the entity's position, in world units,
  /// and its UV as a fraction of `rect`
  pub(crate) vertices: Vec<(Vec2, Vec2)>,
  /// Three a triangle, into `vertices`
  pub(crate) indices: Vec<u32>,
}

/// Draws the probe of `drawable`'s entity, if it has one, at the entity's
/// place among the entities' draws. Whether it had one
pub(super) fn draw(
  world: &World,
  frame: &EntityFrame,
  drawable: &Drawable<'_>,
  out: &mut Emitter<'_>,
) -> bool {
  let Some(probe) = world.get::<MeshProbe>(drawable.entity) else {
    return false;
  };
  let Drawable {
    renderable,
    global,
    previous,
    material,
    lift,
    ..
  } = *drawable;
  let at = render_global(global, previous, frame.alpha).translation() + lift;
  let (effect, custom) = match effective_shader(material, renderable.shader.as_ref()) {
    Some(SpriteShader::BuiltIn(effect)) => (Some(effect), None),
    Some(SpriteShader::Custom { shader, params }) => (None, Some((shader.id(), params))),
    None => (None, None),
  };
  let look = MeshLook {
    image: renderable.image.id(),
    rect: probe.rect,
    blend: renderable.blend,
    // The lane a sprite's instance carries its tint in
    tint: SpriteInstance::new()
      .with_tint(renderable.tint, renderable.alpha)
      .tint,
    shading: RunLook {
      effect,
      custom,
      emission: renderable.emission,
    },
  };
  let vertices = probe.vertices.iter().map(|&(offset, uv)| (at + offset, uv));
  out.draw_mesh(
    renderable.z_order,
    &look,
    vertices,
    probe.indices.iter().copied(),
  );
  true
}
