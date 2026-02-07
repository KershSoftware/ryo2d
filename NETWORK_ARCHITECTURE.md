# Ryo2D Network Architecture

## Overview

Ryo2D provides a **client-server networking system** designed for casual co-op games. The architecture prioritizes simplicity and ease of use while providing the building blocks for responsive multiplayer.

**Key Design Decisions:**
- Host-authoritative model (server owns truth)
- Listen Server support (player hosts while playing)
- Generation-based delta replication (efficient bandwidth)
- Type-safe RPC system (compile-time checked events)
- NetworkAction pattern (dual local + network execution)
- Client-side prediction with server reconciliation
- Signaling-resilient WebRTC (P2P survives signaling server loss)
- User-managed networking (game code controls lifecycle)

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              Game Code                                      │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │              ReplicationManager + RpcRegistry                         │  │
│  │         (automatic entity sync)    (game events)                      │  │
│  └───────────────────────────────┬───────────────────────────────────────┘  │
│                                  │                                          │
│  ┌───────────────────┐     ┌─────┴──────┐     ┌───────────────────┐         │
│  │   NetworkServer   │     │ ServerLike │     │   NetworkClient   │         │
│  │   ListenServer    │◄────┤   trait    ├────►│    (players)      │         │
│  └─────────┬─────────┘     └────────────┘     └─────────┬─────────┘         │
└────────────┼────────────────────────────────────────────┼───────────────────┘
             │                                            │
             ▼                                            ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Transport Abstraction Layer                         │
│  ┌───────────────────────────────┐     ┌───────────────────────────────┐    │
│  │        UDP (default)          │     │     WebRTC (matchbox feat)    │    │
│  │   • Lowest latency            │     │   • NAT traversal (STUN)      │    │
│  │   • LAN / direct connect      │     │   • TURN relay fallback       │    │
│  │   • renet Netcode             │     │   • Internet multiplayer      │    │
│  └───────────────────────────────┘     └───────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Transport Layer

The transport layer provides a unified abstraction over different network backends, allowing games to switch between UDP (LAN) and WebRTC (internet with NAT traversal) with minimal code changes.

### Transport Configuration

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         TransportConfig                                     │
│                                                                             │
│  ┌─────────────────────────────┐    ┌─────────────────────────────────┐     │
│  │    TransportConfig::Udp     │    │  TransportConfig::WebRtc(...)   │     │
│  │        (default)            │    │    (matchbox feature)           │     │
│  │                             │    │                                 │     │
│  │  • Direct UDP sockets       │    │  • STUN hole punching           │     │
│  │  • Lowest latency           │    │  • Optional TURN relay          │     │
│  │  • No NAT traversal         │    │  • Requires signaling server    │     │
│  │  • Best for LAN/dev         │    │  • Best for internet play       │     │
│  └─────────────────────────────┘    └─────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────────────┘
```

| Scenario | Transport | Why |
|----------|-----------|-----|
| LAN party / development | UDP (default) | Lowest latency, simplest setup |
| Internet multiplayer | WebRTC | NAT traversal via STUN |
| Strict NAT / mobile | WebRTC + TURN | Guaranteed connectivity via relay |

### UDP Transport (Default)

Built on [renet](https://github.com/lucaspoffo/renet), a reliable UDP networking library.

- UDP-based with netcode.io authentication
- Protocol IDs must match between server/client (version mismatch prevention)
- Non-blocking I/O (game loop friendly)
- Unsecure mode for LAN/casual games (encryption available if needed)

### WebRTC Transport (Optional)

Enabled with the `matchbox` feature flag. Uses a **locally patched** [matchbox_socket](https://github.com/johanhelsing/matchbox) (`patches/matchbox_socket/`) for WebRTC.

- **STUN** - NAT hole punching for peer-to-peer connections
- **TURN** - Relay fallback for symmetric NAT environments
- **Signaling Server** - Required for initial peer discovery (`matchbox_server`)
- **Survives signaling disconnect** - Once WebRTC peer connections are established, the game continues if the signaling server drops (no new players can join, but existing sessions are unaffected)

WebRtcConfig example:
```rust
let config = WebRtcConfig::new("wss://signaling.example.com", "room-id")
    .with_turn("turn:relay.example.com:3478", "user", "pass");
