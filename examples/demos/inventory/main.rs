use std::any::Any;

use rand::Rng;
use ryo2d::prelude::*;

// === Custom Item Data ===

#[derive(Clone, Debug)]
struct CombatStats {
  damage: i32,
  defense: i32,
  color: (u8, u8, u8),
}

impl ItemData for CombatStats {
  fn as_any(&self) -> &dyn Any {
    self
  }
  fn as_any_mut(&mut self) -> &mut dyn Any {
    self
  }
  fn clone_box(&self) -> Box<dyn ItemData> {
    Box::new(self.clone())
  }
}

// === Equipment Slots ===

const SLOT_WEAPON: usize = 0;
const SLOT_ARMOR: usize = 1;
const SLOT_HELMET: usize = 2;
const SLOT_NAMES: [&str; 3] = ["Weapon", "Armor", "Helmet"];

// === Tag-Based Equipment Validator ===

struct TagValidator;

impl EquipValidator for TagValidator {
  fn can_equip(&self, slot: usize, item: &ItemStack, registry: &ItemRegistry) -> bool {
    let Some(template) = registry.get(item.item_id()) else {
      return false;
    };
    match slot {
      SLOT_WEAPON => template.has_tag("weapon"),
      SLOT_ARMOR => template.has_tag("armor"),
      SLOT_HELMET => template.has_tag("helmet"),
      _ => false,
    }
  }
}

// === Player Visual ===

struct PlayerVisual {
  _body: Entity,
  _head: Entity,
  weapon: Entity,
  armor_overlay: Entity,
  helmet_overlay: Entity,
}

// === Demo Scene ===

struct InventoryDemoScene {
  registry: ItemRegistry,
  inventory: Inventory,
  equipment: Equipment,
  loot_table: LootTable,
  selected_slot: usize,
  focus_equipment: bool,
  world_drops: Vec<Entity>,
  entity_count: u32,
  player_visual: Option<PlayerVisual>,
  status_message: String,
  status_timer: f32,
  drop_counter: u32,
}

impl InventoryDemoScene {
  fn new() -> Self {
    Self {
      registry: ItemRegistry::new(),
      inventory: Inventory::new(8),
      equipment: Equipment::new(3),
      loot_table: LootTable::new(),
      selected_slot: 0,
      focus_equipment: false,
      world_drops: Vec::new(),
      entity_count: 0,
      player_visual: None,
      status_message: String::new(),
      status_timer: 0.0,
      drop_counter: 0,
    }
  }

  fn set_status(&mut self, msg: &str) {
    self.status_message = msg.to_string();
    self.status_timer = 3.0;
  }

  /// Look up the CombatStats for a stack, checking instance data first, then template.
  fn get_stats(&self, stack: &ItemStack) -> Option<CombatStats> {
    if let Some(data) = stack.instance_data()
      && let Some(stats) = data.as_any().downcast_ref::<CombatStats>()
    {
      return Some(stats.clone());
    }
    if let Some(template) = self.registry.get(stack.item_id())
      && let Some(data) = template.data()
      && let Some(stats) = data.as_any().downcast_ref::<CombatStats>()
    {
      return Some(stats.clone());
    }
    None
  }

  /// Get CombatStats color from a stack.
  fn item_color(&self, stack: &ItemStack) -> (u8, u8, u8) {
    self
      .get_stats(stack)
      .map(|s| s.color)
      .unwrap_or((255, 255, 255))
  }

  /// Get (damage, defense) from a stack.
  fn combat_stats(&self, stack: &ItemStack) -> (i32, i32) {
    self
      .get_stats(stack)
      .map(|s| (s.damage, s.defense))
      .unwrap_or((0, 0))
  }

  /// Sum stats from all equipped items.
  fn total_stats(&self) -> (i32, i32) {
    let mut atk = 0;
    let mut def = 0;
    for (_, stack) in self.equipment.equipped_items() {
      let (d, f) = self.combat_stats(stack);
      atk += d;
      def += f;
    }
    (atk, def)
  }

