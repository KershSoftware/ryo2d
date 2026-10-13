//! The main camera: its spawn, the systems that compute every camera's
//! view, and `CameraMut`, what `ctx.camera()` hands a scene.

use glam::Vec2;

use super::camera::CameraShake;
use super::camera2d::{
  Camera2d, CameraBounds, CameraTarget, CameraView, MainCamera, Projection, Viewport,
};
use super::screen::ScreenMapping;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::transform::{GlobalTransform, Interpolated, PreviousGlobalTransform};
use crate::core::ecs::world_ext::WorldExt;
use crate::core::ecs::{Ease, Entity, Query, Res, Resource, Transform, With, World};
use crate::core::time::Time;

/// The design area the main camera fits (the configured resolution), kept
/// so a camera cleanup can restore it.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct MainCameraDesign(pub(crate) Vec2);

/// Spawn the main camera: centred on the `design` area, which it fits.
pub(crate) fn spawn_main_camera(world: &mut World, design: Vec2) -> Entity {
  world.insert_resource(MainCameraDesign(design));
  world
    .spawn((
      Transform::from_xy(design.x / 2.0, design.y / 2.0),
      main_camera2d(design),
      MainCamera,
      CameraShake::default(),
      Interpolated,
    ))
    .id()
}

fn main_camera2d(design: Vec2) -> Camera2d {
  Camera2d {
    projection: Projection::Fit {
      width: design.x,
      height: design.y,
    },
    ..Camera2d::default()
  }
}

/// Put the main camera back as it was spawned: centred on the design area,
/// zoom 1, no rotation, the design projection and viewport, no shake.
pub(crate) fn reset_main_camera(world: &mut World) {
  let design = world
    .get_resource::<MainCameraDesign>()
    .map_or(Vec2::ONE, |design| design.0);
  let Some(entity) = main_camera_entity(world) else {
    return;
  };
  if let Some(mut transform) = world.get_mut::<Transform>(entity) {
    *transform = Transform::from_xy(design.x / 2.0, design.y / 2.0);
  }
  if let Some(mut camera) = world.get_mut::<Camera2d>(entity) {
    let target = camera.target;
    *camera = Camera2d {
      target,
      ..main_camera2d(design)
    };
  }
  if let Some(mut shake) = world.get_mut::<CameraShake>(entity) {
    *shake = CameraShake::default();
  }
  world.skip_interpolation(entity);
}

/// The main camera's entity, if there is one.
pub(crate) fn main_camera_entity(world: &mut World) -> Option<Entity> {
  world
    .query_filtered::<Entity, With<MainCamera>>()
    .iter(world)
    .next()
}

/// The main camera's view this frame, or the default view when there is none.
pub(crate) fn main_view(world: &mut World) -> CameraView {
  world
    .query_filtered::<&CameraView, With<MainCamera>>()
    .iter(world)
    .next()
    .copied()
    .unwrap_or_default()
}

/// `PostUpdateSet::Camera`: shake runs on real time.
pub(crate) fn camera_shake_system(time: Res<Time>, mut shakes: Query<&mut CameraShake>) {
  let dt = time.real.delta();
  for mut shake in &mut shakes {
    shake.update(dt);
  }
}

/// What the view system reads and writes for each camera.
type ViewInputs = (
  &'static Camera2d,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static CameraShake>,
  &'static mut CameraView,
);

/// `PostUpdateSet::Camera`, after the shake: every camera's view from its
/// interpolated transform.
pub(crate) fn camera_view_system(
  time: Res<Time>,
  mapping: Option<Res<ScreenMapping>>,
  mut cameras: Query<ViewInputs>,
) {
  let alpha = time.fixed.overstep_fraction();
  let screen = mapping.map_or(Vec2::ONE, |mapping| mapping.screen_size());
  for (camera, global, previous, shake, mut view) in &mut cameras {
    let at = render_global(global, previous, alpha);
    let target = match camera.target {
      CameraTarget::Window => screen,
      // Texture targets get their size with render targets
      CameraTarget::Texture(_) => screen,
    };
    let shake = shake.map_or(Vec2::ZERO, CameraShake::offset);
    *view = CameraView::compute(at.translation(), at.rotation(), camera, shake, target);
  }
}

/// The main camera, as a scene changes it (`ctx.camera()`).
///
/// Changes apply to its entity at once; [`view`](Self::view) and the
/// conversions use this step's (uninterpolated) camera.
pub struct CameraMut<'w> {
  world: &'w mut World,
  entity: Entity,
}

