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

## Notes for AI Agents

1. **Call update() every frame** - Both server and client need regular updates
2. **Drain events and messages** - Use `while let Some()` to get all pending data
3. **Collect clients before iterating** - `server.clients()` returns an iterator that borrows self
4. **Protocol IDs must match** - Mismatched IDs cause silent connection failures
5. **Register same components** - Server and client must register identical component types
6. **Use ServerLike for generic code** - Write replication code against the trait
7. **Handle disconnection gracefully** - Network can fail at any time
