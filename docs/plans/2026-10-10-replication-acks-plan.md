# Replication Acknowledgements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clients acknowledge the replication change batches they apply and the server re-sends changes until acknowledged; replication moves onto two dedicated channels so it never consumes game messages.

**Architecture:** The transport layer gains two reserved channels (renet IDs 3 and 4; matchbox indices 2 and 3) from one shared `connection_config()`, plus a `ClientLike` trait. `ReplicationManager` sends spawns/despawns/full syncs on `ReplicationReliable` and change batches, refreshes and acks on `ReplicationUnreliable`; the server records client acks (`DirtyTracker::record_ack`) and computes each client's batch from its last *acknowledged* generation.

**Tech Stack:** Rust 2024, renet 2.0, matchbox_socket (optional `matchbox` feature), serde_json.

**Spec:** `docs/plans/2026-10-10-replication-acks-design.md`

## Global Constraints

- Rust 2024, 2-space indentation, no `unwrap`/`expect` in library code (tests may).
- Format only lines you change; never plain-rustfmt a file declaring `mod x;` children (`src/lib.rs`, any `mod.rs` — including `src/core/network/mod.rs` and `src/core/network/replication/mod.rs`): use `rustfmt --edition 2024 --check` and fix only your hunks.
- Check `file <path>` before scripted edits: CRLF files (all skill files, some `.rs`) must stay CRLF on every line.
- Channel IDs: `Unreliable` 0, `ReliableUnordered` 1, `ReliableOrdered` 2 (renet defaults), `ReplicationReliable` 3, `ReplicationUnreliable` 4. Matchbox indices: reliable 0, unreliable 1, replication reliable 2, replication unreliable 3.
- `DEFAULT_REFRESH_INTERVAL = 300`.
- One commit per task with the given message; no co-author lines; don't push.
- Done = `cargo test --workspace` 0 failed; `cargo clippy --workspace --all-targets` no new warnings in touched files; `cargo check --features matchbox` passes (or is reported unverified if the dependency can't be fetched offline).

## Review Focus

1. A game that sends its own messages on `Unreliable`/`ReliableOrdered` while replication runs must receive every one of them, on both client and server — Task 2 `replication_leaves_game_messages_alone` (unit, both sides) and e2e `game_messages_survive_replication`.
2. A late, older change batch must not roll a value back — Task 2 `client_applies_the_newest_batch_and_acknowledges_it`.
3. A refresh arriving after a lost change batch must not stop that batch being re-sent — Task 2 `refreshes_are_applied_but_not_acknowledged`.
4. A client and server built from the same engine must agree on channels for both UDP and WebRTC — Task 1 `connection_config_carries_the_replication_channels_both_ways` and the shared `webrtc_channel` mapping.
5. A malicious or buggy ack for a future generation must not suppress changes the client never received — Task 2 `acks_never_go_backwards_or_past_the_present` (clamping).

---

### Task 1: Replication channels and `ClientLike`

**Files:**
- Modify: `src/core/network/mod.rs` (`Channel` ~228-247, `impl From<Channel> for DefaultChannel` ~249-257, `NetworkServer::new` ~374, `NetworkClient::new` ~685, `send`/`broadcast`/`receive` on `NetworkServer` ~450-517 and `NetworkClient` ~759-792, tests `channel_conversion` ~1076)
- Modify: `src/core/network/transport/webrtc.rs` (socket builders ~231, ~438, ~1289; client `receive_buffers: [VecDeque<Vec<u8>>; 2]` ~417 and its initialiser; doc comments "Channel 0 is reliable, channel 1 is unreliable")
- Modify: `src/lib.rs` (prelude network list ~146: add `ClientLike`)
- Test: `src/core/network/mod.rs` tests; `tests/e2e/network.rs`

**Interfaces:**
- Produces: `Channel::{ReplicationReliable, ReplicationUnreliable}`; `impl From<Channel> for u8`; `pub(crate) fn connection_config() -> ConnectionConfig`; `pub trait ClientLike { fn send(&mut self, channel: Channel, data: &[u8]); fn receive(&mut self, channel: Channel) -> Option<Vec<u8>>; }` implemented for `NetworkClient`.

- [ ] **Step 1: Write the failing tests**

In `src/core/network/mod.rs` tests, delete `channel_conversion` and add:

```rust
  const ALL_CHANNELS: [Channel; 5] = [
    Channel::Unreliable,
    Channel::ReliableUnordered,
    Channel::ReliableOrdered,
    Channel::ReplicationReliable,
    Channel::ReplicationUnreliable,
  ];

  #[test]
  fn channel_ids_are_distinct_and_match_renet_defaults() {
    assert_eq!(u8::from(Channel::Unreliable), u8::from(DefaultChannel::Unreliable));
    assert_eq!(
      u8::from(Channel::ReliableUnordered),
      u8::from(DefaultChannel::ReliableUnordered)
    );
    assert_eq!(
      u8::from(Channel::ReliableOrdered),
      u8::from(DefaultChannel::ReliableOrdered)
    );
    let ids: std::collections::HashSet<u8> = ALL_CHANNELS.into_iter().map(u8::from).collect();
    assert_eq!(ids.len(), ALL_CHANNELS.len());
  }

  #[test]
  fn connection_config_carries_the_replication_channels_both_ways() {
    let config = connection_config();
    for list in [&config.server_channels_config, &config.client_channels_config] {
      let mut ids: Vec<u8> = list.iter().map(|c| c.channel_id).collect();
      ids.sort_unstable();
      let mut expected: Vec<u8> = ALL_CHANNELS.into_iter().map(u8::from).collect();
      expected.sort_unstable();
      assert_eq!(ids, expected);
    }
  }
```

In `tests/e2e/network.rs` add `replication_channels_carry_messages_both_ways`: a real server and client (same connect-and-poll pattern as the nearest existing test, with `get_test_port()` and a fresh protocol id), then the client sends `b"up-r"` on `ReplicationReliable` and `b"up-u"` on `ReplicationUnreliable`, the server sends `b"down-r"`/`b"down-u"` to the client on the same two channels; poll `update` on both until all four arrive (3 s timeout) via `receive` on the matching channel; assert all four arrived.

- [ ] **Step 2: Run to watch them fail**

Run: `cargo test --lib network::tests::channel_ids 2>&1 | tail -5` and `cargo test --test e2e network::replication_channels 2>&1 | tail -5`
Expected: compile errors (`ReplicationReliable`, `connection_config` don't exist).

- [ ] **Step 3: Implement**

`Channel` gains (after `ReliableUnordered`):

```rust
  /// Reserved for [`ReplicationManager`](replication::ReplicationManager): spawns,
  /// despawns and full-state syncs. Don't send game messages on it.
  ReplicationReliable,

  /// Reserved for [`ReplicationManager`](replication::ReplicationManager): component
  /// changes, refreshes and acknowledgements. Don't send game messages on it.
  ReplicationUnreliable,
```

Replace `impl From<Channel> for DefaultChannel` with:

```rust
/// The renet channel ID for a `Channel` (renet's defaults plus the replication channels).
impl From<Channel> for u8 {
  fn from(channel: Channel) -> Self {
    match channel {
      Channel::Unreliable => DefaultChannel::Unreliable.into(),
      Channel::ReliableUnordered => DefaultChannel::ReliableUnordered.into(),
      Channel::ReliableOrdered => DefaultChannel::ReliableOrdered.into(),
      Channel::ReplicationReliable => 3,
      Channel::ReplicationUnreliable => 4,
    }
  }
}

/// The renet connection config every server and client uses: renet's three default
/// channels plus the two replication channels, in both directions.
pub(crate) fn connection_config() -> ConnectionConfig {
  let mut channels = DefaultChannel::config();
  channels.push(renet::ChannelConfig {
    channel_id: Channel::ReplicationReliable.into(),
    max_memory_usage_bytes: 5 * 1024 * 1024,
    send_type: renet::SendType::ReliableOrdered {
      resend_time: std::time::Duration::from_millis(300),
    },
  });
  channels.push(renet::ChannelConfig {
    channel_id: Channel::ReplicationUnreliable.into(),
    max_memory_usage_bytes: 5 * 1024 * 1024,
    send_type: renet::SendType::Unreliable,
  });
  ConnectionConfig {
    server_channels_config: channels.clone(),
    client_channels_config: channels,
    ..ConnectionConfig::default()
  }
}

/// The matchbox (WebRTC) channel index for a `Channel`.
#[cfg(feature = "matchbox")]
fn webrtc_channel(channel: Channel) -> usize {
  match channel {
    Channel::ReliableOrdered | Channel::ReliableUnordered => 0,
    Channel::Unreliable => 1,
    Channel::ReplicationReliable => 2,
    Channel::ReplicationUnreliable => 3,
  }
}
```

- `NetworkServer::new` and `NetworkClient::new`: `let connection_config = connection_config();`.
- Every UDP `send_message`/`broadcast_message`/`receive_message` call: pass `u8::from(channel)` instead of `DefaultChannel::from(channel)`.
- Every `#[cfg(feature = "matchbox")]` `let channel_idx = match channel { ... };` block: `let channel_idx = webrtc_channel(channel);`.
- `webrtc.rs`: each `WebRtcSocket::builder(...)` chain gains `.add_reliable_channel().add_unreliable_channel()` after the existing two (so indices 2/3 are replication reliable/unreliable), the client's `receive_buffers` becomes `[VecDeque<Vec<u8>>; 4]` (update its initialiser), and doc comments describing "Channel 0 is reliable, channel 1 is unreliable" list all four.
- Add, after `ServerLike` and its impls:

```rust
/// The client side of a connection - the counterpart of [`ServerLike`], so code like
/// [`ReplicationManager::process_incoming`](replication::ReplicationManager::process_incoming)
/// works with a [`NetworkClient`] or a test double.
pub trait ClientLike {
  /// Send a message to the server.
  fn send(&mut self, channel: Channel, data: &[u8]);

  /// Receive a message from the server, or `None` when none is waiting.
  fn receive(&mut self, channel: Channel) -> Option<Vec<u8>>;
}

impl ClientLike for NetworkClient {
  fn send(&mut self, channel: Channel, data: &[u8]) {
    NetworkClient::send(self, channel, data);
  }

  fn receive(&mut self, channel: Channel) -> Option<Vec<u8>> {
    NetworkClient::receive(self, channel)
  }
}
```

- `src/lib.rs` prelude: add `ClientLike` to the `crate::core::network::{...}` list (edit byte-preservingly; `rustfmt --check` only).

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib network:: 2>&1 | grep "test result"` and `cargo test --test e2e network:: 2>&1 | grep "test result"`
Expected: all pass, including the existing network e2e tests (replication still uses the default channels until Task 2).

- [ ] **Step 5: Check the WebRTC build, suite, lint, commit**

Run: `cargo check --features matchbox 2>&1 | tail -3` (if it fails only because the dependency can't be downloaded, report it unverified), `cargo test --workspace 2>&1 | grep -E "^test result|FAILED"`, `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/core/network/(mod|transport/webrtc)\.rs"`.

```bash
git add src/core/network/mod.rs src/core/network/transport/webrtc.rs src/lib.rs tests/e2e/network.rs
git commit -m "feat(network): add reserved replication channels and ClientLike"
```

---

### Task 2: Acknowledged replication on the dedicated channels

**Files:**
- Modify: `src/core/network/replication/messages.rs` (`ReplicationMessage` ~20-38; new `AckMessage`)
- Modify: `src/core/network/replication/mod.rs` (export `AckMessage`)
- Modify: `src/core/network/replication/dirty.rs` (`record_ack`, tests)
- Modify: `src/core/network/replication/manager.rs` (`DEFAULT_REFRESH_INTERVAL`, struct + constructors, `send_full_state`, `broadcast_updates`, `process_incoming`, `handle_message`, `client_acked_generation`, tests)
- Modify: `tests/e2e/network.rs` (`HeadlessClient::process_replication` channels; `ComponentRefresh` handling; new e2e tests)

**Interfaces:**
- Consumes (Task 1): `Channel::{ReplicationReliable, ReplicationUnreliable}`, `ClientLike`.
- Produces: `ReplicationMessage::{ComponentRefresh(ComponentBatchMessage), Ack(AckMessage)}`; `pub struct AckMessage { pub generation: u64 }`; `DirtyTracker::record_ack(&mut self, client_id: ClientId, generation: u64)`; `ReplicationManager::client_acked_generation(&self, client_id: ClientId) -> Option<u64>`; `process_incoming<C: ClientLike>(&mut self, world: &mut World, client: &mut C)`.

- [ ] **Step 1: Write the failing tests**

`dirty.rs` tests:

```rust
  #[test]
  fn acks_never_go_backwards_or_past_the_present() {
    let mut tracker = DirtyTracker::new();
    for _ in 0..10 {
      tracker.next_generation();
    }
    tracker.record_ack(1, 5);
    tracker.record_ack(1, 3);
    assert_eq!(tracker.client_generation(1), Some(5));
    tracker.record_ack(1, 999);
    assert_eq!(tracker.client_generation(1), Some(10));
  }
```

`manager.rs` tests — extend the existing `RecordingServer` with queued incoming messages, add a `FakeClient`, and switch helpers to the replication channel:

```rust
  // RecordingServer gains:
  //   incoming: HashMap<(ClientId, Channel), VecDeque<Vec<u8>>>,
  // and `receive` pops from it:
  //   self.incoming.get_mut(&(client_id, channel)).and_then(VecDeque::pop_front)
  impl RecordingServer {
    fn deliver(&mut self, client_id: ClientId, channel: Channel, data: Vec<u8>) {
      self.incoming.entry((client_id, channel)).or_default().push_back(data);
    }
  }

  /// A client that records what it sends and replays queued messages.
  #[derive(Default)]
  struct FakeClient {
    incoming: HashMap<Channel, VecDeque<Vec<u8>>>,
    sent: Vec<(Channel, Vec<u8>)>,
  }

  impl ClientLike for FakeClient {
    fn send(&mut self, channel: Channel, data: &[u8]) {
      self.sent.push((channel, data.to_vec()));
    }

    fn receive(&mut self, channel: Channel) -> Option<Vec<u8>> {
      self.incoming.get_mut(&channel).and_then(VecDeque::pop_front)
    }
  }

  impl FakeClient {
    fn deliver(&mut self, channel: Channel, msg: &ReplicationMessage) {
      self.incoming.entry(channel).or_default().push_back(msg.to_bytes().unwrap());
    }

    fn acks(&self) -> Vec<u64> {
      self
        .sent
        .iter()
        .filter(|(channel, _)| *channel == Channel::ReplicationUnreliable)
        .filter_map(|(_, bytes)| match ReplicationMessage::from_bytes(bytes) {
          Ok(ReplicationMessage::Ack(ack)) => Some(ack.generation),
          _ => None,
        })
        .collect()
    }
  }

  /// A batch carrying one `Health` value for `network_id`.
  fn health_batch(network_id: NetworkId, health: u32, generation: u64) -> ComponentBatchMessage {
    ComponentBatchMessage {
      updates: vec![ComponentUpdateMessage::new(
        network_id,
        SerializedComponent::from_component(&Health(health)).unwrap(),
      )],
      generation,
    }
  }
```

Update `sent_batches` to read `Channel::ReplicationUnreliable` and keep only `ComponentBatch`; add `sent_refreshes` (same, `ComponentRefresh`). Update the three existing refresh tests to read `sent_refreshes` and to call `server.set_refresh_interval(60)` explicitly where they relied on the old default of 60.

New tests (`server_with_health()` already exists; mark the component dirty with `server.mark_dirty::<Health>(entity)` — check the real method name):

```rust
  #[test]
  fn unacknowledged_changes_are_resent_each_broadcast() {
    let (mut server, world, network_id) = server_with_health();
    let entity = server.get_entity(network_id).unwrap();
    let mut net = RecordingServer { clients: vec![1], ..Default::default() };
    server.register_client(1);
    server.mark_dirty::<Health>(entity);

    for _ in 0..3 {
      server.broadcast_updates(&world, &mut net);
    }

    assert_eq!(sent_health(&net), vec![Health(42); 3]);
  }

  #[test]
  fn an_ack_stops_the_resend() {
    let (mut server, world, network_id) = server_with_health();
    let entity = server.get_entity(network_id).unwrap();
    let mut net = RecordingServer { clients: vec![1], ..Default::default() };
    server.register_client(1);
    server.mark_dirty::<Health>(entity);
    server.broadcast_updates(&world, &mut net); // generation 0
    let ack = ReplicationMessage::Ack(AckMessage { generation: 0 }).to_bytes().unwrap();
    net.deliver(1, Channel::ReplicationUnreliable, ack);

    server.broadcast_updates(&world, &mut net);

    assert_eq!(sent_health(&net).len(), 1);
    assert_eq!(server.client_acked_generation(1), Some(0));
  }

  #[test]
  fn client_applies_the_newest_batch_and_acknowledges_it() {
    let (server, world, network_id) = server_with_health();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(Channel::ReplicationReliable, &ReplicationMessage::SpawnEntity(spawn));
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 50, 5)),
    );
    // A late, older batch must not roll the value back
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 30, 3)),
    );

    client.process_incoming(&mut client_world, &mut net);

    let entity = client.get_entity(network_id).unwrap();
    assert_eq!(client_world.get::<Health>(entity).map(|h| (*h).clone()), Some(Health(50)));
    assert_eq!(net.acks(), vec![5]);
  }

  #[test]
  fn refreshes_are_applied_but_not_acknowledged() {
    let (server, world, network_id) = server_with_health();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(Channel::ReplicationReliable, &ReplicationMessage::SpawnEntity(spawn));
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentRefresh(health_batch(network_id, 70, 7)),
    );

    client.process_incoming(&mut client_world, &mut net);

    let entity = client.get_entity(network_id).unwrap();
    assert_eq!(client_world.get::<Health>(entity).map(|h| (*h).clone()), Some(Health(70)));
    assert!(net.acks().is_empty());
  }

  #[test]
  fn replication_leaves_game_messages_alone() {
    // Client side
    let mut client = ReplicationManager::new_client();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.incoming.entry(Channel::Unreliable).or_default().push_back(b"game".to_vec());
    net.incoming.entry(Channel::ReliableOrdered).or_default().push_back(b"chat".to_vec());
    client.process_incoming(&mut client_world, &mut net);
    assert_eq!(ClientLike::receive(&mut net, Channel::Unreliable), Some(b"game".to_vec()));
    assert_eq!(ClientLike::receive(&mut net, Channel::ReliableOrdered), Some(b"chat".to_vec()));

    // Server side
    let (mut server, world, _) = server_with_health();
    let mut recording = RecordingServer { clients: vec![1], ..Default::default() };
    server.register_client(1);
    recording.deliver(1, Channel::Unreliable, b"input".to_vec());
    server.broadcast_updates(&world, &mut recording);
    assert_eq!(recording.receive(1, Channel::Unreliable), Some(b"input".to_vec()));
  }
```

`tests/e2e/network.rs`:
- `HeadlessClient::process_replication` reads `Channel::ReplicationReliable` then `Channel::ReplicationUnreliable`, and its message handler treats `ComponentRefresh` like `ComponentBatch` (the harness doesn't acknowledge; the server simply keeps re-sending, which the existing tests tolerate).
- `game_messages_survive_replication`: with the existing `HeadlessServer`/`HeadlessClient` connected and replication running each loop iteration (server `process_events` + `broadcast_updates`, client `process_replication`), the server sends `b"to-client"` on `Channel::Unreliable` and `b"chat"` on `Channel::ReliableOrdered` to the client, and the client sends `b"to-server"` on `Channel::Unreliable`; inside the loop, drain the game channels via the raw `client.client.receive(...)` / `server.server.receive(client_id, ...)` into flags; assert all three arrive within 3 s.
- `replication_manager_client_acknowledges_changes`: a `HeadlessServer` and a raw `NetworkClient` paired with `ReplicationManager::new_client()` (register the same components as the harness) and its own `World`; loop: server `process_events` + `broadcast_updates`, client `update` + `replication.process_incoming(&mut world, &mut net_client)`, until `server.replication.client_acked_generation(client_id)` is `Some` (3 s); assert it is, and that the client world contains the client's player entity with a `PlayerPosition`.

- [ ] **Step 2: Run to watch them fail**

Run: `cargo test --lib replication:: 2>&1 | tail -15`
Expected: compile errors (`AckMessage`, `record_ack`, `ComponentRefresh`, `client_acked_generation` don't exist, `process_incoming` isn't generic).

- [ ] **Step 3: Implement**

`messages.rs` — append to `ReplicationMessage`:

```rust
  /// Periodic full refresh of every replicated component. Applied like a change
  /// batch but never acknowledged.
  ComponentRefresh(ComponentBatchMessage),

  /// Client to server: the newest change batch generation the client has applied
  Ack(AckMessage),
```

and add:

```rust
/// Client to server acknowledgement of applied change batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckMessage {
  /// Generation of the newest change batch the client has applied
  pub generation: u64,
}
```

Export `AckMessage` from `replication/mod.rs` (alongside the other message types).

`dirty.rs`:

```rust
  /// Record that a client applied every change up to `generation`.
  ///
  /// Keeps the newest acknowledgement, and clamps one beyond the current
  /// generation (a client can't have seen changes that weren't sent yet).
  pub fn record_ack(&mut self, client_id: ClientId, generation: u64) {
    let generation = generation.min(self.generation);
    let acked = self.client_generations.entry(client_id).or_insert(generation);
    *acked = (*acked).max(generation);
  }
```

`manager.rs`:
- `DEFAULT_REFRESH_INTERVAL: u64 = 300;` with doc "about five seconds at 60 updates per second - a safety net, as clients acknowledge changes".
- Field `/// Client: generation of the newest change batch applied` `newest_batch_generation: Option<u64>,` (both constructors: `None`).
- `pub fn client_acked_generation(&self, client_id: ClientId) -> Option<u64> { self.dirty_tracker.client_generation(client_id) }` with a one-line doc.
- `send_full_state`, spawn and despawn sends: `Channel::ReliableOrdered` → `Channel::ReplicationReliable`.
- `broadcast_updates`: right after computing `remote_clients`, drain acks:

```rust
    // Record what each client has applied before deciding what to re-send
    for &client_id in &remote_clients {
      while let Some(data) = server.receive(client_id, Channel::ReplicationUnreliable) {
        if let Ok(ReplicationMessage::Ack(ack)) = ReplicationMessage::from_bytes(&data) {
          self.dirty_tracker.record_ack(client_id, ack.generation);
        }
      }
    }
```

  The per-client change batch goes on `Channel::ReplicationUnreliable`, and **remove** `self.dirty_tracker.acknowledge_client(client_id);` (acknowledgement now comes from the client). The refresh sends `ReplicationMessage::ComponentRefresh(...)` on `Channel::ReplicationUnreliable`. Update the method's rustdoc to describe acks.
- `process_incoming`:

```rust
  /// Process incoming replication messages (client), then acknowledge the newest
  /// change batch applied so the server stops re-sending it.
  pub fn process_incoming<C: ClientLike>(&mut self, world: &mut World, client: &mut C) {
    while let Some(data) = client.receive(Channel::ReplicationReliable) {
      if let Ok(msg) = ReplicationMessage::from_bytes(&data) {
        self.handle_message(world, msg);
      }
    }

    let newest_before = self.newest_batch_generation;
    while let Some(data) = client.receive(Channel::ReplicationUnreliable) {
      if let Ok(msg) = ReplicationMessage::from_bytes(&data) {
        self.handle_message(world, msg);
      }
    }

    if let Some(generation) = self.newest_batch_generation
      && Some(generation) != newest_before
      && let Ok(bytes) = ReplicationMessage::Ack(AckMessage { generation }).to_bytes()
    {
      client.send(Channel::ReplicationUnreliable, &bytes);
    }
  }
```

- `handle_message`:

```rust
      ReplicationMessage::ComponentBatch(batch) => {
        if self.is_current(batch.generation) {
          for update in &batch.updates {
            self.apply_component_update(world, update);
          }
          self.newest_batch_generation = Some(batch.generation);
        }
      }
      ReplicationMessage::ComponentRefresh(batch) => {
        if self.is_current(batch.generation) {
          for update in &batch.updates {
            self.apply_component_update(world, update);
          }
        }
      }
      ReplicationMessage::Ack(_) => {
        // Client-to-server only; the server reads acks in broadcast_updates
      }
```

  and

```rust
  /// Whether a batch is at least as new as the newest change batch applied.
  fn is_current(&self, generation: u64) -> bool {
    self
      .newest_batch_generation
      .is_none_or(|newest| generation >= newest)
  }
```

- `use` the new types (`ClientLike`, `AckMessage`).

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib network:: 2>&1 | grep -E "test result|FAILED"` and `cargo test --test e2e network:: 2>&1 | grep -E "test result|FAILED"` (run the e2e network tests 20 times in a loop afterwards to check for flakiness: `for i in $(seq 1 20); do cargo test -q --test e2e network:: || echo FAIL; done | grep -c FAIL` → 0).

- [ ] **Step 5: Suite, lint, WebRTC check, commit**

As Task 1 Step 5.

```bash
git add src/core/network/replication tests/e2e/network.rs
git commit -m "feat(network): acknowledge replicated changes and re-send until acknowledged" -m "- Replication uses its own channels, so it no longer consumes game messages
- The periodic full refresh drops to every ~5 seconds as a safety net"
```

---

### Task 3: Documentation

**Files:**
- Modify: `src/cli/skills/ryo-network.md`, `.claude/skills/ryo-network/SKILL.md` (CRLF, identical edits)
- Modify: `src/core/network/replication/AGENTS.md`
- Modify: `NETWORK_ARCHITECTURE.md` (repo root; check line endings)

- [ ] **Step 1: ryo-network skill (both copies)**
  - Channels table: add `ReplicationReliable` / `ReplicationUnreliable` as "reserved for replication - don't send game messages on them".
  - Replication section: changes are sent until the client acknowledges them (`process_incoming` acknowledges automatically); a full refresh runs every 300 `broadcast_updates()` calls (~5 s) as a safety net (`set_refresh_interval`); `process_incoming` takes any `ClientLike` (e.g. `NetworkClient`).
  - Remove any sentence saying a lost change isn't re-sent.
- [ ] **Step 2: replication AGENTS.md**
  - Replace the "Client `process_incoming()` drains ALL `ReliableOrdered` and `Unreliable` messages..." gotcha with: replication uses `Channel::ReplicationReliable`/`ReplicationUnreliable` only; game code owns the default channels.
  - Update the "Spawns carry initial state" bullet's last sentence: changes are re-sent until acknowledged (`DirtyTracker::record_ack`), refreshes (`ComponentRefresh`, never acknowledged) every `DEFAULT_REFRESH_INTERVAL` = 300 broadcasts.
  - Add to Integration Requirements: clients call `process_incoming` each update (it sends the acks); servers call `broadcast_updates` (it reads them).
- [ ] **Step 3: NETWORK_ARCHITECTURE.md** — add the two replication channels wherever channels are listed, and one line on acknowledged replication.
- [ ] **Step 4: Verify and commit**

Run: `file` on both skill copies (CRLF), `cmp` them, `git diff --stat` (small diffs).

```bash
git add src/cli/skills/ryo-network.md .claude/skills/ryo-network/SKILL.md src/core/network/replication/AGENTS.md NETWORK_ARCHITECTURE.md
git commit -m "docs: describe acknowledged replication and its reserved channels"
```

---

## Addendum: budgeted batches with per-component delivery (owner decision after final review)

See the spec's "Revision after final review". Tasks 1-3 are done (commits `71ec064`, `ba20d43`, `182c790`, `c052f83`).

### Addendum Global Constraints

- `MAX_BATCH_BYTES = 16 * 1024` (encoded bytes of the updates in one change batch), `MAX_REFRESH_BYTES = 8 * 1024` (per refresh message), `MAX_IN_FLIGHT_BATCHES = 128` (per client). `REFRESH_BATCH_SIZE` is removed.
- Sizes are measured exactly with `encoded_len` (serde_json into a byte counter, no allocation) — never estimated.
- A batch always carries at least one update, even one larger than the budget.
- Acks name a batch generation; only a batch the server recorded as sent and still in flight counts.
- Everything else from the Global Constraints above still applies (formatting, CRLF, no unwrap in library code, one commit per task, no trailer lines in commit messages).

### Addendum Review Focus

1. A late joiner to a world with a long change history must not be sent that history — Task 4 `a_late_joiner_starts_from_its_full_state_sync`.
2. A backlog far over renet's per-tick budget must reach a real UDP client — Task 4 e2e `a_large_backlog_reaches_a_client_over_udp`.
3. With a backlog and no acks, every pending component is sent once before any is sent again (no starvation) — Task 4 `every_pending_component_is_sent_before_any_is_resent`.
4. A forged or future ack must not suppress anything — Task 4 `acks_for_batches_never_sent_are_ignored`.
5. A despawned entity's delivery state must not leak or suppress a later change — Task 4 `despawning_forgets_delivery_state`.

---

### Task 4: Per-component delivery tracking and budgeted change batches

**Files:**
- Modify: `src/core/network/replication/dirty.rs` (per-client delivery state replaces `client_generations`; new `PendingComponent`, `pending_for_client`, `record_sent`, `record_full_sync`, `MAX_IN_FLIGHT_BATCHES`; `record_ack` re-defined; `register_client`, `remove_client`, `acknowledge_client`, `client_generation`, `get_dirty_for_client`, `has_dirty_for_client`, `mark_despawned`, `clear_entity` adapted; struct rustdoc example)
- Modify: `src/core/network/replication/messages.rs` (`pub(crate) fn encoded_len`)
- Modify: `src/core/network/replication/manager.rs` (`MAX_BATCH_BYTES`; `send_full_state` takes `&mut self`; budgeted per-client batch in `broadcast_updates`; `collect_dirty_updates` via `pending_for_client`; client `pending_acks` replaces `newest_complete`; `FullStateSync` sets `newest_applied = Some(sync.generation)`; `spawn_from_message` maps the entity first; `set_refresh_interval` rustdoc)
- Modify: `src/core/network/replication/mod.rs` (export `PendingComponent`, `MAX_BATCH_BYTES`, `MAX_IN_FLIGHT_BATCHES` alongside existing exports — check how `DEFAULT_REFRESH_INTERVAL` is exported and follow it)
- Modify: `tests/e2e/network.rs` (new e2e test), `tests/integration/network.rs` (only if `send_full_state(&mut self)` needs a `mut` binding)

**Interfaces:**
- Produces: `pub struct PendingComponent { pub network_id: NetworkId, pub component_id: ComponentTypeId, pub change: u64 }`; `DirtyTracker::{pending_for_client(&self, ClientId) -> Vec<PendingComponent>, record_sent(&mut self, ClientId, Vec<PendingComponent>), record_full_sync(&mut self, ClientId), record_ack(&mut self, ClientId, u64)}`; `pub const MAX_BATCH_BYTES: usize`; `pub const MAX_IN_FLIGHT_BATCHES: usize`; `pub(crate) fn encoded_len<T: Serialize>(&T) -> usize` (messages.rs); `ReplicationManager::send_full_state(&mut self, ...)`.

- [ ] **Step 1: Write the failing tests**

`dirty.rs` tests — delete `acks_never_go_backwards_or_past_the_present`, add:

```rust
  fn pending_ids(tracker: &DirtyTracker, client: ClientId) -> Vec<(NetworkId, ComponentTypeId)> {
    tracker
      .pending_for_client(client)
      .iter()
      .map(|p| (p.network_id, p.component_id))
      .collect()
  }

  #[test]
  fn an_ack_delivers_exactly_the_batch_it_names() {
    let mut tracker = DirtyTracker::new();
    let (a, b) = (NetworkId::from_raw(1), NetworkId::from_raw(2));
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent); // batch 0 carries a
    tracker.next_generation();
    tracker.mark_dirty(b, health);
    let sent = tracker.pending_for_client(1);
    assert_eq!(sent.first().map(|p| p.network_id), Some(b)); // never sent goes first
    tracker.record_sent(1, sent); // batch 1 carries b and a
    tracker.next_generation();

    tracker.record_ack(1, 0);

    assert_eq!(pending_ids(&tracker, 1), vec![(b, health)]);
    assert_eq!(tracker.client_generation(1), Some(0));
  }

  #[test]
  fn acks_for_batches_never_sent_are_ignored() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);

    tracker.record_ack(1, 0);
    tracker.record_ack(1, u64::MAX);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
    assert_eq!(tracker.client_generation(1), None);
  }

  #[test]
  fn a_newer_change_is_pending_after_an_older_one_was_acknowledged() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent);
    tracker.next_generation();
    tracker.record_ack(1, 0);
    assert!(pending_ids(&tracker, 1).is_empty());

    tracker.mark_dirty(a, health);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
  }

  #[test]
  fn a_full_sync_covers_changes_before_it_but_not_during_it() {
    let mut tracker = DirtyTracker::new();
    let (a, b) = (NetworkId::from_raw(1), NetworkId::from_raw(2));
    let health = make_component_id("Health");
    tracker.mark_dirty(a, health); // generation 0
    tracker.next_generation();
    tracker.register_client(1);
    tracker.record_full_sync(1); // built during generation 1
    tracker.mark_dirty(b, health); // generation 1, possibly after the sync was built

    assert_eq!(pending_ids(&tracker, 1), vec![(b, health)]);
  }

  #[test]
  fn registering_again_keeps_a_clients_sync() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.mark_dirty(a, health);
    tracker.next_generation();
    tracker.register_client(1);
    tracker.record_full_sync(1);

    tracker.register_client(1);

    assert!(pending_ids(&tracker, 1).is_empty());
  }

  #[test]
  fn never_sent_components_go_before_resends() {
    let mut tracker = DirtyTracker::new();
    let (a, b) = (NetworkId::from_raw(1), NetworkId::from_raw(2));
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    tracker.mark_dirty(b, health);
    let only_a: Vec<PendingComponent> = tracker
      .pending_for_client(1)
      .into_iter()
      .filter(|p| p.network_id == a)
      .collect();
    tracker.record_sent(1, only_a);
    tracker.next_generation();

    assert_eq!(pending_ids(&tracker, 1), vec![(b, health), (a, health)]);
  }

  #[test]
  fn despawning_forgets_delivery_state() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent);
    tracker.record_ack(1, 0);
    tracker.mark_despawned(a);

    // The same id and generation again must not look delivered
    tracker.mark_dirty(a, health);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
  }

  #[test]
  fn only_the_newest_batches_are_remembered() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    for _ in 0..=MAX_IN_FLIGHT_BATCHES {
      tracker.mark_dirty(a, health);
      let sent = tracker.pending_for_client(1);
      tracker.record_sent(1, sent);
      tracker.next_generation();
    }

    tracker.record_ack(1, 0); // evicted
    assert_eq!(tracker.client_generation(1), None);
    tracker.record_ack(1, MAX_IN_FLIGHT_BATCHES as u64);
    assert_eq!(tracker.client_generation(1), Some(MAX_IN_FLIGHT_BATCHES as u64));
  }
