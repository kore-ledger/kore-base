// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network composed behaviour.
//!

use crate::{
    control_list, node,
    routing::{self, DhtValue},
    utils::is_memory,
    Config, Error, NodeType,
};

use libp2p::{
    identify::Info as IdentifyInfo,
    identity::PublicKey,
    request_response::{
        self, Config as ReqResConfig, InboundRequestId, OutboundRequestId, ProtocolSupport,
        ResponseChannel,
    },
    swarm::NetworkBehaviour,
    Multiaddr, PeerId, StreamProtocol,
};
use tell::{
    binary, Event as TellEvent, InboundTellId, OutboundTellId, ProtocolSupport as TellProtocol,
    TellMessage,
};

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    iter,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::{debug, info};

/// The network composed behaviour.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
pub struct Behaviour {
    control_list: control_list::Behaviour,

    /// The `request-response` behaviour.
    req_res: request_response::cbor::Behaviour<ReqResMessage, ReqResMessage>,

    /// The `tell` behaviour.
    tell: binary::Behaviour,

    /// The `routing` behaviour.
    routing: routing::Behaviour,

    /// The `node` behaviour.
    node: node::Behaviour,
}

impl Behaviour {
    /// Create a new `Behaviour`.
    pub fn new(
        public_key: &PublicKey,
        config: Config,
        external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
    ) -> Self {
        let protocol_tell = iter::once((
            StreamProtocol::new("/kore/tell/1.0.0"),
            TellProtocol::InboundOutbound,
        ));
        let protocol_rqrs = iter::once((
            StreamProtocol::new("/kore/reqres/1.0.0"),
            ProtocolSupport::Full,
        ));
        let boot_nodes = config.routing.boot_nodes();
        let is_dht_random_walk =
            config.routing.get_dht_random_walk() && config.node_type == NodeType::Bootstrap;
        let config_routing = config.routing.with_dht_random_walk(is_dht_random_walk);
        let config_req_res = ReqResConfig::default().with_request_timeout(Duration::from_secs(2));

        Self {
            control_list: control_list::Behaviour::new(config.control_list, &boot_nodes),
            routing: routing::Behaviour::new(PeerId::from_public_key(public_key), config_routing),
            node: node::Behaviour::new(&config.user_agent, public_key, external_addresses),
            tell: binary::Behaviour::new(protocol_tell, config.tell),
            req_res: request_response::cbor::Behaviour::new(protocol_rqrs, config_req_res),
        }
    }

    /// Bootstrap the network.
    pub fn bootstrap(&mut self) -> Result<(), Error> {
        self.routing.bootstrap()
    }

    /// Bootstrap node list.
    pub fn boot_nodes(&mut self) -> Vec<(PeerId, Vec<Multiaddr>)> {
        self.routing.boot_nodes()
    }

    /// Discover closets peers.
    pub fn discover(&mut self, peer_id: &PeerId) {
        info!("Discovering closets peers to peer: {:?}", peer_id);
        self.routing.discover(peer_id);
    }

    /// Returns true if the given `PeerId` is known.
    pub fn is_known_peer(&mut self, peer_id: &PeerId) -> bool {
        info!("Checking if peer is known: {:?}", peer_id);
        self.routing.is_known_peer(peer_id)
    }

    /// Remove node from routing table.
    pub fn remove_node(&mut self, peer_id: &PeerId, address: &Vec<Multiaddr>) {
        self.routing.remove_node(peer_id, address);
    }

    /// Add identified peer to routing table.
    pub fn add_identified_peer(&mut self, peer_id: PeerId, info: IdentifyInfo) {
        info!("Adding identified peer to routing table: {:?}", peer_id);
        for addr in info.listen_addrs {
            if is_memory(&addr) {
                debug!("Ignoring memory address: {:?}", addr);
                continue;
            }
            // Add node with self reported address to DHT.
            debug!("Adding self reported address to DHT: {:?}", addr);
            self.routing
                .add_self_reported_address(&peer_id, &info.protocols, addr.clone());
            self.routing.add_known_address(peer_id, addr);
        }
    }

    /// Add known address to routing table.
    pub fn add_known_address(&mut self, peer_id: PeerId, address: Multiaddr) {
        self.routing.add_known_address(peer_id, address);
    }

    /// Finish the prerouting state.
    pub fn finish_prerouting_state(&mut self) {
        self.routing.finish_prerouting_state();
    }

    /// Send tell message to peer.
    pub fn send_tell(&mut self, peer_id: &PeerId, message: Vec<u8>) -> OutboundTellId {
        info!("Sending tell message to peer: {:?}", peer_id);
        self.tell.send_message(peer_id, message)
    }

