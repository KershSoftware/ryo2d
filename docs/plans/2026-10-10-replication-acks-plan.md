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
