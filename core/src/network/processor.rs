// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::routing::{RoutingBehavior, RoutingBehaviorEvent as RoutingEvent};

use crate::{
    crypto::{KeyMaterial, KeyPair},
    message::{Command, NetworkEvent},
    ListenAddr, NetworkSettings,
};

use tell::{binary, ProtocolSupport};

use futures::StreamExt;
use libp2p::{
    identify,
    identity::{ed25519, Keypair, PublicKey},
    kad::{
        self, AddProviderOk, GetClosestPeersError, GetClosestPeersOk, GetProvidersOk, GetRecordOk,
        PutRecordOk, QueryResult,
    },
    multiaddr::Protocol,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter,
    time::Duration,
};

const LOG_TARGET: &str = "Kore_Ledger::Network";
const RETRY_TIMEOUT: u64 = 30000;

/// Network Structure for connect message-sender, message-receiver and LibP2P network stack
pub struct NetworkProcessor {
    /// Node listen addresses.
    addr: Vec<ListenAddr>,
    /// The LibP2P Swarm.
    swarm: Swarm<Behaviour>,
    /// The command sender.
    command_sender: mpsc::Sender<Command>,
    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,
    event_sender: mpsc::Sender<NetworkEvent>,
    /// Pending messages to send.
    pendings: HashMap<PeerId, VecDeque<Vec<u8>>>,
    /// Active kad queries.
    active_get_querys: HashSet<PeerId>,
    /// Cancellation token for the network processor.
    token: CancellationToken,
    /// The bootstrap nodes list.
    bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
    /// The pending bootstrap nodes.
    pending_bootstrap_nodes: HashMap<PeerId, Multiaddr>,
    /// The number of retries for the bootstrap process.
    bootstrap_retries_steam:
        futures::stream::futures_unordered::FuturesUnordered<tokio::time::Sleep>,
    /// Node public key.
    node_public_key: Vec<u8>,
    /// Node external addresses.
    external_addresses: Vec<Multiaddr>,
}

impl NetworkProcessor {
    /// Create a new `NetworkProcessor` instance.
    pub fn new(
        key_pair: KeyPair,
        token: CancellationToken,
        event_sender: mpsc::Sender<NetworkEvent>,
        settings: &NetworkSettings,
    ) -> Self {
        // Create channels to communicate events and commands
        let (command_sender, command_receiver) = mpsc::channel(10000);
        /*let transport_protocol =
        check_listen_addr_integrity(&addr).expect("Invalid listen addresses");*/
        let node_public_key = key_pair.public_key_bytes();
        let keys = {
            let sk = ed25519::SecretKey::try_from_bytes(key_pair.secret_key_bytes())
                .expect("Invalid keypair");
            let kp = ed25519::Keypair::from(sk);
            Keypair::from(kp)
        };

        let bootstrap_nodes =
            super::network_access_points(&settings.known_nodes).expect("Invalid bootstrap nodes");
        let external_addresses = super::external_addresses(&settings.external_address)
            .expect("Invalid external addresses");
        //let mut bytes = [0u8; 32];
        //bytes.copy_from_slice(key_pair.secret_key_bytes().as_slice());
        //let node_key = identity::Keypair::ed25519_from_bytes(bytes).expect("Invalid keypair");
        //let peer_id = PeerId::from_public_key(&node_key.public());
        // Build the LibP2P Swarm
        let swarm = SwarmBuilder::with_existing_identity(keys)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .expect("Failed to build TCP transport")
            .with_dns()
            .expect("Failed to build DNS transport")
            .with_behaviour(|keys| {
                let config = tell::Config::default()
                    .with_max_concurrent_streams(settings.max_concurrent_streams)
                    .with_message_timeout(Duration::from_secs(settings.message_timeout));
                let protocols = iter::once((
                    StreamProtocol::new(super::KORE_PROTOCOL),
                    ProtocolSupport::InboundOutbound,
                ));
                Behaviour {
                    routing: RoutingBehavior::new(&keys.public(), bootstrap_nodes.clone(), None),
                    tell: binary::Behaviour::new(protocols, config),
                }
            })
            .expect("Failed to build behaviour")
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let pending_bootstrap_nodes = HashMap::new();
        let bootstrap_retries_steam = futures::stream::futures_unordered::FuturesUnordered::new();
        NetworkProcessor {
            addr: settings.listen_addr.clone(),
            swarm,
            command_sender,
            command_receiver,
            event_sender,
            pendings: HashMap::new(),
            active_get_querys: HashSet::new(),
            token,
            bootstrap_nodes,
            pending_bootstrap_nodes,
            bootstrap_retries_steam,
            node_public_key,
            external_addresses,
        }
    }

