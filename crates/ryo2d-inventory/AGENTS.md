# AGENTS.md - ryo2d-inventory

Items, inventories, equipment and loot tables for Ryo2D, as an optional crate on top of `ryo2d`. The core never depends on it. The crate's rustdoc (`src/lib.rs`) is the reference; there is no skill.

## Using It

```toml
[dependencies]
ryo2d = { path = "<ryo2d checkout>" }
ryo2d-inventory = { path = "<ryo2d checkout>/crates/ryo2d-inventory" }
```

```rust
use ryo2d::prelude::*;
use ryo2d_inventory::prelude::*;
```

Inside this repository a demo uses `{ path = "../../../crates/ryo2d-inventory" }`. Its errors (`InventoryError`, `InventoryResult`) live here, not in `ryo2d`'s `EngineError`.

## Model

Items use a hybrid value/entity model: lightweight `ItemStack` values inside inventories and equipment, full ECS entities (`Transform` + `ItemDrop`) only when dropped in the world (`spawn_item_drop(world, stack, (x, y))`, `pickup_item(world, entity)`).

## Tips for AI Agents

1. **Registry first** - register templates before creating `ItemStack`s; an unregistered `ItemId` falls back to `max_stack` 1.
2. **Equipment slots are abstract** - define slot constants (`const SLOT_WEAPON: usize = 0`) in game code; the engine prescribes no slot meanings.
3. **JSON loading order** - call `register_data_type` *before* `load_from_json`/`load_from_file`; the JSON `data_type` field maps to registered factory names.
4. **No notifications** - inventory operations return results; nothing writes messages. A game that wants one (e.g. to refresh UI) defines its own `#[derive(Message)]` type and writes it after the operation.
5. **Compact sparingly** - `Inventory::compact` rewrites all slots; call it on explicit user action (e.g. a "sort" button), not every frame.
6. **Stacks with `instance_data` never merge** - not in `add`, `add_partial`, `transfer` into a slot, or `compact`; plain items never top up a stack with data. Removing part of such a stack copies the data onto both parts.
7. **`add` is all-or-nothing** - on `InventoryFull { remainder }` nothing was placed, so callers (and `transfer`'s rollback) can treat `Err` as "no change". Use `add_partial` to place as many as fit; it returns the leftover stack.
