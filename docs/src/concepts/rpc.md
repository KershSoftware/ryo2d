# Remote Procedure Calls (RPC)

RPCs let you send game events and actions across the network. Unlike replication (which syncs continuous state), RPCs are for one-shot events like "player jumped", "item used", or "chat message sent".

## Overview

The RPC system provides:
- **Type-safe messaging** - Define strongly-typed RPC structs
- **Automatic routing** - Register handlers, dispatch incoming RPCs
- **Flexible targeting** - Send to server, specific client, or broadcast
- **Easy serialization** - Uses serde for encoding

## When to Use RPCs

| Use RPCs | Use Replication |
|----------|-----------------|
| Player jumped | Player position |
| Attack triggered | Health value |
| Chat message | Animation state |
| Game event (score, round start) | Continuous movement |
| Item pickup/use | Inventory contents |

**Rule of thumb:** If it happens once and doesn't need interpolation, use an RPC. If it's continuous state that changes every frame, use replication.

## Quick Start

### Define an RPC

```rust
use serde::{Serialize, Deserialize};
use ryo2d::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlayerJumpRpc {
    pub height: f32,
}

impl Rpc for PlayerJumpRpc {
    fn rpc_id() -> RpcTypeId {
        RpcTypeId::from_name("PlayerJumpRpc")
    }

    fn rpc_name() -> &'static str {
        "PlayerJumpRpc"
    }
}
```

### Server: Register Handler

```rust
let mut rpc_registry = RpcRegistry::new();

rpc_registry.register::<PlayerJumpRpc>(|world, rpc| {
    let client_id = rpc.sender;
    let jump = &rpc.payload;

    println!("Player {} jumped with height {}", client_id, jump.height);

    // Apply jump to player entity...

    Ok(())
});
```

### Client: Send RPC

```rust
fn send_jump(&self, client: &mut NetworkClient) {
    let rpc = PlayerJumpRpc { height: 10.0 };

    let serialized = SerializedRpc::from_rpc(
        &rpc,
        client.client_id(),
        RpcTarget::Server
    ).unwrap();

    client.send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());
}
```

### Server: Process Incoming RPCs

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Receive RPCs from clients
    for client_id in self.server.clients().collect::<Vec<_>>() {
        while let Some(data) = self.server.receive(client_id, Channel::ReliableOrdered) {
            if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
                match self.rpc_registry.dispatch(&mut ctx.world, &serialized) {
                    Ok(()) => {}
                    Err(e) => eprintln!("RPC error: {}", e),
                }
            }
        }
    }
    SceneAction::Continue
}
```

## RPC Targets

RPCs can be sent to different destinations:

```rust
pub enum RpcTarget {
    Server,                    // Client → Server
    Client(ClientId),          // Server → One client
    Broadcast,                 // Server → All clients
    BroadcastExcept(ClientId), // Server → All except one
}
```

### Client to Server

```rust
// Client sends input to server
let rpc = PlayerInputRpc { x: 1.0, y: 0.0, jump: true };
let serialized = SerializedRpc::from_rpc(&rpc, client_id, RpcTarget::Server)?;
client.send(Channel::ReliableOrdered, &serialized.to_bytes()?);
```

### Server to One Client

```rust
// Server sends private message to one client
let rpc = ChatMessageRpc { text: "Welcome!".to_string() };
let serialized = SerializedRpc::from_rpc(&rpc, 0, RpcTarget::Client(target_id))?;
server.send(target_id, Channel::ReliableOrdered, &serialized.to_bytes()?);
```

### Server Broadcast

```rust
// Server broadcasts event to all clients
let rpc = GameEventRpc { event_type: "round_start".to_string() };
let serialized = SerializedRpc::from_rpc(&rpc, 0, RpcTarget::Broadcast)?;
server.broadcast(Channel::ReliableOrdered, &serialized.to_bytes()?);
```

### Broadcast Except Sender

```rust
// Relay a message to everyone except the sender
let rpc = ChatMessageRpc { text: message.clone() };
let serialized = SerializedRpc::from_rpc(&rpc, sender_id, RpcTarget::BroadcastExcept(sender_id))?;

for client_id in server.clients().collect::<Vec<_>>() {
    if client_id != sender_id {
        server.send(client_id, Channel::ReliableOrdered, &serialized.to_bytes()?);
    }
}
```

## The Rpc Trait

All RPC types must implement the `Rpc` trait:

```rust
pub trait Rpc: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    /// Unique identifier for this RPC type
    fn rpc_id() -> RpcTypeId;

    /// Human-readable name (for debugging)
    fn rpc_name() -> &'static str;
}
```

### Example Implementations

```rust
// Simple RPC
#[derive(Clone, Serialize, Deserialize)]
struct PingRpc;

impl Rpc for PingRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("Ping") }
    fn rpc_name() -> &'static str { "Ping" }
}