```

#### Signaling Resilience

The signaling server is only needed for the initial WebRTC handshake. Once peers are connected via data channels, communication is fully peer-to-peer.

```
Signaling Server alive:      Signaling Server dead:
┌──────────────┐             ┌──────────────┐
│   Signaling  │             │   Signaling  │
│   Server     │             │   Server X   │
└──────┬───────┘             └──────────────┘
       │ (new player
       │  handshakes)         Existing P2P connections
       │                      continue working:
┌──────┴───────┐             ┌──────────────┐
│  Player A ◄──────────────► │  Player B    │
│              │  WebRTC P2P │              │
└──────────────┘             └──────────────┘
```

**How it works:**

The upstream matchbox_socket kills the internal message loop (which drives data channel I/O) when the signaling WebSocket errors. Our patch in `patches/matchbox_socket/src/webrtc_socket/socket.rs` removes the `break` from `run_socket()`'s select loop on signaling errors, so the message loop continues driving established data channels.

Additionally:
- The server builder uses `reconnect_attempts(None)` to retry signaling indefinitely, staying available for new players as long as signaling is reachable
- Both transports track a `signaling_failed` flag and wrap socket operations in `catch_unwind` for defense-in-depth
- `signaling_connected()` is exposed on `NetworkServer` and `NetworkClient` so game code can show UI warnings (e.g., "no new players can join")

### Quick Testing with `ryo2d netplay`

For testing internet multiplayer without deploying infrastructure:

```bash
ryo2d netplay start            # Start temp signaling server + tunnel
ryo2d netplay start --room xyz # Custom room name
ryo2d netplay ip               # Show LAN/WAN IPs
```

This uses Cloudflare Tunnel for temporary internet exposure. **Not for production** - deploy your own signaling server for shipped games.

### Channels

Three channels with different delivery guarantees:

| Channel | Delivery | Ordering | Use Case |
|---------|----------|----------|----------|
| **Unreliable** | None | None | Position updates, frequent state (can miss packets) |
| **ReliableOrdered** | Guaranteed | FIFO | Spawns, despawns, RPCs, critical events |
| **ReliableUnordered** | Guaranteed | None | Independent events that don't need ordering |

## Connection Layer

### Server Types

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Server Types                                       │
│                                                                             │
│  ┌─────────────────────────────┐    ┌───────────────────────────────┐       │
│  │      NetworkServer          │    │       ListenServer            │       │
│  │      (Dedicated)            │    │    (Player-Hosted)            │       │
│  │                             │    │                               │       │
│  │  • All clients are remote   │    │  • Host plays while serving   │       │
│  │  • Pure networking logic    │    │  • LOCAL_HOST_ID = 0          │       │
│  │  • No local player          │    │  • Host messages skip network │       │
│  │                             │    │  • Wraps NetworkServer        │       │
│  └─────────────────────────────┘    └───────────────────────────────┘       │
│                                                                             │
│  Both implement ServerLike trait for generic game code                      │
└─────────────────────────────────────────────────────────────────────────────┘
```

**NetworkServer** - Dedicated server (no local player)
- Binds to `0.0.0.0:{port}`
- All clients are remote
- Used for dedicated hosting

**ListenServer** - Player-hosted games
- Host player has reserved `LOCAL_HOST_ID = 0`
- Host messages NOT serialized (zero overhead)
- `all_clients()` includes host, `remote_clients()` excludes host
- `is_host(client_id)` identifies local player

**NetworkClient** - Remote players
- Connection states: connecting → connected → disconnected
- Receives state updates from server
- Sends input/RPCs to server

### ServerLike Trait

Abstracts over server types for generic game code:

| Method | Purpose |
|--------|---------|
| `send(client_id, channel, data)` | Send to specific client |
| `broadcast(channel, data)` | Send to all clients |
| `receive(client_id, channel)` | Get incoming data |
| `clients()` | Iterate all client IDs |
| `is_host(client_id)` | Check if client is local host |

### Connection Events

| Event | When |
|-------|------|
| `ClientConnected { client_id }` | New client joined (server) |
| `ClientDisconnected { client_id, reason }` | Client left (server) |
| `Connected` | Successfully connected (client) |
| `Disconnected { reason }` | Lost connection (client) |

