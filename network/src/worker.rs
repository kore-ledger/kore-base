// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network worker.
//!

use crate::{
    behaviour::{Behaviour, Event as BehaviourEvent},
    service::NetworkService,
    transport::build_transport,
    utils::{convert_external_addresses, is_relay_circuit},
    Command, Config, Error, Event as NetworkEvent, NodeType,
};

use identity::keys::{KeyMaterial, KeyPair};

use libp2p::{
    core::ConnectedPoint,
    dcutr::Event as DcutrEvent,
    identity::{ed25519, Keypair},
    multiaddr::Protocol,
    relay::{client::Event as RelayClientEvent, Event as RelayServerEvent},
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
    collections::{HashMap, VecDeque},
    sync::{atomic::AtomicU16, Arc, Mutex, RwLock},
};

const TARGET_WORKER: &str = "KoreNetwork-Worker";

/// Main network worker. Must be polled in order for the network to advance.
///
/// The worker is responsible for handling the network events and commands.
///
pub struct NetworkWorker {
    /// Local Peer ID.
    local_peer_id: PeerId,

    /// Network service.
    service: Arc<RwLock<NetworkService>>,

    /// The libp2p swarm.
    swarm: Swarm<Behaviour>,

    /// The network state.
    state: NetworkState,

    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,

    /// The event sender.
    event_sender: mpsc::Sender<NetworkEvent>,

    /// The cancellation token.
    cancel: CancellationToken,

    /// Node type.
    node_type: NodeType,

    /// Dynamic list of boot nodes.
    boot_nodes: Vec<(PeerId, Multiaddr)>,

    /// Relay circuits.
    relay_circuits: HashMap<PeerId, Multiaddr>,

    /// Pending reservartions.
    pending_reservations: HashMap<PeerId, PeerId>,

    /// Pendings outbound messages to the peer
    pending_outbound_messages: HashMap<PeerId, VecDeque<Vec<u8>>>,

    /// Connect attempts.
    connect_attempts: Arc<AtomicU16>,

    /// Maximum attempts to dial a peer or request a circuit relay.
    max_attempts: u16,

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

        // Create the listen addressess.
        let external_addresses = convert_external_addresses(&config.listen_addresses)?;

        // Is Ephemeral?
        let node_type = config.node_type.clone();

        // Set the maximum attempts.
        let max_attempts = config.routing.boot_nodes().len() as u16;

        // Build transport.
        let (transport, relay_client) = build_transport(registry, local_peer_id, &key)?;

        // Create the shared external addresses.
        let shared_external_addresses = Arc::new(Mutex::new(external_addresses.clone()));

        // Create the swarm.
        let mut swarm = Swarm::new(
            transport,
            Behaviour::new(
                &key.public(),
                config.clone(),
                shared_external_addresses.clone(),
                relay_client,
            ),
            local_peer_id,
            swarm::Config::with_tokio_executor(),
        );

        // Add confirmed external addresses.
        match config.node_type {
            NodeType::Bootstrap | NodeType::Addressable => {
                for addr in external_addresses.iter() {
                    swarm.add_external_address(addr.clone());
                }
            }
            _ => {}
        }

        // Register metrics
        let messages_metric = Family::default();
        registry.register(
            "Messages",
            "Counts messages sent or received from other peers.",
            messages_metric.clone(),
        );

        let service = Arc::new(RwLock::new(NetworkService::new(command_sender)?));

        if external_addresses.is_empty() {
            // Listen on all tcp addresses.
            if swarm
                .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
                .is_err()
            {
                error!(TARGET_WORKER, "Error listening on all interfaces");
                panic!("Error listening on all interfaces");
            }
        } else {
            // Listen on the external addresses.
            for addr in external_addresses.iter() {
                if swarm.listen_on(addr.clone()).is_err() {
                    error!(
                        TARGET_WORKER,
                        "Transport does not support the listening addresss: {:?}.", addr
                    );
                    panic!(
                        "Transport does not support the listening addresss: {:?}.",
                        addr
                    );
                }
            }
        }