// RPC with data
#[derive(Clone, Serialize, Deserialize)]
struct AttackRpc {
    pub target_id: u64,
    pub damage: f32,
    pub attack_type: String,
}

impl Rpc for AttackRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("Attack") }
    fn rpc_name() -> &'static str { "Attack" }
}

// RPC with complex data
#[derive(Clone, Serialize, Deserialize)]
struct ChatRpc {
    pub channel: ChatChannel,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Clone, Serialize, Deserialize)]
enum ChatChannel {
    Global,
    Team,
    Whisper(u64),
}

impl Rpc for ChatRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("Chat") }
    fn rpc_name() -> &'static str { "Chat" }
}
```

## RpcRegistry

The registry stores handlers and dispatches incoming RPCs:

```rust
let mut registry = RpcRegistry::new();

// Register handlers
registry.register::<JumpRpc>(handle_jump);
registry.register::<AttackRpc>(handle_attack);
registry.register::<ChatRpc>(handle_chat);

// Check if handler exists
if registry.is_registered::<JumpRpc>() {
    println!("Jump handler registered");
}

// Dispatch incoming RPC
match registry.dispatch(&mut world, &serialized_rpc) {
    Ok(()) => { /* handled */ }
    Err(ReplicationError::RpcNotFound(name)) => {
        eprintln!("Unknown RPC: {}", name);
    }
    Err(e) => {
        eprintln!("RPC error: {}", e);
    }
}
```

## Handler Signature

RPC handlers receive the world and the received RPC:

```rust
registry.register::<MyRpc>(|world, rpc| {
    // rpc.sender - who sent this RPC
    // rpc.payload - the deserialized RPC data

    let sender = rpc.sender;
    let data = &rpc.payload;

    // Modify world state...

    Ok(())  // Return Ok or ReplicationError
});
```

### ReceivedRpc

```rust
pub struct ReceivedRpc<T: Rpc> {
    pub payload: T,       // The RPC data
    pub sender: ClientId, // Who sent it (0 = server)
}
```

## Complete Example: Player Input System

### Common Types (shared between client and server)

```rust
use serde::{Serialize, Deserialize};
use ryo2d::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInputRpc {
    pub movement_x: f32,
    pub movement_y: f32,
    pub jump: bool,
    pub attack: bool,
    pub sequence: u32,  // For input ordering
}

impl Rpc for PlayerInputRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("PlayerInput") }
    fn rpc_name() -> &'static str { "PlayerInput" }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEventRpc {
    pub event: GameEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    PlayerScored { player_id: u64, points: i32 },
    RoundStart { round: u32 },
    RoundEnd { winner: Option<u64> },
}

impl Rpc for GameEventRpc {
    fn rpc_id() -> RpcTypeId { RpcTypeId::from_name("GameEvent") }
    fn rpc_name() -> &'static str { "GameEvent" }
}
```

### Server

```rust
struct GameServer {
    server: NetworkServer,
    rpc_registry: RpcRegistry,
    players: HashMap<ClientId, Entity>,
    input_sequences: HashMap<ClientId, u32>,
}

impl Scene for GameServer {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // ... server setup ...

        // Register input handler
        self.rpc_registry.register::<PlayerInputRpc>(|world, rpc| {
            // This closure can't access self, so we handle in update()
            Ok(())
        });
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        self.server.update(Duration::from_secs_f32(dt));

        // Process RPCs manually for access to game state
        for client_id in self.server.clients().collect::<Vec<_>>() {
            while let Some(data) = self.server.receive(client_id, Channel::ReliableOrdered) {
                self.handle_client_rpc(&mut ctx.world, client_id, &data);
            }
        }
        SceneAction::Continue
    }
}

impl GameServer {
    fn handle_client_rpc(&mut self, world: &mut World, client_id: ClientId, data: &[u8]) {
        let Ok(serialized) = SerializedRpc::from_bytes(data) else { return };

        // Check RPC type and handle
        if serialized.rpc_id == PlayerInputRpc::rpc_id() {
            if let Ok(received) = serialized.clone().into_rpc::<PlayerInputRpc>() {
                self.handle_player_input(world, client_id, &received.payload);
            }
        }
    }

    fn handle_player_input(&mut self, world: &mut World, client_id: ClientId, input: &PlayerInputRpc) {
        // Validate sequence (prevent replay attacks)
        let last_seq = self.input_sequences.get(&client_id).copied().unwrap_or(0);
        if input.sequence <= last_seq {
            return; // Old input, ignore
        }
        self.input_sequences.insert(client_id, input.sequence);

        // Apply input to player
        if let Some(&entity) = self.players.get(&client_id) {
            if let Some(mut vel) = world.get_component_mut::<Velocity>(entity) {
                vel.dx = input.movement_x * 200.0;
                vel.dy = input.movement_y * 200.0;
            }

            if input.jump {
                // Handle jump
            }

            if input.attack {
                // Handle attack
            }
        }
    }