  /// Sync equipment overlay visibility and tint to match current equipment.
  fn sync_equipment_visuals(&self, world: &mut World) {
    let Some(visual) = &self.player_visual else {
      return;
    };

    // Weapon
    if let Some(stack) = self.equipment.get(SLOT_WEAPON) {
      let (r, g, b) = self.item_color(stack);
      if let Some(mut rend) = world.get_mut::<Renderable>(visual.weapon) {
        rend.visible = true;
        rend.tint = Color::rgb(r, g, b);
      }
    } else if let Some(mut rend) = world.get_mut::<Renderable>(visual.weapon) {
      rend.visible = false;
    }

    // Armor
    if let Some(stack) = self.equipment.get(SLOT_ARMOR) {
      let (r, g, b) = self.item_color(stack);
      if let Some(mut rend) = world.get_mut::<Renderable>(visual.armor_overlay) {
        rend.visible = true;
        rend.tint = Color::rgb(r, g, b);
      }
    } else if let Some(mut rend) = world.get_mut::<Renderable>(visual.armor_overlay) {
      rend.visible = false;
    }

    // Helmet
    if let Some(stack) = self.equipment.get(SLOT_HELMET) {
      let (r, g, b) = self.item_color(stack);
      if let Some(mut rend) = world.get_mut::<Renderable>(visual.helmet_overlay) {
        rend.visible = true;
        rend.tint = Color::rgb(r, g, b);
      }
    } else if let Some(mut rend) = world.get_mut::<Renderable>(visual.helmet_overlay) {
      rend.visible = false;
    }
  }

  /// Spawn a dropped item entity with a colored visual.
  fn spawn_drop(&mut self, world: &mut World, stack: ItemStack, color: (u8, u8, u8)) {
    let mut rng = rand::thread_rng();
    let x = rng.gen_range(40.0..360.0_f32);
    let y = 500.0_f32;

    let entity = spawn_item_drop(world, stack, (x, y));

    self.drop_counter += 1;
    let sprite = Sprite::solid_rect(12, 12, color.0, color.1, color.2, 255);
    let handle = SpriteHandle::from_sprite(sprite, format!("drop_{}", self.drop_counter));
    world.add_component(entity, Renderable::sprite(handle).with_z_order(5));

    self.world_drops.push(entity);
  }

  // --- Input handlers (called from update) ---

  fn handle_equip(&mut self) {
    let slot = self.selected_slot;

    // Gather info with immutable borrows
    let equip_info = self.inventory.get(slot).and_then(|stack| {
      let template = self.registry.get(stack.item_id())?;
      let equip_slot = if template.has_tag("weapon") {
        Some(SLOT_WEAPON)
      } else if template.has_tag("armor") {
        Some(SLOT_ARMOR)
      } else if template.has_tag("helmet") {
        Some(SLOT_HELMET)
      } else {
        None
      };
      let to_equip = stack.clone();
      let qty = stack.quantity();
      let name = template.display_name().to_string();
      equip_slot.map(|es| (to_equip, qty, es, name))
    });

    if let Some((to_equip, qty, equip_slot, name)) = equip_info {
      match self.equipment.equip(equip_slot, to_equip, &self.registry) {
        Ok(prev) => {
          let _ = self.inventory.remove(slot, qty);
          if let Some(prev_stack) = prev {
            let _ = self.inventory.add(prev_stack, &self.registry);
          }
          self.set_status(&format!("Equipped {} to {}", name, SLOT_NAMES[equip_slot]));
        }
        Err(e) => self.set_status(&format!("Cannot equip: {}", e)),
      }
    } else if self.inventory.get(slot).is_some() {
      self.set_status("Item cannot be equipped");
    } else {
      self.set_status("No item in selected slot");
    }
  }

