# Network Replication

The replication system automatically synchronizes game state between server and clients. Instead of manually serializing and sending every component change, you register components for replication and the system handles the rest.

## Overview

Replication provides:
- **Automatic entity sync** - Networked entities appear on all clients
- **Component synchronization** - Registered components stay in sync
- **Delta updates** - Only changed components are sent
- **Full state sync** - New clients receive complete game state
- **Authority control** - Define who can modify each entity

## When to Use Replication

| Use Replication | Use Raw Messages |
|-----------------|------------------|
| Entity transforms (position, rotation) | Custom binary protocols |
| Component state (health, score) | Real-time voice chat |
| Continuous game state | Large file transfers |
| Most game objects | Streaming data |

## Quick Start

### Server Setup

```rust
use ryo2d::prelude::*;
use std::time::Duration;

struct MyServer {
    server: NetworkServer,
    replication: ReplicationManager,
}

impl Scene for MyServer {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Create replication manager
        self.replication = ReplicationManager::new_server();

        // Register components that should sync
        self.replication.register::<Transform>();
        self.replication.register::<Velocity>();

        // Spawn a networked entity
        let entity = self.replication.spawn_networked(
            &mut ctx.world,
            NetworkEntityBuilder::new()
                .server_owned()
                .with_component(Transform::new(100.0, 100.0))
                .with_component(Velocity::new(5.0, 0.0))
        );
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        self.server.update(Duration::from_secs_f32(dt));

        // Handle new clients
        for event in self.server.events() {
            if let NetworkEvent::ClientConnected { client_id } = event {
                // Send full state to new client
                self.replication.send_full_state(&ctx.world, &mut self.server, client_id);
            }
        }

        // Broadcast all changes to clients
        self.replication.broadcast_updates(&ctx.world, &mut self.server);
        SceneAction::Continue
    }
}
```

### Client Setup

```rust
struct MyClient {
    client: NetworkClient,
    replication: ReplicationManager,
}

impl Scene for MyClient {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        self.replication = ReplicationManager::new_client();

        // Register the SAME components as server
        self.replication.register::<Transform>();
        self.replication.register::<Velocity>();
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        self.client.update(Duration::from_secs_f32(dt));

        // Process incoming replication messages
        self.replication.process_incoming(&mut ctx.world, &mut self.client);

        // Entities are now updated! Render them normally.
        SceneAction::Continue
    }
}
```

## Core Concepts

### NetworkId

Every networked entity has a unique `NetworkId`. This ID is consistent across server and all clients.

```rust
// Get network ID from entity
if let Some(network_id) = replication.get_network_id(entity) {
    println!("Entity has network ID: {}", network_id);
}

// Get entity from network ID
if let Some(entity) = replication.get_entity(network_id) {
    // Use the local entity
}
```

### NetworkEntity Component

Entities are marked for replication with the `NetworkEntity` component:

```rust
pub struct NetworkEntity {
    pub network_id: NetworkId,  // Unique identifier
    pub owner: ClientId,        // Who owns this entity
    pub authority: Authority,   // Who can modify it
}
```

### Authority

Authority determines who can modify an entity:

```rust
pub enum Authority {
    Server,              // Server controls (default)
    Client(ClientId),    // Specific client controls
}
```

**Server authority** (default): Server modifies the entity, clients receive updates.

**Client authority**: Used for player-controlled entities where the client sends authoritative input.

```rust
// Server-controlled NPC
let npc = replication.spawn_networked(&mut world,
    NetworkEntityBuilder::new()
        .server_owned()  // authority: Server
        .with_component(Transform::new(0.0, 0.0))
);

// Client-controlled player
let player = replication.spawn_networked(&mut world,
    NetworkEntityBuilder::new()
        .client_owned(client_id)  // authority: Client(client_id)
        .with_component(Transform::new(0.0, 0.0))
);
```

## The Replicated Trait

Components must implement `Replicated` for network sync:

