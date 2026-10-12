//! Connection messages for game code to write when the network layer reports
//! a change (`ctx.write_message`) and for other logic to react to
//! (`ctx.messages`). The network types report connections through
//! `NetworkEvent`; nothing writes these automatically.

use super::ClientId;
use crate::core::ecs::Message;

/// A client joined (host only): write it when `ListenServer` reports one.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConnected {
  pub client_id: ClientId,
}

/// A client left (host only): write it when `ListenServer` reports one.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientDisconnected {
  pub client_id: ClientId,
}

/// The local client connected (client only).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedToServer {
  /// The client ID the server assigned
  pub client_id: ClientId,
}

/// The local client lost its server connection (client only).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectedFromServer {
  pub reason: DisconnectReason,
}

/// Why the local client was disconnected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisconnectReason {
  #[default]
  Unknown,
  /// The server closed the connection
  ServerClosed,
  Timeout,
  /// Kicked by the server
  Kicked,
}
