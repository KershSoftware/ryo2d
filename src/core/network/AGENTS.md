# AGENTS.md - Network Module

Client-server networking on renet. For architecture, server types, channels and setup walkthroughs, see `NETWORK_ARCHITECTURE.md` (repo root) and the `ryo-network` skill.

Submodule docs:
- `transport/AGENTS.md` - UDP/WebRTC transports, signaling
- `replication/AGENTS.md` - Entity/component synchronization
- `rpc/AGENTS.md` - Remote procedure calls

## Design Decisions

### User-Managed Networking

Unlike audio (engine-managed), networking is **user-managed**:
- The game owns the server/client and calls `update()` in its own update loop
- The game decides when to use replication vs RPCs vs raw messages

### ListenServer Design

`ListenServer` wraps `NetworkServer` for player-hosted games:
- Host has a virtual client ID (`LOCAL_HOST_ID = 0`)
- Host messages are NOT networked - handle them directly in game logic
- `all_clients()` iterates host + remotes; `is_host()` identifies the local player

## Replication vs RPC vs NetworkAction

| Use | For | Examples |
|-----|-----|----------|
| Replication | Continuous state | Position, health, ammo |
| RPC | Player input (client → server), one-shot events | Move/jump input, chat, score, round start |
| NetworkAction | Discrete state change needing instant local feedback | Respawn |

Both networked examples send move/jump input as an RPC (`LeapInputRpc`, `PlayerInputRpc`). Rule of thumb: happens once, no interpolation → RPC; continuous → replication.

## Client-Side Prediction

- On `UpdateResult::StoredForReconciliation`, replay unacked inputs (`InputProcessor::reconcile`)
- Server-driven respawn/teleport: call `PredictionHelper::force_correction()` so the local entity takes the server position
- Client-initiated respawn: do NOT force a correction, because the server position is stale until it catches up (see leap-king `scenes/game.rs`)

## NetworkInput Required Fields

| Mark `#[network_input(required)]` | Leave optional |
|-----------------------------------|----------------|
| Direction/facing | Button press states |
| Player slot/ID | Magnitudes that can be 0 |
| Target position | Optional actions |
| State that affects others | Local-only data |

## Testing

- `tests/integration/{network,network_action,network_input,prediction}.rs` - `./run test:integration`
- WebRTC tests in `tests/integration/network.rs` need `--features matchbox`
- `tests/e2e/network.rs` - `cargo test --test e2e`

## Notes for AI Agents

1. **Collect clients before iterating** - `server.clients()` borrows the server
2. **Protocol IDs must match** - Mismatched IDs cause silent connection failures
3. **Use debug assertions** - `warn_if_not_dirty::<T>()` catches a missing `mark_dirty` (no-op in release)
4. **Use the NetworkInput builder** - Required fields prevent missing-data bugs
5. **Use NetworkAction for dual execution** - `execute_action_server` always applies on the server, ignoring `apply_locally()`
6. **Use `Predicted` for client-side prediction** - Handles spawn tracking and reconciliation