impl<'w> CameraMut<'w> {
  /// The main camera of `world`, if there is one.
  pub fn main(world: &'w mut World) -> Option<CameraMut<'w>> {
    let entity = main_camera_entity(world)?;
    Some(CameraMut { world, entity })
  }

  /// The main camera, spawned again over the design area if the game
  /// despawned it.
  pub(crate) fn main_or_spawn(world: &'w mut World) -> CameraMut<'w> {
    let entity = match main_camera_entity(world) {
      Some(entity) => entity,
      None => {
        let design = world
          .get_resource::<MainCameraDesign>()
          .map(|design| design.0)
          .or_else(|| {
            world
              .get_resource::<ScreenMapping>()
              .map(ScreenMapping::screen_size)
          })
          .unwrap_or(Vec2::ONE);
        spawn_main_camera(world, design)
      }
    };
    CameraMut { world, entity }
  }

  /// The camera's entity.
  pub fn entity(&self) -> Entity {
    self.entity
  }

  /// The world point at the centre of the view.
  pub fn position(&self) -> Vec2 {
    self
      .world
      .get::<Transform>(self.entity)
      .map_or(Vec2::ZERO, |transform| transform.translation)
  }

  /// Centre the view on `(x, y)`.
  pub fn set_position(&mut self, x: f32, y: f32) {
    if let Some(mut transform) = self.world.get_mut::<Transform>(self.entity) {
      transform.translation = Vec2::new(x, y);
    }
  }

  /// Magnification on top of the projection (2 shows half as much world).
  pub fn zoom(&self) -> f32 {
    self.camera().zoom
  }

  /// Set the zoom, clamped to 0.01..=100 (NaN is ignored).
  pub fn set_zoom(&mut self, zoom: f32) {
    if zoom.is_nan() {
      return;
    }
    if let Some(mut camera) = self.world.get_mut::<Camera2d>(self.entity) {
      camera.zoom = zoom.clamp(0.01, 100.0);
    }
  }

  /// The view's rotation, in radians.
  pub fn rotation(&self) -> f32 {
    self
      .world
      .get::<Transform>(self.entity)
      .map_or(0.0, |transform| transform.rotation)
  }

  /// Turn the view by `radians` (the world turns the other way on screen).
  pub fn set_rotation(&mut self, radians: f32) {
    if let Some(mut transform) = self.world.get_mut::<Transform>(self.entity) {
      transform.rotation = radians;
    }
  }

  /// How world units map to the view.
  pub fn set_projection(&mut self, projection: Projection) {
    if let Some(mut camera) = self.world.get_mut::<Camera2d>(self.entity) {
      camera.projection = projection;
    }
  }

  /// The part of the window the camera draws into.
  pub fn set_viewport(&mut self, viewport: Viewport) {
    if let Some(mut camera) = self.world.get_mut::<Camera2d>(self.entity) {
      camera.viewport = viewport;
    }
  }

  /// Move part of the way to `target`: `lerp` 0 stays, 1 snaps (typically
  /// 0.05 to 0.2 each step).
  pub fn follow(&mut self, target: Vec2, lerp: f32) {
    let position = self.position();
    let position = position + (target - position) * lerp.clamp(0.0, 1.0);
    self.set_position(position.x, position.y);
  }

  /// Jump to `target` without blending from the old position.
  pub fn snap_to(&mut self, target: Vec2) {
    self.set_position(target.x, target.y);
    self.world.skip_interpolation(self.entity);
  }

  /// Keep the view inside the world box `min..max` (centred on it when the
  /// view is larger than the box).
  pub fn clamp_to_bounds(&mut self, min: Vec2, max: Vec2) {
    let bounds = self.view().bounds;
    let half = Vec2::new(bounds.width(), bounds.height()) / 2.0;
    let position = self.position();
    let axis = |position: f32, min: f32, max: f32| {
      if min > max {
        (min + max) / 2.0
      } else {
        position.clamp(min, max)
      }
    };
    let (low, high) = (min + half, max - half);
    let clamped = Vec2::new(
      axis(position.x, low.x, high.x),
      axis(position.y, low.y, high.y),
    );
    self.set_position(clamped.x, clamped.y);
  }

  /// Add trauma to shake the camera (clamped to 0.0-1.0).
  pub fn add_trauma(&mut self, amount: f32) {
    if let Some(mut shake) = self.world.get_mut::<CameraShake>(self.entity) {
      shake.add_trauma(amount);
    }
  }

  /// The offset at full trauma, in screen units, and how fast trauma decays
  /// per second (defaults 10.0 and 1.0).
  pub fn configure_shake(&mut self, max_offset: f32, decay_rate: f32) {
    if let Some(mut shake) = self.world.get_mut::<CameraShake>(self.entity) {
      shake.configure(max_offset, decay_rate);
    }
  }

  /// How trauma maps to shake intensity (default `Ease::InQuad`).
  pub fn set_shake_curve(&mut self, curve: Ease) {
    if let Some(mut shake) = self.world.get_mut::<CameraShake>(self.entity) {
      shake.set_curve(curve);
    }
  }

  /// The current trauma (0.0 to 1.0).
  pub fn trauma(&self) -> f32 {
    self
      .world
      .get::<CameraShake>(self.entity)
      .map_or(0.0, CameraShake::trauma)
  }

  /// The view as the camera stands now (this step's transform, not the
  /// frame's interpolated one).
  pub fn view(&self) -> CameraView {
    let (position, rotation) = self
      .world
      .get::<Transform>(self.entity)
      .map_or((Vec2::ZERO, 0.0), |transform| {
        (transform.translation, transform.rotation)
      });
    let shake = self
      .world
      .get::<CameraShake>(self.entity)
      .map_or(Vec2::ZERO, CameraShake::offset);
    let target = self
      .world
      .get_resource::<ScreenMapping>()
      .map_or(Vec2::ONE, ScreenMapping::screen_size);
    CameraView::compute(position, rotation, &self.camera(), shake, target)
  }

  /// A world point on the screen.
  pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
    self.view().world_to_screen(world)
  }

  /// A screen point in the world (e.g. the pointer).
  pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
    self.view().screen_to_world(screen)
  }

