---
name: ryo-network
description: Multiplayer networking with Ryo2D - client-server, replication, RPCs.
metadata:
  version: 1.0.0
---

# Ryo2D Networking

You help users build multiplayer games using Ryo2D's networking system. You understand client-server architecture, state replication, and RPC patterns.

**For complete API signatures, see the `ryo-api` skill.**

## Architecture Overview

Ryo2D uses **renet** for reliable networking with a host-authoritative model. Supports both UDP (LAN) and WebRTC (internet with NAT traversal):

```
┌─────────────────────────────────────────────────────────────┐
│                        Game Code                            │
│  ┌───────────────────────────────────────────────────────┐  │
│  │         ReplicationManager + RpcRegistry              │  │
│  └─────────────────────────┬─────────────────────────────┘  │
│                            │                                │
│  ┌─────────────────┐  ┌────┴────┐  ┌─────────────────┐      │
│  │ NetworkServer   │  │ServerLike│  │ NetworkClient   │      │
│  │ ListenServer    │◄─┤  trait   │  │ (players)       │      │
│  └────────┬────────┘  └─────────┘  └────────┬────────┘      │
└───────────┼─────────────────────────────────┼───────────────┘
            │                                 │
            ▼                                 ▼
┌─────────────────────────────────────────────────────────────┐
│                    Transport Layer                          │
│  ┌───────────────────┐        ┌───────────────────────┐     │
│  │   UDP (default)   │        │  WebRTC (matchbox)    │     │
│  │   LAN / direct    │        │  NAT traversal        │     │
│  └───────────────────┘        └───────────────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

## Server Types

| Type | Use Case | Description |
|------|----------|-------------|
| `NetworkServer` | Dedicated server | Pure server, no local player |
| `ListenServer` | Player-hosted | Host plays while serving others |
| `NetworkClient` | Remote players | Connects to either server type |

## Quick Start: ListenServer (Player-Hosted)

```rust
use ryo2d::prelude::*;

// Host creates server
let server = ListenServer::new(NetworkServerConfig {
    port: 7777,
    max_clients: 4,
    protocol_id: 0x12345678,
    ..Default::default()  // UDP transport
})?;

let host_id = server.host_id();  // Host's virtual client ID

// Client connects
let client = NetworkClient::connect(NetworkClientConfig::udp(
    "127.0.0.1:7777".parse()?,
    0x12345678,  // protocol_id - must match!
))?;
```

## Update Loop Pattern

**Critical:** Both server and client need `update()` called every frame.

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Update network
    if let Some(server) = &mut self.server {
        server.update(Duration::from_secs_f32(dt));

        // Process events (collect first: events() borrows the server mutably)
        let events: Vec<_> = server.events().collect();
        for event in events {
            match event {
                NetworkEvent::ClientConnected { client_id } => {
                    self.spawn_player(ctx, client_id);
                }
                NetworkEvent::ClientDisconnected { client_id, .. } => {
                    self.remove_player(ctx, client_id);
                }
                _ => {}
            }
        }
    }

    if let Some(client) = &mut self.client {
        client.update(Duration::from_secs_f32(dt));

        let events: Vec<_> = client.events().collect();
        for event in events {
            match event {
                NetworkEvent::Connected => {
                    self.connected = true;
                }
                NetworkEvent::Disconnected { .. } => {
                    self.connected = false;
                }
                _ => {}
            }
        }
    }

    SceneAction::Continue
}
```

## Channels

| Channel | Guarantee | Use For |
|---------|-----------|---------|
| `Unreliable` | Best-effort | Position updates, frequent state |
| `ReliableOrdered` | Delivery + FIFO order | Spawns, RPCs, chat |
| `ReliableUnordered` | Delivery, any order | Independent one-off events |
| `ReplicationReliable` | Delivery + FIFO order | Reserved for replication; don't send game messages on it |
| `ReplicationUnreliable` | Best-effort | Reserved for replication; don't send game messages on it |

```rust
// Send unreliable (position updates)
server.send(client_id, Channel::Unreliable, &position_bytes);

// Send reliable (important events)
client.send(Channel::ReliableOrdered, &rpc_bytes);

// Broadcast to all clients
server.broadcast(Channel::Unreliable, &state_bytes);
```

## Receiving Messages

