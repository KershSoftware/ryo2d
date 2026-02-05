# Networking

ryo2d includes multiplayer networking powered by [renet](https://github.com/lucaspoffo/renet), a reliable UDP networking library designed for games, with optional WebRTC support via [matchbox](https://github.com/johanhelsing/matchbox) for NAT traversal.

## Overview

The networking system provides:
- **Client-server architecture** - Dedicated or player-hosted servers
- **Listen server mode** - Host plays while serving remote clients
- **Transport abstraction** - UDP (LAN) or WebRTC (internet with NAT traversal)
- **Multiple channels** - Unreliable, reliable ordered, and reliable unordered
- **Connection events** - Know when players connect and disconnect
- **Protocol versioning** - Prevent mismatched clients from connecting
- **Quick testing** - `ryo2d netplay` CLI for instant multiplayer testing

## Quick Start

### Server Setup

```rust
use ryo2d::prelude::*;
use std::time::Duration;

struct ServerGame {
    server: Option<NetworkServer>,
}

impl Game for ServerGame {
    fn init(&mut self, _ctx: &mut GameContext) {
        let config = NetworkServerConfig {
            port: 5000,
            max_clients: 4,
            protocol_id: 12345,
            ..Default::default()  // Uses UDP transport
        };

        match NetworkServer::new(config) {
            Ok(server) => {
                println!("Server started on port 5000");
                self.server = Some(server);
            }
            Err(e) => eprintln!("Failed to start server: {}", e),
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        let Some(server) = &mut self.server else { return };

        // Update networking each frame
        server.update(Duration::from_secs_f32(dt));

        // Handle connection events
        for event in server.events() {
            match event {
                NetworkEvent::ClientConnected { client_id } => {
                    println!("Player {} connected", client_id);
                }
                NetworkEvent::ClientDisconnected { client_id, reason } => {
                    println!("Player {} disconnected: {}", client_id, reason);
                }
                _ => {}
            }
        }

        // Receive messages from clients
        for client_id in server.clients().collect::<Vec<_>>() {
            while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
                // Process client message
                println!("Received {} bytes from {}", data.len(), client_id);
            }
        }
    }
}
```

### Client Setup

```rust
use ryo2d::prelude::*;
use std::net::SocketAddr;
use std::time::Duration;

struct ClientGame {
    client: Option<NetworkClient>,
}

impl Game for ClientGame {
    fn init(&mut self, _ctx: &mut GameContext) {
        let server_addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        // Use helper constructor for UDP connections
        let config = NetworkClientConfig::udp(server_addr, 12345);

        match NetworkClient::connect(config) {
            Ok(client) => {
                println!("Connecting to server...");
                self.client = Some(client);
            }
            Err(e) => eprintln!("Failed to connect: {}", e),
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        let Some(client) = &mut self.client else { return };

        client.update(Duration::from_secs_f32(dt));

        for event in client.events() {
            match event {
                NetworkEvent::Connected => {
                    println!("Connected to server!");
                }
                NetworkEvent::Disconnected { reason } => {
                    println!("Disconnected: {}", reason);
                }
                _ => {}
            }
        }

        // Receive messages from server
        while let Some(data) = client.receive(Channel::ReliableOrdered) {
            println!("Received {} bytes from server", data.len());
        }

        // Send input to server
        if ctx.input.key_pressed(KeyCode::Space) {
            client.send(Channel::ReliableOrdered, b"jump");
        }
    }
}
```

## Listen Server (Player-Hosted)

For casual co-op games where one player hosts and friends join, use `ListenServer`. The host player participates in the game while serving remote clients.

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

struct CoopGame {
    server: Option<ListenServer>,
    host_player: Player,
    remote_players: HashMap<ClientId, Player>,
}

impl Game for CoopGame {
    fn init(&mut self, _ctx: &mut GameContext) {
        let config = NetworkServerConfig {
            port: 5000,
            max_clients: 3, // 3 remote + 1 host = 4 players
            protocol_id: 12345,
            ..Default::default()
        };

        match ListenServer::new(config) {
            Ok(server) => {
                println!("Hosting game on port 5000");
                self.server = Some(server);
            }
            Err(e) => eprintln!("Failed to host: {}", e),
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        let Some(server) = &mut self.server else { return };

        server.update(Duration::from_secs_f32(dt));

        // Handle remote client events
        for event in server.events() {
            match event {
                NetworkEvent::ClientConnected { client_id } => {
                    println!("Remote player {} joined", client_id);
                    self.remote_players.insert(client_id, Player::new());
                }
                NetworkEvent::ClientDisconnected { client_id, .. } => {
                    println!("Remote player {} left", client_id);
                    self.remote_players.remove(&client_id);
                }
                _ => {}
            }
        }

        // Receive input from remote players
        for client_id in server.remote_clients().collect::<Vec<_>>() {
            while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
                // Process remote player input
            }
        }

        // Broadcast game state to remote clients
        let state = serialize_game_state();
        server.broadcast(Channel::Unreliable, &state);

        // Host player input handled directly - no network overhead!
        self.host_player.handle_input(&ctx.input);
    }
}
```

### ListenServer vs NetworkServer

| Feature | NetworkServer | ListenServer |
|---------|---------------|--------------|
| Use case | Dedicated server | Player-hosted game |
| Local player | None | Host plays too |
| `player_count()` | Same as `connected_count()` | `remote_count() + 1` |
| Host overhead | N/A | Zero (no serialization) |

### Key ListenServer Methods

```rust
// Host identification
server.host_id()           // Returns LOCAL_HOST_ID (always 0)
server.is_host(client_id)  // Check if ID is the local host

// Player iteration
server.all_clients()       // Host + all remote clients
server.remote_clients()    // Only remote clients (networked)

// Player counts
server.player_count()      // Total players (remote + 1 for host)
server.remote_count()      // Only remote clients
```

## Channels

Three delivery modes for different types of data:

| Channel | Delivery | Order | Use Case |
|---------|----------|-------|----------|
| `Unreliable` | Best effort | None | Position updates, frequent state |
| `ReliableOrdered` | Guaranteed | FIFO | Chat, game events, RPCs |
| `ReliableUnordered` | Guaranteed | None | Independent events |

```rust
// Frequent updates - some loss is acceptable
client.send(Channel::Unreliable, &position_bytes);

// Important events - must arrive in order
client.send(Channel::ReliableOrdered, &chat_message);

// Events that don't depend on order
client.send(Channel::ReliableUnordered, &achievement_unlocked);
```

### Choosing the Right Channel

**Use Unreliable for:**
- Position/velocity updates (sent every frame)
- Animation states
- Any data that's immediately superseded by new data

**Use ReliableOrdered for:**
- Player actions (jump, attack, use item)
- Chat messages
- Game state changes (score, health)
- Spawn/despawn commands

**Use ReliableUnordered for:**
- Sound effect triggers
- Particle effect spawns
- Achievement notifications

## Network Events

Both server and client emit events through `events()`:

### Server Events

```rust
for event in server.events() {
    match event {
        NetworkEvent::ClientConnected { client_id } => {
            // A new player connected
            // Send them the current game state
        }
        NetworkEvent::ClientDisconnected { client_id, reason } => {
            // A player left
            // Clean up their entities
        }
        _ => {}
    }
}
```

### Client Events

```rust
for event in client.events() {
    match event {
        NetworkEvent::Connected => {
            // Successfully connected to server
            // Request initial game state
        }
        NetworkEvent::Disconnected { reason } => {
            // Lost connection
            // Show reconnect UI or return to menu
        }
        _ => {}
    }
}
```

## Sending Messages

### Server to Client

```rust
// Send to one client
server.send(client_id, Channel::ReliableOrdered, &data);

// Broadcast to all clients
server.broadcast(Channel::Unreliable, &game_state);

// Broadcast to all except one (e.g., relay a message)
server.broadcast_except(sender_id, Channel::ReliableOrdered, &data);
```

### Client to Server

```rust
client.send(Channel::ReliableOrdered, &input_data);
```

## Receiving Messages

Messages must be drained each frame:

```rust
// Server receiving from clients
for client_id in server.clients().collect::<Vec<_>>() {
    while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
        // Process message
    }
}

