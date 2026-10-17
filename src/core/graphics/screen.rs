//! Screen space and the window: one mapping between them.

use glam::Vec2;

use crate::core::ecs::Resource;

/// How the world reaches the window.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Presentation {
  /// Render at the window's physical resolution; screen space is logical pixels
  #[default]
  ResolutionIndependent,
  /// Render at the window's resolution, with screen space a fixed
  /// `width`x`height` scaled to fit the window by any factor, centred with
  /// bars where the aspect ratios differ
  Fit { width: u32, height: u32 },
  /// Render into a fixed target scaled by a whole number, centred with bars
  PixelArt { width: u32, height: u32 },
}

/// Converts between window pixels and screen space (UI, mouse, `screen_size`).
///
/// A resource: the engine replaces it on every resize and scale-factor change.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ScreenMapping {
  presentation: Presentation,
  physical: (u32, u32),
  scale_factor: f32,
}

impl ScreenMapping {
  /// The mapping for a window of `physical` pixels at `scale_factor`
  /// (degenerate sizes and factors are clamped to something drawable).
  pub fn new(presentation: Presentation, physical: (u32, u32), scale_factor: f64) -> Self {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
      scale_factor as f32
    } else {
      1.0
    };
    let presentation = match presentation {
      Presentation::Fit { width, height } => Presentation::Fit {
        width: width.max(1),
        height: height.max(1),
      },
      Presentation::PixelArt { width, height } => Presentation::PixelArt {
        width: width.max(1),
        height: height.max(1),
      },
      other => other,
    };
    Self {
      presentation,
      physical: (physical.0.max(1), physical.1.max(1)),
      scale_factor,
    }
  }

  /// How the world reaches the window.
  pub fn presentation(&self) -> Presentation {
    self.presentation
  }

  fn physical(&self) -> Vec2 {
    Vec2::new(self.physical.0 as f32, self.physical.1 as f32)
  }

  /// The size of screen space: logical window pixels, or the fit or
  /// pixel-art resolution.
  pub fn screen_size(&self) -> Vec2 {
    match self.presentation {
      Presentation::ResolutionIndependent => self.physical() / self.scale_factor,
      Presentation::Fit { width, height } | Presentation::PixelArt { width, height } => {
        Vec2::new(width as f32, height as f32)
      }
    }
  }

  /// Physical window pixels per screen unit (a whole number for pixel art).
  pub fn scale(&self) -> f32 {
    let fit = self.physical() / self.screen_size();
    match self.presentation {
      Presentation::ResolutionIndependent => self.scale_factor,
      Presentation::Fit { .. } => fit.x.min(fit.y),
      Presentation::PixelArt { .. } => fit.x.min(fit.y).floor().max(1.0),
    }
  }

  /// Where screen space sits in the window: top-left and size, in physical
  /// pixels (fit and pixel art are centred, with bars around them; a fit
  /// rounds to whole pixels and sits on one).
  pub fn window_rect(&self) -> (Vec2, Vec2) {
    let size = self.screen_size() * self.scale();
    match self.presentation {
      Presentation::Fit { .. } => {
        let size = size.round().max(Vec2::ONE);
        (((self.physical() - size) / 2.0).floor(), size)
      }
      _ => ((self.physical() - size) / 2.0, size),
    }
  }

  /// A physical window position in screen space (positions on the bars fall
  /// outside `0..screen_size`).
  pub fn window_to_screen(&self, physical: Vec2) -> Vec2 {
    (physical - self.window_rect().0) / self.scale()
  }

  /// A screen-space position in physical window pixels.
  pub fn screen_to_window(&self, screen: Vec2) -> Vec2 {
    screen * self.scale() + self.window_rect().0
  }

  /// The size window-target images render at: the window's physical size,
  /// the fit's scaled size in physical pixels, or the pixel-art target.
  pub fn render_size(&self) -> (u32, u32) {
    match self.presentation {
      Presentation::ResolutionIndependent => self.physical,
      Presentation::Fit { .. } => {
        let size = self.window_rect().1;
        (size.x as u32, size.y as u32)
      }
      Presentation::PixelArt { width, height } => (width, height),
    }
  }

  /// The window's physical size.
  pub(crate) fn physical_size(&self) -> (u32, u32) {
    self.physical
  }

  /// The same presentation and scale factor for a window of `physical` pixels.
  pub(crate) fn with_physical(&self, physical: (u32, u32)) -> Self {
    Self::new(self.presentation, physical, f64::from(self.scale_factor))
  }

  /// The screen rectangle in the window's normalised device coordinates:
  /// scale x, scale y, offset x, offset y.
  pub fn ndc_rect(&self) -> [f32; 4] {
    let (origin, size) = self.window_rect();
    let physical = self.physical();
    let scale = size / physical;
    // Centre of the rectangle relative to the window centre, in NDC (y up)
    let centre = (origin + size / 2.0) / physical * 2.0 - Vec2::ONE;
    [scale.x, scale.y, centre.x, -centre.y]
  }
}

