// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Addressable node implementation.
//! 

use crate::{Config, Error, behaviour::BehaviourService};

use libp2p::{
    dcutr, tcp, noise, yamux,
    identity::{Keypair, PublicKey},
    relay::{Behaviour as RelayServer, client::Behaviour as RelayClient},
    swarm::NetworkBehaviour,
    Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The addressable node behaviour.
#[derive(NetworkBehaviour)]
pub struct Behaviour {
    /// Common network behaviour.
    common: crate::behaviour::Behaviour,
    /// Direct Connection Upgrade through Relay (DCUTR) behaviour.
    dcutr: dcutr::Behaviour,
    /// Relay client behaviour.
    relay_client: RelayClient,
    /// Relay server behaviour,
    relay_server: RelayServer,
}

impl Behaviour {
    /// Create a new `Behaviour`.
    pub fn new(
        public_key: &PublicKey,
        config: Config,
        external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
        relay_client: RelayClient,
    ) -> Self {
        let peer_id = PeerId::from_public_key(&public_key);

        Self {
            common: crate::behaviour::Behaviour::new(
                public_key,
                config,
                external_addresses,
            ),
            dcutr: dcutr::Behaviour::new(peer_id),
            relay_client,
            relay_server: RelayServer::new(peer_id, Default::default()),
        }
    }
}

///  Build ephemeral swarm.
pub fn build_swarm(
    key: Keypair,
    config: Config,
    external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
) -> Result<Swarm<Behaviour>, Error> {
	
    // TCP configuration.
    // With TCP_NODELAY we disable Nagle's algorithm and with PORT_REUSE we allow reusing the port
    // in ephemeral nodes.
    let tcp_config = tcp::Config::new().nodelay(true).port_reuse(true);

    Ok(SwarmBuilder::with_existing_identity(key)
        .with_tokio()
        .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
            .map_err(|e| Error::Transport(format!("Failed to create TCP transport -> {}", e)))?
        .with_quic()
        .with_dns()
            .map_err(|e| Error::Dns(format!("{}", e)))?
        .with_relay_client(noise::Config::new, yamux::Config::default)
            .map_err(|e| Error::Relay(format!("Failed to build client -> {}", e)))?
        .with_behaviour(|keypair, relay_behaviour| {
            Behaviour::new(
                &keypair.public(),
                config,
                external_addresses,
                relay_behaviour,
            )
        })
            .map_err(|e| Error::Behaviour(format!("Failed to build behaviour -> {}", e)))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build())
}
