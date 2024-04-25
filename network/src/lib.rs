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

use libp2p::swarm::StreamProtocol;

use serde::{Deserialize, Serialize};

/// The maximum allowed number of established connections per peer.
///
/// Typically, and by design of the network behaviours in this crate,
/// there is a single established connection per peer. However, to
/// avoid unnecessary and nondeterministic connection closure in
/// case of (possibly repeated) simultaneous dialing attempts between
/// two peers, the per-peer connection limit is not set to 1 but 2.
const MAX_CONNECTIONS_PER_PEER: usize = 2;

/// The maximum number of concurrent established connections that were incoming.
const MAX_CONNECTIONS_ESTABLISHED_INCOMING: u32 = 10_000;

/// Required protocols for the network.
const REQUIRED_PROTOCOLS: [StreamProtocol; 6] = [
    StreamProtocol::new("/kore/routing/1.0.0"),
    StreamProtocol::new("/kore/tell/1.0.0"),
    StreamProtocol::new("/libp2p/circuit/relay/0.2.0/stop"),
    StreamProtocol::new("/ipfs/id/1.0.0"),
    StreamProtocol::new("/ipfs/id/push/1.0.0"),
    StreamProtocol::new("/ipfs/ping/1.0.0"),
];

/// The network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The user agent.
    user_agent: String,

    /// The node type.
    node_type: NodeType,

    /// Message telling configuration.
    tell: tell::Config,

    /// Routing configuration.
    routing: routing::Config,
}

/// The transport type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransportType {
    /// Normal
    Normal {
        /// If true, the network will use mDNS to discover other libp2p nodes on the local network
        /// and connect to them if they support the same chain.
        enable_mdns: bool,

        /// If true, allow connecting to private IPv4/IPv6 addresses (as defined in
        /// [RFC1918](https://tools.ietf.org/html/rfc1918)). Irrelevant for addresses that have
        /// been passed in `network::routing::Config::boot_nodes`.
        allow_private_ip: bool,
    },
    /// Memory transport.
    Memory,
}

/// Type of a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// Bootstrap node.
    Bootstrap { external_addresses: Vec<String> },
    /// Addressable node.
    Addressable { external_addresses: Vec<String> },
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
    MessageReceived { message: Vec<u8> },
    PeerDisconnected { peer: Vec<u8> },
}
