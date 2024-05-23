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
    identity::{
        ed25519::{self, PublicKey as PublicKeyEd25519},
        Keypair, PublicKey,
    },
    multiaddr::Protocol,
    relay::{client::Event as RelayClientEvent, Event as RelayServerEvent},
    swarm::{self, dial_opts::DialOpts, SwarmEvent},
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

    /// Dynamic list of relay nodes.
    relay_nodes: Vec<(PeerId, Vec<Multiaddr>)>,

    /// List of boot noodes.
    boot_nodes: Vec<(PeerId, Vec<Multiaddr>)>,

    /// Relay circuits.
    relay_circuits: HashMap<PeerId, Multiaddr>,

    /// Pending reservartions.
    pending_reservations: HashMap<PeerId, PeerId>,

    /// Pendings outbound messages to the peer
    pending_outbound_messages: HashMap<PeerId, VecDeque<Vec<u8>>>,

    /// Connect attempts.
    _connect_attempts: Arc<AtomicU16>,

    /// Maximum attempts to dial a peer or request a circuit relay.
    _max_attempts: u16,

    /// Messages metric.
    messages_metric: Family<MetricLabels, Counter>,

    ///
    successful_dials: u64,
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

        let boot_nodes = swarm.behaviour_mut().boot_nodes();

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
            boot_nodes,
            relay_nodes: Vec::new(),
            relay_circuits: HashMap::default(),
            pending_reservations: HashMap::default(),
            pending_outbound_messages: HashMap::default(),
            _connect_attempts: Arc::new(AtomicU16::new(0)),
            _max_attempts: max_attempts,
            messages_metric,
            successful_dials: 0,
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
            //let relay_addr = relay_addr.clone().with(Protocol::P2p(peer));
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
        let (relay_peer, relay_addr) = match self.relay_node() {
            Some(relay) => relay,
            None => {
                error!(TARGET_WORKER, "No relay nodes available");
                return;
            }
        };
        for addrs in relay_addr {
            // Listen on the relay address.
            let listen_addr = addrs
                .with(Protocol::P2p(relay_peer))
                .with(Protocol::P2pCircuit);
            if self.swarm.listen_on(listen_addr.clone()).is_err() {
                error!(
                    TARGET_WORKER,
                    "Transport does not support the listening addresss: {:?}.", listen_addr
                );
            } else {
                // Pending reservation to the peer.
                self.pending_reservations.insert(relay_peer, peer);
                break;
            }
        }
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

    /// Gets the next relay node.
    fn relay_node(&mut self) -> Option<(PeerId, Vec<Multiaddr>)> {
        trace!(TARGET_WORKER, "Getting next boot node.");
        if self.relay_nodes.is_empty() {
            self.relay_nodes = self.swarm.behaviour_mut().boot_nodes();
        }
        loop {
            if let Some((peer_id, addr)) = self.relay_nodes.pop() {
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
        self.send_event(NetworkEvent::StateChanged(state)).await;
    }

    /// Send event
    async fn send_event(&mut self, event: NetworkEvent) {
        if self.event_sender.send(event).await.is_err() {
            error!(TARGET_WORKER, "Can't send network event.")
        }
    }

    /// Run the network worker.
    pub async fn run(&mut self) {
        // Run connection to bootstrap node.
        if let Err(error) = self.run_connection().await {
            error!(TARGET_WORKER, "Error running connection: {:?}", error);
            self.send_event(NetworkEvent::Error(error)).await;
            // Irrecoverable error. Cancel the node.
            self.cancel.cancel();
            return;
        }

        // Finish pre routing state, activating random walk (if node is a bootstrap).
        self.swarm.behaviour_mut().finish_prerouting_state();
        // Run main loop.
        self.run_main().await;
    }

    /// Run connection to bootstrap node.
    pub async fn run_connection(&mut self) -> Result<(), Error> {
        info!(TARGET_WORKER, "Running connection loop");
        let mut result = Ok(());
        // If is the first node of kore network.
        if self.node_type == NodeType::Bootstrap && self.boot_nodes.is_empty() {
            self.change_state(NetworkState::Running).await;
        } else {
            loop {
                match self.state {
                    NetworkState::Dial => {
                        // Dial to boot node.
                        if self.boot_nodes.is_empty() {
                            error!(TARGET_WORKER, "No bootstrap nodes.");
                            if self
                                .event_sender
                                .send(NetworkEvent::Error(Error::Network(
                                    "No more bootstrap nodes.".to_owned(),
                                )))
                                .await
                                .is_err()
                            {
                                error!(TARGET_WORKER, "Error sending network error event.");
                            }
                            error!(TARGET_WORKER, "Can't connect to kore network");
                            self.change_state(NetworkState::Disconnected).await;
                        }

                        let copy_boot_nodes = self.boot_nodes.clone();
                        for node in copy_boot_nodes {
                            if self
                                .swarm
                                .dial(DialOpts::peer_id(node.0).addresses(node.1.clone()).build())
                                .is_err()
                            {
                                error!(TARGET_WORKER, "Error dialing boot node {}", node.0);
                                self.swarm.behaviour_mut().remove_node(&node.0, &node.1);
                                if let Some(pos) = self
                                    .boot_nodes
                                    .iter()
                                    .position(|val| val.clone() == (node.0, node.1.clone()))
                                {
                                    self.boot_nodes.remove(pos);
                                }
                            }
                        }

                        self.change_state(NetworkState::Dialing).await;
                    }
                    NetworkState::Dialing => {
                        // No more bootnodes to send dial and none was successful
                        if self.boot_nodes.is_empty() && self.successful_dials == 0 {
                            self.change_state(NetworkState::Disconnected).await;
                        // No more bootnodes to send dial and one or more was successful
                        } else if self.boot_nodes.is_empty() {
                            break;
                        }
                    }
                    NetworkState::Running => {
                        break;
                    }
                    NetworkState::Disconnected => {
                        result = Err(Error::Network("Can't connect to kore network".to_owned()));
                        break;
                    }
                    _ => {}
                }
                tokio::select! {
                    event = self.swarm.select_next_some() => {
                        self.handle_connection_events(event).await;
                    }
                    _ = self.cancel.cancelled() => {
                        break;
                    }
                }
            }
        }
        result
    }

    /// Handle connection events.
    async fn handle_connection_events(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!(TARGET_WORKER, "Listening on {:?}", address);
                if self.state == NetworkState::Start {
                    trace!(TARGET_WORKER, "Bootstrap to the kore network");
                    self.change_state(NetworkState::Dial).await;
                }
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id: _,
                peer_id: Some(peer_id),
                error: _,
            } => {
                    error!(TARGET_WORKER, "Error dialing peer {}", peer_id);
                    if let Some(pos) = self.boot_nodes.iter().position(|val| val.0 == peer_id) {
                        self.swarm
                            .behaviour_mut()
                            .remove_node(&peer_id, &self.boot_nodes[pos].1);
                        self.boot_nodes.remove(pos);
                    }
            }
            SwarmEvent::IncomingConnection {
                local_addr,
                send_back_addr,
                ..
            } => {
                info!(
                    TARGET_WORKER,
                    "Incoming connection from {} to {}.", send_back_addr, local_addr
                );
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identified { peer_id, info }) => {
                info!(TARGET_WORKER, "Identified peer {}", peer_id);
                // Add identified peer to the behaviour.
                self.swarm
                    .behaviour_mut()
                    .add_identified_peer(peer_id, *info.clone());

                // If the identified peer is the current dialing, send event and change the state to running.

                trace!(TARGET_WORKER, "Connected to bootstrap node {}", peer_id);
                self.send_event(NetworkEvent::ConnectedToBootstrap {
                    peer: peer_id.to_string(),
                })
                .await;

                if let Some(pos) = self.boot_nodes.iter().position(|val| val.0 == peer_id) {
                    self.boot_nodes.remove(pos);
                    self.successful_dials += 1;
                }
            }
            _ => {}
        }
    }
    /// Run network worker.
    pub async fn run_main(&mut self) {
        info!(TARGET_WORKER, "Running main loop");

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
                if let Ok(public_key) = PublicKeyEd25519::try_from_bytes(peer.as_slice()) {
                    let peer = PublicKey::from(public_key);
                    self.send_message(peer.to_peer_id(), message);
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
                if self.node_type == NodeType::Bootstrap {
                    trace!(TARGET_WORKER, "Bootstrap to the kore network");
                    if self.swarm.behaviour_mut().bootstrap().is_err() {
                        warn!(TARGET_WORKER, "Empty boot nodes list.");
                        self.change_state(NetworkState::Running).await;
                    } else {
                        self.change_state(NetworkState::Dialing).await;
                    }
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
                }
                ConnectedPoint::Listener { send_back_addr, .. } => {
                    info!(
                        TARGET_WORKER,
                        "Connection established from address {}.", send_back_addr,
                    );
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

                self.send_event(NetworkEvent::PeerIdentified {
                    peer: peer_id.to_string(),
                    addresses,
                })
                .await;

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
                        //let addr = addr.clone().with(Protocol::P2p(self.local_peer_id));
                        self.relay_circuits.insert(peer_id, addr.clone());
                        is_relay = true;
                        break;
                    }
                }

                // Send pending messages.
                if !is_relay && self.pending_outbound_messages.contains_key(&peer_id) {
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
                            fact: Fact::Received,
                            peer_id: peer_id.to_string(),
                        })
                        .inc();
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageSent { peer_id, .. }) => {
                trace!(TARGET_WORKER, "Message sent to peer {}", peer_id);
                self.messages_metric
                    .get_or_create(&MetricLabels {
                        fact: Fact::Sent,
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
                RelayServerEvent::ReservationReqAccepted { src_peer_id, .. },
            )) => {
                // Relay server started.
                info!(
                    TARGET_WORKER,
                    "Accepted relay server from peer {}", src_peer_id
                );
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
                    self.send_pending_outbound_messages(peer_id);
                } else {
                    warn!(
                        TARGET_WORKER,
                        "Reservation accepted from relay {} but no pending reservation.",
                        relay_peer_id
                    );
                }
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id: Some(peer),
                ..
            } => {
                if self.pending_reservations.contains_key(&peer) {
                    // We must retry with another relay node when we have an error due to a pending
                    // reservation.
                    warn!(
                        TARGET_WORKER,
                        "Error connecting to peer {} for circuit reservation", peer
                    );
                    self.request_circuit_reservation(peer);
                } else if self.relay_circuits.remove(&peer).is_some() {
                    warn!(
                        TARGET_WORKER,
                        "Error connecting to dctur to node {}. Removing relay circuit.", peer
                    );
                    self.send_event(NetworkEvent::Error(Error::Relay("Dctur error".to_owned())))
                        .await;
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
    /// Dial.
    Dial,
    /// Dialing boot node.
    Dialing,
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
    Sent,
    /// Message received.
    Received,
}

#[cfg(test)]
mod tests {

    use crate::routing::RoutingNode;

    use super::*;

    use identity::keys::KeyPair;

    use serial_test::serial;
    use tokio::sync::mpsc::{self, Receiver};

    //use tracing_test::traced_test;

    #[tokio::test]
    #[serial]
    async fn test_no_boot_nodes() {
        let boot_nodes = vec![];
        let token = CancellationToken::new();

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
            let _ = node.run_connection().await;
        });

        loop {
            tokio::select! {
                event = node_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::Error(Error::Network(value)) => {
                                assert_eq!(value, "No more bootstrap nodes.".to_owned());
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
    #[serial]
    async fn test_fake_boot_node() {
        let mut boot_nodes = vec![];
        let token = CancellationToken::new();

        // Build a fake bootstrap node.
        let fake_boot_peer = PeerId::random();
        let fake_boot_addr = "/ip4/127.0.0.1/tcp/54999";
        let fake_node = RoutingNode {
            peer_id: fake_boot_peer.to_string(),
            address: vec![fake_boot_addr.to_owned()],
        };
        boot_nodes.push(fake_node);

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
            let _ = node.run_connection().await;
        });

        loop {
            tokio::select! {
                event = node_receiver.recv() => {
                    if let Some(event) = event {
                        match event {
                            NetworkEvent::StateChanged(NetworkState::Dialing) => {
                                //break;
                            }
                            NetworkEvent::Error(Error::Network(value)) => {
                                assert_eq!(value, "No more bootstrap nodes.".to_owned());
                            }
                            NetworkEvent::StateChanged(NetworkState::Dial) => {}
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
    #[serial]
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
        let boot_node = RoutingNode {
            peer_id: boot.local_peer_id().to_string(),
            address: vec![boot_addr.to_owned()],
        };
        boot_nodes.push(boot_node);
        let boot_peer_id = boot.local_peer_id().to_string();

        // Build a fake bootstrap node.
        let fake_boot_peer = PeerId::random();
        let fake_boot_addr = "/ip4/127.0.0.1/tcp/54999";
        let fake_node = RoutingNode {
            peer_id: fake_boot_peer.to_string(),
            address: vec![fake_boot_addr.to_owned()],
        };
        boot_nodes.push(fake_node);

        // Build a node.
        let node_addr = "/ip4/127.0.0.1/tcp/54422";
        let (mut node, mut node_receiver) = build_worker(
            boot_nodes.clone(),
            false,
            NodeType::Addressable,
            token.clone(),
            Some(node_addr.to_owned()),
        );

        // Spawn the boot node
        tokio::spawn(async move {
            boot.run_main().await;
        });

        // Wait for connection.
        //if node.run_connection().await.is_err() {
        //    error!(TARGET_WORKER, "Error connecting to the network");
        //}

        // Spawn the node
        tokio::spawn(async move {
            node.run().await;
        });

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
                            NetworkEvent::ConnectedToBootstrap { peer } => {
                                assert_eq!(peer, boot_peer_id.to_string());
                                break;
                            }
                            NetworkEvent::StateChanged(state) => {
                                match state {
                                    NetworkState::Running => {

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
    #[ignore]
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
        let boot_node = RoutingNode {
            peer_id: boot.local_peer_id().to_string(),
            address: vec![boot_addr.to_owned()],
        };
        boot_nodes.push(boot_node);

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

        // Wait for connect boot node.
        if boot.run_connection().await.is_err() {
            error!(TARGET_WORKER, "Error connecting to the network");
        }

        // Spawn the boot node
        tokio::spawn(async move {
            boot.run_main().await;
        });

        // Wait for connect ephemeral node.
        if ephemeral.run_connection().await.is_err() {
            error!(TARGET_WORKER, "Error connecting to the network");
        }

        // Wait for connect addressable node.
        if addressable.run_connection().await.is_err() {
            error!(TARGET_WORKER, "Error connecting to the network");
        }

        // Spawn the ephemeral node
        tokio::spawn(async move {
            ephemeral.run_main().await;
        });

        // Spawn the addressable node
        tokio::spawn(async move {
            addressable.run_main().await;
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
                                break;
                            }
                          _ => {}
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
                            NetworkEvent::Error(error) => {
                                error!(TARGET_WORKER, "Error: {:?}", error);
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
        boot_nodes: Vec<RoutingNode>,
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
        boot_nodes: Vec<RoutingNode>,
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
