# AGENTS.md - Inventory & Item System

## Overview

Items use a hybrid value/entity model: lightweight `ItemStack` values inside inventories and equipment, full ECS entities (`Transform` + `ItemDrop`) only when dropped in the world.

## Tips for AI Agents

1. **Registry first** - register templates before creating `ItemStack`s; an unregistered `ItemId` falls back to `max_stack` 1.
2. **Equipment slots are abstract** - define slot constants (`const SLOT_WEAPON: usize = 0`) in game code; the engine prescribes no slot meanings.
3. **JSON loading order** - call `register_data_type` *before* `load_from_json`/`load_from_file`; the JSON `data_type` field maps to registered factory names.
4. **Events are not emitted** - `ItemPickedUp`, `InventoryChanged`, etc. live in `events/builtin.rs`, but no inventory code emits them. Game code emits after operations (e.g. for UI updates).
5. **Compact sparingly** - `Inventory::compact` rewrites all slots; call it on explicit user action (e.g. a "sort" button), not every frame.
6. **Stacks with `instance_data` never merge** - not in `add`, `add_partial`, `transfer` into a slot, or `compact`; plain items never top up a stack with data. Removing part of such a stack copies the data onto both parts.
7. **`add` is all-or-nothing** - on `InventoryFull { remainder }` nothing was placed, so callers (and `transfer`'s rollback) can treat `Err` as "no change". Use `add_partial` to place as many as fit; it returns the leftover stack.