    #[cfg(test)]
    pub fn _new_ephemeral(
        token: CancellationToken,
        event_sender: mpsc::Sender<NetworkEvent>,
        settings: &NetworkSettings,
    ) -> Self {
        use libp2p_swarm_test::SwarmExt;
        // Create channels to communicate events and commands
        let (command_sender, command_receiver) = mpsc::channel(10000);
        /*let transport_protocol =
        check_listen_addr_integrity(&addr).expect("Invalid listen addresses");*/
        //let node_public_key = key_pair.public_key_bytes();
        let keys = Keypair::generate_ed25519();
        let public_key = keys.public();
        let bootstrap_nodes =
            super::network_access_points(&settings.known_nodes).expect("Invalid bootstrap nodes");
        let external_addresses = super::external_addresses(&settings.external_address)
            .expect("Invalid external addresses");

        let swarm = Swarm::new_ephemeral(|_| {
            let config = tell::Config::default()
                .with_max_concurrent_streams(settings.max_concurrent_streams)
                .with_message_timeout(Duration::from_secs(settings.message_timeout));
            let protocols = iter::once((
                StreamProtocol::new(super::KORE_PROTOCOL),
                ProtocolSupport::InboundOutbound,
            ));
            Behaviour {
                routing: RoutingBehavior::new(&public_key, bootstrap_nodes.clone(), None),
                tell: binary::Behaviour::new(protocols, config),
            }
        });
        let node_public_key = public_key.try_into_ed25519().unwrap().to_bytes().to_vec();
        Self {
            addr: settings.listen_addr.clone(),
            swarm,
            command_sender,
            command_receiver,
            event_sender,
            pendings: HashMap::new(),
            active_get_querys: HashSet::new(),
            token,
            bootstrap_nodes,
            pending_bootstrap_nodes: HashMap::new(),
            bootstrap_retries_steam: futures::stream::futures_unordered::FuturesUnordered::new(),
            node_public_key,
            external_addresses,
        }
    }
    /// Run network processor.
    pub async fn run(mut self) {
        log::debug!("Running network");
        for external_address in self.external_addresses.clone().into_iter() {
            self.swarm.add_external_address(external_address);
        }
        for addr in self.addr.iter() {
            if addr.get_port().is_some() {
                let multiadd: Multiaddr = addr
                    .to_string()
                    .unwrap()
                    .parse()
                    .expect("String para multiaddress es válida");
                let result = self.swarm.listen_on(multiadd);
                if result.is_err() {
                    log::error!("Error: {:?}", result.unwrap_err());
                }
            }
        }
        for (_, addr) in self.bootstrap_nodes.iter() {
            let Ok(()) = self.swarm.dial(addr.to_owned()) else {
                panic!("Conection with bootstrap failed");
            };
        }
        loop {
            tokio::select! {
                event = self.swarm.next() => self.handle_event(
                    event.expect("Swarm stream to be infinite.")).await,
                command = self.command_receiver.recv() => match command {
                    Some(c) => self.handle_command(c).await,
                    // Command channel closed, thus shutting down the network
                    // event loop.
                    None =>  {return;},
                },
                Some(_) = self.bootstrap_retries_steam.next() => self.connect_to_pending_bootstraps(),
                _ = self.token.cancelled() => {
                    log::debug!("Shutdown received");
                    break;
                }
            }
        }
        self.token.cancel();
        log::info!("Ended");
    }