  fn handle_unequip(&mut self, equip_slot: usize) {
    match self.equipment.unequip(equip_slot) {
      Ok(stack) => {
        let name = self
          .registry
          .get(stack.item_id())
          .map(|t| t.display_name().to_string())
          .unwrap_or_else(|| "Unknown".to_string());
        match self.inventory.add(stack, &self.registry) {
          Ok(()) => self.set_status(&format!("Unequipped {}", name)),
          Err(_) => self.set_status("Inventory full!"),
        }
      }
      Err(_) => self.set_status(&format!("{} slot is empty", SLOT_NAMES[equip_slot])),
    }
  }

  fn handle_drop(&mut self, world: &mut World) {
    let slot = self.selected_slot;

    // Gather info before mutation
    let drop_info = self.inventory.get(slot).map(|stack| {
      let qty = stack.quantity();
      let name = self
        .registry
        .get(stack.item_id())
        .map(|t| t.display_name().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
      let color = self
        .registry
        .get(stack.item_id())
        .and_then(|t| t.data())
        .and_then(|d| d.as_any().downcast_ref::<CombatStats>())
        .map(|s| s.color)
        .unwrap_or((255, 255, 255));
      (qty, name, color)
    });

    if let Some((qty, name, color)) = drop_info {
      match self.inventory.remove(slot, qty) {
        Ok(stack) => {
          self.spawn_drop(world, stack, color);
          self.set_status(&format!("Dropped {}", name));
        }
        Err(e) => self.set_status(&format!("Cannot drop: {}", e)),
      }
    } else {
      self.set_status("No item in selected slot");
    }
  }

  fn handle_pickup(&mut self, world: &mut World) {
    if self.world_drops.is_empty() {
      self.set_status("No items on the ground");
      return;
    }

    let entity = self.world_drops[0];
    match pickup_item(world, entity) {
      Ok(stack) => {
        let name = self
          .registry
          .get(stack.item_id())
          .map(|t| t.display_name().to_string())
          .unwrap_or_else(|| "Unknown".to_string());
        let color = self
          .registry
          .get(stack.item_id())
          .and_then(|t| t.data())
          .and_then(|d| d.as_any().downcast_ref::<CombatStats>())
          .map(|s| s.color)
          .unwrap_or((255, 255, 255));

        match self.inventory.add(stack.clone(), &self.registry) {
          Ok(()) => {
            self.world_drops.remove(0);
            self.set_status(&format!("Picked up {}", name));
          }
          Err(_) => {
            // Re-spawn since pickup already despawned the entity
            self.spawn_drop(world, stack, color);
            self.world_drops.remove(0); // remove old dead entity
            self.set_status("Inventory full!");
          }
        }
      }
      Err(e) => {
        self.world_drops.remove(0);
        self.set_status(&format!("Pickup failed: {}", e));
      }
    }
  }

  fn handle_loot(&mut self) {
    let mut rng = rand::thread_rng();
    match self.loot_table.roll(&mut rng) {
      Some(stack) => {
        let name = self
          .registry
          .get(stack.item_id())
          .map(|t| t.display_name().to_string())
          .unwrap_or_else(|| "Unknown".to_string());
        let qty = stack.quantity();
        match self.inventory.add(stack, &self.registry) {
          Ok(()) => self.set_status(&format!("Looted {} x{}", name, qty)),
          Err(_) => self.set_status("Inventory full!"),
        }
      }
      None => self.set_status("Loot roll: nothing!"),
    }
  }
}

impl Scene for InventoryDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Register combat stats data type factory
    self.registry.register_data_type("combat_stats", |value| {
      let damage = value["damage"].as_i64()? as i32;
      let defense = value["defense"].as_i64()? as i32;
      let color_arr = value["color"].as_array()?;
      let r = color_arr.first()?.as_u64()? as u8;
      let g = color_arr.get(1)?.as_u64()? as u8;
      let b = color_arr.get(2)?.as_u64()? as u8;
      Some(Box::new(CombatStats {
        damage,
        defense,
        color: (r, g, b),
      }) as Box<dyn ItemData>)
    });

