// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network worker.
//!

use crate::{
    behaviour::{Behaviour, Event as BehaviourEvent},
    service::NetworkService,
    transport::build_transport,
    utils::{compare_arrays, convert_external_addresses, is_relay_circuit},
    Command, Config, Error, Event as NetworkEvent, NodeType, REQUIRED_PROTOCOLS,
};

use identity::keys::{KeyMaterial, KeyPair};

use libp2p::{
    dcutr::Event as DcutrEvent,
    identity::{ed25519, Keypair},
    multiaddr::Protocol,
    relay::client::Event as RelayClientEvent,
    swarm::{self, SwarmEvent},
    Multiaddr, PeerId, Swarm,
};

use futures::StreamExt;
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{counter::Counter, family::Family},
    registry::Registry,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use tracing::{error, info, trace, warn};

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
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

    /// Ephemaral node flag.
    ephemeral_node: bool,

    /// Relay nodes.
    relay_nodes: Vec<(PeerId, Multiaddr)>,

    /// Pending reservartion to the peer.
    pending_reservation: Option<PeerId>,

    /// Pendings outbound messages to the peer
    pending_outbound_messages: HashMap<PeerId, VecDeque<Vec<u8>>>,

    /// Messages metric.
    messages_metric: Family<MetricLabels, Counter>,
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
        /*println!("{:?}", &REQUIRED_PROTOCOLS);
        println!("{:?}", swarm.behaviour().protocol_names().as_slice());
        
        if compare_arrays(
            &REQUIRED_PROTOCOLS,
            swarm.behaviour().protocol_names().as_slice(),
            true,
        ) {
            error!(TARGET_WORKER, "Missing required protocols");
            return Err(Error::Worker("Missing required protocols".to_owned()));
        }*/

        // Register metrics
        let messages_metric = Family::default();
        registry.register("Message", "", messages_metric.clone());

        let service = Arc::new(NetworkService::new(command_sender)?);

        Ok(Self {
            service,
            external_addresses,
            swarm,
            command_receiver,
            event_sender,
            cancel,
            ephemeral_node,
            relay_nodes: Vec::new(),
            pending_reservation: None,
            pending_outbound_messages: HashMap::default(),
            messages_metric,
        })
    }

    /// Send message to a peer.
    ///
    ///
    async fn send_message(&mut self, peer: PeerId, message: Vec<u8>) {
        // If the peer is known, send the message.
        if self.swarm.behaviour_mut().is_known_peer(&peer) {
            self.swarm.behaviour_mut().send_message(&peer, message);
        // Else, add the message to the pending queue and discover the peer.
        } else {
            self.add_pending_outbound_message(peer, message);
            self.swarm.behaviour_mut().discover(&peer);
        }
    }

    /// Request circuit reservation.
    fn request_circuit_reservation(&mut self, peer: PeerId) {
        let (relay_peer, relay_addr) = match self.relay_node() {
            Some(relay) => relay,
            None => {
                error!(TARGET_WORKER, "No relay nodes available");
                return;
            }
        };
        let listen_addr = relay_addr
            .with(Protocol::P2p(relay_peer))
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(*self.swarm.local_peer_id()));
        if self.swarm.listen_on(listen_addr.clone()).is_err() {
            error!(
                TARGET_WORKER,
                "Transport does not support the listening addresss: {:?}.", listen_addr
            );
            return;
        }
        // Pending reservation to the peer.
        self.pending_reservation = Some(peer);
    }

    /// Add pending message to peer.
    fn add_pending_outbound_message(&mut self, peer: PeerId, message: Vec<u8>) {
        let pending_messages = self.pending_outbound_messages.entry(peer).or_default();
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

    /// Gets the next relay node.
    fn relay_node(&mut self) -> Option<(PeerId, Multiaddr)> {
        // TODO: Trace
        if self.relay_nodes.is_empty() {
            self.relay_nodes = self.swarm.behaviour_mut().boot_nodes();
        }
        if let Some(relay) = self.relay_nodes.pop() {
            Some(relay)
        } else {
            None
        }
    }

    /// Get the network service.
    pub fn service(&self) -> Arc<NetworkService> {
        self.service.clone()
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
                if let Ok(peer) = PeerId::from_bytes(&peer) {
                    self.send_message(peer, message).await;
                } else {
                    error!(TARGET_WORKER, "Invalid peer id");
                }
            }
            Command::Bootstrap => {
                trace!(TARGET_WORKER, "Bootstrap en la red kore");
                self.swarm.behaviour_mut().bootstrap();
            }
            Command::StartProviding { .. } => {
                // TODO: Implement
                //self.swarm.start_providing(keys);
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!(TARGET_WORKER, "Listening on {:?}", address);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identified { peer_id, info }) => {
                // Add known peer to the behaviour.
                info!(TARGET_WORKER, "Identified peer {}", peer_id);
                self.swarm
                    .behaviour_mut()
                    .add_identified_peer(peer_id, *info.clone());

                // Send pending messages.
                if let Some(_) = self.pending_outbound_messages.get(&peer_id) {
                    trace!(
                        TARGET_WORKER,
                        "Sending pending messages to peer {}.",
                        peer_id
                    );
                    self.send_pending_outbound_messages(peer_id);
                }

                // If identified peer has relay address, dial it.
                if let Some(relay_addr) = info
                    .listen_addrs
                    .iter()
                    .find(|addr| is_relay_circuit(*addr))
                {
                    if self.swarm.dial(relay_addr.clone()).is_err() {
                        error!(
                            TARGET_WORKER,
                            "Error dialing relay node {} for peer {}.", relay_addr, peer_id
                        );
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
                //trace!(TARGET_WORKER, "Message received from peer {}", peer_id);
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
                    trace!(TARGET_WORKER, "Message received from peer {}.", peer_id);
                    self.messages_metric
                        .get_or_create(&MetricLabels {
                            fact: Fact::RECEIVED,
                            peer_id: peer_id.to_string(),
                        })
                        .inc();
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageSent { peer_id, .. }) => {
                trace!(TARGET_WORKER, "Message sent to peer {}", peer_id);
                self.messages_metric
                    .get_or_create(&MetricLabels {
                        fact: Fact::SENT,
                        peer_id: peer_id.to_string(),
                    })
                    .inc();

                if self.ephemeral_node {
                    // Request circuit reservation from boot nodes
                    trace!(
                        TARGET_WORKER,
                        "Requesting circuit reservation from peer {}",
                        peer_id
                    );
                    self.request_circuit_reservation(peer_id);
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                RelayClientEvent::ReservationReqAccepted { relay_peer_id, .. },
            )) => {
                // Circuit reservation accepted. Remove pending reservation.
                if self.pending_reservation == Some(relay_peer_id) {
                    trace!(
                        TARGET_WORKER,
                        "Circuit reservation accepted from relay {}. Removing pending reservation.",
                        relay_peer_id
                    );
                    self.pending_reservation = None;
                }
            }
            SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                if self.pending_reservation == peer_id {
                    // Error connecting to peer for circuit reservation.
                    if let Some(relay) = self.pending_reservation {
                        warn!(
                            TARGET_WORKER,
                            "Error connecting to peer {} for circuit reservation", relay
                        );
                        self.request_circuit_reservation(relay);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Metric labels for the messages.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct MetricLabels {
    /// Fact.
    fact: Fact,
    /// Peer ID.
    peer_id: String,
}

/// Fact related to the message (sent or received).
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelValue)]
enum Fact {
    /// Message sent.
    SENT,
    /// Message received.
    RECEIVED,
}

#[cfg(test)]
mod tests {

    use super::*;

    use identity::keys::KeyPair;

    use tokio::sync::mpsc::{self, Receiver};

    #[tokio::test]
    async fn test_network_worker() {
        let listen_addr = format!("/memory/{}", rand::random::<u64>());
        let token = CancellationToken::new();
        let (mut worker, _) = build_worker(
            vec![],
            NodeType::Bootstrap {
                external_addresses: vec![listen_addr],
            },
            token.clone(),
        );

        tokio::spawn(async move {
            worker.run().await;
        });

        token.cancel();
    }

    // Build a relay server.
    fn build_worker(
        boot_nodes: Vec<(String, String)>,
        node_type: NodeType,
        token: CancellationToken,
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