```

`manager.rs` tests — add a large test component and helpers next to `Health`:

```rust
  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  struct Label(String);

  impl Replicated for Label {
    fn component_id() -> ComponentTypeId {
      ComponentTypeId::from_name("Label")
    }

    fn component_name() -> &'static str {
      "Label"
    }
  }

  /// A server whose `count` networked entities each carry a ~200-byte `Label`,
  /// all marked dirty, with client 1 registered (no full-state sync).
  fn server_with_backlog(count: usize) -> (ReplicationManager, World, RecordingServer) {
    let mut server = ReplicationManager::new_server();
    server.register::<Label>();
    server.set_refresh_interval(0);
    let mut world = World::new();
    for i in 0..count {
      let entity = server.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());
      world.insert_one(entity, Label(format!("{i:0>200}"))).unwrap();
      server.mark_dirty::<Label>(entity);
    }
    let net = RecordingServer { clients: vec![1], ..Default::default() };
    server.register_client(1);
    (server, world, net)
  }

  /// Network ids in the batch sent by the most recent broadcast, if any.
  fn last_batch_ids(net: &RecordingServer, batches_before: usize) -> Option<Vec<NetworkId>> {
    let batches = sent_batches(net);
    (batches.len() > batches_before)
      .then(|| batches.last().unwrap().updates.iter().map(|u| u.network_id).collect())
  }
