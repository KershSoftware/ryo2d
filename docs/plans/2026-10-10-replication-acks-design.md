# Replication Acknowledgements - Design

## Overview

Make replicated component changes reliable without making them slow: clients acknowledge the change batches they apply, and the server keeps re-sending changes until they're acknowledged. Replication also moves onto its own channels so it never swallows game messages.

**Why:** component changes go out on the unreliable channel and the server marks a client up to date when it *sends* them; a lost packet is only corrected by the periodic full refresh (today every ~1 s). And replication shares the three default channels with game code: the client's `process_incoming` drains every message on the reliable-ordered and unreliable channels and silently drops anything that isn't replication, so a game that sends its own messages on them loses some.

**Owner decisions (2026-10-10):** replication gets its own channels; the periodic full refresh stays as a safety net but slows to ~5 s by default.

**Out of scope:** the listen-server host (local, no network, unchanged); RPCs (`rpc/`, their own path); bandwidth tuning beyond what's described; interpolation/prediction.

## Channels

- `Channel` gains `ReplicationReliable` and `ReplicationUnreliable`, documented as reserved for `ReplicationManager` (game messages sent on them may be consumed by replication).
- UDP (renet): channel IDs follow renet's defaults (`Unreliable` 0, `ReliableUnordered` 1, `ReliableOrdered` 2) plus `ReplicationReliable` 3 (`ReliableOrdered`, 300 ms resend, 5 MB) and `ReplicationUnreliable` 4 (`Unreliable`, 5 MB). One `connection_config()` builds this for both the server and client channel lists and is used by `NetworkServer::new` and `NetworkClient::new`, so peers agree. `impl From<Channel> for u8` replaces `impl From<Channel> for DefaultChannel` (which can't represent the new channels).
- WebRTC (matchbox): the sockets gain a second reliable and a second unreliable channel (indices 2 and 3); one mapping function replaces the duplicated `match channel { ... }` blocks.
- New `ClientLike` trait (`send(channel, data)`, `receive(channel)`), the client-side counterpart of `ServerLike`, implemented by `NetworkClient` and exported in the prelude. `ReplicationManager::process_incoming` takes any `ClientLike`, so existing calls with `&mut NetworkClient` keep compiling and tests can use a fake.

## Protocol

- Spawns, despawns and full-state syncs go on `ReplicationReliable`; change batches, refreshes and acks on `ReplicationUnreliable`.
- `ReplicationMessage` gains `ComponentRefresh(ComponentBatchMessage)` (the periodic full refresh, previously sent as `ComponentBatch`) and `Ack(AckMessage { generation: u64 })` (client → server).
- **Server:** at the start of `broadcast_updates`, it drains each remote client's `ReplicationUnreliable` messages and records the newest acknowledged generation (`DirtyTracker::record_ack`, keeping the maximum). Each client's change batch contains everything that changed since that client's last *acknowledged* generation — so an unacknowledged change is re-sent every broadcast until acknowledged — as one `ComponentBatch` message (renet delivers a large unreliable message whole or not at all, so a batch is never partially received). The server no longer marks a client up to date when it sends. A client that hasn't acknowledged anything yet receives every tracked change, as today.
- **Client:** applies a `ComponentBatch` only if its generation is at least the newest it has applied (a late, older batch can't roll values back; a newer batch already contains everything the older one did), then, once per `process_incoming` call that applied a newer batch, sends `Ack { generation }` for the newest. `ComponentRefresh` batches are applied under the same "not older" rule but are never acknowledged (a refresh must not hide a lost change batch).
- **Refresh:** unchanged mechanism, now sent as `ComponentRefresh`; `DEFAULT_REFRESH_INTERVAL` becomes 300 broadcasts (~5 s at 60 Hz); `set_refresh_interval(0)` still disables it.
- `ReplicationManager::client_acked_generation(client_id) -> Option<u64>` exposes the newest acknowledged generation (debugging, tests, network stats).

## Error Handling

Malformed messages on the replication channels are ignored (as today). An ack for a generation the server hasn't reached is clamped to the current generation. A client that never acknowledges keeps receiving all tracked changes each broadcast — bounded by the number of replicated components, the same order as a full refresh.

## Testing

**Unit (fake server/client):** changes are re-sent each broadcast until acknowledged; an ack stops the re-send and an older ack doesn't undo a newer one; the client applies the newest batch, ignores a stale one and acknowledges the newest; refreshes are applied but not acknowledged; replication on either side leaves game messages on the default channels untouched; channel IDs are distinct and match renet's defaults; the connection config carries the two extra channels both ways.

**End to end (real UDP):** messages on the replication channels arrive both ways; game messages on `Unreliable`/`ReliableOrdered` survive replication running on both sides; a `ReplicationManager` client acknowledges and the server records it; the existing replication e2e tests pass after the harness moves to the replication channels. The `matchbox` feature compiles (if its dependency is available offline; otherwise reported as unverified).

## Documentation

`ryo-network` skill (both copies, CRLF): reserved channels, acknowledged changes, refresh every ~5 s, `ClientLike`. `src/core/network/replication/AGENTS.md`: replace the "process_incoming drains game messages" gotcha with the channel rule; describe acks. `NETWORK_ARCHITECTURE.md` (repo root): channel list. Rustdoc on every new public item.

## Design Decisions

- **Dedicated channels:** acks need a client → server path the server can drain without stealing game messages; the same separation removes the existing client-side trap.
- **Re-send until acknowledged over reliable delivery:** reliable channels stall behind a lost packet; re-sending the *current* value of every unacknowledged change keeps latency low and never delivers stale intermediate states.
- **One batch per client per broadcast:** renet's whole-or-nothing delivery of large unreliable messages means acknowledging a generation is always safe.
- **Refreshes don't count as acks:** otherwise a refresh arriving after a lost change batch would mark that batch as delivered.
