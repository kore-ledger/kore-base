// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network composed behaviour.
//!

use crate::{
    node,
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

use serde::{Deserialize, Serialize};
use tell::{
    binary, Event as TellEvent, InboundTellId, OutboundTellId, ProtocolSupport as TellProtocol,
    TellMessage,
};

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
    /// The `routing` behaviour.
    routing: routing::Behaviour,

    /// The `node` behaviour.
    node: node::Behaviour,

    /// The `tell` behaviour.
    tell: binary::Behaviour,

    /// The `request-response` behaviour.
    req_res: request_response::cbor::Behaviour<ReqResMessage, ReqResMessage>,
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

        let is_dht_random_walk =
            config.routing.get_dht_random_walk() && config.node_type == NodeType::Bootstrap;
        let config_routing = config.routing.with_dht_random_walk(is_dht_random_walk);
        let config_req_res = ReqResConfig::default();

        Self {
            tell: binary::Behaviour::new(protocol_tell, config.tell),
            routing: routing::Behaviour::new(PeerId::from_public_key(public_key), config_routing),
            node: node::Behaviour::new(&config.user_agent, public_key, external_addresses),
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

    pub fn finish_prerouting_state(&mut self) {
        self.routing.finish_prerouting_state();
    }

    /// Borrows `self` and returns a struct giving access to the information about a node.
    ///
    /// Returns `None` if we don't know anything about this node. Always returns `Some` for nodes
    /// we're connected to, meaning that if `None` is returned then we're not connected to that
    /// node.
    #[allow(dead_code)]
    pub fn node(&self, peer_id: &PeerId) -> Option<node::Node> {
        self.node.node(peer_id)
    }

    /// Send message to peer.
    pub fn send_tell(&mut self, peer_id: &PeerId, message: Vec<u8>) -> OutboundTellId {
        info!("Sending tell message to peer: {:?}", peer_id);
        self.tell.send_message(peer_id, message)
    }

    /// Send request messasge to peer.
    pub fn send_request(&mut self, peer_id: &PeerId, message: Vec<u8>) -> OutboundRequestId {
        info!("Sending request message to peer: {:?}", peer_id);
        let result = self.req_res.send_request(peer_id, ReqResMessage(message));
        result
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

    /// Discover closets peers.
    pub fn discover(&mut self, peer_id: &PeerId) {
        info!("Discovering closets peers to peer: {:?}", peer_id);
        self.routing.discover(peer_id);
    }

    /* 
    /// Sets random walk delay.
    #[cfg(test)]
    pub fn set_random_walk(&mut self, seconds: u64) {
        use std::time::Duration;
        self.routing.set_random_walk(Duration::from_secs(seconds));
    }
*/
    /* 
    /// Gets addresses of a known peer.
    #[cfg(test)]
    pub fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.routing.addresses_of_peer(peer_id)
    }
    */

    /// Returns true if the given `PeerId` is known.
    pub fn is_known_peer(&mut self, peer_id: &PeerId) -> bool {
        info!("Checking if peer is known: {:?}", peer_id);
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
            // Add known address.
            debug!("Adding known address to routing table: {:?}", addr);
            self.routing.add_known_address(peer_id, addr.clone());

        }
    }

    /// Get known peer addresses.
    pub fn _known_peer_addresses(&mut self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        self.routing.known_peer_addresses(peer_id)
    }

    /// Remove node from routing table.
    pub fn remove_node(&mut self, peer_id: &PeerId, address: &Vec<Multiaddr>) {
        self.routing.remove_node(peer_id, address);
    }

    /// Node protocols supported.
    pub fn _protocol_names(&self) -> Vec<StreamProtocol> {
        self.routing._protocol_names()
    }
}

/// Wrapper for request-response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqResMessage(pub Vec<u8>);

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
            routing::Event::UnroutablePeer(peer) => Event::UnreachablePeer(peer),
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{Config, NodeType, RoutingNode};

    use futures::prelude::*;
    use futures::{join, select};
    use libp2p::{
        core::transport::{upgrade::Version, MemoryTransport, Transport},
        identity, plaintext,
        swarm::{self, SwarmEvent},
        tcp, yamux, Swarm,
    };
    use libp2p_swarm_test::SwarmExt;
    use request_response::Message;
    use serial_test::serial;

    use std::vec;
    use std::{pin::pin, sync::Arc};


    #[tokio::test]
    #[serial]
    #[tracing_test::traced_test]
    async fn test_reqres() {
        let boot_nodes = vec![];

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_node(config);
        let node_a_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50001".parse().unwrap();
        let _ = node_a.listen_on(node_a_addr.clone());
        node_a.add_external_address(node_a_addr.clone());
        
        // Build node b.
        let config = create_config(boot_nodes.clone(), true, NodeType::Addressable);
        let mut node_b = build_node(config);
        let node_b_addr: Multiaddr = "/ip4/127.0.0.1/tcp/50002".parse().unwrap();
        let _ = node_b.listen_on(node_b_addr.clone());
        node_b.add_external_address(node_b_addr.clone());
        let node_b_peer_id = *node_b.local_peer_id();
        
        node_a.connect(&mut node_b).await;

        let peer_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        node_b.behaviour_mut().add_identified_peer(peer_id, *info);
                    }
                    SwarmEvent::Behaviour(Event::ReqresMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Request message received from peer: {:?}", peer_id);
                        match message {
                            Message::Request { channel, request, .. } => {
                                assert_eq!(request.0, b"Hello Node B".to_vec());
                                // Send response to node a.
                                let _ = node_b
                                    .behaviour_mut()
                                    .send_response(channel, b"Hello Node A".to_vec());
                            }
                            Message::Response { .. } => {}
                        }
                    }
                    SwarmEvent::NewListenAddr { .. } => {}
                    SwarmEvent::ConnectionEstablished { .. } => {}
                    SwarmEvent::IncomingConnection { .. } => {}

                    event => {
                        info!("Node B event: {:?}", event);
                    }
                }
            }
        };

        let peer_a = async move {
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        node_a.behaviour_mut().add_identified_peer(peer_id, *info);
                        node_a.behaviour_mut().send_request(&node_b_peer_id, b"Hello Node B".to_vec());                    
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
                    SwarmEvent::ConnectionEstablished { .. } => {}
                    SwarmEvent::IncomingConnection { .. } => {}
                    SwarmEvent::Behaviour(Event::Discovered(_)) => {}
                    SwarmEvent::NewExternalAddrCandidate { .. } => {}
                    SwarmEvent::Behaviour(Event::ReqresOutboundFailure { .. }) => {}
                    e => {
                        info!("Node A event: {:?}", e);
                        break;
                    }
                }
            }
        };

        tokio::task::spawn(Box::pin(peer_b));
        peer_a.await;
    }

    #[tokio::test]
    #[serial]
    #[tracing_test::traced_test]
    async fn test_full_with_reqres() {
        let mut boot_nodes = vec![];

        // Build bootstrap node.
        let config = create_config(boot_nodes.clone(), true, NodeType::Bootstrap);
        let mut boot_node = build_node(config);
        let (_, boot_addr) = boot_node.listen().await;
        let boot_node_peer_id = *boot_node.local_peer_id();
        info!("Boot node peer id: {:?}", boot_node_peer_id);
        info!("Boot node address: {:?}", boot_addr);
        boot_node.add_external_address(boot_addr.clone());
        assert!(boot_node.external_addresses().next().is_some());

        let routing_node = RoutingNode {
            peer_id: boot_node_peer_id.to_base58(),
            address: vec![boot_addr.to_string()],
        };

        boot_nodes.push(routing_node);
        //boot_node.behaviour_mut().set_random_walk(1);

        // Build node a.
        let config = create_config(boot_nodes.clone(), false, NodeType::Ephemeral);
        let mut node_a = build_node(config);
        let node_a_peer_id = *node_a.local_peer_id();
        info!("Node A peer id: {:?}", node_a_peer_id);
        node_a.listen().await;
        assert!(node_a.external_addresses().next().is_none());
        node_a.dial(boot_addr.clone()).unwrap();

        // Build node b.
        let config = create_config(boot_nodes.clone(), true, NodeType::Addressable);
        let mut node_b = build_node(config);
        let node_b_peer_id = *node_b.local_peer_id();
        info!("Node B peer id: {:?}", node_b_peer_id);
        let (_, node_b_addr) = node_b.listen().await;
        node_b.add_external_address(node_b_addr.clone());
        assert!(node_b.external_addresses().next().is_some());
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
            loop {
                match node_a.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        node_a.behaviour_mut().add_identified_peer(peer_id, *info);
                        // If node b identified, send message.
                        if peer_id == node_b_peer_id && pending_message {
                            node_a
                                .behaviour_mut()
                                .send_request(&node_b_peer_id, b"Hello Node B".to_vec());
                            pending_message = false;
                        } else if pending_message {
                            // Discover node b.
                            node_a.behaviour_mut().discover(&node_b_peer_id);
                        }
                    }
                    //SwarmEvent::Behaviour(Event::D)
                    SwarmEvent::Behaviour(Event::ReqresMessageSent { peer_id, .. }) => {
                        // If message sent, make reservation to response.
                        info!("Request message sent to peer: {:?}", peer_id);
                        assert_eq!(peer_id, node_b_peer_id);
                        //break;
                    }
                    SwarmEvent::IncomingConnection { .. } => {}
                    SwarmEvent::ConnectionEstablished { .. } => {}
                    SwarmEvent::Behaviour(Event::Discovered(_)) => {}
                    SwarmEvent::NewExternalAddrCandidate { .. } => {}
                    SwarmEvent::Dialing { .. } => {}
                    SwarmEvent::Behaviour(Event::PeersFounded(_, _)) => {}
                    SwarmEvent::Behaviour(Event::ReqresOutboundFailure { .. }) => {
                        info!("Outbound failure");
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        if peer_id == node_b_peer_id {
                            info!("Closed node B");
                            break;
                        } else {
                            info!("Closed boot node: {:?}", peer_id);
                            break;
                        }
                    }
                    e => {
                        info!("Node A event: {:?}", e);
                        break;
                    }
                }
            }
        };

        let loop_b = async move {
            loop {
                match node_b.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Identified { peer_id, info }) => {
                        node_b.behaviour_mut().add_identified_peer(peer_id, *info);
                    }
                    SwarmEvent::Behaviour(Event::ReqresMessage { peer_id, message }) => {
                        // Message received from node a.
                        info!("Request message received from peer: {:?}", peer_id);
                        assert_eq!(peer_id, node_a_peer_id);
                        match message {
                            Message::Request { channel, request, .. } => {
                                assert_eq!(request.0, b"Hello Node B".to_vec());
                                // Send response to node a.
                                let _ = node_b
                                    .behaviour_mut()
                                    .send_response(channel, b"Hello Node A".to_vec());
                            }
                            Message::Response { .. } => {}
                        }

                        break;
                    }
                    /*SwarmEvent::IncomingConnection { .. } => {}
                    SwarmEvent::ConnectionEstablished { .. } => {}
                    SwarmEvent::Behaviour(Event::Discovered(_)) => {}
                    SwarmEvent::NewExternalAddrCandidate { .. } => {}
                    SwarmEvent::Behaviour(Event::UnreachablePeer(peer_id)) => {
                        info!("Unreachable peer: {:?}", peer_id);
                        assert_eq!(peer_id, node_a_peer_id);
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        if peer_id == node_a_peer_id {
                            info!("Closed node A");
                            break;
                        }
                    }*/
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

    // Build relay server.
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
            listen_addresses: vec![],
            port_reuse: false,
        }
    }

}
