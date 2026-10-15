//! What is inside each trigger, kept on the trigger as a component.

use super::collider::Collider;
use super::world::Physics;
use crate::core::ecs::{Allow, Changed, Commands, Component, Disabled, Entity, Has, Query, ResMut};

/// The bodies (or bare colliders) inside a trigger now, in the order they
/// entered. The engine adds it to every trigger collider and updates it
/// after each step; read it for pressure plates, zones and "is anyone in
/// here" checks.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerContents {
  entities: Vec<Entity>,
}

impl TriggerContents {
  pub fn entities(&self) -> &[Entity] {
    &self.entities
  }

  pub fn contains(&self, entity: Entity) -> bool {
    self.entities.contains(&entity)
  }

  pub fn len(&self) -> usize {
    self.entities.len()
  }

  pub fn is_empty(&self) -> bool {
    self.entities.is_empty()
  }
}

/// After the step's messages: add `TriggerContents` to new triggers (and
/// take it from colliders that stopped being one), and bring the changed
/// ones up to date. Disabled triggers included: one spawned disabled has its
/// contents when enabled, and one disabled (which empties it) is not left
/// listing what was inside.
type Edited<'w, 's> = Query<
  'w,
  's,
  (Entity, &'static Collider, Has<TriggerContents>),
  (Changed<Collider>, Allow<Disabled>),
>;

pub(crate) fn update_trigger_contents(
  mut physics: ResMut<Physics>,
  edited: Edited,
  mut contents: Query<&mut TriggerContents, Allow<Disabled>>,
  mut commands: Commands,
) {
  for (entity, collider, has) in &edited {
    match (collider.is_trigger, has) {
      (true, false) => {
        let entities = physics
          .trigger_contents
          .get(&entity)
          .cloned()
          .unwrap_or_default();
        commands.entity(entity).insert(TriggerContents { entities });
      }
      (false, true) => {
        commands.entity(entity).remove::<TriggerContents>();
      }
      _ => {}
    }
  }
  let mut changed = std::mem::take(&mut physics.changed_triggers);
  for trigger in changed.drain() {
    if let Ok(mut held) = contents.get_mut(trigger) {
      let now = physics
        .trigger_contents
        .get(&trigger)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
      if held.entities != now {
        held.entities = now.to_vec();
      }
    }
  }
  physics.changed_triggers = changed;
}
