// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network composed behaviour.
//!

use crate::{
    node,
    routing::{self, DhtValue},
    utils::is_relay_circuit,
    Config, NodeType,
};

use libp2p::{
    dcutr::{self, Behaviour as Dcutr},
    identify::Info as IdentifyInfo,
    identity::PublicKey,
    relay::{self, client::Behaviour as RelayClient, Behaviour as RelayServer},
    swarm::behaviour::toggle::Toggle,
    swarm::NetworkBehaviour,
    Multiaddr, PeerId, StreamProtocol,
};
use tell::{
    binary, Event as TellEvent, InboundFailure, InboundTellId, OutboundFailure, OutboundTellId,
    ProtocolSupport, TellMessage,
};

use std::{
    collections::HashSet,
    iter,
    sync::{Arc, Mutex},
};

//const TARGET_BEHAVIOUR: &str = "KoreNetwork-Behaviour";

/// The network composed behaviour.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
pub struct Behaviour {
    /// The `routing` behaviour.
    routing: routing::Behaviour,

    /// The `node` behaviour.
    node: node::Behaviour,

    /// The `relay` server behaviour.
    relay_server: Toggle<RelayServer>,

    /// The `relay` client behaviour.
    relay_client: RelayClient,

    /// The Direct Connection Upgrade through Relay (DCUTR) behaviour.
    dcutr: Toggle<Dcutr>,

    /// The `tell` behaviour.
    tell: binary::Behaviour,
}

impl Behaviour {
    /// Create a new `Behaviour`.
    pub fn new(
        public_key: &PublicKey,
        config: Config,
        external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
        relay_client: RelayClient,
    ) -> Self {
        let peer_id = PeerId::from_public_key(public_key);
        let protocols = iter::once((
            StreamProtocol::new("/kore/tell/1.0.0"),
            ProtocolSupport::InboundOutbound,
        ));

        let (dcutr, relay_server) = match config.node_type {
            NodeType::Bootstrap { .. } => {
                //let dcutr = dcutr::Behaviour::new(peer_id);
                let relay_server = RelayServer::new(peer_id, Default::default());
                (Toggle::from(None), Toggle::from(Some(relay_server)))
            }
            NodeType::Ephemeral => {
                let dcutr = dcutr::Behaviour::new(peer_id);
                (Toggle::from(Some(dcutr)), Toggle::from(None))
            }
            NodeType::Addressable { .. } => {
                let dcutr = dcutr::Behaviour::new(peer_id);
                let relay_server = RelayServer::new(peer_id, Default::default());
                (Toggle::from(Some(dcutr)), Toggle::from(Some(relay_server)))
            }
        };
        Self {
            tell: binary::Behaviour::new(protocols, config.tell),
            routing: routing::Behaviour::new(PeerId::from_public_key(public_key), config.routing),
            node: node::Behaviour::new(&config.user_agent, public_key, external_addresses),
            relay_server,
            relay_client,
            dcutr,
        }
    }

    /// Bootstrap the network.
    pub fn bootstrap(&mut self) {
        self.routing.bootstrap();
    }

    /// Bootstrap node list.
    pub fn boot_nodes(&mut self) -> Vec<(PeerId, Multiaddr)> {
        self.routing.boot_nodes()
    }

    /// Get relay node.
    #[cfg(test)]
    pub fn get_relay_node(&self, peer_id: &PeerId) -> Option<&Multiaddr> {
        self.routing.get_relay_node(peer_id)
    }

    /// Borrows `self` and returns a struct giving access to the information about a node.
    ///
    /// Returns `None` if we don't know anything about this node. Always returns `Some` for nodes
    /// we're connected to, meaning that if `None` is returned then we're not connected to that
    /// node.
    pub fn node(&self, peer_id: &PeerId) -> Option<node::Node> {
        self.node.node(peer_id)
    }

    /// Send message to peer.
    pub fn send_message(&mut self, peer_id: &PeerId, message: Vec<u8>) -> OutboundTellId {
        self.tell.send_message(peer_id, message)
    }

    /// Discover closets peers.
    pub fn discover(&mut self, peer_id: &PeerId) {
        self.routing.discover(peer_id);
    }

    /// Sets random walk delay.
    #[cfg(test)]
    pub fn set_random_walk(&mut self, seconds: u64) {
        use std::time::Duration;
        self.routing.set_random_walk(Duration::from_secs(seconds));
    }