```

and these tests:

```rust
  #[test]
  fn a_late_joiner_starts_from_its_full_state_sync() {
    let (mut server, world, network_id) = server_with_health();
    let entity = server.get_entity(network_id).unwrap();
    server.mark_dirty::<Health>(entity);
    let mut net = RecordingServer::default();
    server.broadcast_updates(&world, &mut net); // nobody connected yet
    net.clients = vec![1];
    server.register_client(1);
    server.send_full_state(&world, &mut net, 1);

    server.broadcast_updates(&world, &mut net);

    assert!(sent_batches(&net).is_empty());
  }

  #[test]
  fn a_large_backlog_goes_out_in_budgeted_batches() {
    let (mut server, world, mut net) = server_with_backlog(200);
    let mut seen = std::collections::HashSet::new();

    for _ in 0..50 {
      let before = sent_batches(&net).len();
      server.broadcast_updates(&world, &mut net);
      let Some(ids) = last_batch_ids(&net, before) else {
        break;
      };
      let (_, _, bytes) = net.sent.last().unwrap();
      assert!(ids.len() < 200);
      assert!(bytes.len() <= MAX_BATCH_BYTES + 256, "batch of {} bytes", bytes.len());
      seen.extend(ids);
      let generation = sent_batches(&net).last().unwrap().generation;
      let ack = ReplicationMessage::Ack(AckMessage { generation }).to_bytes().unwrap();
      net.deliver(1, Channel::ReplicationUnreliable, ack);
    }

    assert_eq!(seen.len(), 200);
    let before = sent_batches(&net).len();
    server.broadcast_updates(&world, &mut net);
    assert_eq!(sent_batches(&net).len(), before, "nothing left once all are acknowledged");
  }

  #[test]
  fn every_pending_component_is_sent_before_any_is_resent() {
    let (mut server, world, mut net) = server_with_backlog(200);
    let mut seen = std::collections::HashSet::new();

    while seen.len() < 200 {
      let before = sent_batches(&net).len();
      server.broadcast_updates(&world, &mut net);
      let ids = last_batch_ids(&net, before).expect("a batch every broadcast while pending");
      for id in ids {
        assert!(seen.insert(id), "{id:?} re-sent before every component was sent once");
      }
    }
  }

  #[test]
  fn client_acknowledges_every_complete_batch() {
    let (server, world, network_id) = server_with_health();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(Channel::ReplicationReliable, &ReplicationMessage::SpawnEntity(spawn));
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 50, 5)),
    );
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 60, 6)),
    );

    client.process_incoming(&mut client_world, &mut net);

    assert_eq!(net.acks(), vec![5, 6]);
  }

  #[test]
  fn batches_older_than_the_full_state_sync_are_ignored() {
    let (mut server, world, network_id) = server_with_health();
    for _ in 0..10 {
      server.dirty_tracker_mut().next_generation();
    }
    let sync = server.build_full_state_sync(&world); // generation 10
    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(Channel::ReplicationReliable, &ReplicationMessage::FullStateSync(sync));
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 9, 9)),
    );
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 10, 10)),
    );

    client.process_incoming(&mut client_world, &mut net);

    let entity = client.get_entity(network_id).unwrap();
    assert_eq!(client_world.get::<Health>(entity).map(|h| (*h).clone()), Some(Health(10)));
    assert_eq!(net.acks(), vec![10]);
  }

  #[test]
  fn a_spawn_with_an_unregistered_component_still_maps_the_entity() {
    let (server, world, network_id) = server_with_health();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    let mut client = ReplicationManager::new_client(); // Health not registered
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(Channel::ReplicationReliable, &ReplicationMessage::SpawnEntity(spawn));
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 50, 5)),
    );

    client.process_incoming(&mut client_world, &mut net);

    assert!(client.get_entity(network_id).is_some());
    assert_eq!(net.acks(), vec![5]);
  }
