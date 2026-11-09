//! Attractors: point, vortex, turbulence and repel, each with an emitter
//! beside it

use demo_kit::prelude::*;

use crate::shared::{count_label, emit, fading, particle_image, screen_view};

#[derive(Default)]
pub struct AttractorsPage {
  /// The screen's size, for the attractors' labels
  screen: Vec2,
}

impl DemoPage for AttractorsPage {
  fn name(&self) -> &str {
    "Attractors"
  }

  fn description(&self) -> &str {
    "Point, Vortex, Turbulence, and Repel attractors"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let screen = screen_view(ctx);
    self.screen = screen;
    let image = particle_image(ctx);
    // Filled discs of radius 20, each particle drifting out from the centre
    let disc = |rate: f32, from: Color, to: Color| {
      fading(
        ParticleEffect::new(image.clone())
          .with_rate(rate)
          .with_shape(EmitShape::Circle {
            radius: 20.0,
            arc_deg: 360.0,
            thickness: 1.0,
          })
          .outward()
          .with_lifetime((4.0, 6.0))
          .with_size((4.0, 8.0))
          .with_speed((5.0, 15.0)),
        from,
        to,
      )
    };
    let green = ctx
      .assets()
      .add(disc(25.0, Color::GREEN, Color::new(0, 255, 100, 0)));
    let cyan = ctx
      .assets()
      .add(disc(25.0, Color::CYAN, Color::new(0, 200, 255, 0)));
    let magenta = ctx.assets().add(fading(
      ParticleEffect::new(image.clone())
        .with_rate(30.0)
        .with_shape(EmitShape::Line { length: 160.0 })
        .with_lifetime((3.0, 5.0))
        .with_size((4.0, 6.0))
        .with_speed((5.0, 20.0)),
      Color::MAGENTA,
      Color::new(255, 0, 255, 0),
    ));
    let red = ctx.assets().add(fading(
      ParticleEffect::new(image.clone())
        .with_rate(40.0)
        .with_lifetime((3.0, 5.0))
        .with_size((5.0, 10.0))
        .with_speed((10.0, 30.0)),
      Color::RED,
      Color::new(255, 50, 0, 0),
    ));
    let (cx, cy) = (screen.x / 2.0, screen.y / 2.0);

    // An emitter beside each attractor
    emit(ctx, 150.0, 250.0, &green);
    emit(ctx, screen.x - 150.0, 250.0, &cyan);
    emit(ctx, cx, screen.y - 200.0, &magenta);
    emit(ctx, cx, cy, &red);

    let [point, vortex, turbulence, repel] = attractor_sites(screen.x, screen.y);
    for (site, attractor) in [
      // A gravity well
      (point, ParticleAttractor::point(200.0, 180.0)),
      (vortex, ParticleAttractor::vortex(120.0, 180.0)),
      (turbulence, ParticleAttractor::turbulence(80.0, 40.0, 1.5)),
      // A negative strength repels, pushing the red particles outward
      (repel, ParticleAttractor::point(-120.0, 150.0)),
    ] {
      ctx
        .world
        .spawn((Transform::from_xy(site.x, site.y), attractor));
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label(ui, ctx);
    // Mark each attractor, with its label centred below on a backing so it
    // reads over the particles
    for site in attractor_sites(self.screen.x, self.screen.y) {
      ui.draw_rect(Rect::new(site.x - 5.0, site.y - 1.0, 11.0, 3.0), site.color);
      ui.draw_rect(Rect::new(site.x - 1.0, site.y - 5.0, 3.0, 11.0), site.color);
      let (text_w, text_h) = ui.text_size(site.label);
      let (text_w, text_h) = (text_w as f32, text_h as f32);
      let (x, y) = (site.x - text_w / 2.0, site.y + 10.0);
      ui.draw_rect(
        Rect::new(x - 3.0, y - 2.0, text_w + 6.0, text_h + 4.0),
        Color::new(12, 12, 20, 190),
      );
      ui.draw_text(site.label, x, y, site.color);
    }
  }
}

/// One of the page's attractors: where it sits and its label
struct AttractorSite {
  x: f32,
  y: f32,
  label: &'static str,
  color: Color,
}

/// The page's point, vortex, turbulence and repel attractors on a `w` x `h`
/// screen
fn attractor_sites(w: f32, h: f32) -> [AttractorSite; 4] {
  let site = |x, y, label, color| AttractorSite { x, y, label, color };
  [
    site(150.0, 150.0, "Point (attract)", Color::GREEN),
    site(w - 150.0, 150.0, "Vortex (swirl)", Color::CYAN),
    site(w / 2.0, h - 100.0, "Turbulence", Color::MAGENTA),
    site(w / 2.0, h / 2.0 - 50.0, "Repel", Color::RED),
  ]
}