        Ok(Self {
            local_peer_id,
            service,
            swarm,
            state: NetworkState::Start,
            command_receiver,
            event_sender,
            cancel,
            node_type,
            boot_nodes: Vec::new(),
            relay_circuits: HashMap::default(),
            pending_reservations: HashMap::default(),
            pending_outbound_messages: HashMap::default(),
            connect_attempts: Arc::new(AtomicU16::new(0)),
            max_attempts,
            messages_metric,
        })
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Send message to a peer.
    ///
    ///
    fn send_message(&mut self, peer: PeerId, message: Vec<u8>) {
        self.add_pending_outbound_message(peer, message.clone());
        // It the peer has a relay circuit, dial it.
        if let Some(relay_addr) = self.relay_circuits.get(&peer) {
            if self.swarm.dial(relay_addr.clone()).is_err() {
                error!(
                    TARGET_WORKER,
                    "Error dialing relay node {} for peer {}.", relay_addr, peer
                );
                // TODO: Event PeerDisconnected
            } else {
                info!(
                    TARGET_WORKER,
                    "Dialing relay node {} for peer {}.", relay_addr, peer
                );
                return;
            }
        }
        // If the node is ephemeral, request a circuit reservation.
        if self.node_type == NodeType::Ephemeral {
            self.request_circuit_reservation(peer);
            return;
        }
        // Send pending messages.
        self.send_pending_outbound_messages(peer);
    }

    /// Request circuit reservation.
    fn request_circuit_reservation(&mut self, peer: PeerId) {
        trace!(
            TARGET_WORKER,
            "Requesting circuit reservation to peer {}.",
            peer
        );
        let (relay_peer, relay_addr) = match self.boot_node() {
            Some(relay) => relay,
            None => {
                error!(TARGET_WORKER, "No relay nodes available");
                return;
            }
        };
        //println!("Relay: {} - {:?}", relay_peer, relay_addr);
        let listen_addr = relay_addr
            .with(Protocol::P2p(relay_peer))
            .with(Protocol::P2pCircuit);
        if self.swarm.listen_on(listen_addr.clone()).is_err() {
            error!(
                TARGET_WORKER,
                "Transport does not support the listening addresss: {:?}.", listen_addr
            );
            return;
        }
        // Pending reservation to the peer.
        self.pending_reservations.insert(relay_peer, peer);
    }

    /// Add pending message to peer.
    fn add_pending_outbound_message(&mut self, peer: PeerId, message: Vec<u8>) {
        let pending_messages = self.pending_outbound_messages.entry(peer).or_default();
        pending_messages.push_back(message);
    }