```rust
use serde::{Serialize, Deserialize};
use ryo2d::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Replicated for Health {
    fn component_id() -> ComponentTypeId {
        ComponentTypeId::from_name("Health")
    }

    fn component_name() -> &'static str {
        "Health"
    }

    fn replication_priority() -> u8 {
        5  // 1-255, higher = more frequent updates
    }
}
```

### Built-in Replicated Components

ryo2d provides `Replicated` implementations for common components:

| Component | Priority | Notes |
|-----------|----------|-------|
| `Transform` | 10 | Position, rotation, scale |
| `Velocity` | 5 | Movement speed |

### Priority

Higher priority components are sent more frequently. Use high priority for time-sensitive data like position.

```rust
fn replication_priority() -> u8 {
    10  // High priority - position updates
}

fn replication_priority() -> u8 {
    1   // Low priority - rarely changing data
}
```

## Spawning Entities

### Using NetworkEntityBuilder

```rust
let entity = replication.spawn_networked(&mut world,
    NetworkEntityBuilder::new()
        .server_owned()
        .with_component(Transform::new(x, y))
        .with_component(Velocity::new(0.0, 0.0))
        .with_component(Health { current: 100.0, max: 100.0 })
);
```

### Spawning Player Entities

When a client connects, spawn their player entity:

```rust
for event in server.events() {
    if let NetworkEvent::ClientConnected { client_id } = event {
        // Spawn player with client authority
        let player = replication.spawn_networked(&mut world,
            NetworkEntityBuilder::new()
                .client_owned(client_id)
                .with_component(Transform::new(spawn_x, spawn_y))
                .with_component(PlayerController { client_id })
        );

        // Track player entity
        players.insert(client_id, player);

        // Send full state so they see all existing entities
        replication.send_full_state(&world, &mut server, client_id);
    }
}
```

### Despawning Entities

```rust
// Despawn by entity
replication.despawn_networked(&mut world, entity);

// Despawn by network ID
replication.despawn_by_network_id(&mut world, network_id);
```

## Marking Components Dirty

After modifying a replicated component, mark it dirty so the change is broadcast:

```rust
// Modify the component
if let Some(mut transform) = world.get_component_mut::<Transform>(entity) {
    transform.x += velocity.dx * dt;
    transform.y += velocity.dy * dt;
}

// Mark it dirty for replication
replication.mark_dirty::<Transform>(entity);
```

> **Important:** Always mark components dirty after modification. Unmarked changes won't be sent to clients.

## Full State Sync

When a client connects, send them the complete game state:

```rust
// In server event handling
if let NetworkEvent::ClientConnected { client_id } = event {
    replication.send_full_state(&world, &mut server, client_id);
}
```

This sends all networked entities with all their replicated components.

## ServerLike Trait

The `ServerLike` trait allows replication code to work with both `NetworkServer` and `ListenServer`:

```rust
pub trait ServerLike {
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);
    fn broadcast(&mut self, channel: Channel, data: &[u8]);
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;
    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_>;
    fn is_host(&self, client_id: ClientId) -> bool;
}
```

This means the same replication code works for dedicated servers and listen servers.

## Complete Example: Multiplayer Entities