```

`tests/e2e/network.rs` — `a_large_backlog_reaches_a_client_over_udp`, modelled on `replication_manager_client_acknowledges_changes` (a `HeadlessServer`, a raw `NetworkClient`, a `ReplicationManager::new_client()` registering `Transform` and `PlayerPosition`, its own `World`, fresh port and protocol id):
- `server.replication.set_refresh_interval(0)`; before the client connects, spawn 1500 networked entities on the server (`server.replication.spawn_networked(&mut server.world, NetworkEntityBuilder::new().server_owned().with_component(PlayerPosition { x: 0.0, y: 0.0 }))` — check the builder's real method names) and keep their entities.
- Loop (server `update` + `process_events` + `broadcast_updates`; client `update` + `replication.process_incoming`; 5 ms sleep) until the client's `replication` knows every one of the 1500 network ids (timeout 5 s, assert).
- Then set every one's `PlayerPosition` to `{ x: i as f32, y: 1.0 }` on the server and `server.replication.mark_dirty::<PlayerPosition>(entity)` once.
- Loop the same way until every one of the 1500 has `y == 1.0` in the client world (timeout 5 s); assert it converged, naming how many were still stale on failure.

- [ ] **Step 2: Run to watch them fail**

Run: `cargo test --lib replication:: 2>&1 | tail -15` — compile errors (`pending_for_client`, `record_sent`, `record_full_sync`, `PendingComponent`, `MAX_IN_FLIGHT_BATCHES`, `MAX_BATCH_BYTES` don't exist).
Then stub nothing: implement Step 3 for `dirty.rs` only first if you want the e2e RED on the old manager — run `cargo test --test e2e a_large_backlog 2>&1 | tail -5` on the base commit's manager (record that it times out; if you can't isolate it, say so in the report).

- [ ] **Step 3: Implement**

`messages.rs`, next to `to_bytes`:

```rust
/// The length of a value's wire encoding (as [`ReplicationMessage::to_bytes`]
/// writes it), measured without allocating.
pub(crate) fn encoded_len<T: Serialize>(value: &T) -> usize {
  struct Counter(usize);

  impl std::io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
      self.0 += buf.len();
      Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
      Ok(())
    }
  }

  let mut counter = Counter(0);
  let _ = serde_json::to_writer(&mut counter, value);
  counter.0
}
```

`dirty.rs`:

```rust
/// Most unacknowledged change batches remembered per client. An ack for an
/// older batch is ignored; its components stay pending and are sent again.
pub const MAX_IN_FLIGHT_BATCHES: usize = 128;

