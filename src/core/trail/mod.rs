#![allow(clippy::module_inception)]
//! Trail and Line Rendering System
//!
//! This module provides components for rendering trails (motion paths) and
//! arbitrary lines with GPU acceleration.
//!
//! # Overview
//!
//! - **Trail2D**: Automatically records position history and renders a fading path
//! - **Line2D**: Renders explicitly defined lines, curves, and shapes
//! - **TrailRenderer**: GPU-accelerated batch renderer for trails and lines
//!
//! # Quick Start
//!
//! ## Adding Trails to Moving Objects
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Spawn a bullet with a trail effect
//! let bullet = ctx.world.spawn((
//!     Transform::from_xy(100.0, 100.0),
//!     Renderable::sprite(bullet_sprite),
//!     Trail2D::bullet(Color::YELLOW),
//! )).id();
//!
//! // The engine records the entity's world position into its trail every
//! // fixed step (`FixedSet::Trails`); call `trail.update(x, y, dt)` yourself
//! // only for a trail that follows something other than its entity.
//! ```
//!
//! ## Trail Presets
//!
//! The `Trail2D` type includes several presets for common use cases:
//!
//! | Preset | Description | Best For |
//! |--------|-------------|----------|
//! | `Trail2D::bullet(color)` | Short, fast-fading, tapered | Projectiles |
//! | `Trail2D::particle(color)` | Thin, very short | Particles, sparks |
//! | `Trail2D::swipe(color, width)` | Wide, tapers to point | Sword slashes, melee |
//! | `Trail2D::laser(color, width)` | Consistent width, no fade | Laser beams |
//!
//! ## Custom Trails
//!
//! ```rust,ignore
//! // Create a custom trail with full control
//! let trail = Trail2D::new(Color::CYAN, 8.0)
//!     .with_length(30)           // 30 points max
//!     .with_taper(0.2)           // Taper to 20% of start width
//!     .with_fade(true)           // Fade alpha along length
//!     .with_end_color(Color::BLUE) // Gradient to blue
//!     .with_min_distance(2.0);   // Min 2px between points
//! ```
//!
//! # Line Rendering
//!
//! ## Basic Lines
//!
//! ```rust,ignore
//! // Simple line segment
//! let line = Line2D::segment(
//!     Vec2::new(0.0, 0.0),
//!     Vec2::new(100.0, 50.0),
//!     Color::RED,
//!     2.0,
//! );
//!
//! // Rectangle outline
//! let rect = Line2D::rect(10.0, 10.0, 100.0, 50.0, Color::GREEN, 2.0);
//!
//! // Circle outline
//! let circle = Line2D::circle(
//!     Vec2::new(200.0, 200.0), // center
//!     50.0,                     // radius
//!     Color::BLUE,
//!     2.0,                      // width
//!     32,                       // segments
//! );
//! ```
//!
//! ## Curves
//!
//! ```rust,ignore
//! // Quadratic bezier curve
//! let curve = Line2D::quadratic_bezier(
//!     Vec2::new(0.0, 0.0),      // start
//!     Vec2::new(50.0, -50.0),   // control point
//!     Vec2::new(100.0, 0.0),    // end
//!     Color::PURPLE,
//!     2.0,
//!     20,                        // segments
//! );
//!
//! // Cubic bezier curve
//! let curve = Line2D::bezier(
//!     Vec2::new(0.0, 0.0),      // start
//!     Vec2::new(30.0, -50.0),   // control 1
//!     Vec2::new(70.0, -50.0),   // control 2
//!     Vec2::new(100.0, 0.0),    // end
//!     Color::ORANGE,
//!     2.0,
//!     20,
//! );
//!
//! // Arc
//! let arc = Line2D::arc(
//!     Vec2::new(100.0, 100.0),  // center
//!     50.0,                      // radius
//!     0.0,                       // start angle (radians)
//!     std::f32::consts::PI,      // end angle (half circle)
//!     Color::CYAN,
//!     2.0,
//!     16,
//! );
//! ```
//!
//! ## Arrows
//!
//! ```rust,ignore
//! let arrow = Line2D::arrow(
//!     Vec2::new(0.0, 0.0),      // start
//!     Vec2::new(100.0, 50.0),   // end (tip)
//!     Color::WHITE,
//!     2.0,
//!     15.0,                      // head size
//! );
//! ```
//!
//! # Rendering Styles
//!
//! The `TrailRenderer` supports three rendering styles:
//!
//! | Style | Description |
//! |-------|-------------|
//! | `TrailStyle::Solid` | Sharp edges, uniform color |
//! | `TrailStyle::Soft` | Soft, fading edges |
//! | `TrailStyle::Glow` | Glowing effect with bright core |
//!
//! # Dynamic Lines
//!
//! Lines can be modified at runtime:
//!
//! ```rust,ignore
//! // Start with empty line
//! let mut drawing = Line2D::default();
//! drawing.set_color(Color::WHITE);
//! drawing.set_width(3.0);
//!
//! // Add points as user draws
//! fn on_mouse_move(&mut self, x: f32, y: f32) {
//!     self.drawing.push_point(Vec2::new(x, y));
//! }
//!
//! // Clear when done
//! fn on_mouse_release(&mut self) {
//!     self.drawing.clear();
//! }
//! ```
//!
//! # Performance Notes
//!
//! - Trails and lines are batched and rendered in a single draw call
//! - Maximum of 65,536 vertices per frame across all trails/lines
//! - Trail point count affects GPU cost (more points = more vertices)
//! - Use `with_min_distance()` to prevent point clustering on fast objects
//! - Disable trails when not visible to save processing

mod line;
mod renderer;
#[allow(clippy::module_inception)]
mod trail;

pub use line::Line2D;
pub use renderer::{MAX_TRAIL_VERTICES, TrailRenderer, TrailStyle};
pub use trail::{
  LineCap, LineJoin, MAX_TRAIL_POINTS, TextureMode, Trail2D, TrailEffect, TrailPoint,
};
