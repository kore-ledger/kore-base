// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network worker.
//!

use crate::{
    behaviour::{Behaviour, Event},
    service::NetworkService,
    utils::convert_external_addresses,
    Command, Config, Error, NodeType,
};

use identity::keys::{KeyMaterial, KeyPair};

use libp2p::{
    identity::{ed25519, Keypair},
    metrics::{Metrics, Recorder},
    noise,
    swarm::SwarmEvent,
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use futures::StreamExt;
use prometheus_client::registry::Registry;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use tracing::{error, warn, info, trace};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

const TARGET_ROUTING: &str = "KoreNetwork-Worker";

/// Main network worker. Must be polled in order for the network to advance.
///
/// The worker is responsible for handling the network events and commands.
///
pub struct NetworkWorker {
    /// Network service.
    service: Arc<NetworkService>,

    /// Addresses that the node is listening on.
    /// Updated by the `NetworkWorker` and loaded by the `NetworkService`.
    external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,

    /// The libp2p swarm.
    swarm: Swarm<Behaviour>,

    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,

    /// The cancellation token.
    cancel: CancellationToken,

    /// The metrics registry.
    metrics: Metrics,
}

impl NetworkWorker {
    /// Create a new `NetworkWorker`.
    pub fn new(keys: KeyPair, config: Config, cancel: CancellationToken) -> Result<Self, Error> {
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
        let external_addresses = match config.node_type.clone() {
            NodeType::Bootstrap { external_addresses }
            | NodeType::Addressable { external_addresses } => {
                Arc::new(Mutex::new(convert_external_addresses(&external_addresses)?))
            }
            _ => Arc::new(Mutex::new(HashSet::new())),
        };

        let ext_addr = external_addresses.clone();

        // Create metrics registry.
        let mut registry = Registry::default();

        let node_type = config.node_type.clone();

        // Create the swarm.
        let swarm = match node_type {
            NodeType::Bootstrap { .. } => {
                trace!(TARGET_ROUTING, "Creating worker for bootstrap node");
                let tcp_config = tcp::Config::new().nodelay(true);
                SwarmBuilder::with_existing_identity(key)
                    .with_tokio()
                    .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
                    .map_err(|e| {
                        Error::Transport(format!("Failed to create TCP transport -> {}", e))
                    })?
                    .with_quic()
                    .with_dns()
                    .map_err(|e| Error::Dns(format!("{}", e)))?
                    .with_bandwidth_metrics(&mut registry)
                    .with_behaviour(|keypair| {
                        Behaviour::new(&keypair.public(), config, ext_addr, None)
                    })
                    .map_err(|e| Error::Behaviour(format!("Failed to build behaviour -> {}", e)))?
                    .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                    .build()
            }
            _ => {
                trace!(TARGET_ROUTING, "Creating worker for addressable or ephemeral node");
                let tcp_config = tcp::Config::new().nodelay(true).port_reuse(true);
                SwarmBuilder::with_existing_identity(key)
                    .with_tokio()
                    .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
                    .map_err(|e| {
                        Error::Transport(format!("Failed to create TCP transport -> {}", e))
                    })?
                    .with_quic()
                    .with_dns()
                    .map_err(|e| Error::Dns(format!("{}", e)))?
                    .with_relay_client(noise::Config::new, yamux::Config::default)
                    .map_err(|e| Error::Relay(format!("Failed to build client -> {}", e)))?
                    .with_bandwidth_metrics(&mut registry)
                    .with_behaviour(|keypair, relay_behaviour| {
                        Behaviour::new(&keypair.public(), config, ext_addr, Some(relay_behaviour))
                    })
                    .map_err(|e| Error::Behaviour(format!("Failed to build behaviour -> {}", e)))?
                    .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                    .build()
            }
        };

        let metrics = Metrics::new(&mut registry);

        let service = Arc::new(NetworkService::new(
            command_sender,
            Arc::new(Mutex::new(registry)),
        )?);

        Ok(Self {
            service,
            external_addresses,
            swarm,
            command_receiver,
            cancel,
            metrics,
        })
    }

    /// Send message to a peer.
    ///
    /// If the peer is reachable, it invokes it directly. Otherwise, it selects the closest
    /// accessible peer, to establish a relay circuit with the peer.
    pub async fn send_message(&mut self, peer: PeerId, message: Vec<u8>)  {
        let behaviour = self.swarm.behaviour_mut();
        if let Some(node) = behaviour.node(&peer) {
            if node.reachable() {
                behaviour.send_message(&peer, message);
            } else {
 
            }
        }
    }

    /// Run network worker.
    pub async fn run(&mut self) {
        trace!(TARGET_ROUTING, "Running main loop");
        loop {
            tokio::select! {
                command = self.command_receiver.recv() => {
                    // Handle commands.
                    if let Some(command) = command {
                        self.handle_command(command).await;
                    }
                }
                event = self.swarm.select_next_some() => {
                    // Handle events.
                    self.handle_event(event).await;
                }
                _ = self.cancel.cancelled() => {
                    break;
                }
            }
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::SendMessage { peer, message } => {
                //self.swarm.send_message(peer, message);
            }
            Command::Bootstrap => {
                //self.swarm.bootstrap(boot_nodes);
            }
            Command::StartProviding { keys } => {
                //self.swarm.start_providing(keys);
            }
            _ => {
                self.cancel.cancel();
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<Event>) {
        match event {
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use libp2p::{
        core::{upgrade, transport::Boxed, muxing::StreamMuxerBox},
        identity,
        plaintext,
        yamux,
        Multiaddr,
        PeerId,
        Transport,
    };

    use futures::io::{AsyncRead, AsyncWrite};
    
    fn build_relay() -> Swarm<Behaviour> {
        unimplemented!("build_relay")
    }

    fn build_client() -> Swarm<Behaviour> {
        unimplemented!("build_client")
    }

    // Upgrade the transport.
    fn upgrade_transport<StreamSink>(
        transport: Boxed<StreamSink>,
        identity: &identity::Keypair,
    ) -> Boxed<(PeerId, StreamMuxerBox)>
    where
        StreamSink: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        transport
            .upgrade(upgrade::Version::V1)
            .authenticate(plaintext::Config::new(identity))
            .multiplex(yamux::Config::default())
            .boxed()
    }
}