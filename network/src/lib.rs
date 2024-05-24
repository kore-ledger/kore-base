// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network package.

#![warn(missing_docs)]

mod behaviour;
pub mod error;
mod node;
mod routing;
mod service;
mod transport;
mod utils;
mod worker;

pub use error::Error;
pub use routing::{Config as RoutingConfig, RoutingNode};
pub use service::NetworkService;
pub use worker::{NetworkError, NetworkState, NetworkWorker};
pub use libp2p::PeerId;
pub use tell::Config as TellConfig;

use serde::Deserialize;

/// The maximum allowed number of established connections per peer.
///
/// Typically, and by design of the network behaviours in this crate,
/// there is a single established connection per peer. However, to
/// avoid unnecessary and nondeterministic connection closure in
/// case of (possibly repeated) simultaneous dialing attempts between
/// two peers, the per-peer connection limit is not set to 1 but 2.
const MAX_CONNECTIONS_PER_PEER: usize = 2;

/// The network configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    /// The user agent.
    pub user_agent: String,

    /// The node type.
    pub node_type: NodeType,

    /// Listen addresses.
    pub listen_addresses: Vec<String>,

    /// Message telling configuration.
    pub tell: tell::Config,

    /// Routing configuration.
    pub routing: routing::Config,

    /// Configures port reuse for local sockets, which implies reuse of listening ports for outgoing connections to enhance NAT traversal capabilities.
    pub port_reuse: bool
}

impl Config {
    /// Create a new configuration.
    pub fn new(
        node_type: NodeType,
        listen_addresses: Vec<String>,
        boot_nodes: Vec<RoutingNode>,
        port_reuse: bool
    ) -> Self {
        Self {
            user_agent: "kore-node".to_owned(),
            node_type,
            listen_addresses,
            tell: tell::Config::default(),
            routing: routing::Config::new(boot_nodes),
            port_reuse
        }
    }
}

/// Type of a node.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub enum NodeType {
    /// Bootstrap node.
    Bootstrap,
    /// Addressable node.
    #[default]
    Addressable,
    /// Ephemeral node.
    Ephemeral,
}



/// Command enumeration for the network service.
pub enum Command {
    /// Start providing the given keys.
    StartProviding {
        /// The keys to provide.
        keys: Vec<String>,
    },
    /// Send a message to the given peer.
    SendMessage {
        /// The peer to send the message to.
        peer: Vec<u8>,
        /// The message to send.
        message: Vec<u8>,
    },
    /// Bootstrap the network.
    Bootstrap,
}

/// Event enumeration for the network service.
#[derive(Debug)]
pub enum Event {
    /// Connected to a bootstrap node.
    ConnectedToBootstrap {
        /// The peer ID of the bootstrap node.
        peer: String,
    },

    /// A message was received.
    MessageReceived {
        /// The peer that sent the message.
        peer: String,
        /// The message.
        message: Vec<u8>,
    },

    /// A message was sent.
    MessageSent {
        /// The peer that the message was sent to.
        peer: String,
    },

    /// A peer was identified.
    PeerIdentified {
        /// The peer ID.
        peer: String,
        /// The peer's address.
        addresses: Vec<String>,
    },

    /// Network state changed.
    StateChanged(NetworkState),

    /// Network error.
    Error(Error),
}
