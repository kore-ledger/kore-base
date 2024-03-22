// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network circuit
//!

use libp2p::{
    dcutr::Behaviour as Dcutr,
    relay::{
        client::{Behaviour as RelayClient, Transport},
        Behaviour as RelayServer,
    },
    swarm::behaviour::toggle::{Toggle, ToggleConnectionHandler},
    Multiaddr, PeerId,
};

use serde::{Deserialize, Serialize};

/// The network circuit behaviour.
pub struct Behaviour {
    /// The relay client behaviour.
    client: Toggle<RelayClient>,
    /// The relay server behaviour.
    server: Toggle<RelayServer>,
    /// The Direct Connection Upgrade through Relay (DCUTR) behaviour.
    dcutr: Toggle<Dcutr>,
}

impl Behaviour {
    /// Creates a new network circuit behaviour.
    pub fn new(peer_id: PeerId, config: Option<Config>) -> (Self, Transport) {
        if let Some(config) = config {
            unimplemented!("Relay server behaviour")
        } else {
            let dcutr = Dcutr::new(peer_id);
            unimplemented!("Relay client behaviour")
        }

        unimplemented!("Behaviour::new")
    }
}

/// The network circuit configuration. It is only required for relay server behavior.
///
/// The relay server configuration is optional, and it is only required if the node is going to
/// act as a relay server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {}

pub enum RelayMode {}