```rust
// Server receives from specific client
while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
    // Process data...
}

// Client receives from server
while let Some(data) = client.receive(Channel::Unreliable) {
    // Process data...
}
```

## Replication vs RPCs

| Use Replication | Use RPCs |
|-----------------|----------|
| Position, rotation | Player actions (jump, attack) |
| Health, ammo | One-shot events (score, chat) |
| Continuous state | Menu interactions |

## Component Replication

### 1. Implement Replicated Trait

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
struct PlayerPosition {
    x: f32,
    y: f32,
}

impl Replicated for PlayerPosition {
    fn component_id() -> ComponentTypeId {
        ComponentTypeId::from_name("PlayerPosition")
    }
    fn component_name() -> &'static str {
        "PlayerPosition"
    }
}
```

### 2. Setup ReplicationManager

```rust
// Server
let mut replication = ReplicationManager::new_server();
replication.register::<Transform>();
replication.register::<PlayerPosition>();

// Client (must register same types!)
let mut replication = ReplicationManager::new_client();
replication.register::<Transform>();
replication.register::<PlayerPosition>();
```

### 3. Spawn Networked Entities

```rust
let entity = replication.spawn_networked(
    &mut world,
    NetworkEntityBuilder::new()
        .client_owned(client_id) // Client controls this entity
        .with_component(Transform::new(x, y))
        .with_component(PlayerPosition { x, y }),
);

// Non-replicated components (sprites, colliders) are added as usual
// with world.add_component(entity, ...)

// When a client connects, bring it up to date
replication.register_client(client_id);
replication.send_full_state(&world, &mut server, client_id);
```

Spawn messages (and full-state syncs) carry the current value of every registered component the entity has when `broadcast_updates()` runs: `with_component` values are there from the start, and replicated components added later with `add_component` must be added before the next `broadcast_updates()`. Clients then never see an entity without its initial state. Later changes are sent as unreliable deltas, only for components you `mark_dirty`. Changes are re-sent to each client until it acknowledges them (`process_incoming()` acknowledges automatically), so a lost packet is recovered on the next broadcast. As a safety net the server also re-sends every replicated component's current value every 300 `broadcast_updates()` calls by default (about 5 s; tune with `replication.set_refresh_interval(n)`, `0` turns it off).

### 4. Broadcast Updates

```rust
// After modifying replicated components
replication.mark_dirty::<PlayerPosition>(entity);

// Broadcast to clients
replication.broadcast_updates(&world, &mut server);

// Client processes incoming
replication.process_incoming(&mut world, &mut client);
```

Call `broadcast_updates()` every frame. `process_incoming()` takes any `ClientLike` (e.g. `NetworkClient`) and reads only the reserved `ReplicationReliable` / `ReplicationUnreliable` channels, so the default channels stay yours for RPCs and game messages.

## Remote Procedure Calls (RPCs)

### 1. Define RPC

```rust
#[derive(Clone, Serialize, Deserialize)]
struct PlayerInputRpc {
    move_x: f32,
    jump: bool,
}

impl Rpc for PlayerInputRpc {
    fn rpc_id() -> RpcTypeId {
        RpcTypeId::from_name("PlayerInputRpc")
    }
    fn rpc_name() -> &'static str {
        "PlayerInputRpc"
    }
}
```

### 2. Send RPC

```rust
// Client sends to server
let rpc = PlayerInputRpc { move_x: 1.0, jump: true };
let serialized = SerializedRpc::from_rpc(
    &rpc,
    client.client_id(),
    RpcTarget::Server
)?;
client.send(Channel::ReliableOrdered, &serialized.to_bytes()?);

// Server broadcasts to all
let serialized = SerializedRpc::from_rpc(
    &score_rpc,
    0,  // Server sender ID
    RpcTarget::Broadcast
)?;
server.broadcast(Channel::ReliableOrdered, &serialized.to_bytes()?);
```

### 3. Receive and Process RPC

```rust
// Server processes client RPCs
for client_id in server.clients().collect::<Vec<_>>() {
    while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
            if let Ok(rpc) = serialized.into_rpc::<PlayerInputRpc>() {
                // rpc.payload contains the data
                // rpc.sender is the client who sent it
                apply_input(rpc.sender, &rpc.payload);
            }
        }
    }
}
```

## ListenServer Specifics

```rust
let server = ListenServer::new(config)?;

// Host's virtual client ID (always 0)
let host_id = server.host_id();

