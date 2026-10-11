# AGENTS.md - Replication Module

## Design Decisions

- **No derive for `Replicated` (v1)** - Implemented by hand to avoid proc-macro complexity (`ryo2d-derive` only provides `NetworkInput`)
- **JSON serialization** - Chosen for simplicity and debuggability; binary (e.g. bincode) is a possible future optimization
- **Generation-based dirty tracking** - Changes are tagged with a generation, so each client only receives changes since its last acknowledgment
- **ListenServer optimization** - `ServerLike::is_host()` lets replication skip serializing for the host player
- **Spawns carry initial state** - `spawn_message()` serializes every registered component the entity has, for both spawn broadcasts and `send_full_state`, so a client (including a late joiner) never sees an entity without its state. Later changes use per-client per-component delivery (`DirtyTracker::pending_for_client` / `record_sent` / `record_ack`; an ack names a sent batch) in budgeted batches (`MAX_BATCH_BYTES`, never-sent components first), starting from the baseline `record_full_sync` sets in `send_full_state`. As a safety net, every `refresh_interval` broadcasts (`DEFAULT_REFRESH_INTERVAL` = 300, `set_refresh_interval(0)` disables) the server re-sends every replicated component's current value as `ComponentRefresh` messages spread over broadcasts in `MAX_REFRESH_BYTES` chunks, never acknowledged. Gotcha: renet silently drops an unreliable message larger than the remaining per-tick budget, so never send an unbounded replication message
- **Repeated spawns reuse the entity** - `spawn_from_message` updates an entity already mapped for that network id instead of spawning a second one (a full-state sync is followed by the spawn broadcast)
- **Full state is per entity** - `send_full_state` sends a `FullStateSync` marker (generation, no entities) plus one `SpawnEntity` per entity, keeping each message under WebRTC's 64 KB limit
- **Departed clients are forgotten** - `broadcast_updates` drops delivery state for clients the transport no longer lists

## Integration Requirements

- On client connect: `register_client(id)`, then `send_full_state(&world, &mut server, id)`. On disconnect: `remove_client(id)`
- `NetworkedValue::is_dirty()` does NOT notify the manager. You still have to call `replication.mark_dirty::<T>(entity)`
- Replication uses only `Channel::ReplicationReliable` / `ReplicationUnreliable`; game code owns the default channels
- Clients call `process_incoming` each update (it sends the acks); servers call `broadcast_updates` (it reads them)

## Notes for AI Agents

1. **Register identical components** - Server and client must register the same types
2. **Mark dirty after changes** - Call `mark_dirty::<T>()` after modifying replicated components
3. **Handle entity lookup failures** - `get_entity()` returns `Option`; the entity may not exist yet
4. **Authority matters** - Only the authority (server or the owning client) should modify components
5. **Call `broadcast_updates()` every frame** - It advances the generation even if nothing changed
