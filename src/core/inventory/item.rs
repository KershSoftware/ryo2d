//! Core item types: ItemId, ItemData, ItemStack, ItemTemplate, and ItemTemplateBuilder.

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use std::fmt;

// ---------------------------------------------------------------------------
// ItemId
// ---------------------------------------------------------------------------

/// Unique identifier for an item type in the [`ItemRegistry`].
///
/// `ItemId` is a lightweight `Copy` wrapper around a `u32` index.
/// Two `ItemId` values are equal if and only if they refer to the same
/// registered item template.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(u32);

impl ItemId {
  /// Create an `ItemId` from a raw index.
  ///
  /// Prefer using [`ItemRegistry::register`] to obtain ids; this
  /// constructor is mainly useful for tests and deserialization.
  pub fn new(index: u32) -> Self {
    Self(index)
  }

  /// Return the raw `u32` index.
  pub fn index(&self) -> u32 {
    self.0
  }
}

impl fmt::Debug for ItemId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "ItemId({})", self.0)
  }
}

impl fmt::Display for ItemId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "item#{}", self.0)
  }
}

// ---------------------------------------------------------------------------
// ItemData
// ---------------------------------------------------------------------------

/// Extension trait for custom per-item-type or per-instance properties.
///
/// Game code implements `ItemData` on structs such as `WeaponStats` or
/// `PotionEffect` and attaches them to [`ItemTemplate`] (shared defaults) or
/// [`ItemStack`] (per-instance overrides).
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// struct WeaponStats { damage: f64, attack_speed: f64 }
///
/// impl ItemData for WeaponStats {
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
///     fn clone_box(&self) -> Box<dyn ItemData> { Box::new(self.clone()) }
/// }
/// ```
pub trait ItemData: Any + Send + Sync {
  /// Downcast to `&dyn Any` for runtime type checking.
  fn as_any(&self) -> &dyn Any;

  /// Downcast to `&mut dyn Any` for runtime type checking.
  fn as_any_mut(&mut self) -> &mut dyn Any;

  /// Clone into a boxed trait object.
  fn clone_box(&self) -> Box<dyn ItemData>;
}

impl Clone for Box<dyn ItemData> {
  fn clone(&self) -> Self {
    self.clone_box()
  }
}

// ---------------------------------------------------------------------------
// ItemStack
// ---------------------------------------------------------------------------

/// A stack of identical items inside an inventory slot.
///
/// `ItemStack` is a *value type* — cheap to move, clone-able, and never stored
/// as an ECS entity on its own.  When items are dropped into the game world
/// they become full entities via [`ItemDrop`].
///
/// Optional [`ItemData`] can be attached for per-instance properties
/// (e.g. enchantments, durability).
pub struct ItemStack {
  item_id: ItemId,
  quantity: u32,
  instance_data: Option<Box<dyn ItemData>>,
}

impl ItemStack {
  /// Create a new stack of `quantity` items.
  pub fn new(item_id: ItemId, quantity: u32) -> Self {
    Self {
      item_id,
      quantity,
      instance_data: None,
    }
  }

  /// Attach per-instance data (builder-style, consumes and returns `self`).
  pub fn with_data(mut self, data: impl ItemData) -> Self {
    self.instance_data = Some(Box::new(data));
    self
  }

  /// The item type this stack contains.
  pub fn item_id(&self) -> ItemId {
    self.item_id
  }

  /// How many items are in the stack.
  pub fn quantity(&self) -> u32 {
    self.quantity
  }

  /// Set the quantity (e.g. after splitting or merging stacks).
  pub fn set_quantity(&mut self, quantity: u32) {
    self.quantity = quantity;
  }

  /// Access the per-instance data, if any.
  pub fn instance_data(&self) -> Option<&dyn ItemData> {
    self.instance_data.as_deref()
  }

  /// Mutably access the per-instance data, if any.
  pub fn instance_data_mut(&mut self) -> Option<&mut dyn ItemData> {
    self.instance_data.as_deref_mut()
  }
}

impl Clone for ItemStack {
  fn clone(&self) -> Self {
    Self {
      item_id: self.item_id,
      quantity: self.quantity,
      instance_data: self.instance_data.clone(),
    }
  }
}

impl fmt::Debug for ItemStack {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ItemStack")
      .field("item_id", &self.item_id)
      .field("quantity", &self.quantity)
      .field("has_instance_data", &self.instance_data.is_some())
      .finish()
  }
}

// ---------------------------------------------------------------------------
// ItemTemplate
// ---------------------------------------------------------------------------