### Signaling State (WebRTC only)

| Method | Returns | Meaning |
|--------|---------|---------|
| `server.signaling_connected()` | `bool` | `false` = signaling server lost, no new players can join |
| `client.signaling_connected()` | `bool` | `false` = signaling server lost (peer connection still works) |

Always returns `true` for UDP transport (no signaling server involved).

## Replication System

Automatic entity and component synchronization between server and clients.

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Replication Flow                                     │
│                                                                             │
│  Server                                                                     │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  1. Game modifies components                                        │    │
│  │  2. mark_dirty<T>(entity) flags changes                             │    │
│  │  3. DirtyTracker records (entity, component, generation)            │    │
│  │  4. broadcast_updates() sends deltas to clients                     │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                              │                                              │
│                              ▼ ComponentUpdateMessage                       │
│  Clients                                                                    │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  1. process_incoming() receives messages                            │    │
│  │  2. Deserialize component data                                      │    │
│  │  3. Apply to local entity via NetworkId lookup                      │    │
│  │  4. Acknowledge generation to server                                │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Core Types

| Type | Purpose |
|------|---------|
| **NetworkId** | Unique u64 identifier for networked entities (server-generated) |
| **NetworkEntity** | Component marking entity for replication (contains network_id, owner, authority) |
| **Authority** | Who controls entity: `Server` or `Client(ClientId)` |
| **Replicated** | Trait for components that sync over network |
| **ComponentTypeId** | FNV-1a hash of component type name (deterministic across machines) |
| **DirtyTracker** | Generation-based change tracking per entity/component |
| **ReplicationManager** | Central hub for spawn/despawn/sync operations |

### Authority Model

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Authority                                          │
│                                                                             │
│  Authority::Server              Authority::Client(id)                       │
│  ┌─────────────────────┐        ┌───────────────────────┐                   │
│  │  • NPCs, enemies    │        │  • Player entities    │                   │
│  │  • World objects    │        │  • Client-owned items │                   │
│  │  • Server controls  │        │  • Client sends input │                   │
│  │  • Default for most │        │  • Server validates   │                   │
│  └─────────────────────┘        └───────────────────────┘                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Delta Tracking (Generations)

Instead of per-component acknowledgments, uses generation-based tracking:

1. Server maintains a **current generation** counter (increments each sync cycle)
2. When component changes, record `(entity, component_type, generation)`
3. Each client tracks **last acknowledged generation**
4. On sync, send only components with `generation > client_ack_generation`
5. New clients receive **full state** (all generations)

Benefits:
- Single ack per client (not per component)
- Automatic catch-up for late joiners
- Efficient bandwidth for frequently-changing components

### Replication Messages

| Message | Channel | Purpose |
|---------|---------|---------|
| `SpawnEntityMessage` | ReliableOrdered | Create entity with initial components |
| `DespawnEntityMessage` | ReliableOrdered | Remove entity |
| `ComponentUpdateMessage` | Unreliable | Single component delta |
| `ComponentBatchMessage` | Unreliable | Multiple component deltas (efficient) |
| `FullStateSyncMessage` | ReliableOrdered | Complete state for new clients |

### Built-in Replicated Components

| Component | Priority | Notes |
|-----------|----------|-------|
| Transform | 10 | Position, rotation, scale |
| Velocity | 5 | Movement vector |

Custom components implement the `Replicated` trait with priority 1-255 (higher = more frequent updates).

## RPC System

One-shot game events that don't fit the continuous replication model.

### When to Use

| Use RPCs | Use Replication |
|----------|-----------------|
| Player actions (jump, attack) | Position, velocity |
| Game events (score, round start) | Health, ammo |
| Chat messages | Animation state |
| Sound/effect triggers | Cooldown timers |
| UI notifications | Buff/debuff status |

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RPC Flow                                          │
│                                                                             │
│  Sender                          Network                        Receiver    │
│  ┌──────────────┐               ┌──────────┐               ┌──────────────┐ │
│  │ Create RPC   │               │ Serialize│               │ RpcRegistry  │ │
│  │ PlayerInput  │──► to_bytes ──│ + Send   │──► receive ──►│ dispatch()   │ │
│  │ { x, y, jump}│               │ Reliable │               │ ↓            │ │
│  └──────────────┘               └──────────┘               │ Handler(rpc) │ │
│                                                            └──────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Core Types

