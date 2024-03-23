// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network worker.
//! 

use crate::{Error, Config, NodeType, Command, service::NetworkService, behaviour::Behaviour};

use identity::keys::{KeyPair, KeyMaterial};

use libp2p::{
    identity::{ed25519, Keypair},
    Multiaddr, PeerId, Swarm, SwarmBuilder, noise, yamux, tcp,
};

use prometheus_client::registry::Registry;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Main network worker. Must be polled in order for the network to advance.
/// 
/// The worker is responsible for handling the network events and commands.
/// 
pub struct NetworkWorker {

    /// Network service.
    //service: NetworkService,
    
    /// Addresses that the node is listening on.
    /// Updated by the `NetworkWorker` and loaded by the `NetworkService`.
	listen_addresses: Arc<Mutex<HashSet<Multiaddr>>>,

    /// The libp2p swarm.
    swarm: Swarm<Behaviour>,
    
    /// The command sender.
    command_sender: mpsc::Sender<Command>,
 
    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,

}

impl NetworkWorker {
    /// Create a new `NetworkWorker`.
    pub fn new(keys: KeyPair, config: Config) -> Result<Self, Error> {
        // Create channels to communicate events and commands
        let (command_sender, command_receiver) = mpsc::channel(10000);

        // Prepare the network crypto key.
        let key = {
            let sk = ed25519::SecretKey::try_from_bytes(keys.secret_key_bytes())
                .expect("Invalid keypair");
            let kp = ed25519::Keypair::from(sk);
            Keypair::from(kp)
        };

        // Create the external addresses set.
        let external_addresses = Arc::new(Mutex::new(HashSet::new()));

        let ext_addr = external_addresses.clone();

        // Create metrics registry.
        let mut bandwidth = Registry::default();

        // Create the swarm.
        let swarm = match config.node_type {

            NodeType::Bootstrap { .. } => {
                let tcp_config = tcp::Config::new().nodelay(true);
                SwarmBuilder::with_existing_identity(key)
                    .with_tokio()
                    .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
                        .map_err(|e| Error::Transport(format!("Failed to create TCP transport -> {}", e)))?
                    .with_quic()
                    .with_dns()
                        .map_err(|e| Error::Dns(format!("{}", e)))?
                    .with_bandwidth_metrics(&mut bandwidth)
                    .with_behaviour(|keypair| {
                        Behaviour::new(
                            &keypair.public(),
                            config,
                            ext_addr,
                            None,
                        )
                    })
                        .map_err(|e| Error::Behaviour(format!("Failed to build behaviour -> {}", e)))?
                    .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                    .build()
            }
            _ => {
                let tcp_config = tcp::Config::new().nodelay(true).port_reuse(true);
                SwarmBuilder::with_existing_identity(key)
                    .with_tokio()
                    .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
                        .map_err(|e| Error::Transport(format!("Failed to create TCP transport -> {}", e)))?
                    .with_quic()
                    .with_dns()
                        .map_err(|e| Error::Dns(format!("{}", e)))?
                    .with_relay_client(noise::Config::new, yamux::Config::default)
                        .map_err(|e| Error::Relay(format!("Failed to build client -> {}", e)))?
                    .with_bandwidth_metrics(&mut bandwidth)
                    .with_behaviour(|keypair, relay_behaviour| {
                        Behaviour::new(
                            &keypair.public(),
                            config,
                            ext_addr,
                            Some(relay_behaviour),
                        )
                    })
                        .map_err(|e| Error::Behaviour(format!("Failed to build behaviour -> {}", e)))?
                    .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                    .build()

            }
        };


        Ok(Self {
            //service,
            listen_addresses: external_addresses,
            swarm,
            command_sender,
            command_receiver,
        })
    }
}