/// A component change a client hasn't acknowledged yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingComponent {
  /// The entity
  pub network_id: NetworkId,
  /// The component type
  pub component_id: ComponentTypeId,
  /// Generation of the component's newest change
  pub change: u64,
}

/// One component's delivery state for one client.
#[derive(Debug, Default, Clone, Copy)]
struct ComponentDelivery {
  /// Newest change generation the client acknowledged
  acked: Option<u64>,
  /// Generation of the broadcast that last sent it
  last_sent: Option<u64>,
}

/// A change batch sent to a client and not acknowledged yet.
#[derive(Debug)]
struct SentBatch {
  generation: u64,
  components: Vec<PendingComponent>,
}

/// What one client has been sent and has acknowledged.
#[derive(Debug, Default)]
struct ClientDelivery {
  /// Every change at or before this generation already reached the client
  /// (its full-state sync, or `acknowledge_client`)
  baseline: Option<u64>,
  components: HashMap<NetworkId, HashMap<ComponentTypeId, ComponentDelivery>>,
  in_flight: VecDeque<SentBatch>,
  newest_ack: Option<u64>,
}
```

Replace the `client_generations` field with `clients: HashMap<ClientId, ClientDelivery>` and:

```rust
  /// Register a new client. Until its full-state sync is recorded it is sent
  /// every tracked change. Registering an existing client keeps its state.
  pub fn register_client(&mut self, client_id: ClientId) {
    self.clients.entry(client_id).or_default();
  }

  /// Remove a disconnected client.
  pub fn remove_client(&mut self, client_id: ClientId) {
    self.clients.remove(&client_id);
  }

  /// The newest change batch generation the client acknowledged (or the
  /// generation [`acknowledge_client`](Self::acknowledge_client) marked it at).
  pub fn client_generation(&self, client_id: ClientId) -> Option<u64> {
    self.clients.get(&client_id).and_then(|client| client.newest_ack)
  }

  /// Treat a client as having every change marked so far, for state sent by
  /// other means. Replication tracks delivery itself from acknowledgements;
  /// calling this stops changes the client may not have from being re-sent.
  pub fn acknowledge_client(&mut self, client_id: ClientId) {
    let newest_ack = self.client_generation(client_id).max(Some(self.generation));
    self.clients.insert(
      client_id,
      ClientDelivery {
        baseline: Some(self.generation),
        newest_ack,
        ..Default::default()
      },
    );
  }

  /// Record that a client was sent a full-state sync built in the current
  /// generation: changes from earlier generations are covered; changes in
  /// this one may have been marked after the sync was built, so stay pending.
  pub fn record_full_sync(&mut self, client_id: ClientId) {
    let newest_ack = self.client_generation(client_id);
    self.clients.insert(
      client_id,
      ClientDelivery {
        baseline: self.generation.checked_sub(1),
        newest_ack,
        ..Default::default()
      },
    );
  }

  /// Components whose newest change the client hasn't acknowledged: never
  /// sent first, then least recently sent, then oldest change - so a backlog
  /// sent in budgeted batches reaches every component before re-sending any.
  pub fn pending_for_client(&self, client_id: ClientId) -> Vec<PendingComponent> {
    let client = self.clients.get(&client_id);
    let baseline = client.and_then(|client| client.baseline);
    let mut pending = Vec::new();
    for (&network_id, components) in &self.dirty_components {
      let delivered = client.and_then(|client| client.components.get(&network_id));
      for (&component_id, &change) in components {
        let state = delivered
          .and_then(|delivered| delivered.get(&component_id))
          .copied()
          .unwrap_or_default();
        if baseline.max(state.acked).is_none_or(|covered| change > covered) {
          pending.push((
            state.last_sent,
            PendingComponent {
              network_id,
              component_id,
              change,
            },
          ));
        }
      }
    }
    pending.sort_unstable_by_key(|(last_sent, p)| {
      (*last_sent, p.change, p.network_id.0, p.component_id.0)
    });
    pending.into_iter().map(|(_, p)| p).collect()
  }

  /// Record a change batch sent to a client in the current generation.
  pub fn record_sent(&mut self, client_id: ClientId, components: Vec<PendingComponent>) {
    let generation = self.generation;
    let client = self.clients.entry(client_id).or_default();
    for p in &components {
      client
        .components
        .entry(p.network_id)
        .or_default()
        .entry(p.component_id)
        .or_default()
        .last_sent = Some(generation);
    }
    client.in_flight.push_back(SentBatch { generation, components });
    while client.in_flight.len() > MAX_IN_FLIGHT_BATCHES {
      client.in_flight.pop_front();
    }
  }

  /// Record that a client applied the change batch sent in `generation`: the
  /// changes it carried are delivered. Acks for a batch that isn't in flight
  /// (never sent, already acknowledged, evicted, or from the future) are
  /// ignored.
  pub fn record_ack(&mut self, client_id: ClientId, generation: u64) {
    let Some(client) = self.clients.get_mut(&client_id) else {
      return;
    };
    let Some(index) = client
      .in_flight
      .iter()
      .position(|batch| batch.generation == generation)
    else {
      return;
    };
    let Some(batch) = client.in_flight.remove(index) else {
      return;
    };
    for p in batch.components {
      // Skip components despawned since the batch was sent
      let tracked = self
        .dirty_components
        .get(&p.network_id)
        .is_some_and(|components| components.contains_key(&p.component_id));
      if !tracked {
        continue;
      }
      let state = client
        .components
        .entry(p.network_id)
        .or_default()
        .entry(p.component_id)
        .or_default();
      state.acked = state.acked.max(Some(p.change));
    }
    client.newest_ack = client.newest_ack.max(Some(generation));
  }
