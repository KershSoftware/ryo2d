# Network Module - AGENTS.md

## Purpose

The `network/` module provides client-server networking for multiplayer games using **renet** as the underlying transport layer. It includes:
- Low-level networking (`NetworkServer`, `NetworkClient`, `ListenServer`)
- Transport abstraction (`transport/` submodule) - UDP and WebRTC
- Automatic state replication (`replication/` submodule)
- Remote procedure calls (`rpc/` submodule)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Game Code                                │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              ReplicationManager + RpcRegistry             │  │
│  │  (automatic entity sync)        (game events)             │  │
│  └─────────────────────────┬─────────────────────────────────┘  │
│                            │                                    │
│  ┌─────────────────┐  ┌────┴────┐  ┌─────────────────┐         │
│  │ NetworkServer   │  │ServerLike│  │ NetworkClient   │         │
│  │ ListenServer    │◄─┤  trait   │  │ (players)       │         │
│  └────────┬────────┘  └─────────┘  └────────┬────────┘         │
└───────────┼─────────────────────────────────┼──────────────────┘
            │                                 │
            ▼                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Transport Layer                             │
│  ┌─────────────────────────┐    ┌─────────────────────────┐     │
│  │     UDP (default)       │    │  WebRTC (matchbox feat) │     │
│  │  renet Netcode          │    │  matchbox_socket        │     │
│  │  LAN / direct connect   │    │  NAT traversal (STUN)   │     │
│  └─────────────────────────┘    └─────────────────────────┘     │
└─────────────────────────────────────────────────────────────────┘
```

## Module Structure

| Path | Purpose |
|------|---------|
| `mod.rs` | Core networking types, `ServerLike` trait |
| `action.rs` | NetworkAction trait for dual local+network execution |
| `debug.rs` | Debug assertions for catching common network bugs |
| `input.rs` | NetworkInput derive macro and input handling |
| `prediction.rs` | Client-side prediction components and helpers |
| `transport/` | Transport abstraction layer (UDP, WebRTC) |
| `replication/` | Automatic entity/component synchronization |
| `rpc/` | Remote procedure call system |
| `AGENTS.md` | This documentation |

See submodule AGENTS.md files for detailed documentation:
- `transport/AGENTS.md` - Transport layer and NAT traversal
- `replication/AGENTS.md` - Entity/component synchronization
- `rpc/AGENTS.md` - Remote procedure calls

## Key Types

### Server Types

| Type | Use Case | Description |
|------|----------|-------------|
| `NetworkServer` | Dedicated server | Pure server, no local player |
| `ListenServer` | Player-hosted games | Host plays while serving remote clients |
| `NetworkClient` | Remote players | Connects to either server type |

### ServerLike Trait

Abstracts over server types for replication code:

```rust
pub trait ServerLike {
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);
    fn broadcast(&mut self, channel: Channel, data: &[u8]);
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_>;
    fn is_host(&self, client_id: ClientId) -> bool;
}
```

Both `NetworkServer` and `ListenServer` implement this trait.

### Channel Types

| Channel | Guarantee | Order | Use Case |
|---------|-----------|-------|----------|
| `Unreliable` | None | None | Position updates, frequent state |
| `ReliableOrdered` | Delivery + Order | FIFO | Spawns, despawns, RPCs |
| `ReliableUnordered` | Delivery only | None | Independent events |

## Design Decisions

### User-Managed Networking

Unlike audio (engine-managed), networking is **user-managed**:
- User creates server/client in their game struct
- User calls `update()` in their `Game::update()`
- User decides when to use replication vs raw messages

### ListenServer Design

The `ListenServer` wraps `NetworkServer` for player-hosted games:
- Host player has a "virtual" client ID (`LOCAL_HOST_ID = 0`)
- Host's messages are NOT networked - handled directly in game logic
- `all_clients()` iterates host + remotes for uniform game logic
- `is_host()` identifies the local player

### Host-Authoritative Model

The replication system uses host-authoritative networking:
- Server owns the truth for game state
- Clients receive updates and render predictions
- Authority can be delegated to specific clients (player input)

## Usage Patterns

### Basic Server/Client (No Replication)

```rust
// Server
let mut server = NetworkServer::new(config)?;
server.update(dt);
server.broadcast(Channel::Unreliable, &state_bytes);