// Client receiving from server
while let Some(data) = client.receive(Channel::ReliableOrdered) {
    // Process message
}
```

> **Important:** Always collect client IDs before iterating. The `clients()` iterator borrows the server, so you can't call `receive()` while iterating directly.

## Protocol ID

The `protocol_id` prevents incompatible clients from connecting:

```rust
let server_config = NetworkServerConfig {
    protocol_id: 12345, // Your game's version identifier
    ..Default::default()
};

let client_config = NetworkClientConfig::udp(server_addr, 12345); // Must match server
```

**Best practice:** Use a hash of your network protocol version. When you change message formats, change the protocol ID to prevent old clients from connecting to new servers.

## Connection State

### Client States

```rust
if client.is_connecting() {
    // Show "Connecting..." UI
}

if client.is_connected() {
    // In-game, can send/receive
}

if client.is_disconnected() {
    // Show reconnect option
    if let Some(reason) = client.disconnect_reason() {
        println!("Disconnected: {}", reason);
    }
}
```

### Graceful Disconnect

```rust
// Client disconnecting
client.disconnect();

// Server kicking a client
server.disconnect(client_id);
```

## Serialization

ryo2d's networking is message-agnostic - you send and receive raw bytes. Use [serde](https://serde.rs/) with a binary format for efficient serialization:

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
enum GameMessage {
    PlayerInput { x: f32, y: f32, jump: bool },
    SpawnEntity { id: u32, x: f32, y: f32 },
    ChatMessage { text: String },
}

// Serialize
let msg = GameMessage::PlayerInput { x: 1.0, y: 0.0, jump: true };
let bytes = bincode::serialize(&msg).unwrap();
client.send(Channel::ReliableOrdered, &bytes);

// Deserialize
while let Some(data) = client.receive(Channel::ReliableOrdered) {
    let msg: GameMessage = bincode::deserialize(&data).unwrap();
    match msg {
        GameMessage::SpawnEntity { id, x, y } => {
            // Spawn the entity
        }
        // ...
    }
}
```

