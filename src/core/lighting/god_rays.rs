//! Where god rays come from: the god-ray lights in frame coordinates

use glam::Vec2;

use super::{Atmosphere, Light2D};
use crate::core::graphics::CameraView;
use crate::core::shaders::EffectsManager;

/// Frame coordinates (0 to 1 across the whole frame) of up to four god-ray
/// lights, where `view` shows them on a screen of `screen` units.
pub(crate) fn god_ray_positions<'a>(
  lights: impl Iterator<Item = (Vec2, &'a Light2D)>,
  view: &CameraView,
  screen: Vec2,
) -> Vec<(f32, f32)> {
  lights
    .filter(|(_, light)| light.contributes_to_god_rays())
    .take(4)
    .map(|(position, _)| {
      let at = view.world_to_screen(position) / screen.max(Vec2::ONE);
      (at.x, at.y)
    })
    .collect()
}

/// Keep the god-ray effect on the lights that cast god rays: added with the
/// atmosphere's settings when the first one appears, its positions updated
/// every frame, removed when none is left. A game that set god rays itself
/// keeps its settings; only the positions follow the lights.
pub(crate) fn update_god_rays<'a>(
  effects: &mut EffectsManager,
  lights: impl Iterator<Item = (Vec2, &'a Light2D)>,
  view: &CameraView,
  screen: Vec2,
  atmosphere: &Atmosphere,
) {
  let positions = god_ray_positions(lights, view, screen);
  if positions.is_empty() {
    if effects.has_god_rays() {
      effects.disable_god_rays();
    }
  } else if effects.has_god_rays() {
    effects.update_god_rays_lights(positions);
  } else {
    effects.set_god_rays_custom(
      atmosphere.god_rays_intensity,
      atmosphere.god_rays_samples,
      atmosphere.god_rays_decay,
      1.0,
      atmosphere.god_rays_threshold,
      positions,
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{Camera2d, Viewport};
  use crate::core::ui::Color;

  #[test]
  fn a_light_lands_where_a_half_window_camera_shows_it() {
    // The main camera fills the left half of a 200x100 screen, centred on (0, 0)
    let camera = Camera2d {
      viewport: Viewport::new(0.0, 0.0, 0.5, 1.0),
      ..Default::default()
    };
    let screen = Vec2::new(200.0, 100.0);
    let view = CameraView::compute(Vec2::ZERO, 0.0, &camera, Vec2::ZERO, screen);
    let light = Light2D::point(Color::WHITE, 50.0).with_god_rays();
    let at = god_ray_positions([(Vec2::ZERO, &light)].into_iter(), &view, screen);
    assert_eq!(at.len(), 1);
    assert!(
      (at[0].0 - 0.25).abs() < 1e-5 && (at[0].1 - 0.5).abs() < 1e-5,
      "{at:?}"
    );
  }

  #[test]
  fn only_god_ray_lights_count_and_at_most_four() {
    let screen = Vec2::new(100.0, 100.0);
    let view = CameraView::compute(Vec2::ZERO, 0.0, &Camera2d::default(), Vec2::ZERO, screen);
    let rays = Light2D::point(Color::WHITE, 50.0).with_god_rays();
    let plain = Light2D::point(Color::WHITE, 50.0);
    let lights = [(Vec2::ZERO, &plain)]
      .into_iter()
      .chain(std::iter::repeat_n((Vec2::ZERO, &rays), 6));
    assert_eq!(god_ray_positions(lights, &view, screen).len(), 4);
  }
}
