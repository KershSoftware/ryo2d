# RPC Module - AGENTS.md

## Purpose

The `rpc/` module provides a type-safe Remote Procedure Call (RPC) system for game events that don't fit the component replication model. Use RPCs for one-shot actions and events rather than continuous state.

## When to Use RPCs vs Replication

| Use RPCs | Use Replication |
|----------|-----------------|
| Player actions (jump, attack, use item) | Position, rotation, scale |
| Game events (score, round start) | Health, ammo, status effects |
| Chat messages | Continuous animations |
| One-shot sounds/effects | Interpolated movement |
| Menu/UI interactions | Any state needing prediction |

**Rule of thumb:** If it happens once and doesn't need interpolation, use an RPC. If it's continuous state, use replication.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and documentation |
| `types.rs` | `Rpc` trait, `RpcTypeId`, `RpcTarget`, `SerializedRpc` |
| `registry.rs` | `RpcRegistry` for handler registration and dispatch |

## Key Types

### RpcTypeId

Unique identifier for RPC types (u32 hash of name):

```rust
let id = RpcTypeId::from_name("PlayerJumpRpc");
```

### Rpc Trait

All RPC types must implement this trait:

```rust
pub trait Rpc: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    fn rpc_id() -> RpcTypeId;
    fn rpc_name() -> &'static str;
}
```

### RpcTarget

Specifies where an RPC should be delivered:

```rust
pub enum RpcTarget {
    Server,                    // Client → Server
    Client(ClientId),          // Server → Specific client
    Broadcast,                 // Server → All clients
    BroadcastExcept(ClientId), // Server → All except one
}
```

### ReceivedRpc

Wrapper containing the RPC payload and sender info:

```rust
pub struct ReceivedRpc<T: Rpc> {
    pub payload: T,       // The deserialized RPC
    pub sender: ClientId, // Who sent it (0 = server)
}
```

### SerializedRpc

Network-ready RPC representation:

```rust
// Create from typed RPC
let serialized = SerializedRpc::from_rpc(&my_rpc, sender_id, RpcTarget::Server)?;

// Send over network
let bytes = serialized.to_bytes()?;
client.send(Channel::ReliableOrdered, &bytes);

// Receive and deserialize
let serialized = SerializedRpc::from_bytes(&bytes)?;
let received: ReceivedRpc<MyRpc> = serialized.into_rpc()?;
```

### RpcRegistry

Manages RPC handlers and dispatches incoming RPCs:

```rust
let mut registry = RpcRegistry::new();

// Register handler
registry.register::<PlayerInputRpc>(|world, rpc| {
    let sender = rpc.sender;
    let input = &rpc.payload;

    // Handle the input...
    apply_player_input(world, sender, input);

    Ok(())
});

// Dispatch incoming RPC
registry.dispatch(&mut world, &serialized_rpc)?;
```

## Defining an RPC

```rust
use serde::{Serialize, Deserialize};
use ryo2d::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlayerInputRpc {
    pub movement_x: f32,
    pub movement_y: f32,
    pub jump: bool,
    pub attack: bool,
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

## Usage Patterns

### Client Sending Input to Server

```rust
// Client side
fn send_input(&self, client: &mut NetworkClient) {
    let rpc = PlayerInputRpc {
        movement_x: self.input_x,
        movement_y: self.input_y,
        jump: self.jump_pressed,
        attack: self.attack_pressed,
    };

    let serialized = SerializedRpc::from_rpc(
        &rpc,
        client.client_id(),
        RpcTarget::Server
    ).unwrap();

    client.send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());
}
```

### Server Handling Client RPCs

```rust
// Server side - setup
fn setup_rpc_handlers(&mut self) {
    self.rpc_registry.register::<PlayerInputRpc>(|world, rpc| {
        let client_id = rpc.sender;
        let input = &rpc.payload;

        // Find player entity for this client
        for (entity, (player, mut velocity)) in
            world.query_mut::<(&Player, &mut Velocity)>()
        {
            if player.client_id == client_id {
                velocity.dx = input.movement_x * MOVE_SPEED;
                velocity.dy = input.movement_y * MOVE_SPEED;

                if input.jump {
                    // Handle jump
                }
            }
        }

        Ok(())
    });
}

// Server side - each frame
fn process_client_rpcs(&mut self, server: &mut impl ServerLike) {
    for client_id in server.clients().collect::<Vec<_>>() {
        while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
            if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
                let _ = self.rpc_registry.dispatch(&mut self.world, &serialized);
            }
        }
    }
}
```

### Server Broadcasting Events

```rust
// Server broadcasts game event to all clients
fn broadcast_score_event(&self, server: &mut impl ServerLike, player_id: u64, points: i32) {
    let rpc = ScoreEventRpc { player_id, points };
    let serialized = SerializedRpc::from_rpc(&rpc, 0, RpcTarget::Broadcast).unwrap();
    server.broadcast(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());
}

// Server sends to specific client
fn send_private_message(&self, server: &mut impl ServerLike, target: ClientId, msg: String) {
    let rpc = ChatMessageRpc { message: msg };
    let serialized = SerializedRpc::from_rpc(&rpc, 0, RpcTarget::Client(target)).unwrap();
    server.send(target, Channel::ReliableOrdered, &serialized.to_bytes().unwrap());
}
```

### Client Handling Server RPCs

```rust
// Client side
fn process_server_rpcs(&mut self, client: &mut NetworkClient) {
    while let Some(data) = client.receive(Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
            let _ = self.rpc_registry.dispatch(&mut self.world, &serialized);
        }
    }
}
```

## Design Decisions

### JSON Serialization

RPCs use JSON for simplicity and debugging. The `SerializedRpc` contains:
- `rpc_id`: Type identifier for dispatch
- `name`: Human-readable name for debugging
- `payload`: JSON-serialized RPC data
- `sender`: Who sent the RPC
- `target`: Intended recipient(s)

### Handler Signature

Handlers receive `&mut World` and `&ReceivedRpc<T>`, returning `Result<(), ReplicationError>`. This allows handlers to modify game state and report errors.

### Type-Safe Dispatch

The registry uses the `RpcTypeId` to route RPCs to the correct handler. Unknown RPC types return `ReplicationError::RpcNotFound`.

## Error Handling

```rust
match registry.dispatch(&mut world, &serialized) {
    Ok(()) => { /* Success */ }
    Err(ReplicationError::RpcNotFound(name)) => {
        eprintln!("Unknown RPC: {}", name);
    }
    Err(ReplicationError::RpcError(msg)) => {
        eprintln!("RPC handler error: {}", msg);
    }
    Err(e) => {
        eprintln!("Unexpected error: {}", e);
    }
}
```

## Notes for AI Agents

1. **Use ReliableOrdered channel** - RPCs are important events that shouldn't be lost
2. **Match RPC IDs** - Server and client must use identical `RpcTypeId` values (use same name)
3. **Handle unknown RPCs gracefully** - Dispatch returns `Err` for unregistered types
4. **Keep payloads small** - RPCs are sent frequently; large data should use replication
5. **Sender is always available** - `rpc.sender` tells you who sent the RPC
6. **Server sender is 0** - When server sends RPCs, sender ID is 0
7. **Register before receiving** - Handlers must be registered before RPCs arrive
8. **Don't use RPCs for state** - Continuous state should use component replication
