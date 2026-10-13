//! Lights as batch items: one quad per light, drawn into the light buffer
//! of each lit camera that sees it

use glam::Vec2;

use super::Light2D;
use crate::core::graphics::{RenderLayers, SpriteInstance};
use crate::core::render::DrawLight;

/// A light where it is drawn this frame, and the layers that see it.
#[derive(Debug, Clone)]
pub(crate) struct FrameLight {
  pub(crate) position: Vec2,
  pub(crate) light: Light2D,
  pub(crate) layers: RenderLayers,
}

/// `v` if finite, else `default`.
fn or(v: f32, default: f32) -> f32 {
  if v.is_finite() { v } else { default }
}

/// `light` at `position` as a quad instance for `light.wgsl`, or `None`
/// when it cannot light anything: a radius or intensity that is not
/// positive and finite, or a position that is not finite. Shape parameters
/// that are not finite take usable defaults.
pub(crate) fn light_instance(position: Vec2, light: &Light2D) -> Option<SpriteInstance> {
  let usable = |v: f32| v.is_finite() && v > 0.0;
  let (radius, intensity) = (light.radius(), light.intensity());
  if !(usable(radius) && usable(intensity) && position.is_finite()) {
    return None;
  }
  let [r, g, b, _] = light.color().to_linear();
  let (kind, params) = match light {
    Light2D::Point { falloff, .. } => (0.0, [or(*falloff, 2.0).max(0.0), 0.0, 0.0, 0.0]),
    Light2D::Spot {
      falloff,
      angle,
      direction,
      softness,
      ..
    } => (
      1.0,
      [
        or(*falloff, 2.0).max(0.0),
        or(*angle, 45.0),
        or(*direction, 0.0),
        or(*softness, 0.2).clamp(0.0, 1.0),
      ],
    ),
  };
  let volumetric = light
    .volumetric()
    .map(|v| v.to_gpu_vec4().map(|x| or(x, 0.0)))
    .unwrap_or([0.0; 4]);
  let mut instance =
    SpriteInstance::new().with_transform(position.x, position.y, 0.0, radius * 2.0, radius * 2.0);
  instance.uv_rect = [kind, 0.0, 0.0, 0.0];
  instance.tint = [r, g, b, intensity];
  instance.effect_params = params;
  instance.effect_extra = volumetric;
  instance.uv_bounds = light.shadow_color().to_linear();
  Some(instance)
}

/// `light`'s draw-list item, if it can light anything.
pub(crate) fn light_item(light: &FrameLight) -> Option<DrawLight> {
  let instance = light_instance(light.position, &light.light)?;
  let reach = Vec2::splat(light.light.radius());
  Some(DrawLight {
    instance,
    layers: light.layers,
    min: light.position - reach,
    max: light.position + reach,
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
    assert_eq!(instance.tint[3], 3.0, "intensity, unbounded");
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