    fn broadcast_event(&mut self, event: GameEvent) {
        let rpc = GameEventRpc { event };
        let serialized = SerializedRpc::from_rpc(&rpc, 0, RpcTarget::Broadcast).unwrap();
        self.server.broadcast(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());
    }
}
```

### Client

```rust
struct GameClient {
    client: NetworkClient,
    rpc_registry: RpcRegistry,
    input_sequence: u32,
}

impl Scene for GameClient {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // ... client setup ...

        // Register event handler
        self.rpc_registry.register::<GameEventRpc>(|world, rpc| {
            match &rpc.payload.event {
                GameEvent::PlayerScored { player_id, points } => {
                    println!("Player {} scored {} points!", player_id, points);
                }
                GameEvent::RoundStart { round } => {
                    println!("Round {} starting!", round);
                }
                GameEvent::RoundEnd { winner } => {
                    match winner {
                        Some(id) => println!("Player {} wins!", id),
                        None => println!("Draw!"),
                    }
                }
            }
            Ok(())
        });
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        self.client.update(Duration::from_secs_f32(dt));

        // Send input to server
        self.send_input(&ctx.input);

        // Process server events
        while let Some(data) = self.client.receive(Channel::ReliableOrdered) {
            if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
                let _ = self.rpc_registry.dispatch(&mut ctx.world, &serialized);
            }
        }
        SceneAction::Continue
    }
}

impl GameClient {
    fn send_input(&mut self, input: &InputState) {
        self.input_sequence += 1;

        let rpc = PlayerInputRpc {
            movement_x: if input.key_held(KeyCode::ArrowRight) { 1.0 }
                       else if input.key_held(KeyCode::ArrowLeft) { -1.0 }
                       else { 0.0 },
            movement_y: if input.key_held(KeyCode::ArrowDown) { 1.0 }
                       else if input.key_held(KeyCode::ArrowUp) { -1.0 }
                       else { 0.0 },
            jump: input.key_pressed(KeyCode::Space),
            attack: input.key_pressed(KeyCode::KeyZ),
            sequence: self.input_sequence,
        };

        let serialized = SerializedRpc::from_rpc(
            &rpc,
            self.client.client_id(),
            RpcTarget::Server
        ).unwrap();

        self.client.send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());
    }
}
```

## Error Handling

RPC operations can fail:

```rust
// Serialization might fail
let serialized = SerializedRpc::from_rpc(&rpc, sender, target)?;

// Dispatch might fail
match registry.dispatch(&mut world, &serialized) {
    Ok(()) => { /* success */ }
    Err(ReplicationError::RpcNotFound(name)) => {
        // Unknown RPC type
        eprintln!("Unknown RPC: {}", name);
    }
    Err(ReplicationError::RpcError(msg)) => {
        // Handler returned error
        eprintln!("Handler error: {}", msg);
    }
    Err(e) => {
        // Other error
        eprintln!("Error: {}", e);
    }
}
```

## Best Practices

### 1. Use ReliableOrdered Channel

RPCs are important events that shouldn't be lost:

```rust
// DO: Use reliable channel
client.send(Channel::ReliableOrdered, &rpc_bytes);

// DON'T: RPCs can get lost on unreliable channel
client.send(Channel::Unreliable, &rpc_bytes);
```

### 2. Keep Payloads Small

RPCs are sent frequently. Keep them lightweight:

```rust
// DO: Small, focused RPC
struct JumpRpc;

// DON'T: Large RPC with unnecessary data
struct JumpRpc {
    player_name: String,
    full_inventory: Vec<Item>,
    game_settings: Settings,
    // ... lots of data ...
}
```

### 3. Validate on Server

Never trust client data:

```rust
registry.register::<DamageRpc>(|world, rpc| {
    let damage = &rpc.payload;

    // Validate damage amount
    if damage.amount < 0.0 || damage.amount > MAX_DAMAGE {
        return Err(ReplicationError::RpcError("Invalid damage".into()));
    }

    // Validate target exists and is damageable
    // ...

    Ok(())
});
```

### 4. Use Sequence Numbers

Prevent replay and ordering issues:

```rust
#[derive(Clone, Serialize, Deserialize)]
struct InputRpc {
    // ... input data ...
    sequence: u32,  // Incrementing counter
}

// Server rejects old sequences
if input.sequence <= last_seen_sequence {
    return; // Ignore
}
```

### 5. Register Same RPCs on Both Sides

```rust
// Server
registry.register::<GameEventRpc>(handle_event);

// Client (must also register to receive)
registry.register::<GameEventRpc>(handle_event);
```

## See Also

- [Networking](./networking.md) - Basic networking setup
- [Network Replication](./replication.md) - Automatic state sync
- [ECS Architecture](./ecs.md) - Understanding entities and components
