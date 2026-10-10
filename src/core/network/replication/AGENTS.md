# AGENTS.md - Replication Module

## Design Decisions

- **No derive for `Replicated` (v1)** - Implemented by hand to avoid proc-macro complexity (`ryo2d-derive` only provides `NetworkInput`)
- **JSON serialization** - Chosen for simplicity and debuggability; binary (e.g. bincode) is a possible future optimization
- **Generation-based dirty tracking** - Changes are tagged with a generation, so each client only receives changes since its last acknowledgment
- **ListenServer optimization** - `ServerLike::is_host()` lets replication skip serializing for the host player
- **Spawns carry initial state** - `spawn_message()` serializes every registered component the entity has, for both spawn broadcasts and `send_full_state`, so a client (including a late joiner) never sees an entity without its state. Later changes still go out as unreliable deltas: a lost delta isn't resent until that component changes again

## Integration Requirements

- On client connect: `register_client(id)`, then `send_full_state(&world, &mut server, id)`. On disconnect: `remove_client(id)`
- `NetworkedValue::is_dirty()` does NOT notify the manager. You still have to call `replication.mark_dirty::<T>(entity)`
- Client `process_incoming()` drains ALL `ReliableOrdered` and `Unreliable` messages and silently drops anything that isn't a `ReplicationMessage`. If the client also reads RPCs/raw messages on those channels, one side loses them

## Notes for AI Agents

1. **Register identical components** - Server and client must register the same types
2. **Mark dirty after changes** - Call `mark_dirty::<T>()` after modifying replicated components
3. **Handle entity lookup failures** - `get_entity()` returns `Option`; the entity may not exist yet
4. **Authority matters** - Only the authority (server or the owning client) should modify components
5. **Call `broadcast_updates()` every frame** - It advances the generation even if nothing changed
