// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network package.

mod addressable;
mod behaviour;
mod circuit;
mod ephemeral;
pub mod error;
mod node;
mod routing;
mod service;
mod utils;

pub use error::Error;

use libp2p::{Multiaddr, PeerId};
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

/// The network protocol version.
/// This is used to identify the protocol version of the network.
const NETWORK_PROTOCOL: &str = "/kore/network/1.0.0";

/// The network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The user agent.
    user_agent: String,

    /// The node type.
    node_type: NodeType,

    /// Transport type.
    transport: TransportType,

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
		/// been passed in `::sc_network::config::NetworkConfiguration::boot_nodes`.
		allow_private_ip: bool,
    },
    /// Memory transport.
    Memory,
}

/// Type of a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// Bootstrap node.
    Bootstrap {
        external_addresses: Vec<String>,
    },
    /// Addressable node.
    Addressable {
        external_addresses: Vec<String>,
    },
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
}