// Client
let mut client = NetworkClient::connect(config)?;
client.update(dt);
client.send(Channel::ReliableOrdered, &input_bytes);
```

### With Replication

```rust
// Server
let mut replication = ReplicationManager::new_server();
replication.register::<Transform>();
replication.register::<Velocity>();

// Spawn networked entity
let entity = replication.spawn_networked(&mut world,
    NetworkEntityBuilder::new()
        .with_component(Transform::new(0.0, 0.0))
);

// Each frame
replication.broadcast_updates(&mut world, &mut server);

// Client
let mut replication = ReplicationManager::new_client();
replication.register::<Transform>();
replication.register::<Velocity>();

// Each frame
replication.process_incoming(&mut world, &mut client);
```

### With RPCs

```rust
// Define RPC
#[derive(Clone, Serialize, Deserialize)]
struct JumpRpc { player_id: u64 }

impl Rpc for JumpRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("JumpRpc") }
    fn rpc_name() -> &'static str { "JumpRpc" }
}

// Server: register handler
rpc_registry.register::<JumpRpc>(|world, rpc| {
    // Handle jump for rpc.sender
    Ok(())
});

// Client: send RPC
let serialized = SerializedRpc::from_rpc(&JumpRpc { player_id: 1 }, client_id, RpcTarget::Server)?;
client.send(Channel::ReliableOrdered, &serialized.to_bytes()?);
```

## Protocol ID

The `protocol_id` field serves as a version check:
- Server and client must have matching protocol IDs
- Use different IDs for incompatible game versions
- Prevents mismatched clients from connecting

## Testing

Integration tests in `tests/network_integration.rs` verify:
- Server creation and binding
- Client connection establishment
- Bidirectional message passing
- Event generation (connected, disconnected)
- ListenServer host handling
- Broadcast to multiple clients

## Client-Side Prediction

The `prediction` module provides components and helpers for implementing client-side prediction, where the client simulates locally and reconciles with server corrections.

### Key Types

| Type | Purpose |
|------|---------|
| `Predicted` | Component marking entities for prediction |
| `PredictionHelper` | Helper for handling position updates |
| `PositionUpdate` | Server position update message |
| `UpdateResult` | Outcome of processing an update |

### Usage

```rust
use ryo2d::prelude::*;

// Mark entity as predicted when spawning
let entity = world.spawn((
    Transform::new(0.0, 0.0),
    Predicted::new(client_id),
));

// Create prediction helper
let mut helper = PredictionHelper::new(Some(local_client_id));

// When receiving position update from server
for update in position_updates {
    match helper.handle_position_update(&mut world, entity, &update) {
        UpdateResult::Applied => {
            // Remote player or forced correction - applied directly
        }
        UpdateResult::Spawned => {
            // First update - entity now visible
        }
        UpdateResult::StoredForReconciliation => {
            // Local player - reconcile with input buffer
            input_processor.reconcile(&mut world, update.sequence, dt);
        }
        UpdateResult::Skipped => {
            // Entity not found
        }
    }
}

// After respawn, force correction to apply server state
helper.force_correction(&mut world, entity, &spawn_update);
```

### UpdateResult Flow

```
Server Update Received
         │
         ▼
  Is entity spawned?
         │
    no ──┼── yes
         │    │
  [Spawned]  Is local player?
              │
         no ──┼── yes
              │    │
       [Applied]  Force correction?
                   │
              no ──┼── yes
                   │    │
    [StoredForReconciliation]  [Applied]
```

## NetworkAction Trait

The `action` module provides the `NetworkAction` trait for actions that execute both locally AND are sent over the network. This prevents bugs where you forget one half:

```rust
use ryo2d::prelude::*;

#[derive(Clone, Serialize, Deserialize)]
struct RespawnAction {
    slot: u8,
    spawn_x: f32,
    spawn_y: f32,
}

impl NetworkAction for RespawnAction {
    fn action_id() -> RpcTypeId {
        RpcTypeId::from_name("Respawn")
    }

    fn action_name() -> &'static str { "Respawn" }

    fn apply(&self, ctx: &mut ActionContext) {
        // Reset player position, velocity, state...
        if let Some(entity) = get_player_entity(ctx, self.slot) {
            // Modify entity state
        }
    }
}

// Client: applies locally AND sends to server
execute_action(RespawnAction { slot: 1, spawn_x: 0.0, spawn_y: 0.0 }, &mut ctx, &mut client);