```

- `mark_despawned` and `clear_entity` also remove the entity from every client's `components` (`for client in self.clients.values_mut() { client.components.remove(&network_id); }`).
- `get_dirty_for_client` and `has_dirty_for_client` keep their signatures and rustdoc meaning ("not yet acknowledged"), re-implemented on `pending_for_client` (group by entity for the former; `!is_empty()` for the latter).
- The struct rustdoc example and `register_client` docs: replace "acknowledge after sending" with the send/ack flow (`pending_for_client` → `record_sent` → `record_ack`); the doc example must still compile.
- Add `use std::collections::VecDeque`.

`manager.rs`:

```rust
/// Most encoded bytes of component updates in one change batch - well under
/// renet's per-tick send budget (60 KB per connection), which silently drops
/// a larger unreliable message. A bigger backlog goes out over several
/// broadcasts.
pub const MAX_BATCH_BYTES: usize = 16 * 1024;
```

- `send_full_state(&mut self, ...)`: after sending, `self.dirty_tracker.record_full_sync(client_id);`. Rustdoc: the client starts from this sync and isn't sent the change history before it.
- `broadcast_updates`, per-client change batch (replaces the `collect_dirty_updates` block):

```rust
    // One budgeted change batch per client: never-sent changes first, then
    // re-sends, until acknowledged
    for &client_id in &remote_clients {
      let mut updates = Vec::new();
      let mut carried = Vec::new();
      let mut size = 0;
      for pending in self.dirty_tracker.pending_for_client(client_id) {
        let Some(&entity) = self.network_to_entity.get(&pending.network_id) else {
          continue;
        };
        let Some(serialized) = self.serialize_component(world, entity, pending.component_id) else {
          continue;
        };
        let update = ComponentUpdateMessage::new(pending.network_id, serialized);
        let update_size = encoded_len(&update);
        if !updates.is_empty() && size + update_size > MAX_BATCH_BYTES {
          break;
        }
        size += update_size;
        updates.push(update);
        carried.push(pending);
      }
      if updates.is_empty() {
        continue;
      }
      let msg = ReplicationMessage::ComponentBatch(ComponentBatchMessage {
        updates,
        generation: self.dirty_tracker.generation(),
      });
      if let Ok(bytes) = msg.to_bytes() {
        server.send(client_id, Channel::ReplicationUnreliable, &bytes);
        self.dirty_tracker.record_sent(client_id, carried);
      }
    }