    // Load items from JSON (embedded at compile time for reliable path resolution)
    const ITEMS_JSON: &str = include_str!("assets/items.json");
    match self.registry.load_from_json(ITEMS_JSON) {
      Ok(ids) => self.set_status(&format!("Loaded {} items from JSON", ids.len())),
      Err(e) => {
        self.set_status(&format!("Failed to load items: {}", e));
        return;
      }
    }

    // Set up equipment validator
    self.equipment.set_validator(Box::new(TagValidator));

    // Build loot table from all registered items
    let mut table = LootTable::new();
    for i in 0..self.registry.len() {
      let id = ItemId::new(i as u32);
      if let Some(template) = self.registry.get(id) {
        let weight = if template.has_tag("consumable") {
          5.0
        } else {
          1.0
        };
        let qty = if template.max_stack() > 1 {
          1..=3
        } else {
          1..=1
        };
        table = table.entry(id, weight, qty);
      }
    }
    table = table.nothing(2.0);
    self.loot_table = table;

    // Pre-populate inventory with starter items
    if let Some(id) = self.registry.id_of("iron_sword") {
      let _ = self.inventory.add(ItemStack::new(id, 1), &self.registry);
    }
    if let Some(id) = self.registry.id_of("leather_armor") {
      let _ = self.inventory.add(ItemStack::new(id, 1), &self.registry);
    }
    if let Some(id) = self.registry.id_of("cloth_cap") {
      let _ = self.inventory.add(ItemStack::new(id, 1), &self.registry);
    }
    if let Some(id) = self.registry.id_of("health_potion") {
      let _ = self.inventory.add(ItemStack::new(id, 3), &self.registry);
    }

    // === Spawn player visual ===
    let px = 200.0_f32;
    let py = 300.0_f32;

    // Body
    let body_sprite = Sprite::solid_rect(28, 36, 100, 140, 200, 255);
    let body_handle = SpriteHandle::from_sprite(body_sprite, "player_body");
    let body = ctx.world.spawn((
      Transform::new(px, py).with_pivot_center(),
      Renderable::sprite(body_handle).with_z_order(0),
    ));

    // Head
    let head_sprite = Sprite::solid_rect(20, 20, 220, 180, 140, 255);
    let head_handle = SpriteHandle::from_sprite(head_sprite, "player_head");
    let head = ctx.world.spawn((
      Transform::new(px, py - 28.0).with_pivot_center(),
      Renderable::sprite(head_handle).with_z_order(1),
    ));

    // Weapon overlay (hidden until equipped, white sprite tinted by item color)
    let weapon_sprite = Sprite::solid_rect(6, 30, 255, 255, 255, 255);
    let weapon_handle = SpriteHandle::from_sprite(weapon_sprite, "player_weapon");
    let weapon = ctx.world.spawn((
      Transform::new(px + 17.0, py).with_pivot_center(),
      Renderable::sprite(weapon_handle)
        .with_z_order(2)
        .with_visible(false),
    ));

    // Armor overlay (hidden until equipped)
    let armor_sprite = Sprite::solid_rect(28, 36, 255, 255, 255, 255);
    let armor_handle = SpriteHandle::from_sprite(armor_sprite, "player_armor");
    let armor_overlay = ctx.world.spawn((
      Transform::new(px, py).with_pivot_center(),
      Renderable::sprite(armor_handle)
        .with_z_order(1)
        .with_visible(false)
        .with_alpha(0.7),
    ));

    // Helmet overlay (hidden until equipped)
    let helmet_sprite = Sprite::solid_rect(22, 10, 255, 255, 255, 255);
    let helmet_handle = SpriteHandle::from_sprite(helmet_sprite, "player_helmet");
    let helmet_overlay = ctx.world.spawn((
      Transform::new(px, py - 33.0).with_pivot_center(),
      Renderable::sprite(helmet_handle)
        .with_z_order(3)
        .with_visible(false),
    ));

    self.player_visual = Some(PlayerVisual {
      _body: body,
      _head: head,
      weapon,
      armor_overlay,
      helmet_overlay,
    });

