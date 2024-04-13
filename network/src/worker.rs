// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network worker.
//!

use crate::{
    behaviour::{Behaviour, Event as BehaviourEvent},
    service::NetworkService,
    transport::{build_transport, KoreTransport, RelayClient},
    utils::{convert_external_addresses, is_reachable, is_relay_circuit},
    Command, Config, Error, Event as NetworkEvent, NodeType,
};

use identity::keys::{KeyMaterial, KeyPair};

use libp2p::{
    dcutr::Event as DcutrEvent,
    identity::{ed25519, Keypair},
    metrics::{Metrics, Recorder},
    noise,
    swarm::{self, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use futures::StreamExt;
use prometheus_client::registry::Registry;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use tracing::{error, info, trace, warn};

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

const TARGET_WORKER: &str = "KoreNetwork-Worker";

/// Main network worker. Must be polled in order for the network to advance.
///
/// The worker is responsible for handling the network events and commands.
///
pub struct NetworkWorker {
    /// Network service.
    service: Arc<NetworkService>,

    /// Addresses that the node is listening on.
    /// Updated by the `NetworkWorker` and loaded by the `NetworkService`.
    external_addresses: HashSet<Multiaddr>,

    /// The libp2p swarm.
    swarm: Swarm<Behaviour>,

    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,

    /// The event sender.
    event_sender: mpsc::Sender<NetworkEvent>,

    /// The cancellation token.
    cancel: CancellationToken,

    /// The metrics registry.
    metrics: Metrics,

    /// Ephemaral node flag.
    ephemeral_node: bool,

    /// Pendings outbound messages to the peer
    pending_outbound_messages: HashMap<PeerId, VecDeque<Vec<u8>>>,

    /// Pending inbound messages from the peer
    pending_inbound_messages: HashMap<PeerId, VecDeque<Vec<u8>>>,
}

impl NetworkWorker {
    /// Create a new `NetworkWorker`.
    pub fn new(
        registry: &mut Registry,
        keys: KeyPair,
        config: Config,
        event_sender: mpsc::Sender<NetworkEvent>,
        cancel: CancellationToken,
    ) -> Result<Self, Error> {
        // Create channels to communicate events and commands
        let (command_sender, command_receiver) = mpsc::channel(10000);

        // Prepare the network crypto key.
        let key = {
            let sk = ed25519::SecretKey::try_from_bytes(keys.secret_key_bytes())
                .expect("Invalid keypair");
            let kp = ed25519::Keypair::from(sk);
            Keypair::from(kp)
        };

        // Generate the `PeerId` from the public key.
        let local_peer_id = key.public().to_peer_id();

        // Create the external addresses set.
        let external_addresses = match config.node_type.clone() {
            NodeType::Bootstrap { external_addresses }
            | NodeType::Addressable { external_addresses } => {
                convert_external_addresses(&external_addresses)?
            }
            _ => HashSet::new(),
        };

        // Is Ephemeral?
        let ephemeral_node = config.node_type == NodeType::Ephemeral;

        // Build transport.
        let (transport, relay_client) = build_transport(registry, local_peer_id, &key)?;

        // Create the shared external addresses.
        let shared_external_addresses = Arc::new(Mutex::new(external_addresses.clone()));

        // Create the swarm.
        let swarm = Swarm::new(
            transport,
            Behaviour::new(
                &key.public(),
                config,
                shared_external_addresses.clone(),
                relay_client,
            ),
            local_peer_id,
            swarm::Config::with_tokio_executor(),
        );

        let metrics = Metrics::new(registry);

        let service = Arc::new(NetworkService::new(command_sender)?);

        Ok(Self {
            service,
            external_addresses,
            swarm,
            command_receiver,
            event_sender,
            cancel,
            metrics,
            ephemeral_node,
            pending_outbound_messages: HashMap::default(),
            pending_inbound_messages: HashMap::default(),
        })
    }

    /// Send message to a peer.
    ///
    /// If the peer is reachable, it invokes it directly. Otherwise, it selects the closest
    /// accessible peer, to establish a relay circuit with the peer.
    pub async fn send_message(&mut self, peer: PeerId, message: Vec<u8>) {
        if self.ephemeral_node {
            // Send message
            self.swarm.behaviour_mut().send_message(&peer, message);
        }
    }

    /// Add pending message to peer.
    fn add_pending_outbound_message(&mut self, peer: PeerId, message: Vec<u8>) {
        let pending_messages = self
            .pending_outbound_messages
            .entry(peer)
            .or_insert(VecDeque::new());
        pending_messages.push_back(message);
    }

    /// Send pending messages to peer.
    fn send_pending_outbound_messages(&mut self, peer: PeerId) {
        if let Some(messages) = self.pending_outbound_messages.get(&peer) {
            for message in messages.iter() {
                self.swarm
                    .behaviour_mut()
                    .send_message(&peer, message.clone());
            }
        } else {
            trace!(
                TARGET_WORKER,
                "Pending messages queue is empty for peer {}.",
                peer
            );
        }
    }

    /// Add pending message from peer.
    fn add_pending_inbound_message(&mut self, peer: PeerId, message: Vec<u8>) {
        let pending_messages = self
            .pending_inbound_messages
            .entry(peer)
            .or_insert(VecDeque::new());
        pending_messages.push_back(message);
    }

    /// Run network worker.
    pub async fn run(&mut self) {
        info!(TARGET_WORKER, "Running main loop");
        for address in self.external_addresses.iter() {
            if let Err(error) = self.swarm.listen_on(address.clone()) {
                error!(
                    TARGET_WORKER,
                    "Error listening on address {} with error {}", address, error
                );
            };
        }
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
                trace!(TARGET_WORKER, "Bootstrap en la red kore");
                self.swarm.behaviour_mut().bootstrap();
            }
            Command::StartProviding { keys } => {
                //self.swarm.start_providing(keys);
            }
            _ => {
                self.cancel.cancel();
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!(TARGET_WORKER, "Listening on {:?}", address);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identified { peer_id, info }) => {
                info!(TARGET_WORKER, "Identified peer {}", peer_id);
                if self.pending_outbound_messages.get(&peer_id).is_some() {
                    for addr in info.listen_addrs {
                        if is_reachable(&addr) {
                            // Send pending messages for reachable peer.
                            trace!(
                                TARGET_WORKER,
                                "Sending pending messages to peer {} via a reachable address.",
                                peer_id
                            );
                            self.send_pending_outbound_messages(peer_id);
                        }
                        if is_relay_circuit(&addr) {
                            // Dial to open dcutr connection.
                            if self.swarm.dial(addr).is_ok() {
                                trace!(
                                    TARGET_WORKER,
                                    "Dialing to peer {} for dcutr connection.",
                                    peer_id
                                );
                            } else {
                                error!(
                                    TARGET_WORKER,
                                    "Error dialing to peer {} for dcutr connection.", peer_id
                                );
                            }
                        }
                    }
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Dcutr(DcutrEvent {
                remote_peer_id,
                result,
            })) => {
                if result.is_ok() {
                    // Send pending messages for ephemeral peer.
                    trace!(
                        TARGET_WORKER,
                        "Sending pending messages to peer {} via dcutr.",
                        remote_peer_id
                    );
                    self.send_pending_outbound_messages(remote_peer_id);
                } else {
                    error!(
                        TARGET_WORKER,
                        "Error in dcutr connection to peer {}.", remote_peer_id
                    );
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Message { peer_id, message }) => {
                let result = self
                    .event_sender
                    .send(NetworkEvent::MessageReceived {
                        message: message.message,
                    })
                    .await;
                if result.is_err() {
                    error!(
                        TARGET_WORKER,
                        "Could not receive message from peer {}", peer_id
                    );
                } else {
                    trace!(TARGET_WORKER, "Message received from peer {}", peer_id);
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageSent { peer_id, .. }) => {
                trace!(TARGET_WORKER, "Message sent to peer {}", peer_id);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use identity::keys::KeyPair;

    use libp2p::{
        core::{muxing::StreamMuxerBox, transport::Boxed, upgrade},
        plaintext, yamux, Multiaddr, PeerId, Transport,
    };

    use tokio::{
        pin,
        sync::mpsc::{self, Receiver},
    };

    #[tokio::test]
    async fn test_network_worker() {
        let listen_addr = format!("/memory/{}", rand::random::<u64>());
        let (mut worker, event_receiver) = build_worker(
            vec![],
            NodeType::Bootstrap {
                external_addresses: vec![listen_addr],
            },
        );

        tokio::spawn(async move {
            worker.run().await;
        });
    }

    // Build a relay server.
    fn build_worker(
        boot_nodes: Vec<(String, String)>,
        node_type: NodeType,
    ) -> (NetworkWorker, Receiver<NetworkEvent>) {
        let listen_addr = format!("/memory/{}", rand::random::<u64>());
        let node_type = match node_type {
            NodeType::Bootstrap { .. } => NodeType::Bootstrap {
                external_addresses: vec![listen_addr],
            },
            NodeType::Addressable { .. } => NodeType::Addressable {
                external_addresses: vec![listen_addr],
            },
            _ => node_type,
        };
        let config = create_config(boot_nodes, false, node_type);
        let keys = KeyPair::default();
        let token = CancellationToken::new();
        let mut registry = Registry::default();
        let (event_sender, event_receiver) = mpsc::channel(100);
        let worker = NetworkWorker::new(&mut registry, keys, config, event_sender, token).unwrap();
        (worker, event_receiver)
    }

    // Create a config
    fn create_config(
        boot_nodes: Vec<(String, String)>,
        random_walk: bool,
        node_type: NodeType,
    ) -> Config {
        let private_addr = match node_type {
            NodeType::Bootstrap { .. } => true,
            _ => false,
        };
        let config = crate::routing::Config::new(boot_nodes.clone())
            .with_allow_non_globals_in_dht(true)
            .with_allow_private_ip(private_addr)
            .with_discovery_limit(50)
            .with_dht_random_walk(random_walk);

        Config {
            user_agent: "kore::node".to_owned(),
            node_type,
            tell: Default::default(),
            routing: config,
        }
    }
}
