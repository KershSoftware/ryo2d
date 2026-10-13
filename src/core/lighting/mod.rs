//! 2D Lighting System
//!
//! This module provides a complete 2D lighting solution with:
//!
//! - **Point lights** - Omnidirectional lights that illuminate in all directions
//! - **Spotlights** - Directional cone lights
//! - **Ambient light** - Global base illumination
//! - **Light occluders** - Shapes that block light and cast shadows
//!
//! # Architecture
//!
//! The lighting system uses a deferred lighting approach:
//!
//! 1. **Light accumulation pass** - Each lit camera draws the lights it sees
//!    into its light buffer with additive blending. Each light is a batch quad
//!    covering its radius.
//!
//! 2. **Composition pass** - The scene is multiplied by (ambient + light buffer)
//!    to produce the final lit result.
//!
//! # Quick Start
//!
//! Add lights to your game by spawning entities with `Light2D` components:
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! fn on_enter(&mut self, ctx: &mut SceneContext) {
//!     // Spawn a point light (torch)
//!     ctx.world.spawn((
//!         Transform::from_xy(100.0, 100.0),
//!         Light2D::torch(150.0),
//!     ));
//!
//!     // Spawn a spotlight (flashlight)
//!     ctx.world.spawn((
//!         Transform::from_xy(200.0, 50.0),
//!         Light2D::flashlight(200.0, 90.0), // pointing down
//!     ));
//!
//!     // Spawn a custom colored light
//!     ctx.world.spawn((
//!         Transform::from_xy(300.0, 300.0),
//!         Light2D::point(Color::new(100, 200, 255, 255), 100.0)
//!             .with_intensity(0.8),
//!     ));
//! }
//! ```
//!
//! # Light Types
//!
//! ## Point Lights
//!
//! Point lights emit light equally in all directions from a single point.
//! They're ideal for torches, lamps, campfires, and ambient lighting.
//!
//! ```rust,ignore
//! // Basic point light
//! Light2D::point(Color::WHITE, 100.0)
//!
//! // With custom intensity
//! Light2D::point_with_intensity(Color::YELLOW, 150.0, 0.8)
//!
//! // Full control
//! Light2D::point_full(
//!     Color::ORANGE,  // color
//!     200.0,          // radius
//!     1.2,            // intensity (can exceed 1.0 for bloom-like effect)
//!     2.0,            // falloff (2.0 = quadratic, natural looking)
//!     true,           // cast shadows
//! )
//! ```
//!
//! ## Spotlights
//!
//! Spotlights emit light in a cone shape, perfect for flashlights,
//! car headlights, searchlights, and directional effects.
//!
//! ```rust,ignore
//! // Basic spotlight: 45 degree cone pointing right (0 degrees)
//! Light2D::spot(Color::WHITE, 200.0, 45.0, 0.0)
//!
//! // Spotlight pointing down (90 degrees)
//! Light2D::spot(Color::WHITE, 200.0, 30.0, 90.0)
//!
//! // Full control
//! Light2D::spot_full(
//!     Color::WHITE,   // color
//!     250.0,          // radius
//!     1.0,            // intensity
//!     2.0,            // falloff
//!     45.0,           // cone angle (full width in degrees)
//!     90.0,           // direction (0=right, 90=down, 180=left, 270=up)
//!     0.2,            // edge softness (0.0=hard, 1.0=very soft)
//!     true,           // cast shadows
//! )
//! ```
//!
//! # Light Presets
//!
//! The [`Light2D`] type includes several convenient presets:
//!
//! | Preset | Description |
//! |--------|-------------|
//! | `Light2D::torch(radius)` | Warm orange torch/candle light |
//! | `Light2D::moonlight(radius)` | Cool blue moonlight |
//! | `Light2D::lamp(radius)` | Bright white lamp |
//! | `Light2D::flashlight(radius, dir)` | Focused spotlight |
//! | `Light2D::warning(radius)` | Red warning light |
//! | `Light2D::magic(radius, color)` | Magical glow effect |
//!
//! # Ambient Light
//!
//! Ambient light provides a base level of illumination to prevent
//! areas without lights from being completely black.
//!
//! ```rust,ignore
//! // Set a moonlit ambient
//! ctx.lighting().set_ambient(AmbientLight::moonlit());
//!
//! // Or create custom ambient
//! ctx.lighting().set_ambient(AmbientLight::new(Color::new(30, 30, 50, 255), 0.15));
//! ```
//!
//! ## Ambient Presets
//!
//! | Preset | Description |
//! |--------|-------------|
//! | `AmbientLight::dark()` | Complete darkness (horror games) |
//! | `AmbientLight::night()` | Dim night scene |
//! | `AmbientLight::moonlit()` | Blue-tinted moonlight |
//! | `AmbientLight::indoor()` | Dim indoor lighting |
//! | `AmbientLight::daylight()` | Bright daytime |
//! | `AmbientLight::sunset()` | Warm orange sunset |
//!
//! # Light Occluders (Shadows)
//!
//! Light occluders block light and create shadows. Define them as polygons:
//!
//! ```rust,ignore
//! // Rectangular wall
//! ctx.world.spawn((
//!     Transform::from_xy(150.0, 100.0),
//!     LightOccluder2D::rect(50.0, 100.0),
//! ));
//!
//! // Circular pillar
//! ctx.world.spawn((
//!     Transform::from_xy(200.0, 200.0),
//!     LightOccluder2D::circle(25.0, 8), // 8 segments
//! ));
//!
//! // Custom polygon
//! ctx.world.spawn((
//!     Transform::from_xy(300.0, 150.0),
//!     LightOccluder2D::polygon(vec![
//!         Vec2::new(-20.0, -30.0),
//!         Vec2::new(20.0, -30.0),
//!         Vec2::new(30.0, 30.0),
//!         Vec2::new(-30.0, 30.0),
//!     ]),
//! ));
//! ```
//!
//! # Dynamic Lights
//!
//! Lights follow their entity's transform, so you can create dynamic
//! lighting by moving the entity (or by spawning the light as a child of the
//! entity it should follow, with `ctx.spawn_child`):
//!
//! ```rust,ignore
//! fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!     // Move a light
//!     if let Some(mut transform) = ctx.world.get_mut::<Transform>(self.player_light) {
//!         transform.translation = Vec2::new(self.player_x, self.player_y);
//!     }
//!
//!     // Rotate a spotlight
//!     if let Some(mut light) = ctx.world.get_mut::<Light2D>(self.searchlight) {
//!         light.set_direction(self.time * 45.0); // 45 degrees per second
//!     }
//!
//!     // Flicker a torch
//!     if let Some(mut light) = ctx.world.get_mut::<Light2D>(self.torch) {
//!         let flicker = 0.8 + 0.2 * (self.time * 10.0).sin();
//!         light.set_intensity(flicker);
//!     }
//!     SceneAction::Continue
//! }
//! ```
//!
//! # Performance Notes
//!
//! - Lights are batch quads, culled per camera by its view and `RenderLayers`;
//!   there is no cap on their number
//! - Light radius determines GPU cost (larger = more pixels to shade)
//! - For many small lights, consider using particles with glow effects instead
//! - Shadow casting adds additional cost; disable for lights that don't need it

mod ambient;
mod atmosphere;
mod batch;
mod god_rays;
mod light;
mod occluder;
mod renderer;
mod settings;
mod shadow;
mod volumetric;

pub use ambient::AmbientLight;
pub use atmosphere::{Atmosphere, GodRaysQuality};
pub(crate) use batch::{FrameLight, light_item};
pub(crate) use god_rays::update_god_rays;
pub use light::Light2D;
pub use occluder::LightOccluder2D;
pub(crate) use renderer::LightingRenderer;
pub use settings::LightingSettings;
pub use shadow::{MAX_SHADOW_VERTICES, ShadowGeometry, ShadowVertex, generate_shadow_geometry};
pub use volumetric::VolumetricSettings;
