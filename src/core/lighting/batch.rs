//! Lights as batch items: one quad per light, drawn into the light buffer
//! of each lit camera that sees it

use glam::Vec2;

use super::{Light2D, LightOccluder2D};
use crate::core::ecs::{Floor, GlobalTransform};
use crate::core::graphics::{RenderLayers, SpriteInstance};
use crate::core::render::DrawLight;
use crate::core::render::batch::Triangles;

/// A light where it stands this frame, the layers that see it, and its
/// floor and lift: it casts shadows where it stands, from the occluders on
/// floors that meet its own, and it and its shadows draw `lift` up.
#[derive(Debug, Clone)]
pub(crate) struct FrameLight {
  pub(crate) position: Vec2,
  pub(crate) light: Light2D,
  pub(crate) layers: RenderLayers,
  pub(crate) floor: Floor,
  pub(crate) lift: Vec2,
}

/// `v` if finite, else `default`.
fn or(v: f32, default: f32) -> f32 {
  if v.is_finite() { v } else { default }
}

/// A spot's cone as it draws, (full angle, direction) in degrees, a shape
/// that is not finite taking the defaults (45° and 0°); `None` for a point
/// light. Shadow planning reads the same cone.
pub(crate) fn drawn_cone(light: &Light2D) -> Option<(f32, f32)> {
  light
    .cone()
    .map(|(angle, direction)| (or(angle, 45.0), or(direction, 0.0)))
}

/// `light` at `position` as a quad instance for `light.wgsl`, or `None`
/// when it cannot light anything: a radius or intensity that is not
/// positive and finite, or a position that is not finite. Shape parameters
/// that are not finite take usable defaults, and the intensity is at most
/// 65504 (the largest half float).
pub(crate) fn light_instance(position: Vec2, light: &Light2D) -> Option<SpriteInstance> {
  let usable = |v: f32| v.is_finite() && v > 0.0;
  let (radius, intensity) = (light.radius(), light.intensity());
  if !(usable(radius) && usable(intensity) && position.is_finite()) {
    return None;
  }
  let [r, g, b, _] = light.color().to_linear();
  let falloff = match light {
    Light2D::Point { falloff, .. } | Light2D::Spot { falloff, .. } => or(*falloff, 2.0).max(0.0),
  };
  let (kind, params) = match (drawn_cone(light), light) {
    (Some((angle, direction)), Light2D::Spot { softness, .. }) => (
      1.0,
      [
        falloff,
        angle,
        direction,
        or(*softness, 0.2).clamp(0.0, 1.0),
      ],
    ),
    _ => (0.0, [falloff, 0.0, 0.0, 0.0]),
  };
  let volumetric = light
    .volumetric()
    .map(|v| v.to_gpu_vec4().map(|x| or(x, 0.0)))
    .unwrap_or([0.0; 4]);
  let mut instance =
    SpriteInstance::new().with_transform(position.x, position.y, 0.0, radius * 2.0, radius * 2.0);
  instance.uv_rect = [kind, 0.0, 0.0, 0.0];
  // Within what the light buffer's half floats hold (more is infinite)
  instance.tint = [r, g, b, intensity.min(65504.0)];
  instance.effect_params = params;
  instance.effect_extra = volumetric;
  instance.uv_bounds = light.shadow_color().to_linear();
  Some(instance)
}