    /// Gets addresses of a known peer.
    #[cfg(test)]
    pub fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.routing.addresses_of_peer(peer_id)
    }

    /// Returns true if the given `PeerId` is known.
    pub fn is_known_peer(&mut self, peer_id: &PeerId) -> bool {
        self.routing.is_known_peer(peer_id)
    }

    /* 
    /// Start querying a record from the DHT. Will later produce either a `ValueFound` or a
    /// `ValueNotFound` event.
    pub fn get_value(&mut self, key: &str) {
        self.routing
            .get_value(RecordKey::from(key.as_bytes().to_vec()));
    }

    /// Starts putting a record into DHT. Will later produce either a `ValuePut` or a
    /// `ValuePutFailed` event.
    pub fn put_value(&mut self, key: &str, value: Vec<u8>) {
        let key = RecordKey::from(key.as_bytes().to_vec());
        self.routing.put_value(key, value);
    }
    */

    /// Add identified peer to routing table.
    pub fn add_identified_peer(&mut self, peer_id: PeerId, info: IdentifyInfo) {
        for addr in info.listen_addrs {
            // Add node with relay address.
            if is_relay_circuit(&addr) {
                self.routing.add_relay_node(peer_id, addr.clone());
            } else {
                // Add node with self reported address to DHT.
                self.routing
                    .add_self_reported_address(&peer_id, &info.protocols, addr.clone());
                // Add known address.
                self.routing.add_known_address(peer_id, addr.clone());
            }
        }
    }

    /// Node protocols supported.
    pub fn protocol_names(&self) -> Vec<StreamProtocol> {
        self.routing.protocol_names()
    }
}

/// Network event.
#[derive(Debug)]
pub enum Event {
    /// We have obtained identity information from a peer, including the addresses it is listening
    /// on.
    Identified {
        /// Id of the peer that has been identified.
        peer_id: PeerId,
        /// Information about the peer.
        info: Box<IdentifyInfo>,
    },

    /// Message recieved from a peer.
    Message {
        peer_id: PeerId,
        message: TellMessage<Vec<u8>>,
    },

    /// Message processed from a peer.
    MessageProcessed {
        peer_id: PeerId,
        inbound_id: InboundTellId,
    },

    /// Message sent to a peer.
    MessageSent {
        peer_id: PeerId,
        outbound_id: OutboundTellId,
    },

    /// Inbound failure.
    InboundFailure {
        peer_id: PeerId,
        inbound_id: InboundTellId,
        error: InboundFailure,
    },

    /// Outbound failure.
    OutboundFailure {
        peer_id: PeerId,
        outbound_id: OutboundTellId,
        error: OutboundFailure,
    },

    /// Relay server event.
    RelayServer(relay::Event),

    /// Relay client event.
    RelayClient(relay::client::Event),

    ///Direct Connection Upgrade through Relay (DCUTR) event.
    Dcutr(dcutr::Event),

    /// Started a random iterative Kademlia discovery query.
    RandomKademliaStarted,

    /// Discovered a peer.
    Discovered(PeerId),

    /// DHT events.
    Dht(DhtValue),

    /// Closets peers founded.
    PeersFounded(PeerId, Vec<PeerId>),

    /// Ignored event generated by lower layers.
    None,
}

impl From<TellEvent<Vec<u8>>> for Event {
    fn from(event: TellEvent<Vec<u8>>) -> Self {
        match event {
            TellEvent::Message { peer_id, message } => {
                /*// Ignore messages that starts with the following bytes. This is a workaround to
                // avoid processing messages that are not intended for the network. We should find
                // a better way to handle this.
                if message.message.as_slice().starts_with(&[40, 8, 4, 80, 10, 18, 34, 0, 32]) {
                    return Event::None;
                 }*/
                Event::Message { peer_id, message }
            }
            TellEvent::MessageProcessed {
                peer_id,
                inbound_id,
            } => Event::MessageProcessed {
                peer_id,
                inbound_id,
            },
            TellEvent::MessageSent {
                peer_id,
                outbound_id,
            } => Event::MessageSent {
                peer_id,
                outbound_id,
            },
            TellEvent::InboundFailure {
                peer_id,
                inbound_id,
                error,
            } => Event::InboundFailure {
                peer_id,
                inbound_id,
                error,
            },
            TellEvent::OutboundFailure {
                peer_id,
                outbound_id,
                error,
            } => Event::OutboundFailure {
                peer_id,
                outbound_id,
                error,
            },
        }
    }
}

