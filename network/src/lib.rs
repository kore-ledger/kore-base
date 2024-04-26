// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network package.

//mod addressable;
mod behaviour;
//mod bootstrap;
//mod ephemeral;
pub mod error;
mod node;
mod routing;
mod service;
mod transport;
mod utils;
mod worker;

pub use error::Error;
pub use routing::Config as RoutingConfig;
pub use service::NetworkService;
pub use worker::NetworkWorker;

use serde::{Deserialize, Serialize};

/// The maximum allowed number of established connections per peer.
///
/// Typically, and by design of the network behaviours in this crate,
/// there is a single established connection per peer. However, to
/// avoid unnecessary and nondeterministic connection closure in
/// case of (possibly repeated) simultaneous dialing attempts between
/// two peers, the per-peer connection limit is not set to 1 but 2.
const MAX_CONNECTIONS_PER_PEER: usize = 2;

// The maximum number of concurrent established connections that were incoming.
//const MAX_CONNECTIONS_ESTABLISHED_INCOMING: u32 = 10_000;

/// Required protocols for the network.
/*const REQUIRED_PROTOCOLS: [StreamProtocol; 6] = [
    StreamProtocol::new("/kore/routing/1.0.0"),
    StreamProtocol::new("/kore/tell/1.0.0"),
    StreamProtocol::new("/libp2p/circuit/relay/0.2.0/stop"),
    StreamProtocol::new("/ipfs/id/1.0.0"),
    StreamProtocol::new("/ipfs/id/push/1.0.0"),
    StreamProtocol::new("/ipfs/ping/1.0.0"),
];*/

/// The network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The user agent.
    user_agent: String,

    /// The node type.
    node_type: NodeType,

    /// Listen addresses.
    listen_addresses: Vec<String>,

    /// Message telling configuration.
    tell: tell::Config,

    /// Routing configuration.
    routing: routing::Config,
}

/// Type of a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// Bootstrap node.
    Bootstrap,
    /// Addressable node.
    Addressable,
    /// Ephemeral node.
    Ephemeral,
}

/// Command enumeration for the network service.
pub enum Command {
    /// Start providing the given keys.
    StartProviding { keys: Vec<String> },
    /// Send a message to the given peer.
    SendMessage { peer: Vec<u8>, message: Vec<u8> },
    /// Bootstrap the network.
    Bootstrap,
}

/// Event enumeration for the network service.
pub enum Event {

    /// Inbound connection established.
    InboundConnection{ peer: String, address: String},

    /// Outbound connection established.
    OutboundConnection { peer: String, address: String },

    /// A message was received.
    MessageReceived {peer: String, message: Vec<u8>},

    /// A peer was disconnected.
    PeerDisconnected { peer: Vec<u8> },

    /// A peer was identified.
    PeerIdentified { peer: String, addresses: Vec<String> },

    /// Peers founded.
    PeersFounded { key: String, peers: Vec<String> },

}
