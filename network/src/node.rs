// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use libp2p::{
    core::{ConnectedPoint, Endpoint},
    identify::{
        Behaviour as Identify, Config as IdentifyConfig, Event as IdentifyEvent,
        Info as IdentifyInfo,
    },
    identity::PublicKey,
    ping::{Behaviour as Ping, Event as PingEvent},
    swarm::{
        behaviour::{ConnectionClosed, ConnectionEstablished, ListenFailure},
        ConnectionDenied, ConnectionHandler, ConnectionHandlerSelect, ConnectionId, DialFailure,
        FromSwarm, NetworkBehaviour, ToSwarm,
    },
    Multiaddr, PeerId,
};

use either::Either;
use fnv::FnvHashMap;
use futures::{stream::unfold, FutureExt, Stream, StreamExt};
use futures_timer::Delay;
use smallvec::SmallVec;
use tracing::{debug, error, trace};

use std::{
    collections::{hash_map::Entry, HashSet},
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
    time::{Duration, Instant},
};

/// Name for logging.
const TARGET_NODE: &str = "KoreNetwork-Node";
/// Time after we disconnect from a node before we purge its information from the cache.
const CACHE_EXPIRE: Duration = Duration::from_secs(10 * 60);
/// Interval at which we perform garbage collection on the node info.
const GARBAGE_COLLECT_INTERVAL: Duration = Duration::from_secs(2 * 60);

/// Information about a node we're connected to.
#[derive(Debug)]
struct NodeInfo {
    /// When we will remove the entry about this node from the list, or `None` if we're connected
    /// to the node.
    info_expire: Option<Instant>,
    /// Non-empty list of connected endpoints, one per connection.
    endpoints: SmallVec<[ConnectedPoint; crate::MAX_CONNECTIONS_PER_PEER]>,
    /// Version reported by the remote, or `None` if unknown.
    client_version: Option<String>,
    /// Latest ping time with this node.
    latest_ping: Option<Duration>,
}

impl NodeInfo {
    fn new(endpoint: ConnectedPoint) -> Self {
        let mut endpoints = SmallVec::new();
        endpoints.push(endpoint);
        Self {
            info_expire: None,
            endpoints,
            client_version: None,
            latest_ping: None,
        }
    }
}

/// Utility struct for tracking external addresses. The data is shared with the `NetworkService`.
#[derive(Debug, Clone, Default)]
pub struct ExternalAddresses {
    addresses: Arc<Mutex<HashSet<Multiaddr>>>,
}

impl ExternalAddresses {
    /// Add an external address.
    pub fn add(&mut self, addr: Multiaddr) {
        let mut addresses = self.addresses.lock().expect("Mutex should not be poisoned");
        addresses.insert(addr);
    }

    /// Remove an external address.
    pub fn remove(&mut self, addr: &Multiaddr) {
        let mut addresses = self.addresses.lock().expect("Mutex should not be poisoned");
        addresses.remove(addr);
    }
}

/// The network node behaviour.
pub struct Behaviour {
    /// The `ping` behaviour.
    ping: Ping,

    /// The `identify` behaviour.ç
    identify: Identify,

    /// Information that we know about all nodes.
    nodes_info: FnvHashMap<PeerId, NodeInfo>,

    /// Interval at which we perform garbage collection in `nodes_info`.
    garbage_collect: Pin<Box<dyn Stream<Item = ()> + Send>>,

    /// Record keeping of external addresses. Data is queried by the `NetworkService`.
    external_addresses: ExternalAddresses,
}

impl Behaviour {
    pub fn new(
        user_agent: &str,
        public_key: &PublicKey,
        external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,
    ) -> Self {
        let identify = {
            let identify_config =
                IdentifyConfig::new(crate::NETWORK_PROTOCOL.to_owned(), public_key.clone())
                    .with_agent_version(user_agent.to_owned())
                    .with_cache_size(0); // We don't need to cache anything.
            Identify::new(identify_config)
        };
        Self {
            ping: Ping::new(Default::default()),
            identify,
            nodes_info: FnvHashMap::default(),
            garbage_collect: Box::pin(interval(GARBAGE_COLLECT_INTERVAL)),
            external_addresses: ExternalAddresses {
                addresses: external_addresses,
            },
        }
    }