  /// The world box the camera can see.
  pub fn visible_bounds(&self) -> CameraBounds {
    self.view().bounds
  }

  fn camera(&self) -> Camera2d {
    self
      .world
      .get::<Camera2d>(self.entity)
      .copied()
      .unwrap_or_default()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;
  use crate::core::graphics::{Presentation, ScreenMapping};

  fn world_with_main_camera() -> (World, Entity) {
    let mut world = World::new();
    world.insert_resource(ScreenMapping::new(
      Presentation::ResolutionIndependent,
      (800, 600),
      1.0,
    ));
    let camera = spawn_main_camera(&mut world, Vec2::new(800.0, 600.0));
    (world, camera)
  }

  #[test]
  fn the_main_camera_starts_with_world_equal_to_screen() {
    let (mut world, _) = world_with_main_camera();
    let camera = CameraMut::main(&mut world).unwrap();
    assert_eq!(camera.position(), Vec2::new(400.0, 300.0));
    assert!(
      (camera.world_to_screen(Vec2::new(10.0, 20.0)) - Vec2::new(10.0, 20.0)).length() < 1e-3
    );
  }

  #[test]
  fn moving_and_zooming_change_the_view_at_once() {
    let (mut world, entity) = world_with_main_camera();
    let mut camera = CameraMut::main(&mut world).unwrap();
    camera.set_position(100.0, 100.0);
    camera.set_zoom(2.0);
    assert!(
      (camera.world_to_screen(Vec2::new(110.0, 100.0)) - Vec2::new(420.0, 300.0)).length() < 1e-3
    );
    camera.set_zoom(-3.0);
    assert_eq!(camera.zoom(), 0.01);
    assert_eq!(
      world.get::<Transform>(entity).unwrap().translation,
      Vec2::new(100.0, 100.0)
    );
  }

  #[test]
  fn follow_moves_part_of_the_way() {
    let (mut world, _) = world_with_main_camera();
    let mut camera = CameraMut::main(&mut world).unwrap();
    camera.follow(Vec2::new(500.0, 300.0), 0.5);
    assert_eq!(camera.position(), Vec2::new(450.0, 300.0));
  }

  #[test]
  fn clamping_keeps_the_view_inside_the_bounds() {
    let (mut world, _) = world_with_main_camera();
    let mut camera = CameraMut::main(&mut world).unwrap();
    camera.set_zoom(2.0); // 400 x 300 world units visible
    camera.set_position(0.0, 0.0);
    camera.clamp_to_bounds(Vec2::ZERO, Vec2::new(1000.0, 1000.0));
    assert_eq!(camera.position(), Vec2::new(200.0, 150.0));
  }
}