    // Ground line
    let ground_sprite = Sprite::solid_rect(380, 2, 100, 100, 100, 255);
    let ground_handle = SpriteHandle::from_sprite(ground_sprite, "ground_line");
    ctx.world.spawn((
      Transform::new(200.0, 480.0).with_pivot_center(),
      Renderable::sprite(ground_handle).with_z_order(-1),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Tick status timer
    if self.status_timer > 0.0 {
      self.status_timer -= dt;
      if self.status_timer <= 0.0 {
        self.status_message.clear();
      }
    }

    // Track entity count for display
    self.entity_count = ctx.world.len();

    // ESC to quit
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    let digit_keys = [
      KeyCode::Digit1,
      KeyCode::Digit2,
      KeyCode::Digit3,
      KeyCode::Digit4,
      KeyCode::Digit5,
      KeyCode::Digit6,
      KeyCode::Digit7,
      KeyCode::Digit8,
    ];

    // Arrow keys: navigate between equipment (3 slots) and inventory (8 slots)
    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      if self.focus_equipment {
        if self.selected_slot > 0 {
          self.selected_slot -= 1;
        }
      } else if self.selected_slot > 0 {
        self.selected_slot -= 1;
      } else {
        // At top of inventory, move to bottom of equipment
        self.focus_equipment = true;
        self.selected_slot = 2;
      }
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      if self.focus_equipment {
        if self.selected_slot < 2 {
          self.selected_slot += 1;
        } else {
          // At bottom of equipment, move to top of inventory
          self.focus_equipment = false;
          self.selected_slot = 0;
        }
      } else if self.selected_slot < 7 {
        self.selected_slot += 1;
      }
    }

    // Space = equip (from inventory) or unequip (from equipment)
    if ctx.input.key_pressed(KeyCode::Space) {
      if self.focus_equipment {
        self.handle_unequip(self.selected_slot);
      } else {
        self.handle_equip();
      }
    }

    // U held + 1/2/3 = unequip
    if ctx.input.key_down(KeyCode::KeyU) {
      for (i, &key) in digit_keys[..3].iter().enumerate() {
        if ctx.input.key_pressed(key) {
          self.handle_unequip(i);
        }
      }
    } else {
      // Slot selection (1-8), also switches focus to inventory
      for (i, &key) in digit_keys.iter().enumerate() {
        if ctx.input.key_pressed(key) {
          self.selected_slot = i;
          self.focus_equipment = false;
        }
      }
    }

    // E = equip selected item (inventory only)
    if ctx.input.key_pressed(KeyCode::KeyE) && !self.focus_equipment {
      self.handle_equip();
    }

    // D = drop selected item
    if ctx.input.key_pressed(KeyCode::KeyD) {
      self.handle_drop(ctx.world);
    }

    // P = pick up nearest ground item
    if ctx.input.key_pressed(KeyCode::KeyP) {
      self.handle_pickup(ctx.world);
    }

    // L = roll loot table
    if ctx.input.key_pressed(KeyCode::KeyL) {
      self.handle_loot();
    }

    // Sync equipment visuals to match current equipment state
    self.sync_equipment_visuals(ctx.world);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let white = Color::rgb(240, 240, 240);
    let gray = Color::rgb(160, 160, 170);
    let highlight = Color::rgb(100, 160, 255);
    let panel_x = 424.0_f32;

    // Right panel background
    ui.draw_rect(Rect::new(412.0, 0.0, 612.0, 700.0), Color::rgb(25, 25, 35));
    // Divider line
    ui.draw_rect(Rect::new(412.0, 0.0, 2.0, 700.0), Color::rgb(80, 80, 100));

    // === EQUIPMENT ===
    let mut y = 20.0_f32;
    ui.draw_text("EQUIPMENT", panel_x, y, white);
    y += 28.0;

    for (i, slot_name) in SLOT_NAMES.iter().enumerate() {
      let is_selected = self.focus_equipment && i == self.selected_slot;
      if is_selected {
        ui.draw_rect(
          Rect::new(panel_x - 4.0, y - 2.0, 570.0, 20.0),
          Color::rgb(40, 50, 80),
        );
      }
      let text = match self.equipment.get(i) {
        Some(stack) => {
          let name = self
            .registry
            .get(stack.item_id())
            .map(|t| t.display_name().to_string())
            .unwrap_or_else(|| "?".to_string());
          format!("  {}: {}", slot_name, name)
        }
        None => format!("  {}: [empty]", slot_name),
      };
      let color = if is_selected { highlight } else { gray };
      ui.draw_text(&text, panel_x, y, color);
      y += 22.0;
    }

    // === INVENTORY ===
    y += 14.0;
    ui.draw_text("INVENTORY", panel_x, y, white);
    y += 28.0;

    for i in 0..8 {
      let is_selected = !self.focus_equipment && i == self.selected_slot;
      if is_selected {
        ui.draw_rect(
          Rect::new(panel_x - 4.0, y - 2.0, 570.0, 20.0),
          Color::rgb(40, 50, 80),
        );
      }

      let text = match self.inventory.get(i) {
        Some(stack) => {
          let template = self.registry.get(stack.item_id());
          let name = template
            .map(|t| t.display_name().to_string())
            .unwrap_or_else(|| "?".to_string());
          let tag_short = template
            .and_then(|t| t.tags().next())
            .map(|t| match t.as_str() {
              "weapon" => "WPN",
              "armor" => "ARM",
              "helmet" => "HLM",
              "consumable" => "USE",
              _ => "",
            })
            .unwrap_or("");
          if stack.quantity() > 1 {
            format!("  {}: {} x{}  {}", i + 1, name, stack.quantity(), tag_short)
          } else {
            format!("  {}: {}  {}", i + 1, name, tag_short)
          }
        }
        None => format!("  {}: ---", i + 1),
      };

      let color = if is_selected { highlight } else { gray };
      ui.draw_text(&text, panel_x, y, color);
      y += 22.0;
    }

    // === STATS ===
    y += 16.0;
    ui.draw_text("STATS", panel_x, y, white);
    y += 28.0;

    let (atk, def) = self.total_stats();
    ui.draw_text(
      &format!("  ATK: {}   DEF: {}", atk, def),
      panel_x,
      y,
      Color::rgb(220, 220, 120),
    );
    y += 22.0;

    let used = 8 - self.inventory.empty_slots();
    ui.draw_text(
      &format!("  Slots: {}/8   Drops: {}", used, self.world_drops.len()),
      panel_x,
      y,
      gray,
    );

    // === CONTROLS ===
    y += 30.0;
    ui.draw_text("CONTROLS", panel_x, y, white);
    y += 28.0;

    let controls = [
      "[Up/Down] Navigate",
      "[Space] Equip / Unequip",
      "[1-8] Select slot  [E] Equip",
      "[U+1/2/3] Unequip slot",
      "[D] Drop  [P] Pickup  [L] Loot",
      "[ESC] Quit",
    ];
    for line in &controls {
      ui.draw_text(&format!("  {}", line), panel_x, y, gray);
      y += 20.0;
    }

    // === LEFT PANEL INFO ===
    ui.draw_text(
      &format!("Entities: {}", self.entity_count),
      20.0,
      20.0,
      gray,
    );
    ui.draw_text("INVENTORY DEMO", 100.0, 55.0, white);

    // Status message
    if !self.status_message.is_empty() && self.status_timer > 0.0 {
      let a = if self.status_timer < 1.0 {
        (self.status_timer * 255.0) as u8
      } else {
        255
      };
      let c = Color::rgb(255, 255, 100).with_alpha(a);
      ui.draw_text(&self.status_message, 20.0, 650.0, c);
    }

    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Inventory System Demo - Ryo2D")
    .resolution(1024, 700)
    .target_fps(60)
    .start_scene(InventoryDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