```

- `collect_dirty_updates` stays public, built from `pending_for_client` (every pending component, unbudgeted - say so in its rustdoc).
- `set_refresh_interval` rustdoc: drop "aren't re-sent when a packet is lost"; the refresh is a safety net behind acknowledged, re-sent changes.
- Client: replace `newest_complete` with `/// Client: complete change batches applied since the last process_incoming, to acknowledge` `pending_acks: Vec<u64>` (both constructors `Vec::new()`). `ComponentBatch` arm: if `is_current`, apply every update, set `newest_applied = Some(batch.generation)`, and push the generation to `pending_acks` when every entity was known. `process_incoming`: after draining both channels, send one `Ack` per generation in `std::mem::take(&mut self.pending_acks)`. `FullStateSync` arm: `newest_applied = Some(sync.generation)` and `pending_acks.clear()` (comment: older batches than the sync are ignored; a restarted server's generations start over with its sync). Update `process_incoming`'s rustdoc.
- `spawn_from_message`: insert both mappings right after `world.spawn`, then apply every component, remembering the first error and returning it after the loop (`Ok(entity)` if none) — the entity stays mapped so later updates for it count as handled.
- Callers of `send_full_state` that need `mut`: fix them (tests/e2e harness, tests/integration).

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib replication:: 2>&1 | grep -E "test result|FAILED|panicked"`, `cargo test --test e2e network:: 2>&1 | grep -E "test result|FAILED"`, `cargo test --test integration network 2>&1 | grep -E "test result|FAILED"`, and the 20-run loop: `for i in $(seq 1 20); do cargo test -q --test e2e network:: >/dev/null 2>&1 || echo FAIL; done | grep -c FAIL` → 0.

- [ ] **Step 5: Suite, lint, WebRTC check, commit**

`cargo test --workspace 2>&1 | grep -E "^test result|FAILED"`, `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> (src/core/network|tests/e2e/network)"`, `cargo check --features matchbox 2>&1 | tail -2`.

```bash
git add src/core/network/replication tests/e2e/network.rs tests/integration/network.rs
git commit -m "fix(network): send replicated changes in budgeted batches with per-component acknowledgement" -m "- A large backlog no longer exceeds renet's per-tick budget and stalls a client
- New clients start from their full-state sync instead of the change history
- Acks name a sent batch, so a forged or future ack can't hide changes
- A spawn with an unregistered component still maps its entity"
```

---

### Task 5: Spread refresh, compact component data, non-exhaustive enums, docs

**Files:**
- Modify: `src/core/network/replication/manager.rs` (refresh spread over broadcasts; `MAX_REFRESH_BYTES` replaces `REFRESH_BATCH_SIZE`; refresh tests)
- Modify: `src/core/network/replication/component.rs` (`SerializedComponent::data` wire encoding)
- Modify: `src/core/network/replication/messages.rs` (`#[non_exhaustive]` on `ReplicationMessage`)
- Modify: `src/core/network/mod.rs` (`#[non_exhaustive]` on `Channel` — byte-preserving edit, it declares child modules)
- Modify: `src/core/network/replication/mod.rs` (export `MAX_REFRESH_BYTES`, drop `REFRESH_BATCH_SIZE`)
- Modify: `tests/e2e/network.rs` (`game_messages_survive_replication` client side uses a real `ReplicationManager`)
- Modify: `src/cli/skills/ryo-network.md`, `.claude/skills/ryo-network/SKILL.md`, `src/cli/skills/ryo-api.md`, `.claude/skills/ryo-api/SKILL.md` (CRLF, identical pairs), `src/core/network/replication/AGENTS.md`, `NETWORK_ARCHITECTURE.md`

**Interfaces:**
- Consumes (Task 4): `encoded_len`, `MAX_BATCH_BYTES`, `send_full_state(&mut self)`, `PendingComponent`.
- Produces: `pub const MAX_REFRESH_BYTES: usize = 8 * 1024`.

- [ ] **Step 1: Write the failing tests**

`manager.rs`: replace `refresh_is_split_into_bounded_batches` with:

```rust
  #[test]
  fn refresh_is_spread_over_broadcasts_in_bounded_messages() {
    let mut server = ReplicationManager::new_server();
    server.register::<Health>();
    server.set_refresh_interval(50);
    let mut world = World::new();
    for i in 0..300 {
      let entity = server.spawn_networked(&mut world, NetworkEntityBuilder::new());
      world.insert_one(entity, Health(i)).unwrap();
    }
    let mut net = RecordingServer { clients: vec![1], ..Default::default() };
    server.register_client(1);

    for _ in 0..60 {
      server.broadcast_updates(&world, &mut net);
    }

    let refreshes: Vec<&Vec<u8>> = net
      .sent
      .iter()
      .filter(|(_, _, bytes)| {
        matches!(ReplicationMessage::from_bytes(bytes), Ok(ReplicationMessage::ComponentRefresh(_)))
      })
      .map(|(_, _, bytes)| bytes)
      .collect();
    assert!(refreshes.len() > 1, "a 300-component refresh takes several messages");
    assert!(refreshes.iter().all(|bytes| bytes.len() <= MAX_REFRESH_BYTES + 256));
    let generations: std::collections::HashSet<u64> =
      sent_refreshes(&net).iter().map(|b| b.generation).collect();
    assert_eq!(generations.len(), refreshes.len(), "one refresh message per broadcast");
    let mut healths: Vec<u32> = health_in(sent_refreshes(&net)).into_iter().map(|h| h.0).collect();
    healths.sort_unstable();
    assert_eq!(healths, (0..300).collect::<Vec<_>>());
  }
```

