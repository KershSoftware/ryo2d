# Transport Module - AGENTS.md

## Purpose

The `transport/` module provides a unified abstraction layer over different network transports, allowing games to switch between UDP (LAN) and WebRTC (internet with NAT traversal) with minimal code changes.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        NetworkServer / NetworkClient            │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    TransportConfig                        │  │
│  │    ┌─────────────┐         ┌─────────────────────────┐    │  │
│  │    │     Udp     │         │ WebRtc(WebRtcConfig)    │    │  │
│  │    │  (default)  │         │ (matchbox feature)      │    │  │
│  │    └──────┬──────┘         └───────────┬─────────────┘    │  │
│  └───────────┼────────────────────────────┼──────────────────┘  │
│              │                            │                     │
│              ▼                            ▼                     │
│  ┌───────────────────┐        ┌───────────────────────────┐    │
│  │  UdpServerTransport│       │  WebRtcServerTransport    │    │
│  │  UdpClientTransport│       │  WebRtcClientTransport    │    │
│  │  (renet Netcode)   │       │  (matchbox_socket)        │    │
│  └───────────────────┘        └───────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Module Structure

| Path | Purpose |
|------|---------|
| `mod.rs` | `TransportConfig` enum, transport traits |
| `udp.rs` | UDP transport using renet's Netcode |
| `webrtc.rs` | WebRTC transport using matchbox_socket (feature-gated) |
| `AGENTS.md` | This documentation |

## Key Types

### TransportConfig

```rust
pub enum TransportConfig {
    /// Direct UDP (default) - lowest latency, no NAT traversal
    Udp,

    /// WebRTC - NAT hole punching via STUN with TURN fallback
    #[cfg(feature = "matchbox")]
    WebRtc(WebRtcConfig),
}
```

### WebRtcConfig (requires `matchbox` feature)

```rust
pub struct WebRtcConfig {
    pub signaling_url: String,  // e.g., "wss://signaling.example.com"
    pub room_id: String,        // Matchmaking room
    pub ice_servers: Vec<IceServer>,  // STUN/TURN servers
}

impl WebRtcConfig {
    /// Create with default Google STUN servers
    pub fn new(signaling_url: impl Into<String>, room_id: impl Into<String>) -> Self;

    /// Add TURN relay for strict NAT environments
    pub fn with_turn(self, url: &str, username: &str, credential: &str) -> Self;
}
```

### Internal Transport Traits

```rust
pub(crate) trait ServerTransport: Send {
    fn update(&mut self, dt: Duration) -> NetworkResult<()>;
    fn send_packets(&mut self, server: &mut RenetServer);
    fn drain_events(&mut self) -> Vec<NetworkEvent>;
    fn is_ready(&self) -> bool;
}

pub(crate) trait ClientTransport: Send {
    fn update(&mut self, dt: Duration) -> NetworkResult<()>;
    fn send_packets(&mut self, client: &mut RenetClient) -> NetworkResult<()>;
    fn disconnect(&mut self);
    fn disconnect_reason(&self) -> Option<String>;
    fn client_id(&self) -> ClientId;
    fn is_ready(&self) -> bool;
}
```

## Transport Selection Guide

| Scenario | Transport | Reason |
|----------|-----------|--------|
| LAN party | UDP | Lowest latency, simplest setup |
| Development | UDP | No signaling server needed |
| Internet multiplayer | WebRTC | NAT traversal via STUN |
| Strict NAT / mobile | WebRTC + TURN | Guaranteed connectivity |

## Usage Examples

### LAN Game (UDP - default)

```rust
// Server
let config = NetworkServerConfig::default();  // Uses UDP
let server = NetworkServer::new(config)?;

// Client
let config = NetworkClientConfig::udp("192.168.1.100:7777".parse()?, 0x12345678);
let client = NetworkClient::connect(config)?;
```

### Internet Game (WebRTC)

```rust
#[cfg(feature = "matchbox")]
{
    // Server
    let config = NetworkServerConfig {
        transport: TransportConfig::WebRtc(WebRtcConfig::new(
            "wss://signaling.example.com",
            "room-abc123",
        )),
        ..Default::default()
    };
    let server = NetworkServer::new(config)?;

    // Client
    let config = NetworkClientConfig::webrtc(WebRtcConfig::new(
        "wss://signaling.example.com",
        "room-abc123",
    ));
    let client = NetworkClient::connect(config)?;
}
```

### With TURN Fallback

```rust
#[cfg(feature = "matchbox")]
let webrtc = WebRtcConfig::new("wss://signaling.example.com", "room-abc")
    .with_turn("turn:relay.example.com:3478", "user", "pass");
```

## Signaling Server

WebRTC requires a signaling server for peer discovery. Options:

1. **Local testing**: `cargo install matchbox_server && matchbox_server`
2. **Quick test with friends**: `ryo2d netplay start` (uses Cloudflare Tunnel)
3. **Production**: Deploy `matchbox_server` - see `docs/matchbox-server-deploy.md`

## Feature Flag

The `matchbox` feature enables WebRTC support:

```toml
# Cargo.toml
[features]
matchbox = ["matchbox_socket"]

[dependencies]
matchbox_socket = { version = "0.10", optional = true, features = ["ggrs"] }
```

Build with: `cargo build --features matchbox`

## Channel Mapping (WebRTC)

| Ryo2D Channel | Matchbox Channel |
|---------------|------------------|
| `Unreliable` | WebRTC unreliable data channel |
| `ReliableOrdered` | WebRTC reliable ordered channel |
| `ReliableUnordered` | Same reliable channel (renet handles ordering) |

## Notes for AI Agents

1. **Feature-gated** - WebRTC code is behind `#[cfg(feature = "matchbox")]`
2. **Same API** - Once connected, server/client APIs work identically regardless of transport
3. **Polling** - WebRTC uses `pollster::block_on` for sync polling
4. **PeerId mapping** - WebRTC transport internally maps Matchbox PeerIds to renet ClientIds
5. **ICE timing** - WebRTC connections take longer to establish (ICE negotiation)
6. **Default STUN** - `WebRtcConfig::new()` includes Google's public STUN servers
