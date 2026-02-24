//! Item registry: central catalog for item templates with code and JSON loading.

use crate::core::error::InventoryError;
use crate::core::inventory::{ItemData, ItemId, ItemTemplate};
use std::collections::HashMap;

/// Factory function that deserializes a JSON value into a boxed [`ItemData`].
type DataTypeFactory = Box<dyn Fn(&serde_json::Value) -> Option<Box<dyn ItemData>> + Send + Sync>;

/// Central catalog that stores [`ItemTemplate`]s and maps names to [`ItemId`]s.
///
/// The registry assigns sequential IDs starting from 0. Templates are stored
/// in a contiguous `Vec` for cache-friendly iteration, while a `HashMap`
/// provides O(1) lookup by name.
///
/// # JSON Loading
///
/// Custom item data types can be registered via [`register_data_type`](Self::register_data_type)
/// before calling [`load_from_json`](Self::load_from_json) or
/// [`load_from_file`](Self::load_from_file). The JSON `data_type` field is
/// matched against registered factory names to deserialize game-specific
/// data into the template's `data` slot.
///
/// # Example
///
/// ```rust,ignore
/// let mut registry = ItemRegistry::new();
/// let id = registry.register(
///     ItemTemplate::builder("health_potion")
///         .display_name("Health Potion")
///         .max_stack(10)
///         .tag("consumable")
///         .build()
/// );
/// assert_eq!(registry.get(id).unwrap().name(), "health_potion");
/// ```
pub struct ItemRegistry {
  templates: Vec<ItemTemplate>,
  name_index: HashMap<String, ItemId>,
  data_type_factories: HashMap<String, DataTypeFactory>,
}

impl ItemRegistry {
  /// Create an empty registry.
  pub fn new() -> Self {
    Self {
      templates: Vec::new(),
      name_index: HashMap::new(),
      data_type_factories: HashMap::new(),
    }
  }

  /// Register a template and assign it a sequential [`ItemId`].
  ///
  /// The template's `id` field is overwritten with the newly assigned ID.
  /// The template is also indexed by its `name` for O(1) lookup via
  /// [`get_by_name`](Self::get_by_name).
  pub fn register(&mut self, mut template: ItemTemplate) -> ItemId {
    let id = ItemId::new(self.templates.len() as u32);
    template.set_id(id);
    self.name_index.insert(template.name().to_string(), id);
    self.templates.push(template);
    id
  }

  /// Look up a template by its [`ItemId`].
  pub fn get(&self, id: ItemId) -> Option<&ItemTemplate> {
    self.templates.get(id.index() as usize)
  }

  /// Look up a template by its internal name (e.g. `"iron_sword"`).
  pub fn get_by_name(&self, name: &str) -> Option<&ItemTemplate> {
    self.name_index.get(name).and_then(|id| self.get(*id))
  }

  /// Return all templates that carry the given tag.
  pub fn find_by_tag(&self, tag: &str) -> Vec<&ItemTemplate> {
    self.templates.iter().filter(|t| t.has_tag(tag)).collect()
  }

  /// Return the [`ItemId`] for a given internal name, if registered.
  pub fn id_of(&self, name: &str) -> Option<ItemId> {
    self.name_index.get(name).copied()
  }

  /// Number of registered templates.
  pub fn len(&self) -> usize {
    self.templates.len()
  }

  /// Whether the registry is empty.
  pub fn is_empty(&self) -> bool {
    self.templates.is_empty()
  }

  /// Register a factory for a custom [`ItemData`] type.
  ///
  /// When loading items from JSON, the `data_type` field is matched against
  /// registered factory names. The factory receives the `data` JSON value
  /// and should return `Some(boxed_data)` on success or `None` on failure.
  pub fn register_data_type(
    &mut self,
    type_name: &str,
    factory: impl Fn(&serde_json::Value) -> Option<Box<dyn ItemData>> + Send + Sync + 'static,
  ) {
    self
      .data_type_factories
      .insert(type_name.to_string(), Box::new(factory));
  }

  /// Load item templates from a JSON file on disk.
  ///
  /// The file must contain a JSON array of item objects. See
  /// [`load_from_json`](Self::load_from_json) for the expected format.
  pub fn load_from_file(&mut self, path: &str) -> Result<Vec<ItemId>, InventoryError> {
    let json = std::fs::read_to_string(path)?;
    self.load_from_json(&json)
  }

  /// Parse a JSON string containing an array of item definitions and
  /// register each one.
  ///
  /// # JSON Format
  ///
  /// ```json
  /// [
  ///   {
  ///     "name": "iron_sword",
  ///     "display_name": "Iron Sword",
  ///     "description": "A trusty blade",
  ///     "max_stack": 1,
  ///     "tags": ["weapon", "melee"],
  ///     "data_type": "weapon_stats",
  ///     "data": { "damage": 10.0, "attack_speed": 1.0 }
  ///   }
  /// ]
  /// ```
  ///
  /// Only `name` is required. All other fields have sensible defaults.
  /// When `data_type` is present, the corresponding factory (registered via
  /// [`register_data_type`](Self::register_data_type)) is called with the
  /// `data` value. If the factory returns `None` or the type is not
  /// registered, an [`InventoryError::UnknownDataType`] is returned.
  pub fn load_from_json(&mut self, json: &str) -> Result<Vec<ItemId>, InventoryError> {
    let items: Vec<serde_json::Value> = serde_json::from_str(json)?;
    let mut ids = Vec::with_capacity(items.len());

    for item in &items {
      let name = item["name"]
        .as_str()
        .ok_or_else(|| InventoryError::ValidationFailed("item missing 'name' field".to_string()))?;

      let mut builder = ItemTemplate::builder(name);

      if let Some(display_name) = item.get("display_name").and_then(|v| v.as_str()) {
        builder = builder.display_name(display_name);
      }

      if let Some(description) = item.get("description").and_then(|v| v.as_str()) {
        builder = builder.description(description);
      }

      if let Some(max_stack) = item.get("max_stack").and_then(|v| v.as_u64()) {
        builder = builder.max_stack(max_stack as u32);
      }

      if let Some(tags) = item.get("tags").and_then(|v| v.as_array()) {
        for tag_val in tags {
          if let Some(tag) = tag_val.as_str() {
            builder = builder.tag(tag);
          }
        }
      }

      let mut template = builder.build();

      // Handle optional data_type + data
      if let Some(data_type_name) = item.get("data_type").and_then(|v| v.as_str()) {
        let factory = self
          .data_type_factories
          .get(data_type_name)
          .ok_or_else(|| InventoryError::UnknownDataType {
            type_name: data_type_name.to_string(),
          })?;

        let data_value = item.get("data").unwrap_or(&serde_json::Value::Null);
        let data = factory(data_value).ok_or_else(|| InventoryError::UnknownDataType {
          type_name: data_type_name.to_string(),
        })?;

        template.data = Some(data);
      }

      let id = self.register(template);
      ids.push(id);
    }

    Ok(ids)
  }
}

impl Default for ItemRegistry {
  fn default() -> Self {
    Self::new()
  }
}