Add to your `Cargo.toml`:
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
```

## Error Handling

Network operations return `NetworkResult<T>`:

```rust
match NetworkServer::new(config) {
    Ok(server) => { /* started */ }
    Err(NetworkError::InitError(msg)) => {
        eprintln!("Failed to start: {}", msg);
    }
    Err(e) => {
        eprintln!("Network error: {}", e);
    }
}

match NetworkClient::connect(config) {
    Ok(client) => { /* connecting */ }
    Err(NetworkError::ConnectionError(msg)) => {
        eprintln!("Connection failed: {}", msg);
    }
    Err(e) => {
        eprintln!("Network error: {}", e);
    }
}
```

## Best Practices

### 1. Call update() every frame
```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Always update networking, even when paused
    if let Some(server) = &mut self.server {
        server.update(Duration::from_secs_f32(dt));
    }
}
```

### 2. Drain all messages
```rust
// Use while let, not if let
while let Some(data) = client.receive(Channel::ReliableOrdered) {
    // Process all pending messages
}
```

### 3. Handle disconnection gracefully
```rust
for event in client.events() {
    if let NetworkEvent::Disconnected { reason } = event {
        // Don't crash - show UI, attempt reconnect, etc.
        self.show_disconnect_screen(&reason);
    }
}
```

### 4. Use appropriate channels
```rust
// DON'T: Send position on reliable channel (wastes bandwidth)
client.send(Channel::ReliableOrdered, &position);

// DO: Use unreliable for frequently-updated data
client.send(Channel::Unreliable, &position);
```

### 5. Validate received data
```rust
while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
    // Don't trust client data blindly
    if let Ok(input) = bincode::deserialize::<PlayerInput>(&data) {
        if input.is_valid() {
            // Process input
        }
    }
}
```

## Complete Example: Simple Chat

```rust
use ryo2d::prelude::*;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    sender: String,
    text: String,
}

struct ChatServer {
    server: Option<ListenServer>,
    player_names: HashMap<ClientId, String>,
    messages: Vec<ChatMessage>,
}

impl Game for ChatServer {
    fn init(&mut self, _ctx: &mut GameContext) {
        let config = NetworkServerConfig {
            port: 5000,
            max_clients: 8,
            protocol_id: 1,
            ..Default::default()
        };
        self.server = ListenServer::new(config).ok();

        // Host is "Player 0"
        self.player_names.insert(LOCAL_HOST_ID, "Host".to_string());
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f32) {
        let Some(server) = &mut self.server else { return };
        server.update(Duration::from_secs_f32(dt));

        // Handle connections
        for event in server.events() {
            match event {
                NetworkEvent::ClientConnected { client_id } => {
                    let name = format!("Player {}", client_id);
                    self.player_names.insert(client_id, name.clone());

                    // Announce join
                    let msg = ChatMessage {
                        sender: "Server".to_string(),
                        text: format!("{} joined", name),
                    };
                    self.broadcast_message(server, &msg);
                }
                NetworkEvent::ClientDisconnected { client_id, .. } => {
                    if let Some(name) = self.player_names.remove(&client_id) {
                        let msg = ChatMessage {
                            sender: "Server".to_string(),
                            text: format!("{} left", name),
                        };
                        self.broadcast_message(server, &msg);
                    }
                }
                _ => {}
            }
        }

        // Receive chat messages from remote clients
        for client_id in server.remote_clients().collect::<Vec<_>>() {
            while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
                if let Ok(mut msg) = bincode::deserialize::<ChatMessage>(&data) {
                    // Override sender with server's record (don't trust client)
                    if let Some(name) = self.player_names.get(&client_id) {
                        msg.sender = name.clone();
                    }
                    self.broadcast_message(server, &msg);
                }
            }
        }
    }
}

