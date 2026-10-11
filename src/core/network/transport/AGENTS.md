# AGENTS.md - Transport Module

## Signaling Server

WebRTC needs a signaling server for peer discovery:

1. **Local testing**: `cargo install matchbox_server && matchbox_server`
2. **Quick test with friends**: `ryo2d netplay start` (uses Cloudflare Tunnel)
3. **Production**: Deploy `matchbox_server` - see `docs/architecture/matchbox-server.md`

## Feature Flag

WebRTC is behind the `matchbox` feature: `cargo build --features matchbox` (also needed to compile/test `webrtc.rs`).

`matchbox_socket` is patched via `[patch.crates-io]` → `patches/matchbox_socket`. Upstream kills P2P data channels when signaling drops, and the patch keeps them alive. Keep the patch in sync when bumping the version.

## Channel Mapping (WebRTC)

`ReliableOrdered` and `ReliableUnordered` both use the reliable data channel (index 0), and `Unreliable` uses index 1. Unordered delivery isn't distinct on WebRTC.

## Notes for AI Agents

1. **Same API** - Once connected, server/client APIs work the same on either transport
2. **Message loop thread** - The matchbox message-loop future runs on a background thread via `pollster::block_on`. It must keep running for signaling
3. **PeerId mapping** - The WebRTC transport maps Matchbox `PeerId`s to renet `ClientId`s internally
4. **ICE timing** - WebRTC connections take longer to establish (ICE negotiation)