    /// Send request messasge to peer.
    pub fn send_request(&mut self, peer_id: &PeerId, message: Vec<u8>) -> OutboundRequestId {
        info!("Sending request message to peer: {:?}", peer_id);
        self.req_res.send_request(peer_id, ReqResMessage(message))
    }

    /// Send response message to peer.
    pub fn send_response(
        &mut self,
        channel: ResponseChannel<ReqResMessage>,
        message: Vec<u8>,
    ) -> Result<(), Error> {
        info!("Sending response message to peer");
        self.req_res
            .send_response(channel, ReqResMessage(message))
            .map_err(|_| Error::Behaviour("Cannot send response".to_owned()))
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

    /// Tell message recieved from a peer.
    TellMessage {
        peer_id: PeerId,
        message: TellMessage<Vec<u8>>,
    },

    /// Request - Response message received from a peer.
    ReqresMessage {
        peer_id: PeerId,
        message: request_response::Message<ReqResMessage, ReqResMessage>,
    },

    /// Tell message processed from a peer.
    TellMessageProcessed {
        peer_id: PeerId,
        inbound_id: InboundTellId,
    },

    /// Tell message sent to a peer.
    TellMessageSent {
        peer_id: PeerId,
        outbound_id: OutboundTellId,
    },

    /// Response message sent to a peer.
    ReqresMessageSent {
        peer_id: PeerId,
        inbound_id: InboundRequestId,
    },

    /// Tell inbound failure.
    TellInboundFailure {
        peer_id: PeerId,
        inbound_id: InboundTellId,
        error: Error,
    },

    /// Request - Response inbound failure.
    ReqresInboundFailure {
        peer_id: PeerId,
        inbound_id: InboundRequestId,
        error: Error,
    },

    /// Outbound failure.
    TellOutboundFailure {
        peer_id: PeerId,
        outbound_id: OutboundTellId,
        error: Error,
    },

    /// Request - Response outbound failure.
    ReqresOutboundFailure {
        peer_id: PeerId,
        outbound_id: OutboundRequestId,
        error: Error,
    },

    /// Started a random iterative Kademlia discovery query.
    RandomKademliaStarted,

    /// Discovered a peer.
    Discovered(PeerId),

    /// DHT events.
    Dht(DhtValue),

    /// Closets peers founded.
    PeersFounded(PeerId, Vec<PeerId>),

    /// UnreachablePeer.
    UnreachablePeer(PeerId),

    /// Dummy Event for control_list
    Dummy,
}

impl From<control_list::Event> for Event {
    fn from(_: control_list::Event) -> Self {
        Event::Dummy
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
            routing::Event::UnroutablePeer(peer) => Event::UnreachablePeer(peer),
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

impl From<TellEvent<Vec<u8>>> for Event {
    fn from(event: TellEvent<Vec<u8>>) -> Self {
        match event {
            TellEvent::Message { peer_id, message } => Event::TellMessage { peer_id, message },
            TellEvent::MessageProcessed {
                peer_id,
                inbound_id,
            } => Event::TellMessageProcessed {
                peer_id,
                inbound_id,
            },
            TellEvent::MessageSent {
                peer_id,
                outbound_id,
            } => Event::TellMessageSent {
                peer_id,
                outbound_id,
            },
            TellEvent::InboundFailure {
                peer_id,
                inbound_id,
                error,
            } => Event::TellInboundFailure {
                peer_id,
                inbound_id,
                error: Error::Behaviour(error.to_string()),
            },
            TellEvent::OutboundFailure {
                peer_id,
                outbound_id,
                error,
            } => Event::TellOutboundFailure {
                peer_id,
                outbound_id,
                error: Error::Behaviour(error.to_string()),
            },
        }
    }
}

impl From<request_response::Event<ReqResMessage, ReqResMessage>> for Event {
    fn from(event: request_response::Event<ReqResMessage, ReqResMessage>) -> Self {
        match event {
            request_response::Event::Message { peer, message } => Event::ReqresMessage {
                peer_id: peer,
                message,
            },
            request_response::Event::ResponseSent { peer, request_id } => {
                Event::ReqresMessageSent {
                    peer_id: peer,
                    inbound_id: request_id,
                }
            }
            request_response::Event::InboundFailure {
                peer,
                request_id,
                error,
            } => Event::ReqresInboundFailure {
                peer_id: peer,
                inbound_id: request_id,
                error: Error::Behaviour(error.to_string()),
            },
            request_response::Event::OutboundFailure {
                peer,
                request_id,
                error,
            } => Event::ReqresOutboundFailure {
                peer_id: peer,
                outbound_id: request_id,
                error: Error::Behaviour(error.to_string()),
            },
        }
    }
}

/// Wrapper for request-response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqResMessage(pub Vec<u8>);

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{Config, NodeType, RoutingNode};

    use futures::prelude::*;
    use libp2p::{
        core::transport::{upgrade::Version, Transport},
        identity, plaintext,
        swarm::{self, SwarmEvent},
        tcp, yamux, Swarm,
    };

    use request_response::Message;
    use serial_test::serial;

    use std::sync::Arc;
    use std::vec;

    #[tokio::test]
    #[serial]
    #[tracing_test::traced_test]
    async fn test_reqres() {
        let boot_nodes = vec![];

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_node(config);
        node_a.behaviour_mut().finish_prerouting_state();
        let node_a_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50001".parse().unwrap();
        let _ = node_a.listen_on(node_a_addr.clone());
        //node_a.add_external_address(node_a_addr.clone());

        // Build node b.
        let config = create_config(boot_nodes.clone(), true, NodeType::Addressable);
        let mut node_b = build_node(config);
        node_b.behaviour_mut().finish_prerouting_state();
        let node_b_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50002".parse().unwrap();
        let _ = node_b.listen_on(node_b_addr.clone());
        node_b.add_external_address(node_b_addr.clone());
        let node_b_peer_id = *node_b.local_peer_id();

        let _ = node_a.dial(node_b_addr.clone());

        let peer_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, .. }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                    }
                    SwarmEvent::Behaviour(Event::ReqresMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Request message received from peer: {:?}", peer_id);
                        match message {
                            Message::Request {
                                channel, request, ..
                            } => {
                                assert_eq!(request.0, b"Hello Node B".to_vec());
                                // Send response to node a.
                                let _ = node_b
                                    .behaviour_mut()
                                    .send_response(channel, b"Hello Node A".to_vec());
                            }
                            Message::Response { .. } => {}
                        }
                    }
                    _ => {}
                }
            }
        };

        let peer_a = async move {
            let mut counter = 0;
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, .. }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                        //node_a.behaviour_mut().add_identified_peer(peer_id, *info);
                        node_a
                            .behaviour_mut()
                            .send_request(&node_b_peer_id, b"Hello Node B".to_vec());
                    }
                    SwarmEvent::Behaviour(Event::ReqresMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Response message received from peer: {:?}", peer_id);
                        match message {
                            Message::Request { .. } => {}
                            Message::Response { response, .. } => {
                                assert_eq!(response.0, b"Hello Node A".to_vec());
                                if counter == 10 {
                                    break;
                                } else {
                                    counter += 1;
                                    node_a
                                        .behaviour_mut()
                                        .send_request(&node_b_peer_id, b"Hello Node B".to_vec());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        };

        tokio::task::spawn(Box::pin(peer_b));
        peer_a.await;
    }

    #[tokio::test]
    #[serial]
    #[tracing_test::traced_test]
    async fn test_tell() {
        let boot_nodes = vec![];

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Addressable);
        let mut node_a = build_node(config);
        let node_a_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50001".parse().unwrap();
        let _ = node_a.listen_on(node_a_addr.clone());
        node_a.add_external_address(node_a_addr.clone());
        node_a.behaviour_mut().finish_prerouting_state();

        // Build node b.
        let config = create_config(boot_nodes.clone(), true, NodeType::Addressable);
        let mut node_b = build_node(config);
        let node_b_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50002".parse().unwrap();
        let _ = node_b.listen_on(node_b_addr.clone());
        node_b.add_external_address(node_b_addr.clone());
        node_b.behaviour_mut().finish_prerouting_state();
        let node_b_peer_id = *node_b.local_peer_id();

        let _ = node_a.dial(node_b_addr.clone());
        //node_a.connect(&mut node_b).await;

        let peer_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, .. }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                        //node_b.behaviour_mut().add_identified_peer(peer_id, *info);
                    }
                    SwarmEvent::Behaviour(Event::TellMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Tell message received from peer: {:?}", peer_id);
                        assert_eq!(message.message, b"Hello Node B".to_vec());
                        // Send response to node a.
                        let _ = node_b
                            .behaviour_mut()
                            .send_tell(&peer_id, b"Hello Node A".to_vec());
                    }
                    _ => {}
                }
            }
        };

        let peer_a = async move {
            let mut counter = 0;
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, .. }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                        //node_a.behaviour_mut().add_identified_peer(peer_id, *info);
                        node_a
                            .behaviour_mut()
                            .send_tell(&node_b_peer_id, b"Hello Node B".to_vec());
                    }
                    SwarmEvent::Behaviour(Event::TellMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Tell message received from peer: {:?}", peer_id);
                        assert_eq!(message.message, b"Hello Node A".to_vec());
                        if counter == 10 {
                            break;
                        } else {
                            counter += 1;
                            node_a
                                .behaviour_mut()
                                .send_tell(&node_b_peer_id, b"Hello Node B".to_vec());
                        }
                    }
                    _ => {}
                }
            }
        };

        tokio::task::spawn(Box::pin(peer_b));
        peer_a.await;
    }

    #[tokio::test]
    #[serial]
    #[tracing_test::traced_test]
    async fn test_behaviour() {
        let boot_nodes = vec![];

        // Build bootstrap node.
        let config = create_config(boot_nodes.clone(), true, NodeType::Bootstrap);
        let mut boot_node = build_node(config);
        boot_node.behaviour_mut().finish_prerouting_state();
        let boot_node_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50001".parse().unwrap();
        let _ = boot_node.listen_on(boot_node_addr.clone());
        boot_node.add_external_address(boot_node_addr.clone());

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_node(config);
        node_a.behaviour_mut().finish_prerouting_state();
        let node_a_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50002".parse().unwrap();
        let _ = node_a.listen_on(node_a_addr.clone());
        node_a.add_external_address(node_a_addr.clone());

        // Build node b.
        let config = create_config(boot_nodes.clone(), true, NodeType::Addressable);
        let mut node_b = build_node(config);
        node_b.behaviour_mut().finish_prerouting_state();
        let node_b_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50003".parse().unwrap();
        let _ = node_b.listen_on(node_b_addr.clone());
        node_b.add_external_address(node_b_addr.clone());
        let node_b_peer_id = *node_b.local_peer_id();

        node_a.dial(boot_node_addr.clone()).unwrap();
        node_b.dial(boot_node_addr).unwrap();

        let boot_peer = async move {
            loop {
                match boot_node.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                        boot_node
                            .behaviour_mut()
                            .add_identified_peer(peer_id, *info);
                    }
                    _ => {}
                }
            }
        };

        let peer_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                        node_b.behaviour_mut().add_identified_peer(peer_id, *info);
                    }
                    SwarmEvent::Behaviour(Event::ReqresMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Request message received from peer: {:?}", peer_id);
                        match message {
                            Message::Request {
                                channel, request, ..
                            } => {
                                assert_eq!(request.0, b"Hello Node B".to_vec());
                                // Send response to node a.
                                let _ = node_b
                                    .behaviour_mut()
                                    .send_response(channel, b"Hello Node A".to_vec());
                            }
                            Message::Response { .. } => {}
                        }
                    }
                    _ => {}
                }
            }
        };

        let peer_a = async move {
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        // Peer identified.
                        info!("Peer identified: {:?}", peer_id);
                        node_a.behaviour_mut().add_identified_peer(peer_id, *info);
                        if peer_id == node_b_peer_id {
                            node_a
                                .behaviour_mut()
                                .send_request(&peer_id, b"Hello Node B".to_vec());
                        } else {
                            node_a.behaviour_mut().discover(&node_b_peer_id);
                        }
                    }
                    SwarmEvent::Behaviour(Event::ReqresMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Response message received from peer: {:?}", peer_id);
                        match message {
                            Message::Request { .. } => {}
                            Message::Response { response, .. } => {
                                assert_eq!(response.0, b"Hello Node A".to_vec());
                                break;
                            }
                        }
                    }
                    SwarmEvent::NewListenAddr { .. } => {}
                    e => {
                        info!("Event: {:?}", e);
                        //break;
                    }
                }
            }
        };

        tokio::task::spawn(Box::pin(boot_peer));
        tokio::task::spawn(Box::pin(peer_b));
        peer_a.await;
    }

    // Build node.
    fn build_node(config: Config) -> Swarm<Behaviour> {
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = local_key.public().to_peer_id();

        let transport = tcp::tokio::Transport::default()
            .upgrade(Version::V1)
            .authenticate(plaintext::Config::new(&local_key))
            .multiplex(yamux::Config::default())
            .boxed();

        let behaviour = Behaviour::new(
            &local_key.public(),
            config,
            Arc::new(Mutex::new(HashSet::new())),
        );
        Swarm::new(
            transport,
            behaviour,
            local_peer_id,
            swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(std::time::Duration::from_secs(10)),
        )
    }

    // Create a config
    fn create_config(
        boot_nodes: Vec<RoutingNode>,
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
            external_addresses: vec![],
            listen_addresses: vec![],
            port_reuse: false,
            control_list: Default::default(),
        }
    }
}