/// The window's initial logical size: `window_size` if set, else the
/// resolution (a fit shrunk to fit 1280x720 when larger, keeping its aspect),
/// or for pixel art the largest whole multiple of the art that fits 1280x720
/// (at least 1x).
pub(crate) fn initial_window_size(
  presentation: Presentation,
  resolution: (u32, u32),
  window_size: Option<(u32, u32)>,
) -> (u32, u32) {
  if let Some(size) = window_size {
    return size;
  }
  match presentation {
    Presentation::ResolutionIndependent => resolution,
    Presentation::Fit { width, height } => {
      let (width, height) = (width.max(1) as f32, height.max(1) as f32);
      let shrink = (1280.0 / width).min(720.0 / height).min(1.0);
      (
        ((width * shrink).round() as u32).max(1),
        ((height * shrink).round() as u32).max(1),
      )
    }
    Presentation::PixelArt { width, height } => {
      let (width, height) = (width.max(1), height.max(1));
      let scale = (1280 / width).min(720 / height).max(1);
      (width * scale, height * scale)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-4
  }

  #[test]
  fn resolution_independent_screen_space_is_logical_pixels() {
    let m = ScreenMapping::new(Presentation::ResolutionIndependent, (2560, 1440), 2.0);
    assert_eq!(m.screen_size(), Vec2::new(1280.0, 720.0));
    assert_eq!(m.scale(), 2.0);
    assert_eq!(m.render_size(), (2560, 1440));
    assert_eq!(m.window_rect(), (Vec2::ZERO, Vec2::new(2560.0, 1440.0)));
    assert!(close(
      m.window_to_screen(Vec2::new(100.0, 50.0)),
      Vec2::new(50.0, 25.0)
    ));
    assert_eq!(m.ndc_rect(), [1.0, 1.0, 0.0, 0.0]);
  }

  #[test]
  fn fit_scales_the_design_to_fill_with_bars_only_for_the_aspect() {
    let fit = Presentation::Fit {
      width: 1920,
      height: 1080,
    };
    // A full-screen MacBook window: 3456x2168 physical pixels at scale 2
    let m = ScreenMapping::new(fit, (3456, 2168), 2.0);
    assert_eq!(m.screen_size(), Vec2::new(1920.0, 1080.0));
    assert!((m.scale() - 1.8).abs() < 1e-6, "{}", m.scale());
    assert_eq!(m.render_size(), (3456, 1944));
    assert_eq!(
      m.window_rect(),
      (Vec2::new(0.0, 112.0), Vec2::new(3456.0, 1944.0))
    );
    assert!(close(m.window_to_screen(Vec2::new(0.0, 112.0)), Vec2::ZERO));
    assert!(close(
      m.window_to_screen(Vec2::new(3456.0, 2056.0)),
      Vec2::new(1920.0, 1080.0)
    ));
    let [sx, sy, ox, oy] = m.ndc_rect();
    assert_eq!((sx, ox, oy), (1.0, 0.0, 0.0));
    assert!((sy - 1944.0 / 2168.0).abs() < 1e-6);
  }

  #[test]
  fn fit_draws_on_whole_pixels_at_any_scale() {
    let fit = Presentation::Fit {
      width: 1280,
      height: 720,
    };
    let m = ScreenMapping::new(fit, (1300, 740), 1.0);
    // Width fits tighter: 731.25 rows round to 731, and the 9 left over
    // split 4 above and 5 below
    assert_eq!(m.render_size(), (1300, 731));
    assert_eq!(
      m.window_rect(),
      (Vec2::new(0.0, 4.0), Vec2::new(1300.0, 731.0))
    );
  }

  #[test]
  fn fit_opens_at_its_resolution_or_smaller_to_fit_1280x720() {
    let fit = |width, height| Presentation::Fit { width, height };
    assert_eq!(
      initial_window_size(fit(1920, 1080), (1920, 1080), None),
      (1280, 720)
    );
    assert_eq!(
      initial_window_size(fit(400, 900), (400, 900), None),
      (320, 720)
    );
    assert_eq!(
      initial_window_size(fit(400, 600), (400, 600), None),
      (400, 600)
    );
    assert_eq!(
      initial_window_size(fit(400, 600), (400, 600), Some((800, 1200))),
      (800, 1200)
    );
  }

  #[test]
  fn pixel_art_scales_by_a_whole_number_with_bars() {
    let art = Presentation::PixelArt {
      width: 320,
      height: 180,
    };
    let m = ScreenMapping::new(art, (1300, 740), 1.0);
    assert_eq!(m.screen_size(), Vec2::new(320.0, 180.0));
    assert_eq!(m.scale(), 4.0);
    assert_eq!(m.render_size(), (320, 180));
    assert_eq!(
      m.window_rect(),
      (Vec2::new(10.0, 10.0), Vec2::new(1280.0, 720.0))
    );
    assert!(close(m.window_to_screen(Vec2::new(10.0, 10.0)), Vec2::ZERO));
    assert!(close(
      m.window_to_screen(Vec2::new(1290.0, 730.0)),
      Vec2::new(320.0, 180.0)
    ));
    assert!(
      close(
        m.window_to_screen(Vec2::new(6.0, 6.0)),
        Vec2::new(-1.0, -1.0)
      ),
      "the bars still map, outside 0..size"
    );
    let [sx, sy, ox, oy] = m.ndc_rect();
    assert!((sx - 1280.0 / 1300.0).abs() < 1e-6 && (sy - 720.0 / 740.0).abs() < 1e-6);
    assert_eq!((ox, oy), (0.0, 0.0));
  }

  #[test]
  fn a_window_smaller_than_the_art_scales_at_one_and_crops() {
    let art = Presentation::PixelArt {
      width: 320,
      height: 180,
    };
    let m = ScreenMapping::new(art, (300, 170), 1.0);
    assert_eq!(m.scale(), 1.0);
    assert_eq!(
      m.window_rect(),
      (Vec2::new(-10.0, -5.0), Vec2::new(320.0, 180.0))
    );
    assert!(close(m.window_to_screen(Vec2::ZERO), Vec2::new(10.0, 5.0)));
  }

  #[test]
  fn mappings_round_trip() {
    for m in [
      ScreenMapping::new(Presentation::ResolutionIndependent, (1920, 1080), 1.5),
      ScreenMapping::new(
        Presentation::PixelArt {
          width: 256,
          height: 224,
        },
        (1366, 768),
        1.0,
      ),
      ScreenMapping::new(
        Presentation::PixelArt {
          width: 320,
          height: 180,
        },
        (2560, 1440),
        2.0,
      ),
    ] {
      let p = Vec2::new(123.0, 77.0);
      assert!(close(m.screen_to_window(m.window_to_screen(p)), p), "{m:?}");
    }
  }

  #[test]
  fn degenerate_inputs_stay_finite() {
    let m = ScreenMapping::new(Presentation::ResolutionIndependent, (0, 0), f64::NAN);
    assert_eq!(m.render_size(), (1, 1));
    assert!(m.screen_size().is_finite() && m.scale() > 0.0);
    let art = ScreenMapping::new(
      Presentation::PixelArt {
        width: 0,
        height: 0,
      },
      (100, 100),
      1.0,
    );
    assert_eq!(art.render_size(), (1, 1));
  }

  #[test]
  fn the_initial_window_fits_the_art_within_720p() {
    let art = |w, h| Presentation::PixelArt {
      width: w,
      height: h,
    };
    assert_eq!(
      initial_window_size(art(320, 180), (320, 180), None),
      (1280, 720)
    );
    assert_eq!(
      initial_window_size(art(400, 300), (400, 300), None),
      (800, 600)
    );
    assert_eq!(
      initial_window_size(art(2000, 1000), (2000, 1000), None),
      (2000, 1000)
    );
    assert_eq!(
      initial_window_size(art(320, 180), (320, 180), Some((640, 360))),
      (640, 360)
    );
    assert_eq!(
      initial_window_size(Presentation::ResolutionIndependent, (1024, 768), None),
      (1024, 768)
    );
  }
}