    /// Handle networks events.
    async fn handle_event(&mut self, event: SwarmEvent<KoreNetworkEvent>) {
        match event {
            SwarmEvent::Dialing {
                peer_id,
                connection_id,
            } => {
                log::debug!(
                    "Dialing to peer: {:?}; Connection: {:?}",
                    peer_id,
                    connection_id
                );
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                let local_peer_id = *self.swarm.local_peer_id();
                log::info!(
                    "listening on {:?}",
                    &address.with(Protocol::P2p(local_peer_id))
                );
                // let addr_with_peer = address.clone().with(Protocol::P2p(local_peer_id.into()));
                // let addr_with_peer_bytes = addr_with_peer.to_vec();
                // let crypto_proof = self
                //     .controller_mc
                //     .sign(Payload::Buffer(addr_with_peer_bytes))
                //     .unwrap();
                // let value = bincode::serialize(&(addr_with_peer, crypto_proof)).unwrap();
                // match self.swarm.behaviour_mut().routing.put_record(
                //     Record {
                //         key: Key::new(&self.controller_mc.public_key_bytes()),
                //         value,
                //         publisher: None,
                //         expires: None,
                //     },
                //     Quorum::One,
                // ) {
                //     Ok(_) => (),
                //     Err(_) => panic!("HOLA"), // No debería fallar, ¿Tirarlo si falla?
                // }
            }
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                log::debug!(
                    "{}: Connected to {} at {}",
                    LOG_TARGET,
                    peer_id,
                    endpoint.get_remote_address()
                );
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause: Some(error),
                ..
            } => {
                log::debug!(
                    "{}: Disconnected from {} with error {}",
                    LOG_TARGET,
                    peer_id,
                    error
                );
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id: _,
                peer_id,
                error,
            } => {
                // Fixme for refused connections
                log::debug!("{}: Connection error: {}", LOG_TARGET, error);
                if let Some(peer_id) = peer_id {
                    // Check if the peerID was a bootstrap node
                    if let Some((id, multiaddr)) =
                        self.bootstrap_nodes.iter().find(|(id, _)| *id == peer_id)
                    {
                        self.pending_bootstrap_nodes
                            .insert(*id, multiaddr.to_owned());
                        // Insert new timer if there was not any before
                        if self.bootstrap_retries_steam.is_empty() {
                            self.bootstrap_retries_steam
                                .push(tokio::time::sleep(Duration::from_millis(RETRY_TIMEOUT)));
                        }
                    }
                    // match self.peer_to_controller.remove(&peer_id) {
                    //     Some(controller) => {
                    //         self.controller_to_peer.remove(&controller);
                    //     }
                    //     None => {}
                    // }
                }
            }
            SwarmEvent::Behaviour(behaviour_event) => match behaviour_event {
                KoreNetworkEvent::Tell(ev) => match ev {
                    tell::Event::MessageSent {
                        peer_id,
                        outbound_id,
                    } => {
                        log::debug!(
                            "{}: Message sent to: {} with id {}",
                            LOG_TARGET,
                            peer_id,
                            outbound_id
                        );
                    }
                    tell::Event::Message { peer_id, message } => {
                        log::debug!("{}: Message received from: {}", LOG_TARGET, peer_id);
                        self.event_sender
                            .send(NetworkEvent::MessageReceived {
                                message: message.message,
                            })
                            .await
                            .expect("Event receiver not to be dropped.");
                    }
                    tell::Event::InboundFailure {
                        peer_id,
                        inbound_id,
                        error: _,
                    } => {
                        log::debug!(
                            "{}: Message failed from: {} with id: {}",
                            LOG_TARGET,
                            peer_id,
                            inbound_id
                        );
                        // Delete cache peer id and address for that controller
                        // match self.peer_to_controller.remove(&peer_id) {
                        //     Some(controller) => {
                        //         self.controller_to_peer.remove(&controller);
                        //     }
                        //     None => {}
                        // }
                    }
                    tell::Event::OutboundFailure {
                        peer_id,
                        outbound_id,
                        error: _,
                    } => {
                        log::debug!(
                            "
                            {}: Message failed to: {}, with id: {}",
                            LOG_TARGET,
                            peer_id,
                            outbound_id
                        );
                    }
                    tell::Event::MessageProcessed {
                        peer_id,
                        inbound_id,
                    } => {
                        log::debug!(
                            "{}: Message processed from: {} with id: {}",
                            LOG_TARGET,
                            peer_id,
                            inbound_id
                        );
                    }
                },

                KoreNetworkEvent::Routing(ev) => match ev {
                    RoutingEvent::Kad(kad::Event::OutboundQueryProgressed {
                        id: _,
                        result,
                        stats: _,
                        step: _,
                    }) => match result {
                        QueryResult::GetRecord(Ok(GetRecordOk::FoundRecord(record, ..))) => {
                            log::debug!("Got Record {:?}", record);
                            // for PeerRecord {
                            //     record:
                            //         Record {
                            //             key,
                            //             value,
                            //             publisher,
                            //             ..
                            //         },
                            //     ..
                            // } in ok.records
                            // {
                            //     let mc_bytes = key.to_vec();
                            //     let mc = Ed25519KeyPair::from_public_key(&mc_bytes);
                            //     match bincode::deserialize::<(Multiaddr, Vec<u8>)>(&value) {
                            //         Ok((addr, crypto_proof)) => {
                            //             // Comprobar la firma
                            //             match mc
                            //                 .verify(Payload::Buffer(addr.to_vec()), &crypto_proof)
                            //             {
                            //                 Ok(_) => {
                            //                     // Si está bien guardar en los hashmaps la info y hacer dial
                            //                     // Obtener el peerId a partir de la addr:
                            //                     let mut peer_id: Option<PeerId> = None;
                            //                     for protocol in addr.clone().iter() {
                            //                         if let Protocol::P2p(peer_id_multihash) =
                            //                             protocol
                            //                         {
                            //                             match PeerId::from_multihash(peer_id_multihash) {
                            //                                 Ok(pid) => {
                            //                                     if let Some(peer_id_publisher) = publisher {
                            //                                         // Comprobación de que el publisher es el propio peerId que buscamos
                            //                                         if pid != peer_id_publisher {
                            //                                             continue;
                            //                                         }
                            //                                     }
                            //                                     peer_id = Some(pid);
                            //                                     break;
                            //                                 },
                            //                                 Err(_) => debug!("Error al parsear multiaddr a peerId en get"),
                            //                             }
                            //                         }
                            //                     }
                            //                     if peer_id.is_none() {
                            //                         continue;
                            //                     }
                            //                     let peer_id = peer_id.unwrap();
                            //                     match self.swarm.dial(addr.clone()) {
                            //                         Ok(_) => {
                            //                             debug!("Success en DIAL");
                            //                             // Si funciona el dial actualizar las estructuras de datos
                            //                             self.routing_cache
                            //                                 .insert(peer_id, vec![addr]);
                            //                             // self.controller_to_peer
                            //                             //     .insert(mc_bytes.clone(), peer_id);
                            //                             // self.peer_to_controller
                            //                             //     .insert(peer_id, mc_bytes.clone());
                            //                             self.active_get_querys.remove(&peer_id);
                            //                             // Mandar mensajes pendientes
                            //                             self.send_pendings(&peer_id);
                            //                         }
                            //                         Err(e) => {
                            //                             debug!("{}", e);
                            //                             continue;
                            //                         }
                            //                     }
                            //                 }
                            //                 Err(_) => {
                            //                     continue;
                            //                 }
                            //             }
                            //         }
                            //         Err(e) => {
                            //             debug!("DESERIALICE VA MAL");
                            //             debug!("Problemas al recuperar la Multiaddr del value del Record: {:?}", e);
                            //         }
                            //     }
                            // }
                        }
                        QueryResult::GetRecord(Err(err)) => {
                            log::debug!("Failed to get record: {:?}", err);
                            // let mc_bytes = err.key().to_vec();
                            // self.active_get_querys.remove(&mc_bytes);
                        }
                        QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                            log::debug!("Successfully put record {:?}", key);
                        }
                        QueryResult::PutRecord(Err(err)) => {
                            log::debug!("Failed to put record: {:?}", err);
                        }
                        QueryResult::GetProviders(Ok(GetProvidersOk::FoundProviders {
                            key,
                            providers,
                        })) => {
                            for peer in providers {
                                log::debug!("Peer {:?} provides key {:?}", peer, key.as_ref());
                            }
                        }
                        QueryResult::GetProviders(Err(err)) => {
                            log::debug!("Failed to get providers: {:?}", err);
                        }
                        QueryResult::StartProviding(Ok(AddProviderOk { key })) => {
                            log::debug!("Successfully put provider record {:?}", key);
                        }
                        QueryResult::StartProviding(Err(err)) => {
                            log::debug!("Failed to put provider record: {:?}", err);
                        }
                        QueryResult::GetClosestPeers(gcp_res) => match gcp_res {
                            Ok(GetClosestPeersOk { key, .. }) => {
                                log::debug!("GCP OK: {:?}", key);
                                let peer_id = match PeerId::from_bytes(&key) {
                                    Ok(peer_id) => peer_id,
                                    Err(_) => {
                                        log::error!("Error parsing PeerId from GCP Ok response");
                                        return;
                                    }
                                };
                                self.active_get_querys.remove(&peer_id);
                            }
                            Err(GetClosestPeersError::Timeout { key, .. }) => {
                                log::debug!("GCP ERR: {:?}", key);
                                let peer_id = match PeerId::from_bytes(&key) {
                                    Ok(peer_id) => peer_id,
                                    Err(_) => {
                                        log::error!("Error parsing PeerId from GCP Err response");
                                        return;
                                    }
                                };
                                self.active_get_querys.remove(&peer_id);
                            }
                        },
                        e => {
                            log::debug!("Unhandled QueryResult {:?}", e);
                        }
                    },
                    RoutingEvent::Kad(kad::Event::RoutablePeer { peer, address }) => {
                        log::debug!(
                            "{}: Routable Peer: {:?}; Address: {:?}.",
                            LOG_TARGET,
                            peer,
                            address,
                        );
                        if self.active_get_querys.contains(&peer) {
                            //self.swarm.behaviour_mut().tell.set_route(peer, address);
                            self.active_get_querys.remove(&peer);
                            self.send_pendings(&peer);
                        }
                    }
                    RoutingEvent::Identify(identify::Event::Received { peer_id, info }) => {
                        for addr in info.listen_addrs {
                            self.swarm.behaviour_mut().add_address(&peer_id, addr);
                        }
                        log::debug!("{}: Identify: Peer: {:?}.", LOG_TARGET, peer_id);
                    }
                    _ => {
                        //self.swarm.behaviour_mut().routing.handle_event(ev);
                    }
                },
            },
            other => {
                log::debug!("{}: Unhandled event {:?}", LOG_TARGET, other);
            }
        }
    }

    /// Handle network command.
    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::StartProviding { keys } => {
                for key in keys {
                    self.swarm.behaviour_mut().routing.start_providing(&key);
                }
            }
            Command::SendMessage { receptor, message } => {
                // Check if we are the receptor
                if receptor == self.node_public_key {
                    // It is not needed to send the message
                    self.event_sender
                        .send(NetworkEvent::MessageReceived { message })
                        .await
                        .expect("Event receiver not to be dropped.");
                    return;
                }
                log::debug!("{}: Sending Message", LOG_TARGET);
                // Check if we have the peerId of the controller in cache
                let peer_id = match ed25519::PublicKey::try_from_bytes(&receptor) {
                    Ok(public_key) => {
                        let public_key = PublicKey::from(public_key);
                        PeerId::from_public_key(&public_key)
                    }
                    Err(_error) => {
                        log::error!(
                            "Error al tratar de enviar mensaje, el controllerId no es válido"
                        );
                        return;
                    }
                };

                // If we have it check if we have the address (need to fill in with cache addresses)
                /*let addresses_of_peer = self.swarm.behaviour_mut().addresses_of_peer(&peer_id);
                if !addresses_of_peer.is_empty() {
                    debug!("MANDANDO MENSAJE, TENGO DIRECCIÓN");
                    // If we have an address, send the message
                    self.swarm
                        .behaviour_mut()
                        .tell
                        .send_message(&peer_id, &message);
                    return;
                }*/

                // Check if we are not already making the same query in the DHT
                if self.active_get_querys.get(&peer_id).is_none() {
                    // Make petition if we dont have it's PeerId to store it
                    self.active_get_querys.insert(peer_id);
                    let query_id = self
                        .swarm
                        .behaviour_mut()
                        .routing
                        .get_closest_peers(peer_id);
                    log::debug!(
                        "Query get_record {:?} para mandar request a {:?}",
                        query_id,
                        peer_id
                    );
                }
                // Store de message in the pendings for that controller
                match self.pendings.get_mut(&peer_id) {
                    Some(pending_list) => {
                        if pending_list.len() >= 100 {
                            pending_list.pop_front();
                        }
                        pending_list.push_back(message);
                    }
                    None => {
                        let mut pendings = VecDeque::new();
                        pendings.push_back(message);
                        self.pendings.insert(peer_id, pendings);
                    }
                }
            }
            Command::Bootstrap => {
                self.swarm.behaviour_mut().bootstrap();
            }
        }
    }

    /// Network client
    pub fn client(&self) -> mpsc::Sender<Command> {
        self.command_sender.clone()
    }

    /// Returns local peer id.
    pub fn local_peer_id(&self) -> PeerId {
        self.swarm.local_peer_id().clone()
    }

    /// Connect to pending bootstraps.
    fn connect_to_pending_bootstraps(&mut self) {
        let keys: Vec<PeerId> = self.pending_bootstrap_nodes.keys().cloned().collect();
        for peer in keys {
            let addr = self.pending_bootstrap_nodes.remove(&peer).unwrap();
            let Ok(()) = self.swarm.dial(addr.to_owned()) else {
                panic!("Conection with bootstrap failed");
            };
        }
    }

    /// Send all the pending messages to the specified controller
    fn send_pendings(&mut self, peer_id: &PeerId) {
        let pending_messages = self.pendings.remove(peer_id);
        if let Some(pending_messages) = pending_messages {
            for message in pending_messages.into_iter() {
                log::debug!("MANDANDO MENSAJE");
                self.swarm.behaviour_mut().send_message(peer_id, message);
            }
        }
    }
}