impl From<node::Event> for Event {
    fn from(event: node::Event) -> Self {
        match event {
            node::Event::Identified { peer_id, info } => Event::Identified {
                peer_id,
                info: Box::new(info),
            },
        }
    }
}

impl From<routing::Event> for Event {
    fn from(event: routing::Event) -> Self {
        match event {
            routing::Event::RandomKademliaStarted => Event::RandomKademliaStarted,
            routing::Event::Discovered(peer) => Event::Discovered(peer),
            routing::Event::ValueFound(record, _) => Event::Dht(DhtValue::Found(record)),
            routing::Event::ValueNotFound(key, _) => Event::Dht(DhtValue::NotFound(key)),
            routing::Event::ValuePut(key, _) => Event::Dht(DhtValue::Put(key)),
            routing::Event::ValuePutFailed(key, _) => Event::Dht(DhtValue::PutFailed(key)),
            routing::Event::ClosestPeers(key, peers) => Event::PeersFounded(key, peers),
            routing::Event::UnroutablePeer(_) => Event::None,
        }
    }
}

impl From<relay::client::Event> for Event {
    fn from(event: relay::client::Event) -> Self {
        Event::RelayClient(event)
    }
}

impl From<dcutr::Event> for Event {
    fn from(event: dcutr::Event) -> Self {
        Event::Dcutr(event)
    }
}