// Check if a client is the host
if server.is_host(client_id) {
    // This is the local player
}

// Iterate all clients (host + remotes)
for client_id in server.all_clients() {
    // Process each player
}

// Iterate only remote clients (excludes host)
for client_id in server.remote_clients() {
    // Send network messages (host doesn't need them)
}

// Player count
server.player_count()  // Includes host
```

## Common Patterns

### Avoiding Borrow Issues

```rust
// BAD: Borrows server twice
for client_id in server.clients() {  // Borrows server
    server.send(client_id, ...);     // Can't borrow again!
}

// GOOD: Collect first
let clients: Vec<_> = server.clients().collect();
for client_id in clients {
    server.send(client_id, ...);
}

// Also for events
let events: Vec<_> = server.events().collect();
for event in events {
    match event { ... }
}
```

### Position Sync (Simple Approach)

```rust
// Server broadcasts positions every frame
fn broadcast_positions(&mut self) {
    let mut updates = Vec::new();
    for (&client_id, &entity) in &self.players {
        if let Some(t) = self.world.get::<Transform>(entity) {
            updates.push(PositionUpdate { client_id, x: t.x, y: t.y });
        }
    }

    let bytes = serialize(&updates)?;
    for client_id in self.server.remote_clients().collect::<Vec<_>>() {
        self.server.send(client_id, Channel::Unreliable, &bytes);
    }
}

// Client applies received positions
while let Some(data) = client.receive(Channel::Unreliable) {
    let updates: Vec<PositionUpdate> = deserialize(&data)?;
    for update in updates {
        if let Some(entity) = self.players.get(&update.client_id) {
            if let Some(mut t) = world.get_mut::<Transform>(*entity) {
                t.x = update.x;
                t.y = update.y;
            }
        }
    }
}
```

## Error Handling

```rust
// Network operations return Result<_, NetworkError>
match ListenServer::new(config) {
    Ok(server) => { /* success */ }
    Err(NetworkError::InitError(msg)) => { /* port in use, etc */ }
    Err(NetworkError::ConnectionError(msg)) => { /* connection failed */ }
    Err(e) => { /* other errors */ }
}
```

## Protocol ID

The `protocol_id` acts as a version check:
- Server and client must have matching IDs
- Use different IDs for incompatible game versions
- Mismatched clients silently fail to connect

```rust
// Use a unique ID for your game
const PROTOCOL_ID: u64 = 0x4D594741_4D450001; // "MYGAME" + version
```

## Transport Selection

| Scenario | Transport | Config |
|----------|-----------|--------|
| LAN / development | UDP (default) | `NetworkServerConfig::default()` |
| Internet play | WebRTC | `TransportConfig::WebRtc(...)` |
| Strict NAT | WebRTC + TURN | `WebRtcConfig::new(...).with_turn(...)` |

### UDP (Default - LAN)

```rust
// Server
let config = NetworkServerConfig::default();  // Uses UDP
let server = NetworkServer::new(config)?;

// Client
let config = NetworkClientConfig::udp("192.168.1.100:7777".parse()?, 0x12345678);
let client = NetworkClient::connect(config)?;
```

### WebRTC (Internet - requires `matchbox` feature)

```rust
// Enable in Cargo.toml: ryo2d = { features = ["matchbox"] }

// Server
let config = NetworkServerConfig {
    transport: TransportConfig::WebRtc(WebRtcConfig::new(
        "wss://signaling.example.com",
        "my-game-room",
    )),
    ..Default::default()
};

// Client
let config = NetworkClientConfig::webrtc(WebRtcConfig::new(
    "wss://signaling.example.com",
    "my-game-room",
));
```

### Testing Netplay

Use the CLI for quick testing with friends:

```bash
ryo2d netplay start             # Start temp signaling server
ryo2d netplay ip                # Show LAN/WAN IPs
```

## Notes

1. **Call update() every frame** - Required for network processing
2. **Collect iterators before mutating** - Avoid borrow checker issues
3. **Register same components** - Server and client must match
4. **Protocol IDs must match** - Or connections silently fail
5. **Use Unreliable for frequent data** - Positions, non-critical state
6. **Use ReliableOrdered for events** - Spawns, RPCs, important state
7. **Host ID is always 0** - For ListenServer
8. **WebRTC needs signaling** - Use `ryo2d netplay start` for testing