/// The `Behaviour` is a custom network behaviour that combines routing and tell protocols.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "KoreNetworkEvent")]
pub struct Behaviour {
    routing: RoutingBehavior,
    tell: binary::Behaviour,
}

impl Behaviour {
    /// Add a known address for the given peer id.
    pub fn add_address(&mut self, peer_id: &PeerId, addr: Multiaddr) {
        self.tell.add_address(peer_id, addr.clone());
        self.routing.add_address(peer_id, addr);
    }

    /// Send message to the given peer id.
    pub fn send_message(&mut self, peer_id: &PeerId, message: Vec<u8>) {
        self.tell.send_message(peer_id, message);
    }

    /// Bootstrap the local node to the network.
    pub fn bootstrap(&mut self) {
        let _ = self.routing.bootstrap();
    }
}

#[derive(Debug)]
pub enum KoreNetworkEvent {
    Tell(tell::Event<Vec<u8>>),
    Routing(RoutingEvent),
}

/// Adapt `tell::Event` to `KoreNetworkEvent`
impl From<tell::Event<Vec<u8>>> for KoreNetworkEvent {
    fn from(event: tell::Event<Vec<u8>>) -> KoreNetworkEvent {
        KoreNetworkEvent::Tell(event)
    }
}

/// Adapt `RoutingEvent` to `KoreNetworkEvent`
impl From<RoutingEvent> for KoreNetworkEvent {
    fn from(event: RoutingEvent) -> KoreNetworkEvent {
        KoreNetworkEvent::Routing(event)
    }
}

