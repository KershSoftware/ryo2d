# matchbox_socket (patched)

Local patch of [matchbox_socket](https://github.com/johanhelsing/matchbox) v0.10.0.

## Patch: Survive signaling server disconnection

The upstream `run_socket()` breaks the select loop when the signaling server
disconnects, which kills the message loop and drops all WebRTC data channel
MPSC bridges — even though the peer-to-peer connections are still alive.

This patch removes the `break` on signaling errors so the message loop
continues driving established data channels after signaling drops.