impl From<relay::Event> for Event {
    fn from(event: relay::Event) -> Self {
        Event::RelayServer(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{utils::is_relay_circuit, Config, NodeType};

    use futures::prelude::*;
    use futures::{join, select};
    use libp2p::{
        core::transport::{upgrade::Version, MemoryTransport, Transport},
        identity,
        multiaddr::Protocol,
        plaintext, relay,
        swarm::{self, SwarmEvent},
        tcp, yamux, Multiaddr, Swarm,
    };
    use libp2p_swarm_test::SwarmExt;
    use std::str::FromStr;
    use std::{pin::pin, sync::Arc};

    #[tokio::test]
    async fn test_relay() {
        let mut boot_nodes = vec![];
        let config = create_config(
            boot_nodes.clone(),
            false,
            NodeType::Bootstrap {
                external_addresses: vec![],
            },
        );

        let mut boot_node = build_relay(config);
        let (_, listen_addr) = boot_node.listen().await;
        let boot_node_peer_id = *boot_node.local_peer_id();

        boot_node.add_external_address(listen_addr.clone());

        boot_nodes.push((
            boot_node.local_peer_id().to_base58(),
            listen_addr.to_string(),
        ));

        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_relay(config);
        let node_a_peer_id = *node_a.local_peer_id();
        let (_, node_a_addr) = node_a.listen().await;
        assert!(node_a.external_addresses().next().is_none());

        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_b = build_relay(config);
        //let node_b_peer_id = *node_b.local_peer_id();
        node_b.listen().await;
        assert!(node_b.external_addresses().next().is_none());

        tokio::task::spawn(boot_node.loop_on_next());

        let relay_addr = listen_addr
            .clone()
            .with(Protocol::P2p(boot_node_peer_id))
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(node_a_peer_id));
        node_a.listen_on(relay_addr.clone()).unwrap();

        wait_for_reservation(&mut node_a, relay_addr.clone(), boot_node_peer_id, false).await;

        tokio::task::spawn(node_a.loop_on_next());
        node_b.dial_and_wait(relay_addr.clone()).await;

        let node_a_addr = node_a_addr.with(Protocol::P2p(node_a_peer_id));

        let established_conn_id = node_b
            .wait(move |e| match e {
                SwarmEvent::ConnectionEstablished {
                    endpoint,
                    connection_id,
                    ..
                } => (*endpoint.get_remote_address() == node_a_addr).then_some(connection_id),
                _ => None,
            })
            .await;

        let reported_conn_id = node_b
            .wait(move |e| match e {
                SwarmEvent::Behaviour(Event::Dcutr(dcutr::Event {
                    result: Ok(connection_id),
                    ..
                })) => Some(connection_id),
                _ => None,
            })
            .await;

        assert_eq!(established_conn_id, reported_conn_id);
    }

    #[tokio::test]
    async fn test_dcutr() {
        let mut boot_nodes = vec![];

        // Build bootstrap node.
        let config = create_config(
            boot_nodes.clone(),
            false,
            NodeType::Bootstrap {
                external_addresses: vec![],
            },
        );
        let mut boot_node = build_relay(config);
        let (_, listen_addr) = boot_node.listen().await;
        let boot_node_peer_id = *boot_node.local_peer_id();

        boot_node.add_external_address(listen_addr.clone());

        boot_nodes.push((
            boot_node.local_peer_id().to_base58(),
            listen_addr.to_string(),
        ));

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_relay(config);
        let node_a_peer_id = *node_a.local_peer_id();
        node_a.listen().await;
        assert!(node_a.external_addresses().next().is_none());

        // Build node b.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_b = build_relay(config);
        let node_b_peer_id = *node_b.local_peer_id();
        let (node_b_addr, _) = node_b.listen().await;
        node_b.add_external_address(node_b_addr.clone());

        node_b.behaviour_mut().bootstrap();

        let boot_node_addr = listen_addr.clone();

        let listen_addr = listen_addr.with(Protocol::P2p(boot_node_peer_id));

        node_a
            .listen_on(listen_addr.clone().with(Protocol::P2pCircuit))
            .unwrap();

        let loop_boot = async move {
            loop {
                match boot_node.select_next_some().await {
                    _ => {}
                }
            }
        };

        let loop_a = async move {
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Discovered(peer)) => {
                        if peer == boot_node_peer_id {
                            // **** Discovered boot node ****
                            node_a.behaviour_mut().discover(&peer);
                        }
                    }
                    SwarmEvent::Behaviour(Event::PeersFounded(_, peers_founded)) => {
                        if peers_founded.is_empty() {
                            let result = node_a.dial(boot_node_addr.clone());
                            assert!(result.is_ok());
                        }
                    }
                    SwarmEvent::Behaviour(Event::RelayClient(
                        relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
                    )) => {
                        // **** Reservation request accepted ****
                        assert_eq!(relay_peer_id, boot_node_peer_id);
                        node_a.dial(node_b_addr.clone()).unwrap();
                    }
                    SwarmEvent::Behaviour(Event::Message { peer_id, message }) => {
                        // **** Message received ****
                        assert_eq!(peer_id, node_b_peer_id);
                        let message = message.message.as_slice();
                        assert_eq!(message, b"Hello Node A");
                        break;
                    }
                    _ => {}
                }
            }
        };

        let loop_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Discovered(_)) => {}
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        if peer_id == node_a_peer_id {
                            // **** Peer Identified ****
                            for addr in info.listen_addrs {
                                if is_relay_circuit(&addr) {
                                    let result = node_b.dial(addr);
                                    assert!(result.is_ok());
                                }
                            }
                        } else {
                            // **** Peer Identified ****
                            assert_eq!(boot_node_peer_id, peer_id);
                        }
                    }
                    SwarmEvent::Behaviour(Event::Dcutr(dcutr::Event {
                        result: Ok(..), ..
                    })) => {
                        // **** With DCUTR send message ****
                        node_b
                            .behaviour_mut()
                            .send_message(&node_a_peer_id, b"Hello Node A".to_vec());
                    }
                    SwarmEvent::Behaviour(Event::MessageSent { peer_id, .. }) => {
                        // **** Message sent ****
                        assert_eq!(peer_id, node_a_peer_id);
                    }
                    SwarmEvent::OutgoingConnectionError { .. } => break,
                    _ => {}
                }
            }
        };

        let loop_boot = loop_boot.fuse();
        let future = async { join!(loop_a, loop_b) }.fuse();
        let mut future = pin!(future);
        let mut loop_boot = pin!(loop_boot);

        select! {
            (_, _) = future => {},
            () = loop_boot => {},
        };
    }

    #[tokio::test]
    async fn test_full() {
        let mut boot_nodes = vec![];

        // Build bootstrap node.
        let config = create_config(
            boot_nodes.clone(),
            false,
            NodeType::Bootstrap {
                external_addresses: vec![],
            },
        );
        let mut boot_node = build_relay(config);
        let (_, boot_addr) = boot_node.listen().await;
        let boot_node_peer_id = *boot_node.local_peer_id();

        boot_node.add_external_address(boot_addr.clone());

        boot_nodes.push((boot_node.local_peer_id().to_base58(), boot_addr.to_string()));
        boot_node
            .behaviour_mut()
            .set_random_walk(1);

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_relay(config);
        let node_a_peer_id = *node_a.local_peer_id();
        node_a.listen().await;
        assert!(node_a.external_addresses().next().is_none());
        node_a.dial(boot_addr.clone()).unwrap();

        // Build node b.
        let config = create_config(
            boot_nodes.clone(),
            false,
            NodeType::Addressable {
                external_addresses: vec![],
            },
        );
        let mut node_b = build_relay(config);
        let node_b_peer_id = *node_b.local_peer_id();
        let (_, node_b_addr) = node_b.listen().await;
        node_b.add_external_address(node_b_addr.clone());
        node_b
            .behaviour_mut()
            .set_random_walk(1);
        node_b.dial(boot_addr.clone()).unwrap();

        let loop_boot = async move {
            loop {
                match boot_node.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        boot_node
                            .behaviour_mut()
                            .add_identified_peer(peer_id, *info);
                    }
                    _ => {}
                }
            }
        };

        let loop_a = async move {
            let mut pending_message = true;
            let mut reservation = None;
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        node_a.behaviour_mut().add_identified_peer(peer_id, *info);
                        // If node b identified, send message.
                        if peer_id == node_b_peer_id && pending_message {
                            node_a
                                .behaviour_mut()
                                .send_message(&node_b_peer_id, b"Hello Node B".to_vec());
                            pending_message = false;
                        } else if pending_message {
                            // Discover node b.
                            node_a.behaviour_mut().discover(&node_b_peer_id);
                        }
                    }
                    SwarmEvent::Behaviour(Event::RelayClient(
                        relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
                    )) => {
                        // Reservation request accepted.
                        assert_eq!(relay_peer_id, boot_node_peer_id);
                    }

                    SwarmEvent::Behaviour(Event::MessageSent { peer_id, .. }) => {
                        // If message sent, make reservation to response.
                        assert_eq!(peer_id, node_b_peer_id);
                        if reservation.is_none() {
                            // Message to node b. Get node b addresses.
                            let addresses =
                                node_a.behaviour_mut().addresses_of_peer(&boot_node_peer_id);
                            if addresses.is_empty() {
                                // Discover node b.
                                node_a.behaviour_mut().discover(&boot_node_peer_id)
                            } else {
                                if let Some(base_addr) = addresses.iter().next() {
                                    // Create reservation request
                                    let addr = base_addr.clone();
                                    if let Ok(addr) = addr.with_p2p(boot_node_peer_id) {
                                        let listen_addr = addr.with(Protocol::P2pCircuit);
                                        node_a.listen_on(listen_addr.clone()).unwrap();
                                        reservation = Some(base_addr.clone());
                                    }
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(Event::Message { peer_id, message }) => {
                        // Message received from unreachable node b
                        assert_eq!(peer_id, node_b_peer_id);
                        assert_eq!(message.message.as_slice(), b"Hello Node A");
                    }
                    SwarmEvent::Behaviour(Event::MessageProcessed { peer_id, .. }) => {
                        // Message processed. End test.
                        assert_eq!(peer_id, node_b_peer_id);
                        break;
                    }
                    _ => {}
                }
            }
        };

        let loop_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        node_b.behaviour_mut().add_identified_peer(peer_id, *info);
                        // If node a identified from relay address, dial node a.
                        if peer_id == node_a_peer_id {
                            let node = node_b.behaviour_mut().node(&node_a_peer_id);
                            if let Some(node) = node {
                                assert!(!node.reachable())
                            }
                        }
                        if let Some(relay_addr) = node_b.behaviour().get_relay_node(&peer_id) {

                            assert_eq!(peer_id, node_a_peer_id);
                            node_b.dial(relay_addr.clone()).unwrap();
                        }
                    }
                    SwarmEvent::Behaviour(Event::Message { peer_id, message }) => {
                        // Message received from node a.
                        assert_eq!(peer_id, node_a_peer_id);
                        assert_eq!(message.message.as_slice(), b"Hello Node B");
                    }
                    SwarmEvent::Behaviour(Event::Dcutr(dcutr::Event {
                        remote_peer_id,
                        result,
                    })) => {
                        // DCUTR event. Send message to unreachable node a.
                        if result.is_ok() {
                            assert_eq!(remote_peer_id, node_a_peer_id);
                            node_b
                                .behaviour_mut()
                                .send_message(&node_a_peer_id, b"Hello Node A".to_vec());
                        }
                    }
                    SwarmEvent::Behaviour(Event::MessageSent { peer_id, .. }) => {
                        // Message sent to node a.
                        assert_eq!(peer_id, node_a_peer_id);
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                        // Connection error with node a. End test.
                        if let Some(peer) = peer_id {
                            if peer == node_a_peer_id {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        };

        let loop_boot = loop_boot.fuse();
        let future = async { join!(loop_a, loop_b) }.fuse();
        let mut future = pin!(future);
        let mut loop_boot = pin!(loop_boot);

        select! {
            (_, _) = future => {},
            () = loop_boot => {},
        };
    }

    #[tokio::test]
    async fn test_reservation_error() {
        // Build node.
        let config = create_config(vec![], false, NodeType::Ephemeral);
        let mut node = build_relay(config);
        let node_peer_id = *node.local_peer_id();
        node.listen().await;
        assert!(node.external_addresses().next().is_none());

        let fake_addr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/11111").unwrap();
        let fake_peer = PeerId::random();
        let relay_addr = fake_addr
            .with(Protocol::P2p(fake_peer))
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(node_peer_id));

        node.listen_on(relay_addr.clone()).unwrap();

        async move {
            loop {
                match node.select_next_some().await {
                    SwarmEvent::Dialing { peer_id, .. } => {
                        assert_eq!(peer_id, Some(fake_peer));
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                        assert_eq!(peer_id, Some(fake_peer));
                        break;
                    }
                    _ => {}
                }
            }
        }
        .await;
    }

    // Build relay server.
    fn build_relay(config: Config) -> Swarm<Behaviour> {
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = local_key.public().to_peer_id();

        let (relay_transport, client_behaviour) = relay::client::new(local_peer_id);

        let transport = relay_transport
            .or_transport(MemoryTransport::default())
            .or_transport(tcp::tokio::Transport::default())
            .upgrade(Version::V1)
            .authenticate(plaintext::Config::new(&local_key))
            .multiplex(yamux::Config::default())
            .boxed();
        let behaviour = Behaviour::new(
            &local_key.public(),
            config,
            Arc::new(Mutex::new(HashSet::new())),
            client_behaviour,
        );
        Swarm::new(
            transport,
            behaviour,
            local_peer_id,
            swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(std::time::Duration::from_secs(2)),
        )
    }

    // Create a config
    fn create_config(
        boot_nodes: Vec<(String, String)>,
        random_walk: bool,
        node_type: NodeType,
    ) -> Config {
        /*let private_addr = match node_type {
            NodeType::Bootstrap { .. } => true,
            _ => false,
        };*/
        let config = crate::routing::Config::new(boot_nodes.clone())
            .with_allow_non_globals_in_dht(true)
            .with_allow_private_ip(true)
            .with_mdns(false)
            .with_discovery_limit(50)
            .with_dht_random_walk(random_walk);

        Config {
            user_agent: "kore::node".to_owned(),
            node_type,
            tell: Default::default(),
            routing: config,
        }
    }

    async fn wait_for_reservation(
        client: &mut Swarm<Behaviour>,
        client_addr: Multiaddr,
        relay_peer_id: PeerId,
        is_renewal: bool,
    ) {
        let mut new_listen_addr_for_relayed_addr = false;
        let mut reservation_req_accepted = false;
        let mut addr_observed = false;

        loop {
            if new_listen_addr_for_relayed_addr && reservation_req_accepted && addr_observed {
                break;
            }

            match client.next_swarm_event().await {
                SwarmEvent::NewListenAddr { address, .. } if address == client_addr => {
                    new_listen_addr_for_relayed_addr = true;
                }
                SwarmEvent::Behaviour(Event::RelayClient(
                    relay::client::Event::ReservationReqAccepted {
                        relay_peer_id: peer_id,
                        renewal,
                        ..
                    },
                )) if relay_peer_id == peer_id && renewal == is_renewal => {
                    reservation_req_accepted = true;
                }
                SwarmEvent::Dialing {
                    peer_id: Some(peer_id),
                    ..
                } if peer_id == relay_peer_id => {}
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == relay_peer_id => {}
                SwarmEvent::Behaviour(Event::Identified { .. }) => {
                    addr_observed = true;
                }
                SwarmEvent::Behaviour(Event::Discovered(_)) => {}
                SwarmEvent::NewExternalAddrCandidate { .. } => {}
                SwarmEvent::ExternalAddrConfirmed { address } if !is_renewal => {
                    assert_eq!(address, client_addr);
                }
                SwarmEvent::ListenerClosed { .. } => {}
                SwarmEvent::ConnectionClosed { .. } => {}
                //SwarmEvent::NewExternalAddrOfPeer { .. } => {}
                e => panic!("{e:?}"),
            }
        }
    }
}