`component.rs` tests:

```rust
  #[test]
  fn component_data_travels_as_json_text() {
    let component = SerializedComponent {
      type_id: ComponentTypeId(7),
      data: br#"{"hp":42}"#.to_vec(),
    };

    let json = serde_json::to_string(&component).unwrap();

    assert_eq!(json, r#"{"type_id":7,"data":"{\"hp\":42}"}"#);
    let back: SerializedComponent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.data, component.data);
  }

  #[test]
  fn component_data_still_reads_the_byte_array_form() {
    let back: SerializedComponent = serde_json::from_str(r#"{"type_id":7,"data":[123,125]}"#).unwrap();
    assert_eq!(back.data, b"{}".to_vec());
  }
```

(If `SerializedComponent`'s field order or `ComponentTypeId`'s encoding differs, adapt the expected JSON string to the real field order — the point is `data` is a JSON string.)

`tests/e2e/network.rs`: in `game_messages_survive_replication`, replace the harness client's replication processing with a raw `NetworkClient` + `ReplicationManager::new_client()` (registering `Transform` and `PlayerPosition`) + its own `World`, calling `replication.process_incoming(&mut world, &mut net_client)` each iteration before draining the game channels with `net_client.receive(...)`; keep the same three messages and assertions.

- [ ] **Step 2: Run to watch them fail**

Run: `cargo test --lib replication:: 2>&1 | tail -15` — compile error (`MAX_REFRESH_BYTES`) and, once that's stubbed, the two component tests failing on the array encoding.

- [ ] **Step 3: Implement**

`component.rs` — on `SerializedComponent::data`: `#[serde(with = "data_encoding")]`, rustdoc "Serialized component data (JSON text; sent as a string)", and:

```rust
/// `SerializedComponent::data` on the wire: JSON text as a string, about a
/// quarter the size of a byte array; non-UTF-8 data falls back to bytes.
/// Both forms are accepted when reading.
mod data_encoding {
  use serde::de::{self, SeqAccess, Visitor};
  use serde::{Deserializer, Serializer};

  pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    match std::str::from_utf8(data) {
      Ok(text) => serializer.serialize_str(text),
      Err(_) => serializer.serialize_bytes(data),
    }
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    struct DataVisitor;

    impl<'de> Visitor<'de> for DataVisitor {
      type Value = Vec<u8>;

      fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("component data as a string or byte array")
      }

      fn visit_str<E: de::Error>(self, text: &str) -> Result<Vec<u8>, E> {
        Ok(text.as_bytes().to_vec())
      }

      fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
        Ok(bytes.to_vec())
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
        let mut data = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(byte) = seq.next_element()? {
          data.push(byte);
        }
        Ok(data)
      }
    }

    deserializer.deserialize_any(DataVisitor)
  }
}
```

`manager.rs`:

```rust
/// Most encoded bytes of component values per refresh message. A refresh
/// cycle sends one per broadcast until every component has been refreshed.
pub const MAX_REFRESH_BYTES: usize = 8 * 1024;
```

- Remove `REFRESH_BATCH_SIZE`. Add field `/// Server: components still to refresh in the current refresh cycle` `refresh_queue: VecDeque<(NetworkId, ComponentTypeId)>` (both constructors empty).
- Replace the refresh block in `broadcast_updates`:

```rust
    // Periodic refresh, a safety net: a cycle starts every `refresh_interval`
    // broadcasts (never while one is running) and sends one bounded message
    // per broadcast
    let generation = self.dirty_tracker.generation();
    if self.refresh_queue.is_empty()
      && self.refresh_interval > 0
      && generation > 0
      && generation.is_multiple_of(self.refresh_interval)
      && !remote_clients.is_empty()
    {
      self.refresh_queue = self
        .network_to_entity
        .keys()
        .flat_map(|&network_id| {
          self
            .registry
            .registered_types()
            .map(move |component_id| (network_id, component_id))
        })
        .collect();
    }
    if !remote_clients.is_empty() {
      let mut updates = Vec::new();
      let mut size = 0;
      while let Some(&(network_id, component_id)) = self.refresh_queue.front() {
        let update = self
          .network_to_entity
          .get(&network_id)
          .and_then(|&entity| self.serialize_component(world, entity, component_id))
          .map(|serialized| ComponentUpdateMessage::new(network_id, serialized));
        let Some(update) = update else {
          self.refresh_queue.pop_front();
          continue;
        };
        let update_size = encoded_len(&update);
        if !updates.is_empty() && size + update_size > MAX_REFRESH_BYTES {
          break;
        }
        self.refresh_queue.pop_front();
        size += update_size;
        updates.push(update);
      }
      if !updates.is_empty() {
        let msg = ReplicationMessage::ComponentRefresh(ComponentBatchMessage { updates, generation });
        if let Ok(bytes) = msg.to_bytes() {
          for &client_id in &remote_clients {
            server.send(client_id, Channel::ReplicationUnreliable, &bytes);
          }
        }
      }
    }
```

  (If the borrow checker rejects the `flat_map` closure capturing `self.registry` while assigning `self.refresh_queue`, collect into a local first.)
- `set_refresh_interval`: also `self.refresh_queue.clear()` (a change takes effect immediately); rustdoc mentions the cycle is spread over broadcasts.
- `#[non_exhaustive]` on `ReplicationMessage` (messages.rs) and `Channel` (network/mod.rs, byte-preserving); add a rustdoc line to each: "New variants may be added; match with a wildcard arm."

Docs (both copies of each skill identical, CRLF preserved; `file` + `cmp` to verify):
- `ryo-network` skill replication paragraph: changes go out in size-limited batches (`MAX_BATCH_BYTES`), never-sent first, so a large backlog arrives over several broadcasts; a client starts from its full-state sync (call `send_full_state` on connect, as already shown); the refresh is spread in `MAX_REFRESH_BYTES` messages.
- `ryo-api` skill: `send_full_state(&mut self, ...)`; add `MAX_BATCH_BYTES`, `MAX_REFRESH_BYTES` near `DEFAULT_REFRESH_INTERVAL` if constants are listed.
- `src/core/network/replication/AGENTS.md`: replace the protocol bullet with: per-client per-component delivery (`DirtyTracker::pending_for_client` / `record_sent` / `record_ack`, acks name a sent batch), budgeted batches (`MAX_BATCH_BYTES`, never-sent first), baseline from `record_full_sync` in `send_full_state`, refresh spread in `MAX_REFRESH_BYTES` chunks; gotcha: renet silently drops an unreliable message larger than the remaining per-tick budget, so never send an unbounded replication message.
- `NETWORK_ARCHITECTURE.md`: one sentence on budgeted batches next to the acknowledged-replication line.

- [ ] **Step 4: Run the tests**

As Task 4 Step 4.

- [ ] **Step 5: Suite, lint, WebRTC check, commit**

As Task 4 Step 5, then:

```bash
git add src/core/network tests/e2e/network.rs src/cli/skills/ryo-network.md .claude/skills/ryo-network/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-api/SKILL.md NETWORK_ARCHITECTURE.md
git commit -m "feat(network): spread the replication refresh and send component data as text" -m "- The periodic refresh goes out in bounded messages over several broadcasts
- Component data is sent as JSON text instead of a byte array (about 4x smaller)
- Channel and ReplicationMessage are non-exhaustive"
```