```rust
use ryo2d::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

struct MultiplayerGame {
    server: Option<ListenServer>,
    replication: ReplicationManager,
    players: HashMap<ClientId, Entity>,
}

impl Scene for MultiplayerGame {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Start server
        let config = NetworkServerConfig {
            port: 5000,
            max_clients: 4,
            protocol_id: 12345,
        };
        self.server = ListenServer::new(config).ok();

        // Setup replication
        self.replication = ReplicationManager::new_server();
        self.replication.register::<Transform>();
        self.replication.register::<Velocity>();

        // Spawn host's player
        if let Some(server) = &self.server {
            let host_player = self.replication.spawn_networked(&mut ctx.world,
                NetworkEntityBuilder::new()
                    .client_owned(server.host_id())
                    .with_component(Transform::new(400.0, 300.0))
            );
            self.players.insert(server.host_id(), host_player);
        }
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        let Some(server) = &mut self.server else { return };
        server.update(Duration::from_secs_f32(dt));

        // Handle connections
        for event in server.events() {
            match event {
                NetworkEvent::ClientConnected { client_id } => {
                    // Spawn new player
                    let player = self.replication.spawn_networked(&mut ctx.world,
                        NetworkEntityBuilder::new()
                            .client_owned(client_id)
                            .with_component(Transform::new(400.0, 300.0))
                    );
                    self.players.insert(client_id, player);

                    // Send full state
                    self.replication.send_full_state(&ctx.world, server, client_id);
                }
                NetworkEvent::ClientDisconnected { client_id, .. } => {
                    // Remove player
                    if let Some(entity) = self.players.remove(&client_id) {
                        self.replication.despawn_networked(&mut ctx.world, entity);
                    }
                }
                _ => {}
            }
        }

        // Move host player with input
        let host_id = server.host_id();
        if let Some(&entity) = self.players.get(&host_id) {
            let mut moved = false;
            if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
                if ctx.input.key_held(KeyCode::ArrowLeft) {
                    transform.x -= 200.0 * dt;
                    moved = true;
                }
                if ctx.input.key_held(KeyCode::ArrowRight) {
                    transform.x += 200.0 * dt;
                    moved = true;
                }
            }
            if moved {
                self.replication.mark_dirty::<Transform>(entity);
            }
        }

        // Broadcast all updates
        self.replication.broadcast_updates(&ctx.world, server);
        SceneAction::Continue
    }
}
```

## Message Flow

Understanding how messages flow helps with debugging:

```
Server                                    Client
  │                                         │
  │  SpawnEntity (ReliableOrdered)          │
  │────────────────────────────────────────►│
  │                                         │
  │  ComponentBatch (Unreliable)            │
  │────────────────────────────────────────►│
  │                                         │
  │  DespawnEntity (ReliableOrdered)        │
  │────────────────────────────────────────►│
  │                                         │
  │  FullStateSync (ReliableOrdered)        │
  │────────────────────────────────────────►│
```

- **SpawnEntity/DespawnEntity**: Reliable, must arrive
- **ComponentBatch**: Unreliable, sent frequently
- **FullStateSync**: Reliable, sent to new clients

## Best Practices

### 1. Register Components on Both Sides

Server and client must register the same component types:

```rust
// Server
replication.register::<Transform>();
replication.register::<Health>();

// Client (must match!)
replication.register::<Transform>();
replication.register::<Health>();
```

### 2. Mark Dirty After Changes

```rust
// DON'T: Forget to mark dirty
transform.x += 10.0;

// DO: Always mark dirty after modification
transform.x += 10.0;
replication.mark_dirty::<Transform>(entity);
```

### 3. Use Appropriate Priorities

```rust
// Position changes frequently - high priority
impl Replicated for Transform {
    fn replication_priority() -> u8 { 10 }
}

// Score changes rarely - low priority
impl Replicated for Score {
    fn replication_priority() -> u8 { 1 }
}
```

### 4. Handle Missing Entities

```rust
// Entity might not exist on client yet
if let Some(entity) = replication.get_entity(network_id) {
    // Safe to use entity
} else {
    // Entity not yet spawned locally
}
```

### 5. Call broadcast_updates Every Frame

```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Even if nothing changed, this advances the generation
    self.replication.broadcast_updates(&ctx.world, &mut self.server);
    SceneAction::Continue
}
```

## Debugging

### Check Replication Stats

```rust
let stats = replication.stats();
println!("Networked entities: {}", stats.networked_entities);
println!("Registered components: {}", stats.registered_components);
println!("Current generation: {}", stats.generation);
```

### Common Issues

**Entities not appearing on client:**
- Check that components are registered on both sides
- Ensure `send_full_state()` is called for new clients
- Verify `process_incoming()` is called each frame

**Changes not syncing:**
- Check that `mark_dirty()` is called after modifications
- Ensure `broadcast_updates()` is called each frame
- Verify the component implements `Replicated`

## See Also

- [Networking](./networking.md) - Basic networking setup
- [RPC System](./rpc.md) - For game events and actions
- [ECS Architecture](./ecs.md) - Understanding entities and components