/// Blueprint that defines an item type's static properties.
///
/// Templates are registered in the [`ItemRegistry`] and referenced by
/// [`ItemId`].  They hold shared data (name, max stack size, tags) that
/// does *not* vary between individual item stacks.
///
/// Use [`ItemTemplate::builder`] to construct templates:
///
/// ```rust,ignore
/// let sword = ItemTemplate::builder("iron_sword")
///     .display_name("Iron Sword")
///     .description("A sturdy blade.")
///     .max_stack(1)
///     .tag("weapon")
///     .tag("melee")
///     .build();
/// ```
pub struct ItemTemplate {
  id: ItemId,
  name: String,
  display_name: String,
  description: String,
  max_stack: u32,
  tags: HashSet<String>,
  /// Optional default data for this item type.
  ///
  /// `pub(crate)` so the registry can populate it during JSON loading.
  pub(crate) data: Option<Box<dyn ItemData>>,
}

impl ItemTemplate {
  /// Start building a new template with the given internal name.
  pub fn builder(name: &str) -> ItemTemplateBuilder {
    ItemTemplateBuilder::new(name)
  }

  /// The registry-assigned id.
  pub fn id(&self) -> ItemId {
    self.id
  }

  /// Internal (code-facing) name, e.g. `"iron_sword"`.
  pub fn name(&self) -> &str {
    &self.name
  }

  /// Human-readable display name, e.g. `"Iron Sword"`.
  pub fn display_name(&self) -> &str {
    &self.display_name
  }

  /// Flavour / tooltip description.
  pub fn description(&self) -> &str {
    &self.description
  }

  /// Maximum number of this item that can occupy a single inventory slot.
  pub fn max_stack(&self) -> u32 {
    self.max_stack
  }

  /// Check whether the template carries a given tag.
  pub fn has_tag(&self, tag: &str) -> bool {
    self.tags.contains(tag)
  }

  /// Iterate over all tags.
  pub fn tags(&self) -> impl Iterator<Item = &String> {
    self.tags.iter()
  }

  /// Access the default item data, if any.
  pub fn data(&self) -> Option<&dyn ItemData> {
    self.data.as_deref()
  }

  /// Set the registry-assigned id.  Called by [`ItemRegistry::register`].
  pub(crate) fn set_id(&mut self, id: ItemId) {
    self.id = id;
  }
}

impl fmt::Debug for ItemTemplate {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ItemTemplate")
      .field("id", &self.id)
      .field("name", &self.name)
      .field("display_name", &self.display_name)
      .field("max_stack", &self.max_stack)
      .field("tags", &self.tags)
      .field("has_data", &self.data.is_some())
      .finish()
  }
}

// ---------------------------------------------------------------------------
// ItemTemplateBuilder
// ---------------------------------------------------------------------------

/// Builder for [`ItemTemplate`].
///
/// Created via [`ItemTemplate::builder`].
pub struct ItemTemplateBuilder {
  name: String,
  display_name: Option<String>,
  description: String,
  max_stack: u32,
  tags: HashSet<String>,
  data: Option<Box<dyn ItemData>>,
}

impl ItemTemplateBuilder {
  fn new(name: &str) -> Self {
    Self {
      name: name.to_string(),
      display_name: None,
      description: String::new(),
      max_stack: 1,
      tags: HashSet::new(),
      data: None,
    }
  }

  /// Set the human-readable display name.  Defaults to the internal name if
  /// not called.
  pub fn display_name(mut self, name: &str) -> Self {
    self.display_name = Some(name.to_string());
    self
  }

  /// Set the item description / tooltip text.
  pub fn description(mut self, desc: &str) -> Self {
    self.description = desc.to_string();
    self
  }

  /// Set the maximum stack size (defaults to 1). Clamped to at least 1.
  pub fn max_stack(mut self, max: u32) -> Self {
    self.max_stack = max.max(1);
    self
  }

  /// Add a tag (e.g. `"weapon"`, `"consumable"`).
  pub fn tag(mut self, tag: &str) -> Self {
    self.tags.insert(tag.to_string());
    self
  }

  /// Attach default [`ItemData`] to every instance of this item.
  pub fn data(mut self, data: impl ItemData) -> Self {
    self.data = Some(Box::new(data));
    self
  }

  /// Consume the builder and produce a finished [`ItemTemplate`].
  ///
  /// The template's `id` is set to `ItemId(0)` and will be assigned by
  /// the [`ItemRegistry`] upon registration.
  pub fn build(self) -> ItemTemplate {
    let display = self.display_name.unwrap_or_else(|| self.name.clone());

    ItemTemplate {
      id: ItemId::new(0),
      name: self.name,
      display_name: display,
      description: self.description,
      max_stack: self.max_stack,
      tags: self.tags,
      data: self.data,
    }
  }
}