| Type | Purpose |
|------|---------|
| **Rpc** | Trait for RPC payload types (requires Serialize + Deserialize) |
| **RpcTypeId** | FNV-1a hash of RPC type name |
| **RpcTarget** | Destination: `Server`, `Client(id)`, `Broadcast`, `BroadcastExcept(id)` |
| **SerializedRpc** | Network format with type_id, payload, sender, target |
| **ReceivedRpc<T>** | Deserialized RPC with sender metadata |
| **RpcRegistry** | Handler dispatch by RpcTypeId |

### RPC Targets

| Target | Direction | Use Case |
|--------|-----------|----------|
| `Server` | Client → Server | Player input, requests |
| `Client(id)` | Server → Client | Personal notifications |
| `Broadcast` | Server → All | Game state changes |
| `BroadcastExcept(id)` | Server → Others | "Player X did Y" (skip X) |

### Handler Registration

Handlers receive the deserialized RPC with sender info and can modify the World:

```
RpcRegistry
├── PlayerInputRpc → handle_player_input(&mut World, &ReceivedRpc<PlayerInputRpc>)
├── ChatMessageRpc → handle_chat(&mut World, &ReceivedRpc<ChatMessageRpc>)
└── GameEventRpc → handle_game_event(&mut World, &ReceivedRpc<GameEventRpc>)
```

## NetworkAction System

Actions that execute both locally (for immediate feedback) and over the network (for synchronization). Prevents the common bug of implementing local logic but forgetting the network side, or vice versa.

### When to Use

| Use NetworkAction | Use RPC |
|-------------------|---------|
| Actions that modify world state on both sides | One-directional messages (server → client notifications) |
| Respawn, teleport, ability activation | Chat messages, score updates |
| Anything needing immediate local feedback + server sync | Events that only the server processes |

### Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│                      NetworkAction Flow                                  │
│                                                                          │
│  execute_action(action, ctx, server)                                     │
│       │                                                                  │
│       ├─► action.apply(ctx)           // Execute locally (immediate)     │
│       └─► serialize + send to server  // Send for remote execution       │
│                                                                          │
│  Server receives:                                                        │
│       └─► action.apply(server_ctx)    // Execute on server world         │
└──────────────────────────────────────────────────────────────────────────┘
```

### Core Types

| Type | Purpose |
|------|---------|
| **NetworkAction** | Trait: `action_id()`, `action_name()`, `apply(&self, ctx)` |
| **ActionContext** | Provides `&mut World` to the action's `apply` method |
| **execute_action** | Run locally + send to server |
| **execute_action_broadcast** | Run locally + broadcast to all clients |
| **apply_received_action** | Process an action received from the network |

## Client-Side Prediction

Smooth client experience by predicting entity positions locally while waiting for server authority.

### Core Types

| Type | Purpose |
|------|---------|
| **Predicted** | Component marking entities for client-side prediction |
| **PredictionHelper** | Processes server updates, applying corrections with smoothing |
| **ServerPositionUpdate** | Server-authoritative position data |

### How It Works

```
Client Input ──► Local Prediction (immediate movement)
                         │
Server Update ──► Compare with prediction
                         │
                  ┌──────┴──────┐
                  │ Close match │ → Accept (smooth, no visible correction)
                  │ Big delta   │ → Snap/lerp to server position
                  │ Force flag  │ → Immediate correction (respawn, teleport)
                  └─────────────┘