    /// Send pending messages to peer.
    fn send_pending_outbound_messages(&mut self, peer: PeerId) {
        if self.swarm.behaviour_mut().is_known_peer(&peer) {
            if let Some(messages) = self.pending_outbound_messages.remove(&peer) {
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
        } else {
            trace!(
                TARGET_WORKER,
                "Peer {} is not known. Pending messages will be sent when the peer is identified.",
                peer
            );
            // TODO: After three attempts, remove the peer from the pending messages and
            // send a netwokr event `PeerDisconnected`.
            self.swarm.behaviour_mut().discover(&peer);
        }
    }

    /// Gets the next boot node.
    fn boot_node(&mut self) -> Option<(PeerId, Multiaddr)> {
        trace!(TARGET_WORKER, "Getting next boot node.");
        if self.boot_nodes.is_empty() {
            self.boot_nodes = self.swarm.behaviour_mut().boot_nodes();
        }
        loop {
            if let Some((peer_id, addr)) = self.boot_nodes.pop() {
                if peer_id == self.local_peer_id {
                    continue;
                }
                return Some((peer_id, addr));
            } else {
                return None;
            }
        }
    }

    /// Get the network service.
    pub fn service(&self) -> Arc<RwLock<NetworkService>> {
        self.service.clone()
    }

    /// Change the network state.
    async fn change_state(&mut self, state: NetworkState) {
        trace!(TARGET_WORKER, "Change network state to: {:?}", state);
        self.state = state.clone();
        if self
            .event_sender
            .send(NetworkEvent::StateChanged(state))
            .await
            .is_err()
        {
            error!(TARGET_WORKER, "Can't send network event.")
        }
    }

    /// Run network worker.
    pub async fn run(&mut self) {
        info!(TARGET_WORKER, "Running main loop");

        // Dial to the boot nodes.
        for (peer_id, addr) in self.swarm.behaviour_mut().boot_nodes() {
            if self.swarm.dial(addr.clone()).is_err() {
                error!(
                    TARGET_WORKER,
                    "Error dialing boot node {} with address {}", peer_id, addr
                );
            } else {
                info!(
                    TARGET_WORKER,
                    "Dialing boot node {} with address {}", peer_id, addr
                );
                break;
            }
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
                    self.send_message(peer, message);
                } else {
                    error!(TARGET_WORKER, "Invalid peer id");
                }
            }
            Command::Bootstrap => {
                trace!(TARGET_WORKER, "Bootstrap to the kore network");
                if let Err(error) = self.swarm.behaviour_mut().bootstrap() {
                    if self
                        .event_sender
                        .send(NetworkEvent::Error(error))
                        .await
                        .is_err()
                    {
                        error!(TARGET_WORKER, "Error sending bootstrap error event.");
                    }
                }
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
                if self.state == NetworkState::Start {
                    trace!(TARGET_WORKER, "Bootstrap to the kore network");
                    self.change_state(NetworkState::Bootstrapping).await;
                    if let Err(error) = self.swarm.behaviour_mut().bootstrap() {
                        if self
                            .event_sender
                            .send(NetworkEvent::Error(error))
                            .await
                            .is_err()
                        {
                            error!(TARGET_WORKER, "Error sending bootstrap error event.");
                        }
                        self.change_state(NetworkState::Disconnected).await;
                    }
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::BootstrapOk) => {
                self.change_state(NetworkState::Running).await;
            }
            SwarmEvent::Behaviour(BehaviourEvent::BootstrapErr) => {
                if self.node_type == NodeType::Bootstrap {
                    warn!(TARGET_WORKER, "Can't connect to other bootstrap nodes.");
                    self.change_state(NetworkState::Running).await;
                } else {
                    error!(TARGET_WORKER, "Bootstrap error.");
                    self.change_state(NetworkState::Disconnected).await;
                }
            }
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => match endpoint {
                ConnectedPoint::Dialer { address, .. } => {
                    info!(
                        TARGET_WORKER,
                        "Connection established to peer {} with address {}.", peer_id, address
                    );
                    let result = self
                        .event_sender
                        .send(NetworkEvent::OutboundConnection {
                            peer: peer_id.to_string(),
                            address: address.to_string(),
                        })
                        .await;
                    if result.is_err() {
                        error!(TARGET_WORKER, "Error sending `OutboundConnection` event");
                    }
                }
                ConnectedPoint::Listener { send_back_addr, .. } => {
                    info!(
                        TARGET_WORKER,
                        "Connection established from address {}.", send_back_addr,
                    );
                    let result = self
                        .event_sender
                        .send(NetworkEvent::InboundConnection {
                            peer: peer_id.to_string(),
                            address: send_back_addr.to_string(),
                        })
                        .await;
                    if result.is_err() {
                        error!(TARGET_WORKER, "Error sending `OutboundConnection` event");
                    }
                }
            },
            SwarmEvent::Behaviour(BehaviourEvent::PeersFounded(key, peers)) => {
                if peers.contains(&key) {
                    info!(TARGET_WORKER, "Peer {} found in the network", key);
                    self.send_pending_outbound_messages(key);
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identified { peer_id, info }) => {
                // Send identified peer event.
                let addresses = info
                    .listen_addrs
                    .iter()
                    .map(|addr| addr.to_string())
                    .collect();
                let result = self
                    .event_sender
                    .send(NetworkEvent::PeerIdentified {
                        peer: peer_id.to_string(),
                        addresses,
                    })
                    .await;
                if result.is_err() {
                    error!(TARGET_WORKER, "Error sending `PeerIdentified` event");
                }

                // Add identified peer to the behaviour.
                info!(TARGET_WORKER, "Identified peer {}", peer_id);
                self.swarm
                    .behaviour_mut()
                    .add_identified_peer(peer_id, *info.clone());

                // If identified peer has relay address, add it to the relay circuits.
                let mut is_relay = false;
                for addr in info.listen_addrs.iter() {
                    if is_relay_circuit(addr) {
                        info!(
                            TARGET_WORKER,
                            "Peer {} added to relay circuits with address {}", peer_id, addr
                        );
                        self.relay_circuits.insert(peer_id, addr.clone());
                        is_relay = true;
                        break;
                    }
                }

                // Send pending messages.
                if !is_relay && self.pending_outbound_messages.get(&peer_id).is_some() {
                    trace!(
                        TARGET_WORKER,
                        "Sending pending messages to peer {}.",
                        peer_id
                    );
                    self.send_pending_outbound_messages(peer_id);
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
                        peer: peer_id.to_string(),
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
                let result = self
                    .event_sender
                    .send(NetworkEvent::MessageSent {
                        peer: peer_id.to_string(),
                    })
                    .await;
                if result.is_err() {
                    error!(
                        TARGET_WORKER,
                        "Error with message sent event to {}", peer_id
                    );
                }
                // Is the node ephemeral?
                if self.node_type == NodeType::Ephemeral {
                    // Request circuit reservation.
                    self.request_circuit_reservation(peer_id);
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageProcessed { peer_id, .. }) => {
                trace!(TARGET_WORKER, "Message processed from peer {}", peer_id);
            }
            SwarmEvent::Behaviour(BehaviourEvent::RelayServer(
                RelayServerEvent::ReservationReqAccepted {
                    src_peer_id,
                    renewed,
                },
            )) => {
                // Relay server started.
                info!(
                    TARGET_WORKER,
                    "Accepted relay server from peer {}", src_peer_id
                );
                if self.event_sender.send(NetworkEvent::Break).await.is_err() {
                    error!(TARGET_WORKER, "Error sending `Break` event");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                RelayClientEvent::ReservationReqAccepted { relay_peer_id, .. },
            )) => {
                // Circuit reservation accepted. Remove pending reservation.
                if let Some(peer_id) = self.pending_reservations.remove(&relay_peer_id) {
                    trace!(
                        TARGET_WORKER,
                        "Reservation accepted from relay {}. Removing pending reservation.",
                        relay_peer_id
                    );
                    if self.event_sender.send(NetworkEvent::Break).await.is_err() {
                        error!(TARGET_WORKER, "Error sending `Break` event");
                    }
                    //self.send_pending_outbound_messages(peer_id);
                } else {
                    warn!(
                        TARGET_WORKER,
                        "Reservation accepted from relay {} but no pending reservation.",
                        relay_peer_id
                    );
                }
            }
            SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                if let Some(peer) = peer_id {
                    if self.pending_reservations.contains_key(&peer) {
                        // We must retry with another relay node when we have an error due to a pending
                        // reservation.
                        warn!(
                            TARGET_WORKER,
                            "Error connecting to peer {} for circuit reservation", peer
                        );
                        self.request_circuit_reservation(peer);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Network state.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkState {
    /// Start.
    Start,
    /// Dialing boot nodes.
    Bootstrapping,
    /// Running.
    Running,
    /// Disconnected.
    Disconnected,
}

/// Network errors.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkError {
    /// Error dialing peer.
    Dialing(String),
    /// Error relay
    Relay(String),
    /// Error sending message.
    Sending(String),
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
    use tracing_test::traced_test;

    #[tokio::test]
    async fn test_no_boot_nodes() {
        let mut boot_nodes = vec![];
        let token = CancellationToken::new();

        // Build a fake bootstrap node.
        let fake_boot_peer = PeerId::random();
        let fake_boot_addr = "/ip4/50.0.0.1/tcp/54999";
        boot_nodes.push((fake_boot_peer.to_string(), fake_boot_addr.to_owned()));

        // Build a node.
        let node_addr = "/ip4/127.0.0.1/tcp/54422";
        let (mut node, mut node_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Addressable,
            token.clone(),
            Some(node_addr.to_owned()),
        );

        // Spawn the ephemeral node
        tokio::spawn(async move {
            node.run().await;
        });

        loop {
            tokio::select! {
                event = node_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::Error(Error::Network(value)) => {
                                assert_eq!(value, "No known bootstrap nodes.".to_owned());
                            }
                            NetworkEvent::StateChanged(NetworkState::Disconnected) => {
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ = token.cancelled() => {
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_fake_boot_node() {
        let mut boot_nodes = vec![];
        let token = CancellationToken::new();

        // Build a fake bootstrap node.
        let fake_boot_peer = PeerId::random();
        let fake_boot_addr = "/ip4/127.0.0.1/tcp/54999";
        boot_nodes.push((fake_boot_peer.to_string(), fake_boot_addr.to_owned()));

        // Build a node.
        let (mut node, mut node_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Addressable,
            token.clone(),
            None,
        );

        // Spawn the ephemeral node
        tokio::spawn(async move {
            node.run().await;
        });

        loop {
            tokio::select! {
                event = node_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::StateChanged(NetworkState::Bootstrapping) => {}
                            event => {
                                println!("Event: {:?}", event);
                                break;
                            }
                        }
                    }
                }
                _ = token.cancelled() => {
                    break;
                }
            }
        }
    }

    #[tokio::test]
    #[traced_test]
    async fn test_connect() {
        let mut boot_nodes = vec![];

        let token = CancellationToken::new();

        // Build a bootstrap node.
        let boot_addr = "/ip4/127.0.0.1/tcp/54421";
        let (mut boot, mut boot_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Bootstrap,
            token.clone(),
            Some(boot_addr.to_owned()),
        );
        boot_nodes.push((boot.local_peer_id().to_string(), boot_addr.to_owned()));

        // Build a fake bootstrap node.
        let fake_boot_peer = PeerId::random();
        let fake_boot_addr = "/ip4/127.0.0.1/tcp/54999";
        boot_nodes.push((fake_boot_peer.to_string(), fake_boot_addr.to_owned()));

        // Build a node.
        let node_addr = "/ip4/127.0.0.1/tcp/54422";
        let (mut node, mut node_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Addressable,
            token.clone(),
            Some(node_addr.to_owned()),
        );
        let node_service = node.service();
        let node_peer_id = node.local_peer_id();

        // Spawn the boot node
        tokio::spawn(async move {
            boot.run().await;
        });

        // Spawn the ephemeral node
        tokio::spawn(async move {
            node.run().await;
        });

        let mut node_service = node_service.write().unwrap();

        loop {
            tokio::select! {
                event = boot_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            _ => {}
                        }
                    }
                }
                event = node_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::StateChanged(state) => {
                                match state {
                                    NetworkState::Bootstrapping => {
                                        println!("**** Addressable dialing");
                                    }

                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ = token.cancelled() => {
                    break;
                }
            }
        }
    }

    #[tokio::test]
    #[traced_test]
    async fn test_network_worker() {
        let mut boot_nodes = vec![];

        let token = CancellationToken::new();

        // Build a bootstrap node.
        let boot_addr = "/ip4/127.0.0.1/tcp/54421";
        let (mut boot, mut boot_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Bootstrap,
            token.clone(),
            Some(boot_addr.to_owned()),
        );
        boot_nodes.push((boot.local_peer_id().to_string(), boot_addr.to_owned()));

        // Build a ephemeral node.
        let ephemeral_addr = "/ip4/127.0.0.1/tcp/54422";
        let (mut ephemeral, mut ephemeral_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Ephemeral,
            token.clone(),
            Some(ephemeral_addr.to_owned()),
        );
        let ephemeral_service = ephemeral.service();
        let ephemeral_peer_id = ephemeral.local_peer_id();

        // Build a addressable node.
        let addressable_addr = "/ip4/127.0.0.1/tcp/54423";
        let (mut addressable, mut addresable_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Addressable,
            token.clone(),
            Some(addressable_addr.to_owned()),
        );
        let addressable_service = addressable.service();
        let addressable_peer_id = addressable.local_peer_id();

        // Spawn the boot node
        tokio::spawn(async move {
            boot.run().await;
        });

        // Spawn the ephemeral node

        tokio::spawn(async move {
            ephemeral.run().await;
        });

        // Spawn the addressable node
        tokio::spawn(async move {
            addressable.run().await;
        });

        let mut ephemeral_service = ephemeral_service.write().unwrap();
        let mut addressable_service = addressable_service.write().unwrap();

        let mut ephemeral_identified = false;
        let mut addressable_identified = false;
        let mut sent = false;
        let mut received = false;
        let mut response = false;

        // loop to receive events
        loop {
            if ephemeral_identified && addressable_identified && !sent {
                ephemeral_service
                    .send_command(Command::SendMessage {
                        peer: addressable_peer_id.to_bytes().to_vec(),
                        message: b"Hello Addressable".to_vec(),
                    })
                    .await
                    .unwrap();
                sent = true;
            }
            if received && !response {
                addressable_service
                    .send_command(Command::SendMessage {
                        peer: ephemeral_peer_id.to_bytes().to_vec(),
                        message: b"Hello Ephemeral".to_vec(),
                    })
                    .await
                    .unwrap();
                response = true;
            }
            tokio::select! {
                event = boot_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::PeerIdentified { peer, .. } => {
                                if peer == ephemeral_peer_id.to_string() {
                                    ephemeral_identified = true;
                                } else if peer == addressable_peer_id.to_string() {
                                    addressable_identified = true;
                                }
                            }
                            NetworkEvent::Break => {
                                println!("******** Break Bootstrap *********");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                event = ephemeral_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::MessageSent { peer } => {
                                assert_eq!(peer, addressable_peer_id.to_string());
                            }
                            NetworkEvent::MessageReceived { peer, message } => {
                                assert_eq!(peer, addressable_peer_id.to_string());
                                assert_eq!(message, b"Hello Ephemeral".to_vec());
                                println!("******** End *********");
                                break;
                            }
                            NetworkEvent::Break => {
                                println!("******** Break Ephemeral *********");
                                break;
                            }
                            NetworkEvent::PeerIdentified { .. } => {}
                            //NetworkEvent::OutboundConnection { .. } => {}
                            e => {
                                if sent {
                                    println!("Ephemeral Event: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
                event = addresable_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::MessageReceived { peer, message } => {
                                assert_eq!(peer, ephemeral_peer_id.to_string());
                                assert_eq!(message, b"Hello Addressable".to_vec());
                                received = true;
                            }
                            NetworkEvent::Break => {
                                println!("******** Break Addressable *********");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ = token.cancelled() => {
                    break;
                }
            }
        }

        token.cancel();
    }

    // Build a relay server.
    fn build_worker(
        boot_nodes: Vec<(String, String)>,
        random_walk: bool,
        node_type: NodeType,
        token: CancellationToken,
        tcp_addr: Option<String>,
    ) -> (NetworkWorker, Receiver<NetworkEvent>) {
        let listen_addresses = if let Some(addr) = tcp_addr {
            vec![addr]
        } else {
            vec![]
        };
        let config = create_config(boot_nodes, random_walk, node_type, listen_addresses);
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
        listen_addresses: Vec<String>,
    ) -> Config {
        let config = crate::routing::Config::new(boot_nodes.clone())
            .with_allow_non_globals_in_dht(true)
            .with_allow_private_ip(true)
            .with_discovery_limit(50)
            .with_dht_random_walk(random_walk);

        Config {
            user_agent: "kore::node".to_owned(),
            node_type,
            tell: Default::default(),
            routing: config,
            listen_addresses,
        }
    }
}