    /// Borrows `self` and returns a struct giving access to the information about a node.
    ///
    /// Returns `None` if we don't know anything about this node. Always returns `Some` for nodes
    /// we're connected to, meaning that if `None` is returned then we're not connected to that
    /// node.
    pub fn node(&self, peer_id: &PeerId) -> Option<Node> {
        self.nodes_info.get(peer_id).map(Node)
    }

    /// Inserts a ping time in the cache. Has no effect if we don't have any entry for that node,
    /// which shouldn't happen.
    fn handle_ping_report(&mut self, peer_id: &PeerId, ping_time: Duration) {
        trace!(target: TARGET_NODE, "Ping time with {:?}: {:?}", peer_id, ping_time);
        if let Some(entry) = self.nodes_info.get_mut(peer_id) {
            entry.latest_ping = Some(ping_time);
        } else {
            error!(target: TARGET_NODE,
				"Received ping from node we're not connected to {:?}", peer_id);
        }
    }

    /// Inserts an identify record in the cache. Has no effect if we don't have any entry for that
    /// node, which shouldn't happen.
    fn handle_identify_report(&mut self, peer_id: &PeerId, info: &IdentifyInfo) {
        trace!(target: TARGET_NODE, "Identified {:?} => {:?}", peer_id, info);
        if let Some(entry) = self.nodes_info.get_mut(peer_id) {
            entry.client_version = Some(info.agent_version.clone());
        } else {
            error!(target: TARGET_NODE,
				"Received pong from node we're not connected to {:?}", peer_id);
        }
    }
}

/// Gives access to the information about a node.
pub struct Node<'a>(&'a NodeInfo);

impl<'a> Node<'a> {
    /// Returns the endpoint of an established connection to the peer.
    ///
    /// Returns `None` if we are disconnected from the node.
    pub fn endpoint(&self) -> Option<&'a ConnectedPoint> {
        self.0.endpoints.get(0)
    }

    /// Returns the latest version information we know of.
    pub fn client_version(&self) -> Option<&'a str> {
        self.0.client_version.as_deref()
    }

    /// Returns the latest ping time we know of for this node. `None` if we never successfully
    /// pinged this node.
    pub fn latest_ping(&self) -> Option<Duration> {
        self.0.latest_ping
    }
}