#[cfg(test)]
mod tests {

    use crate::network::{
        routing::{RoutingBehavior, RoutingBehaviorEvent},
        KORE_PROTOCOL,
    };

    use super::{Behaviour as TestBehaviour, KoreNetworkEvent};

    use libp2p::{
        identify::Event as IdentifyEvent,
        kad::Event as KadEvent,
        multiaddr::Protocol,
        swarm::{FromSwarm, SwarmEvent},
        Multiaddr, PeerId, StreamProtocol, Swarm,
    };
    use libp2p_swarm_test::SwarmExt;

    use futures::StreamExt;
    use rand::random;

    use std::pin::pin;

    type TestSwarm = Swarm<TestBehaviour>;

    #[async_std::test]
    #[ignore]
    async fn test_send_message() {
        let mut boot_nodes = vec![];

        // Build node 1
        let (node1_addr, mut node1) = build_node(boot_nodes.clone());
        let node1_peer = *node1.local_peer_id();

        // Build node 2
        let (node2_addr, mut node2) = build_node(boot_nodes.clone());
        let node2_peer = *node2.local_peer_id();

        let result = node1.dial(node2_addr);
        assert!(result.is_ok());

        let node1_loop = async move {
            let message = b"Hello node 2".to_vec();
            node1.behaviour_mut().send_message(&node2_peer, message);
            loop {
                match node1.next().await {
                    Some(SwarmEvent::Behaviour(KoreNetworkEvent::Tell(tell::Event::Message {
                        peer_id,
                        message,
                    }))) => {
                        println!(
                            "Message received from {:?}: {}",
                            peer_id,
                            String::from_utf8_lossy(&message.message)
                        );
                        assert_eq!(peer_id, node2_peer);
                        assert_eq!(&message.message, b"Hello node 1");
                        break;
                    }
                    Some(SwarmEvent::Behaviour(KoreNetworkEvent::Routing(
                        RoutingBehaviorEvent::Identify(IdentifyEvent::Received { peer_id, info }),
                    ))) => {
                        assert_eq!(peer_id, node2_peer);
                        for addr in info.listen_addrs {
                            node1.behaviour_mut().add_address(&peer_id, addr);
                        }
                    }
                    e => {
                        println!("Node 1 -> {:?}", e);
                        println!("");
                    }
                }
            }
        };

        let node2_loop = async move {
            loop {
                match node2.next().await {
                    Some(SwarmEvent::Behaviour(KoreNetworkEvent::Tell(tell::Event::Message {
                        peer_id,
                        message,
                    }))) => {
                        println!(
                            "Message received from {:?}: {}",
                            peer_id,
                            String::from_utf8_lossy(&message.message)
                        );
                        assert_eq!(peer_id, node1_peer);
                        assert_eq!(&message.message, b"Hello node 2");
                        let message = b"Hello node 1".to_vec();
                        node2.behaviour_mut().send_message(&node1_peer, message);
                        break;
                    }
                    Some(SwarmEvent::Behaviour(KoreNetworkEvent::Routing(
                        RoutingBehaviorEvent::Identify(IdentifyEvent::Received { peer_id, info }),
                    ))) => {
                        assert_eq!(peer_id, node1_peer);
                        for addr in info.listen_addrs {
                            node2.behaviour_mut().add_address(&peer_id, addr);
                        }
                    }
                    e => {
                        println!("Node 2 -> {:?}", e);
                        println!("");
                    }
                }
            }
        };

        let node1_loop = pin!(node1_loop);
        let node2_loop = pin!(node2_loop);

        futures::join!(node1_loop, node2_loop);
    }