```

`Predicted::force_correction()` forces the client to accept the server position without smoothing — used for respawns, teleports, and other discontinuous movements.

## Integration Pattern

Networking is **user-managed** (unlike engine-managed systems like audio):

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     Game Integration Pattern                                │
│                                                                             │
│  struct GameScene {                                                         │
│      server: Option<ListenServer>,      // If hosting                       │
│      client: Option<NetworkClient>,     // If joining                       │
│      replication: ReplicationManager,                                       │
│      rpcs: RpcRegistry,                                                     │
│  }                                                                          │
│                                                                             │
│  fn update() {                                                              │
│      // 1. Update network transport                                         │
│      server.update(dt);                                                     │
│      client.update(dt);                                                     │
│                                                                             │
│      // 2. Process incoming messages                                        │
│      replication.process_incoming(&mut world, &mut server);                 │
│      rpcs.process_incoming(&mut world, &mut server);                        │
│                                                                             │
│      // 3. Game logic (modifies components)                                 │
│      // ... game code ...                                                   │
│                                                                             │
│      // 4. Mark dirty and broadcast                                         │
│      replication.mark_dirty::<Transform>(player_entity);                    │
│      replication.broadcast_updates(&mut server);                            │
│  }                                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

## ListenServer Optimization

When using ListenServer, the host player (`LOCAL_HOST_ID = 0`) is special:

- Messages to host skip serialization entirely
- `is_host(client_id)` returns true for local player
- `remote_clients()` excludes host from iteration
- Zero network overhead for host's own updates

This makes ListenServer efficient for the common "host plays too" scenario.

## Message Flow Examples

### Entity Spawn (Server → Clients)

```
Server: spawn_networked(Transform, Velocity, Health)
    │
    ├─► Assign NetworkId (monotonic counter)
    ├─► Create NetworkEntity component
    ├─► Build SpawnEntityMessage with serialized components
    │
    ▼
Broadcast via ReliableOrdered
    │
    ▼
Clients: process_incoming()
    ├─► Deserialize SpawnEntityMessage
    ├─► Create local entity with components
    └─► Register NetworkId → Entity mapping
```

### Component Update (Server → Clients)

```
Server: game logic changes Transform
    │
    ├─► mark_dirty<Transform>(entity)
    ├─► DirtyTracker records (entity, Transform, generation=42)
    │
    ▼
broadcast_updates()
    │
    ├─► For each client:
    │   └─► If generation > client_last_ack:
    │       └─► Send ComponentUpdateMessage
    │
    ▼
Clients: process_incoming()
    ├─► Lookup entity by NetworkId
    └─► Apply deserialized Transform
```

### RPC (Client → Server)

```
Client: send PlayerInputRpc { jump: true }
    │
    ├─► Serialize to SerializedRpc
    ├─► Set target = RpcTarget::Server
    │
    ▼
Send via ReliableOrdered
    │
    ▼
Server: process_incoming()
    ├─► Deserialize SerializedRpc
    ├─► RpcRegistry.dispatch()
    └─► Handler: apply_player_input(&mut world, rpc)
```

## File Structure

```
src/core/network/
├── mod.rs                 # NetworkServer, ListenServer, NetworkClient, Channel
├── action.rs              # NetworkAction trait (dual local+network execution)
├── debug.rs               # Network debug overlays
├── input.rs               # Networked input handling
├── prediction.rs          # Client-side prediction (Predicted component)
├── AGENTS.md              # Module documentation
├── transport/
│   ├── mod.rs             # TransportConfig, ServerTransport/ClientTransport traits
│   ├── udp.rs             # UDP transport (renet Netcode)
│   ├── webrtc.rs          # WebRTC transport (matchbox) [matchbox feature]
│   └── AGENTS.md          # Transport layer documentation
├── replication/
│   ├── mod.rs             # Exports
│   ├── types.rs           # NetworkId, NetworkEntity, Authority
│   ├── component.rs       # Replicated trait, ComponentRegistry
│   ├── dirty.rs           # DirtyTracker
│   ├── builder.rs         # NetworkEntityBuilder
│   ├── manager.rs         # ReplicationManager
│   ├── messages.rs        # All message types
│   ├── networked_value.rs # NetworkedValue for single-value sync
│   └── AGENTS.md          # Replication documentation
└── rpc/
    ├── mod.rs             # Exports
    ├── types.rs           # Rpc trait, RpcTypeId, RpcTarget
    ├── registry.rs        # RpcRegistry
    └── AGENTS.md          # RPC documentation

patches/matchbox_socket/   # Patched matchbox_socket (signaling resilience)
```

## See Also

- [Networking Guide](docs/src/concepts/networking.md) - Getting started with multiplayer
- [Replication Guide](docs/src/concepts/replication.md) - Entity synchronization details
- [RPC Guide](docs/src/concepts/rpc.md) - Remote procedure calls
- [Matchbox Server Deployment](docs/matchbox-server-deploy.md) - Deploy signaling server for WebRTC