/// Event that can be emitted by the behaviour.
#[derive(Debug)]
pub enum Event {
    /// We have obtained identity information from a peer, including the addresses it is listening
    /// on.
    Identified {
        /// Id of the peer that has been identified.
        peer_id: PeerId,
        /// Information about the peer.
        info: IdentifyInfo,
    },
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = ConnectionHandlerSelect<
        <Ping as NetworkBehaviour>::ConnectionHandler,
        <Identify as NetworkBehaviour>::ConnectionHandler,
    >;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        let ping_handler = self.ping.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )?;
        let identify_handler = self.identify.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )?;
        Ok(ping_handler.select(identify_handler))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        let ping_handler = self.ping.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
        )?;
        let identify_handler = self.identify.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
        )?;
        Ok(ping_handler.select(identify_handler))
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.ping
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)?;
        self.identify
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        // Only `Discovery::handle_pending_outbound_connection` must be returning addresses to
        // ensure that we don't return unwanted addresses.
        Ok(Vec::new())
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(
                e @ ConnectionEstablished {
                    peer_id, endpoint, ..
                },
            ) => {
                self.ping
                    .on_swarm_event(FromSwarm::ConnectionEstablished(e));
                self.identify
                    .on_swarm_event(FromSwarm::ConnectionEstablished(e));

                match self.nodes_info.entry(peer_id) {
                    Entry::Vacant(e) => {
                        e.insert(NodeInfo::new(endpoint.clone()));
                    }
                    Entry::Occupied(e) => {
                        let e = e.into_mut();
                        if e.info_expire
                            .as_ref()
                            .map(|exp| *exp < Instant::now())
                            .unwrap_or(false)
                        {
                            e.client_version = None;
                            e.latest_ping = None;
                        }
                        e.info_expire = None;
                        e.endpoints.push(endpoint.clone());
                    }
                }
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                endpoint,
                remaining_established,
            }) => {
                self.ping
                    .on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
                        peer_id,
                        connection_id,
                        endpoint,
                        remaining_established,
                    }));
                self.identify
                    .on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
                        peer_id,
                        connection_id,
                        endpoint,
                        remaining_established,
                    }));

                if let Some(entry) = self.nodes_info.get_mut(&peer_id) {
                    if remaining_established == 0 {
                        entry.info_expire = Some(Instant::now() + CACHE_EXPIRE);
                    }
                    entry.endpoints.retain(|ep| ep != endpoint)
                } else {
                    error!(target: TARGET_NODE,
						"Unknown connection to {:?} closed: {:?}", peer_id, endpoint);
                }
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id,
                error,
                connection_id,
            }) => {
                self.ping
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id,
                        error,
                        connection_id,
                    }));
                self.identify
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id,
                        error,
                        connection_id,
                    }));
            }
            FromSwarm::ListenerClosed(e) => {
                self.ping.on_swarm_event(FromSwarm::ListenerClosed(e));
                self.identify.on_swarm_event(FromSwarm::ListenerClosed(e));
            }
            FromSwarm::ListenFailure(ListenFailure {
                local_addr,
                send_back_addr,
                error,
                connection_id,
            }) => {
                self.ping
                    .on_swarm_event(FromSwarm::ListenFailure(ListenFailure {
                        local_addr,
                        send_back_addr,
                        error,
                        connection_id,
                    }));
                self.identify
                    .on_swarm_event(FromSwarm::ListenFailure(ListenFailure {
                        local_addr,
                        send_back_addr,
                        error,
                        connection_id,
                    }));
            }
            FromSwarm::ListenerError(e) => {
                self.ping.on_swarm_event(FromSwarm::ListenerError(e));
                self.identify.on_swarm_event(FromSwarm::ListenerError(e));
            }
            FromSwarm::ExternalAddrExpired(e) => {
                self.ping.on_swarm_event(FromSwarm::ExternalAddrExpired(e));
                self.identify
                    .on_swarm_event(FromSwarm::ExternalAddrExpired(e));
            }
            FromSwarm::NewListener(e) => {
                self.ping.on_swarm_event(FromSwarm::NewListener(e));
                self.identify.on_swarm_event(FromSwarm::NewListener(e));
            }
            FromSwarm::ExpiredListenAddr(e) => {
                self.ping.on_swarm_event(FromSwarm::ExpiredListenAddr(e));
                self.identify
                    .on_swarm_event(FromSwarm::ExpiredListenAddr(e));
                self.external_addresses.remove(e.addr);
            }
            FromSwarm::NewExternalAddrCandidate(e) => {
                self.ping
                    .on_swarm_event(FromSwarm::NewExternalAddrCandidate(e));
                self.identify
                    .on_swarm_event(FromSwarm::NewExternalAddrCandidate(e));
            }
            FromSwarm::AddressChange(e) => {
                self.ping.on_swarm_event(FromSwarm::AddressChange(e));
                self.identify.on_swarm_event(FromSwarm::AddressChange(e));
            }
            FromSwarm::NewListenAddr(e) => {
                self.ping.on_swarm_event(FromSwarm::NewListenAddr(e));
                self.identify.on_swarm_event(FromSwarm::NewListenAddr(e));
            }
            FromSwarm::ExternalAddrConfirmed(e) => {
                self.ping
                    .on_swarm_event(FromSwarm::ExternalAddrConfirmed(e));
                self.identify
                    .on_swarm_event(FromSwarm::ExternalAddrConfirmed(e));
                self.external_addresses.add(e.addr.clone());
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            Either::Left(event) => {
                self.ping
                    .on_connection_handler_event(peer_id, connection_id, event)
            }
            Either::Right(event) => {
                self.identify
                    .on_connection_handler_event(peer_id, connection_id, event)
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        // Loop for the ping behaviour.
        loop {
            match self.ping.poll(cx) {
                Poll::Pending => break,
                Poll::Ready(ToSwarm::GenerateEvent(ev)) => {
                    if let PingEvent {
                        peer,
                        result: Ok(rtt),
                        ..
                    } = ev
                    {
                        self.handle_ping_report(&peer, rtt)
                    }
                }
                Poll::Ready(ToSwarm::Dial { opts }) => return Poll::Ready(ToSwarm::Dial { opts }),
                Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                }) => {
                    return Poll::Ready(ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event: Either::Left(event),
                    })
                }
                Poll::Ready(ToSwarm::ExternalAddrConfirmed(addr)) => {
                    return Poll::Ready(ToSwarm::ExternalAddrConfirmed(addr))
                }
                Poll::Ready(ToSwarm::CloseConnection {
                    peer_id,
                    connection,
                }) => {
                    return Poll::Ready(ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                Poll::Ready(ToSwarm::ListenOn { opts }) => {
                    return Poll::Ready(ToSwarm::ListenOn { opts })
                }
                Poll::Ready(ToSwarm::ExternalAddrExpired(addr)) => {
                    return Poll::Ready(ToSwarm::ExternalAddrExpired(addr))
                }
                Poll::Ready(ToSwarm::NewExternalAddrCandidate(addr)) => {
                    return Poll::Ready(ToSwarm::NewExternalAddrCandidate(addr))
                }
                Poll::Ready(ToSwarm::RemoveListener { id }) => {
                    return Poll::Ready(ToSwarm::RemoveListener { id })
                }
                Poll::Ready(_) => {}
            }
        }

        // Loop for the identify behaviour.
        loop {
            match self.identify.poll(cx) {
                Poll::Pending => break,
                Poll::Ready(ToSwarm::GenerateEvent(event)) => match event {
                    IdentifyEvent::Received { peer_id, info, .. } => {
                        self.handle_identify_report(&peer_id, &info);
                        let event = Event::Identified { peer_id, info };
                        return Poll::Ready(ToSwarm::GenerateEvent(event));
                    }
                    IdentifyEvent::Error { peer_id, error } => {
                        debug!(target: TARGET_NODE, "Identification with peer {:?} failed => {}", peer_id, error)
                    }
                    IdentifyEvent::Pushed { .. } => {}
                    IdentifyEvent::Sent { .. } => {}
                },
                Poll::Ready(ToSwarm::Dial { opts }) => return Poll::Ready(ToSwarm::Dial { opts }),
                Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                }) => {
                    return Poll::Ready(ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event: Either::Right(event),
                    })
                }
                Poll::Ready(ToSwarm::ExternalAddrConfirmed(addr)) => {
                    return Poll::Ready(ToSwarm::ExternalAddrConfirmed(addr))
                }
                Poll::Ready(ToSwarm::CloseConnection {
                    peer_id,
                    connection,
                }) => {
                    return Poll::Ready(ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                Poll::Ready(ToSwarm::ListenOn { opts }) => {
                    return Poll::Ready(ToSwarm::ListenOn { opts })
                }
                Poll::Ready(ToSwarm::ExternalAddrExpired(addr)) => {
                    return Poll::Ready(ToSwarm::ExternalAddrExpired(addr))
                }
                Poll::Ready(ToSwarm::NewExternalAddrCandidate(addr)) => {
                    return Poll::Ready(ToSwarm::NewExternalAddrCandidate(addr))
                }
                Poll::Ready(ToSwarm::RemoveListener { id }) => {
                    return Poll::Ready(ToSwarm::RemoveListener { id })
                }
                Poll::Ready(_) => {}
            }
        }

        // Garbage collect the nodes info.
        while let Poll::Ready(Some(())) = self.garbage_collect.poll_next_unpin(cx) {
            self.nodes_info.retain(|_, node| {
                node.info_expire
                    .as_ref()
                    .map(|exp| *exp >= Instant::now())
                    .unwrap_or(true)
            });
        }

        Poll::Pending
    }
}

/// Creates a stream that returns a new value every `duration`.
fn interval(duration: Duration) -> impl Stream<Item = ()> + Unpin {
    unfold((), move |_| Delay::new(duration).map(|_| Some(((), ())))).map(drop)
}
