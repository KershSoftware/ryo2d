use super::item::ItemStack;
use crate::core::ecs::{Entity, Transform, World};
use crate::core::error::InventoryError;

/// ECS component for items dropped in the game world.
///
/// The bridge between inventory value types and world entities.
/// When items are in inventory, they're lightweight `ItemStack` values.
/// When dropped in the world, they become entities with this component.
#[derive(Clone)]
pub struct ItemDrop {
  pub item_stack: ItemStack,
}

/// Spawn an item entity in the world with position.
///
/// Creates an entity with `Transform` and `ItemDrop` components.
/// Game devs can add additional components (sprite, collision) after spawning.
pub fn spawn_item_drop(world: &mut World, stack: ItemStack, position: (f32, f32)) -> Entity {
  world.spawn((
    Transform::new(position.0, position.1),
    ItemDrop { item_stack: stack },
  ))
}

/// Pick up an item entity from the world.
///
/// Despawns the entity and returns its `ItemStack`.
pub fn pickup_item(world: &mut World, entity: Entity) -> Result<ItemStack, InventoryError> {
  // Get the ItemDrop component first
  let stack = {
    let drop_ref = world.get::<ItemDrop>(entity);
    match drop_ref {
      Some(drop_component) => drop_component.item_stack.clone(),
      None => {
        return Err(InventoryError::ItemNotFound(format!(
          "Entity {:?} has no ItemDrop component",
          entity
        )));
      }
    }
  };
  // Despawn the entity
  let _ = world.despawn(entity);
  Ok(stack)
}