/// `light`'s draw-list item with its shadow volumes from `occluders` (each
/// on its floor) in `triangles`, all drawn lifted, if it can light
/// anything.
pub(crate) fn light_item(
  light: &FrameLight,
  occluders: &[(GlobalTransform, &LightOccluder2D, Floor)],
  triangles: &mut Triangles,
) -> Option<DrawLight> {
  let drawn = light.position + light.lift;
  let instance = light_instance(drawn, &light.light)?;
  let reach = Vec2::splat(light.light.radius());
  let shadows = super::push_shadow_volumes(
    light.position,
    light.lift,
    &light.light,
    light.floor,
    occluders,
    triangles,
  );
  let [r, g, b, _] = light.light.shadow_color().to_linear();
  Some(DrawLight {
    instance,
    layers: light.layers,
    min: drawn - reach,
    max: drawn + reach,
    shadows,
    coloured: r + g + b > 0.0,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  #[test]
  fn a_light_is_a_quad_of_its_reach_with_its_decoded_colour() {
    let light = Light2D::point(Color::rgb(128, 255, 0), 40.0).with_intensity(3.0);
    let Some(instance) = light_instance(Vec2::new(10.0, 20.0), &light) else {
      panic!("a usable light");
    };
    assert_eq!(instance.transform[2][..2], [10.0, 20.0]);
    assert_eq!(instance.transform[0][0], 80.0, "two radii wide");
    assert!(
      (instance.tint[0] - 0.2159).abs() < 1e-3,
      "{:?}",
      instance.tint
    );
    assert_eq!(instance.tint[1], 1.0);
    assert_eq!(instance.tint[3], 3.0, "intensity, past 1");
  }

  #[test]
  fn intensity_stays_within_half_float_range() {
    // The light buffer holds half floats: more would overflow to infinity
    let light = Light2D::point(Color::WHITE, 40.0).with_intensity(1e9);
    let Some(instance) = light_instance(Vec2::ZERO, &light) else {
      panic!("a usable light");
    };
    assert_eq!(instance.tint[3], 65504.0);
  }

  /// A shadow-casting light at the origin as a frame light, with an
  /// occluder 40 to its right.
  fn shadowed(light: Light2D) -> (Option<DrawLight>, Triangles) {
    use crate::core::ecs::Transform;
    let frame = FrameLight {
      position: Vec2::ZERO,
      light: light.with_shadows(),
      layers: RenderLayers::default(),
      floor: Floor::GROUND,
      lift: Vec2::ZERO,
    };
    let occluder = LightOccluder2D::rect(4.0, 16.0);
    let at = GlobalTransform::from_transform(&Transform::from_xy(40.0, 0.0));
    let mut triangles = Triangles::default();
    let item = light_item(&frame, &[(at, &occluder, Floor::GROUND)], &mut triangles);
    (item, triangles)
  }

  #[test]
  fn a_shadowed_light_carries_its_shadow_volumes() {
    let (Some(black), _) = shadowed(Light2D::point(Color::WHITE, 100.0)) else {
      panic!("a usable light");
    };
    assert!(!black.shadows.is_empty());
    assert!(!black.coloured, "a black shadow lets nothing through");
    let red = Light2D::point(Color::WHITE, 100.0).with_shadow_color(Color::rgb(255, 0, 0));
    let (Some(red), _) = shadowed(red) else {
      panic!("a usable light");
    };
    assert!(red.coloured);
  }

  #[test]
  fn a_light_that_draws_nothing_casts_no_shadows() {
    let mut light = Light2D::point(Color::WHITE, 100.0);
    if let Light2D::Point { intensity, .. } = &mut light {
      *intensity = f32::NAN;
    }
    let (item, triangles) = shadowed(light);
    assert!(item.is_none());
    assert!(triangles.indices.is_empty() && triangles.vertices.is_empty());
  }

  #[test]
  fn lights_that_cannot_light_anything_are_dropped() {
    let light = Light2D::point(Color::WHITE, 40.0);
    assert!(light_instance(Vec2::new(f32::NAN, 0.0), &light).is_none());
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
      let mut light = light.clone();
      if let Light2D::Point { radius, .. } = &mut light {
        *radius = bad;
      }
      assert!(light_instance(Vec2::ZERO, &light).is_none(), "radius {bad}");
      let mut light = Light2D::point(Color::WHITE, 40.0);
      if let Light2D::Point { intensity, .. } = &mut light {
        *intensity = bad;
      }
      assert!(
        light_instance(Vec2::ZERO, &light).is_none(),
        "intensity {bad}"
      );
    }
  }

  #[test]
  fn nan_shape_parameters_fall_back_to_usable_ones() {
    let mut light = Light2D::spot(Color::WHITE, 40.0, 45.0, 0.0).with_volumetric();
    if let Light2D::Spot {
      falloff,
      angle,
      direction,
      softness,
      ..
    } = &mut light
    {
      *falloff = f32::NAN;
      *angle = f32::NAN;
      *direction = f32::INFINITY;
      *softness = f32::NAN;
    }
    let Some(instance) = light_instance(Vec2::ZERO, &light) else {
      panic!("still a light");
    };
    let shape = instance.effect_params.iter().chain(&instance.effect_extra);
    assert!(shape.copied().all(f32::is_finite), "{instance:?}");
  }
}