impl ChatServer {
    fn broadcast_message(&mut self, server: &mut ListenServer, msg: &ChatMessage) {
        self.messages.push(msg.clone());

        let bytes = bincode::serialize(msg).unwrap();
        server.broadcast(Channel::ReliableOrdered, &bytes);
    }
}
```

## Transport Selection

ryo2d supports two transport options:

| Transport | Use Case | NAT Traversal | Latency |
|-----------|----------|---------------|---------|
| **UDP** (default) | LAN games, development | No | Lowest |
| **WebRTC** | Internet multiplayer | Yes (STUN/TURN) | Low |

### UDP Transport (Default)

UDP is the default and requires no configuration changes:

```rust
// UDP is used automatically
let config = NetworkServerConfig {
    port: 5000,
    max_clients: 4,
    protocol_id: 12345,
    ..Default::default()
};
```

### WebRTC Transport (Internet)

For internet multiplayer with NAT traversal, enable the `matchbox` feature and configure WebRTC:

```toml
# Cargo.toml
[dependencies]
ryo2d = { version = "0.1", features = ["matchbox"] }
```

```rust
use ryo2d::prelude::*;

// Server with WebRTC
let config = NetworkServerConfig {
    max_clients: 4,
    protocol_id: 12345,
    transport: TransportConfig::WebRtc(WebRtcConfig::new(
        "wss://signaling.example.com",
        "my-game-room",
    )),
    ..Default::default()
};

// Client with WebRTC
let config = NetworkClientConfig {
    protocol_id: 12345,
    transport: TransportConfig::WebRtc(WebRtcConfig::new(
        "wss://signaling.example.com",
        "my-game-room",
    )),
    ..Default::default()
};
```

### WebRTC with TURN Fallback

For players behind strict NATs (corporate networks, some mobile carriers), add a TURN server:

```rust
let webrtc = WebRtcConfig::new("wss://signaling.example.com", "room-abc")
    .with_turn("turn:relay.example.com:3478", "username", "credential");

let config = NetworkServerConfig {
    transport: TransportConfig::WebRtc(webrtc),
    ..Default::default()
};
```

## Quick Testing with Netplay

The `ryo2d netplay` CLI command provides instant multiplayer testing:

```bash
# Start a server with public URL (via Cloudflare Tunnel)
ryo2d netplay --server

# Connect to a server
ryo2d netplay --client <tunnel-url>

# Custom port
ryo2d netplay --server --port 7777
```

This automatically:
1. Creates a Cloudflare Quick Tunnel for public access
2. Displays connection URLs for remote players
3. Shows WebRTC config snippets for your game code

## Security Notes

The current implementation uses `ServerAuthentication::Unsecure`:
- Suitable for LAN games and development
- Fine for casual co-op with friends
- For public internet games, consider implementing authentication with connect tokens

## Advanced Features

ryo2d includes higher-level networking features built on top of the basic client-server system:

### Network Replication

Automatic synchronization of game state between server and clients. Register components for replication and they stay in sync automatically.

```rust
// Server: spawn networked entity
let entity = replication.spawn_networked(&mut world,
    NetworkEntityBuilder::new()
        .with_component(Transform::new(100.0, 100.0))
);

// Changes are automatically broadcast to clients
```

See [Network Replication](./replication.md) for the complete guide.

### Remote Procedure Calls (RPC)

Type-safe messaging for game events that don't fit the component model:

```rust
// Define RPC
#[derive(Clone, Serialize, Deserialize)]
struct JumpRpc { height: f32 }

impl Rpc for JumpRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("Jump") }
    fn rpc_name() -> &'static str { "Jump" }
}

// Client sends to server
let serialized = SerializedRpc::from_rpc(&JumpRpc { height: 10.0 }, client_id, RpcTarget::Server)?;
client.send(Channel::ReliableOrdered, &serialized.to_bytes()?);
```

See [Remote Procedure Calls](./rpc.md) for the complete guide.

## See Also

- [Network Replication](./replication.md) - Automatic state synchronization
- [Remote Procedure Calls](./rpc.md) - Game events and actions
- [Game Loop](./game-loop.md) - Understanding update cycles
- [ECS Architecture](./ecs.md) - Entity management for multiplayer
- [Save System](./save-system.md) - Persisting game state
- [Error Handling](./error-handling.md) - Handling network errors