    /// Build node.
    fn build_node(bootstrap_nodes: Vec<(PeerId, Multiaddr)>) -> (Multiaddr, TestSwarm) {
        let mut swarm = Swarm::new_ephemeral(|key_pair| TestBehaviour {
            routing: RoutingBehavior::new(&key_pair.public(), bootstrap_nodes.clone(), None),
            tell: tell::binary::Behaviour::new(
                vec![(
                    StreamProtocol::new(KORE_PROTOCOL),
                    tell::ProtocolSupport::InboundOutbound,
                )],
                Default::default(),
            ),
        });

        let address: Multiaddr = Protocol::Memory(random::<u64>()).into();
        swarm.listen_on(address.clone()).unwrap();
        swarm.add_external_address(address.clone());

        (address, swarm)
    }

    /*
        use crate::network::routing::RoutingBehaviorEvent::*;

        use super::*;

        use libp2p::{
            identify::Event as IdentifyEvent,
            kad::{Event as KadEvent, RoutingUpdate},
            swarm::SwarmEvent,
            Swarm,
        };
        use libp2p_swarm_test::SwarmExt;

        use IdentifyEvent::*;
        use KadEvent::*;

        #[async_std::test]
        async fn test_network_behaviour() {
            let mut bootstrap_nodes = vec![];

            let mut bootstrap = Swarm::new_ephemeral(|key_pair| {
                Behaviour::new(&key_pair.public(), bootstrap_nodes.clone())
            });
            let bootstrap_peer_id = *bootstrap.local_peer_id();
            let (memory_addr, _) = bootstrap.listen().await;
            bootstrap.add_external_address(memory_addr.clone());

            bootstrap_nodes.push((bootstrap_peer_id, memory_addr.clone()));

            let mut node1 = Swarm::new_ephemeral(|key_pair| {
                Behaviour::new(&key_pair.public(), bootstrap_nodes.clone())
            });

            let node1_peer = *node1.local_peer_id();

            let peer = node1
                .wait(|e| match e {
                    SwarmEvent::Behaviour(KoreNetworkEvent::Routing(RoutingEvent::Kad(
                        RoutingUpdated { peer, .. },
                    ))) => Some(peer),
                    _ => None,
                })
                .await;
            println!("{:?}", peer);
            assert_eq!(peer, bootstrap_peer_id);
            node1.behaviour_mut().bootstrap();

            let mut node2 = Swarm::new_ephemeral(|key_pair| {
                Behaviour::new(&key_pair.public(), bootstrap_nodes.clone())
            });

            let node2_peer = *node2.local_peer_id();

            let peer = node2
                .wait(|e| match e {
                    SwarmEvent::Behaviour(KoreNetworkEvent::Routing(RoutingEvent::Kad(
                        RoutingUpdated { peer, .. },
                    ))) => Some(peer),
                    _ => None,
                })
                .await;

            assert_eq!(peer, bootstrap_peer_id);
            node2.behaviour_mut().bootstrap();

            node1
                .behaviour_mut()
                .tell
                .send_message(&node2_peer, b"Hello node 2".to_vec());

            let peer1 = async move {
                loop {
                    match node1.next_swarm_event().await.try_into_behaviour_event() {
                        Ok(KoreNetworkEvent::Tell(tell::Event::Message { peer_id, message })) => {
                            println!("{}", String::from_utf8_lossy(&message.message));
                            assert_eq!(peer_id, node2_peer);
                            assert_eq!(&message.message, b"Hello node 1");
                            node1
                                .behaviour_mut()
                                .tell
                                .send_message(&node2_peer, b"Hello node 2".to_vec());
                        }
                        e => println!("{:?}", e),
                    }
                }
            };

            let peer2 = async move {
                node2
                    .behaviour_mut()
                    .tell
                    .send_message(&node1_peer, b"Hello node 1".to_vec());
                loop {
                    match node2.next_swarm_event().await.try_into_behaviour_event() {
                        Ok(KoreNetworkEvent::Tell(tell::Event::Message { peer_id, message })) => {
                            println!("{}", String::from_utf8_lossy(&message.message));
                            assert_eq!(peer_id, node1_peer);
                            assert_eq!(&message.message, b"Hello node 2");
                        }
                        e => println!("{:?}", e),
                    }
                }
            };

            async_std::task::spawn(Box::pin(peer1));
            peer2.await;
        }
    */
}