// Server: applies locally AND broadcasts to other clients
execute_action_server(action, &mut ctx, &mut server, sender_client_id);
```

### When to Use NetworkAction vs RPC

| Use NetworkAction | Use RPC |
|------------------|---------|
| State changes that need local prediction | Server-only events |
| Actions that modify game state | Notifications without state change |
| Respawn, jump, use item | Chat messages, achievements |
| Needs immediate local feedback | Can wait for server response |

### Customization

```rust
impl NetworkAction for MyAction {
    // Override to skip local application (wait for server)
    fn apply_locally(&self) -> bool { false }

    // Override for frequent, loss-tolerant actions
    fn channel(&self) -> Channel { Channel::Unreliable }
}
```

## NetworkInput Builder

The `input` module provides compile-time safety for network input structs using the `NetworkInput` derive macro:

```rust
use ryo2d::prelude::*;

#[derive(NetworkInput, Clone, Default, Serialize, Deserialize)]
pub struct LeapInput {
    #[network_input(required)]
    pub charging: bool,
    #[network_input(required)]
    pub facing_right: bool,  // Easy to forget without this!
    pub aim_angle: f32,       // Optional, defaults to 0.0
    pub release: bool,        // Optional, defaults to false
}

// Compiler enforces required fields
let input = LeapInput::builder()
    .charging(true)
    .facing_right(player.facing == Facing::Right)
    .aim_angle(aim)
    .build();

// Won't compile - missing facing_right:
// let input = LeapInput::builder()
//     .charging(true)
//     .build();  // ERROR!
```

### When to Mark Fields Required

| Mark Required | Leave Optional |
|--------------|----------------|
| Direction/facing | Button press states |
| Player slot/ID | Magnitudes that can be 0 |
| Target position | Optional actions |
| State that affects others | Local-only data |

### Input Buffer

For client-side prediction, use `InputBuffer<T>` to store pending inputs:

```rust
let mut buffer: InputBuffer<LeapInput> = InputBuffer::new(64);

// Store input with sequence number
buffer.push(SequencedInput::new(input, sequence, timestamp));

// When server acknowledges
buffer.acknowledge(server_sequence);

// Replay unacked inputs after correction
for input in buffer.inputs_after(server_sequence) {
    apply_input(&mut state, &input.input);
}
```

## Debug Assertions

The `debug` module provides assertions that only run in debug builds to catch common networking bugs:

| Function | Purpose |
|----------|---------|
| `warn_if_not_dirty<T>()` | Warns if a component was modified but not marked dirty |
| `warn_default_input_field()` | Warns if an input field has its default value |
| `warn_if_not_connected()` | Warns if sending while disconnected |
| `debug_assert_role()` | Warns if code runs on wrong role (server/client) |

### Usage

```rust
use ryo2d::core::network::debug::*;

// After modifying a component
if let Some(mut transform) = world.get_component_mut::<Transform>(entity) {
    transform.x += 10.0;
}
replication.mark_dirty::<Transform>(entity);
warn_if_not_dirty::<Transform>(entity, &replication);  // Verify it was marked

// Verify input fields are populated
let input = MyInput { facing_right: player.facing, ..Default::default() };
warn_default_input_field(&input.facing_right, "facing_right");

// Verify network connection before sending
warn_if_not_connected(client.is_connected(), "send player input");

// Verify code runs on correct role
debug_assert_role(true, replication.is_server(), "spawn_player");
```

All these functions are no-ops in release builds (zero runtime cost).

## Notes for AI Agents

1. **Call update() every frame** - Both server and client need regular updates
2. **Drain events and messages** - Use `while let Some()` to get all pending data
3. **Collect clients before iterating** - `server.clients()` returns an iterator that borrows self
4. **Protocol IDs must match** - Mismatched IDs cause silent connection failures
5. **Register same components** - Server and client must register identical component types
6. **Use ServerLike for generic code** - Write replication code against the trait
7. **Handle disconnection gracefully** - Network can fail at any time
8. **Use debug assertions** - Call `warn_if_not_dirty` after modifying replicated components
9. **Use NetworkInput builder** - Mark required fields to prevent missing data bugs
10. **Use NetworkAction for dual execution** - Single call handles local + network
11. **Use Predicted for client-side prediction** - Handles spawn tracking and reconciliation
