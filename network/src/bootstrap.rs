// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Bootstrap node implementation.
//!

use crate::{Config, Error};

use libp2p::{
    dcutr,
    identity::{Keypair, PublicKey},
    noise,
    relay::Behaviour as RelayServer,
    swarm::NetworkBehaviour,
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use prometheus_client::registry::Registry;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The bootstrap node behaviour.
#[derive(NetworkBehaviour)]
pub struct Behaviour {
    /// Common network behaviour.
    common: crate::behaviour::Behaviour,
    /// Direct Connection Upgrade through Relay (DCUTR) behaviour.
    dcutr: dcutr::Behaviour,
    /// Relay server behaviour,
    relay_server: RelayServer,
}

impl Behaviour {
    /// Create a new `Behaviour`.
    pub fn new(
        public_key: &PublicKey,
        config: Config,
        external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
    ) -> Self {
        let peer_id = PeerId::from_public_key(&public_key);

        Self {
            common: crate::behaviour::Behaviour::new(public_key, config, external_addresses),
            dcutr: dcutr::Behaviour::new(peer_id),
            relay_server: RelayServer::new(peer_id, Default::default()),
        }
    }
}

///  Build bootstrap swarm.
pub fn build_swarm(
    key: Keypair,
    config: Config,
    external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
    registry: &mut Registry,
) -> Result<Swarm<Behaviour>, Error> {
    // TCP configuration.
    // With TCP_NODELAY we disable Nagle's algorithm.
    let tcp_config = tcp::Config::new().nodelay(true);

    Ok(SwarmBuilder::with_existing_identity(key)
        .with_tokio()
        .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
        .map_err(|e| Error::Transport(format!("Failed to create TCP transport -> {}", e)))?
        .with_quic()
        .with_dns()
        .map_err(|e| Error::Dns(format!("{}", e)))?
        .with_bandwidth_metrics(registry)
        .with_behaviour(|keypair| Behaviour::new(&keypair.public(), config, external_addresses))
        .map_err(|e| Error::Behaviour(format!("Failed to build behaviour -> {}", e)))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build())
}
