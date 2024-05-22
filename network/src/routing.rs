// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    utils::{convert_boot_nodes, is_reachable, LruHashSet},
    Error,
};

use futures_timer::Delay;
use ip_network::IpNetwork;
use libp2p::{
    core::Endpoint,
    futures::FutureExt,
    kad::{
        store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, GetClosestPeersError, GetRecordOk, QueryId, QueryResult, Quorum,
        Record, RecordKey, K_VALUE,
    },
    mdns::{self, tokio::Behaviour as MdnsTokio, Config as MdnsConfig},
    multiaddr::Protocol,
    swarm::{
        behaviour::{
            toggle::{Toggle, ToggleConnectionHandler},
            ExternalAddrConfirmed,
        },
        ConnectionDenied, ConnectionId, DialError, DialFailure, FromSwarm, NetworkBehaviour,
        THandler, ToSwarm,
    },
    Multiaddr, PeerId, StreamProtocol,
};
use serde::Deserialize;
use tracing::{debug, error, info, trace, warn};

use std::{
    cmp,
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    num::NonZeroUsize,
    task::Poll,
    time::Duration,
};

const TARGET_ROUTING: &str = "KoreNetwork-Routing";

/// The discovery behaviour.
pub struct Behaviour {
    /// The local node peer id.
    local_peer_id: PeerId,

    /// Bootnodes to connect to.
    boot_nodes: Vec<(PeerId, Multiaddr)>,

    /// Nodes discovered with public address.
    public_nodes: HashMap<PeerId, Vec<Multiaddr>>,

    /// Circuit relay nodes.
    relay_nodes: HashMap<PeerId, Multiaddr>,

    /// Boolean that activates the random walk if the node has already finished the initial pre-routing phase.
    pre_routing: bool,

    /// Kademlia behavior.
    kademlia: Toggle<Kademlia<MemoryStore>>,

    /// MDNS behavior.
    mdns: Toggle<MdnsTokio>,

    /// The next random walk in the Kademlia DHT. `None` if random walks are disabled.
    next_random_walk: Option<Delay>,

    /// Duration between random walks.
    duration_to_next_kad: Duration,

    /// Number of nodes we're currently connected to.
    num_connections: u64,

    /// Number of active connections over which we interrupt the discovery process.
    discovery_only_if_under_num: u64,

    /// Whether to allow non-global addresses in the DHT.
    allow_non_globals_in_dht: bool,

    /// If false, `addresses_of_peer` won't return any private IPv4/IPv6 address, except for the
    /// ones stored in `permanent_addresses` or `ephemeral_addresses`.
    allow_private_ip: bool,

    /// Known external addresses of our node.
    known_external_addresses: LruHashSet<Multiaddr>,

    /// Records that we want to publish to the DHT.
    records_to_publish: HashMap<QueryId, Record>,

    /// Pending queries.
    active_queries: HashSet<PeerId>,

    /// Events to return in priority when polled.
    pending_events: VecDeque<Event>,
}

impl Behaviour {
    /// Creates a new routing `Behaviour`.
    pub fn new(peer_id: PeerId, config: Config) -> Self {
        let Config {
            boot_nodes,
            dht_random_walk,
            discovery_only_if_under_num,
            allow_non_globals_in_dht,
            allow_private_ip,
            enable_mdns,
            kademlia_disjoint_query_paths,
            kademlia_replication_factor,
            protocol_names,
            pre_routing
        } = config;

        // Convert boot nodes to `PeerId` and `Multiaddr`.
        let boot_nodes = convert_boot_nodes(boot_nodes);

        let kademlia = if !protocol_names.is_empty() {
            let mut kad_config = KademliaConfig::default();

            // Set the supported protocols.
            //kad_config.set_protocol_names(crate::REQUIRED_PROTOCOLS.to_vec());
            kad_config.set_protocol_names(
                protocol_names
                    .into_iter()
                    .map(|p| StreamProtocol::new(p.leak()))
                    .collect(),
            );

            kad_config.disjoint_query_paths(kademlia_disjoint_query_paths);

            if let Some(replication_factor) = kademlia_replication_factor {
                kad_config.set_replication_factor(replication_factor);
            }
            // By default Kademlia attempts to insert all peers into its routing table once a
            // dialing attempt succeeds. In order to control which peer is added, disable the
            // auto-insertion and instead add peers manually.
            kad_config.set_kbucket_inserts(libp2p::kad::BucketInserts::Manual);

            let store = MemoryStore::new(peer_id);
            let mut kad = Kademlia::with_config(peer_id, store, kad_config);

            // Add boot nodes to the Kademlia routing table.
            for (peer_id, addr) in &boot_nodes {
                kad.add_address(peer_id, addr.clone());
            }

            Some(kad)
        } else {
            None
        };

        Self {
            local_peer_id: peer_id,
            boot_nodes,
            public_nodes: HashMap::new(),
            relay_nodes: HashMap::new(),
            kademlia: Toggle::from(kademlia),
            mdns: if enable_mdns {
                match MdnsTokio::new(MdnsConfig::default(), peer_id) {
                    Ok(mdns) => Toggle::from(Some(mdns)),
                    Err(err) => {
                        warn!(target: TARGET_ROUTING, "Failed to initialize mDNS: {:?}", err);
                        Toggle::from(None)
                    }
                }
            } else {
                Toggle::from(None)
            },
            next_random_walk: if dht_random_walk {
                Some(Delay::new(Duration::new(0, 0)))
            } else {
                None
            },
            duration_to_next_kad: Duration::from_secs(1),
            num_connections: 0,
            discovery_only_if_under_num,
            allow_non_globals_in_dht,
            allow_private_ip,
            known_external_addresses: LruHashSet::new(K_VALUE),
            records_to_publish: Default::default(),
            active_queries: Default::default(),
            pending_events: VecDeque::new(),
            pre_routing
        }
    }

    /// Bootstrap node list.
    pub fn boot_nodes(&self) -> Vec<(PeerId, Multiaddr)> {
        self.boot_nodes.clone()
    }

    pub fn finish_prerouting_state(&mut self) {
        self.pre_routing = false;
    }

    /// Returns true if the given peer is known.
    pub fn is_known_peer(&mut self, peer_id: &PeerId) -> bool {
        self.public_nodes.contains_key(peer_id)
            || self.known_peers().contains(peer_id)
            || self.relay_nodes.contains_key(peer_id)
    }

    /// Sets the DHT random walk delay.
    #[cfg(test)]
    pub fn set_random_walk(&mut self, delay: Duration) {
        self.next_random_walk = Some(Delay::new(delay));
    }

    /// Get relay node.
    pub fn get_relay_node(&self, peer_id: &PeerId) -> Option<&Multiaddr> {
        self.relay_nodes.get(peer_id)
    }

    /// Returns the list of known external addresses of a peer.
    /// If the peer is not known, returns an empty list.
    ///
    #[cfg(test)]
    pub fn addresses_of_peer(&self, peer_id: &PeerId) -> Vec<Multiaddr> {
        let mut addrs = Vec::new();
        if let Some(list) = self.public_nodes.get(peer_id) {
            addrs.extend(list.iter().cloned());
        }
        addrs
    }

    /// Bootstrap node.
    pub fn bootstrap(&mut self) -> Result<(), Error> {
        if let Some(kad) = self.kademlia.as_mut() {
            let _ = kad
                .bootstrap()
                .map_err(|_| Error::Network("No known bootstrap nodes.".to_owned()))?;
            Ok(())
        } else {
            Err(Error::Network("Kademlia is not supported.".to_owned()))
        }
    }

    /// Returns the list of known external addresses of our node.
    pub fn _protocol_names(&self) -> Vec<StreamProtocol> {
        if let Some(k) = self.kademlia.as_ref() {
            k.protocol_names().to_vec()
        } else {
            Vec::new()
        }
    }

    /// Returns the list of nodes that we know exist in the network.
    pub fn known_peers(&mut self) -> HashSet<PeerId> {
        let mut peers = HashSet::new();
        if let Some(k) = self.kademlia.as_mut() {
            for b in k.kbuckets() {
                for e in b.iter() {
                    if !peers.contains(e.node.key.preimage()) {
                        peers.insert(*e.node.key.preimage());
                    }
                }
            }
        }
        peers
    }

    /// Get known peer address.
    #[allow(dead_code)]
    pub fn known_peer_addresses(&mut self, peer_id: &PeerId) -> Option<Vec<Multiaddr>> {
        if let Some(k) = self.kademlia.as_mut() {
            for b in k.kbuckets() {
                for e in b.iter() {
                    if e.node.key.preimage() == peer_id {
                        return Some(e.node.value.iter().cloned().collect());
                    }
                }
            }
        }
        None
    }

    /// Adds a hard-coded address for the given peer, that never expires.
    ///
    /// If we didn't know this address before, also generates a `Discovered` event.
    pub fn add_known_address(&mut self, peer_id: PeerId, addr: Multiaddr) {
        let addrs_list = self.public_nodes.entry(peer_id).or_default();
        if addrs_list.contains(&addr) {
            return;
        }

        if let Some(k) = self.kademlia.as_mut() {
            k.add_address(&peer_id, addr.clone());
        }

        self.pending_events.push_back(Event::Discovered(peer_id));
        addrs_list.push(addr);
    }

    /// Adds a relay node.
    /// If we didn't know this address before, also generates a `Discovered` event.
    pub fn add_relay_node(&mut self, peer_id: PeerId, addr: Multiaddr) {
        if self.relay_nodes.insert(peer_id, addr.clone()).is_none() {
            self.pending_events.push_back(Event::Discovered(peer_id));
        }
    }

    /// Add a self-reported address of a remote peer to the k-buckets of the DHT
    /// if it has compatible `supported_protocols`.
    ///
    /// **Note**: It is important that you call this method. The discovery mechanism will not
    /// automatically add connecting peers to the Kademlia k-buckets.
    pub fn add_self_reported_address(
        &mut self,
        peer_id: &PeerId,
        supported_protocols: &[StreamProtocol],
        addr: Multiaddr,
    ) {
        if let Some(kademlia) = self.kademlia.as_mut() {
            if !self.allow_non_globals_in_dht && !is_reachable(&addr) {
                trace!(
                    target: TARGET_ROUTING,
                    "Ignoring self-reported non-global address {} from {}.", addr, peer_id
                );
                return;
            }
            //println!("Supported protocols: {:?}", supported_protocols);
            //println!("Kademlia protocols: {:?}", kademlia.protocol_names());
            //kademlia.add_address(peer_id, addr.clone());
            if let Some(matching_protocol) = supported_protocols
                .iter()
                .find(|p| kademlia.protocol_names().iter().any(|k| &k == p))
            {
                trace!(
                    target: TARGET_ROUTING,
                    "Adding self-reported address {} from {} to Kademlia DHT {}.",
                    addr, peer_id, matching_protocol,
                );
                //println!("Adding self-reported address {} from {} to Kademlia DHT {}.",
                //addr, peer_id, matching_protocol);
                kademlia.add_address(peer_id, addr.clone());
            } else {
                trace!(
                    target: TARGET_ROUTING,
                    "Ignoring self-reported address {} from {} as remote node is not part of the \
                     Kademlia DHT supported by the local node.", addr, peer_id,
                );
                //println!("Ignoring self-reported address {} from {} as remote node is not part of the \
                //Kademlia DHT supported by the local node.", addr, peer_id,);
            }
        }
    }

    /// Discover closet peers to the given `PeerId`.
    pub fn discover(&mut self, peer_id: &PeerId) {
        if !self.active_queries.contains(peer_id) {
            if let Some(k) = self.kademlia.as_mut() {
                k.get_closest_peers(*peer_id);
            }
        }
    }

    /// Remove node from the DHT.
    pub fn remove_node(&mut self, peer_id: &PeerId, address: &Multiaddr) {
        if let Some(k) = self.kademlia.as_mut() {
            k.remove_address(peer_id, address);
        }
    }
    /*
        /// Start fetching a record from the DHT.
        ///
        /// A corresponding `ValueFound` or `ValueNotFound` event will later be generated.
        pub fn get_value(&mut self, key: RecordKey) {
            if let Some(k) = self.kademlia.as_mut() {
                k.get_record(key.clone());
            }
        }

        /// Start putting a record into the DHT. Other nodes can later fetch that value with
        /// `get_value`.
        ///
        /// A corresponding `ValuePut` or `ValuePutFailed` event will later be generated.
        pub fn put_value(&mut self, key: RecordKey, value: Vec<u8>) {
            if let Some(k) = self.kademlia.as_mut() {
                if let Err(e) = k.put_record(Record::new(key.clone(), value.clone()), Quorum::All) {
                    warn!(target: TARGET_ROUTING, "Kademlia => Failed to put record: {:?}", e);
                    self.pending_events
                        .push_back(Event::ValuePutFailed(key.clone(), Duration::from_secs(0)));
                }
            }
        }

        /// Returns the number of nodes in each Kademlia kbucket for each Kademlia instance.
        ///
        /// Identifies Kademlia instances by their [`ProtocolId`] and kbuckets by the base 2 logarithm
        /// of their lower bound.
        pub fn num_entries_per_kbucket(&mut self) -> Option<Vec<(u32, usize)>> {
            self.kademlia.as_mut().map(|kad| {
                kad.kbuckets()
                    .map(|bucket| (bucket.range().0.ilog2().unwrap_or(0), bucket.iter().count()))
                    .collect()
            })
        }

        /// Returns the number of records in the Kademlia record stores.
        pub fn num_kademlia_records(&mut self) -> Option<usize> {
            // Note that this code is ok only because we use a `MemoryStore`.
            self.kademlia
                .as_mut()
                .map(|kad| kad.store_mut().records().count())
        }

        /// Returns the total size in bytes of all the records in the Kademlia record stores.
        pub fn kademlia_records_total_size(&mut self) -> Option<usize> {
            // Note that this code is ok only because we use a `MemoryStore`. If the records were
            // for example stored on disk, this would load every single one of them every single time.
            self.kademlia.as_mut().map(|kad| {
                kad.store_mut()
                    .records()
                    .fold(0, |tot, rec| tot + rec.value.len())
            })
        }
    */
    /// Can the given `Multiaddr` be put into the DHT?
    ///
    /// This test is successful only for global IP addresses and DNS names.
    // NB: Currently all DNS names are allowed and no check for TLD suffixes is done
    // because the set of valid domains is highly dynamic and would require frequent
    // updates, for example by utilising publicsuffix.org or IANA.
    fn _can_add_to_dht(addr: &Multiaddr) -> bool {
        let ip = match addr.iter().next() {
            Some(Protocol::Ip4(ip)) => IpNetwork::from(ip),
            Some(Protocol::Ip6(ip)) => IpNetwork::from(ip),
            Some(Protocol::Dns(_)) | Some(Protocol::Dns4(_)) | Some(Protocol::Dns6(_)) => {
                return true
            }
            _ => return false,
        };
        ip.is_global()
    }
}

/// Event generated by the `DiscoveryBehaviour`.
#[derive(Debug)]
pub enum Event {
    /// A connection to a peer has been established but the peer has not been
    /// added to the routing table. It mustbe explicitly added via
    /// [`Behaviour::add_self_reported_address`].
    Discovered(PeerId),

    /// A peer connected to this node for whom no listen address is known.
    ///
    /// In order for the peer to be added to the Kademlia routing table, a known
    /// listen address must be added via
    /// [`DiscoveryBehaviour::add_self_reported_address`], e.g. obtained through
    /// the `identify` protocol.
    UnroutablePeer(PeerId),

    /// The DHT yielded results for the record request.
    ///
    /// Returning the result grouped in (key, value) pairs as well as the request duration.
    ValueFound(Vec<(RecordKey, Vec<u8>)>, Duration),

    /// The record requested was not found in the DHT.
    ///
    /// Returning the corresponding key as well as the request duration.
    ValueNotFound(RecordKey, Duration),

    /// The record with a given key was successfully inserted into the DHT.
    ///
    /// Returning the corresponding key as well as the request duration.
    ValuePut(RecordKey, Duration),

    /// Inserting a value into the DHT failed.
    ///
    /// Returning the corresponding key as well as the request duration.
    ValuePutFailed(RecordKey, Duration),

    /// Started a random Kademlia query.
    ///
    /// Only happens if [`Config::with_dht_random_walk`] has been configured to `true`.
    RandomKademliaStarted,

    /// Closest peers to a given key have been found
    ClosestPeers(PeerId, Vec<PeerId>),
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        ToggleConnectionHandler<<Kademlia<MemoryStore> as NetworkBehaviour>::ConnectionHandler>;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.kademlia.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.kademlia.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::AddressChange(e) => {
                self.kademlia.on_swarm_event(FromSwarm::AddressChange(e));
            }
            FromSwarm::ConnectionEstablished(e) => {
                self.num_connections += 1;
                self.kademlia
                    .on_swarm_event(FromSwarm::ConnectionEstablished(e));
            }
            FromSwarm::ConnectionClosed(e) => {
                self.num_connections -= 1;
                self.kademlia.on_swarm_event(FromSwarm::ConnectionClosed(e));
            }
            FromSwarm::DialFailure(e @ DialFailure { peer_id, error, .. }) => {
                if let Some(peer_id) = peer_id {
                    if let DialError::Transport(errors) = error {
                        if let Entry::Occupied(mut entry) = self.public_nodes.entry(peer_id) {
                            for (addr, _error) in errors {
                                entry.get_mut().retain(|a| a != addr);
                            }
                            if entry.get().is_empty() {
                                entry.remove();
                            }
                        }
                    }
                }

                self.kademlia.on_swarm_event(FromSwarm::DialFailure(e));
            }
            FromSwarm::ExpiredListenAddr(e) => {
                self.kademlia
                    .on_swarm_event(FromSwarm::ExpiredListenAddr(e));
            }
            FromSwarm::ExternalAddrConfirmed(e @ ExternalAddrConfirmed { addr }) => {
                let new_addr = addr.clone().with(Protocol::P2p(self.local_peer_id));

                if is_reachable(addr) {
                    // NOTE: we might re-discover the same address multiple times
                    // in which case we just want to refrain from logging.
                    if self.known_external_addresses.insert(new_addr.clone()) {
                        info!(
                          target: TARGET_ROUTING,
                          "🔍 Discovered new external address for our node: {}",
                          new_addr,
                        );
                    }
                }

                self.kademlia
                    .on_swarm_event(FromSwarm::ExternalAddrConfirmed(e));
            }
            FromSwarm::ExternalAddrExpired(e) => {
                // We intentionally don't remove the element from `known_external_addresses` in
                // order to not print the log line again.

                self.kademlia
                    .on_swarm_event(FromSwarm::ExternalAddrExpired(e));
            }

            FromSwarm::ListenerClosed(e) => {
                self.kademlia.on_swarm_event(FromSwarm::ListenerClosed(e));
            }
            FromSwarm::ListenFailure(e) => {
                self.kademlia.on_swarm_event(FromSwarm::ListenFailure(e));
            }
            FromSwarm::ListenerError(e) => {
                self.kademlia.on_swarm_event(FromSwarm::ListenerError(e));
            }
            FromSwarm::NewListener(e) => {
                self.kademlia.on_swarm_event(FromSwarm::NewListener(e));
            }
            FromSwarm::NewListenAddr(e) => {
                self.kademlia.on_swarm_event(FromSwarm::NewListenAddr(e));
                self.mdns.on_swarm_event(FromSwarm::NewListenAddr(e));
            }
            FromSwarm::NewExternalAddrCandidate(e) => {
                self.kademlia
                    .on_swarm_event(FromSwarm::NewExternalAddrCandidate(e));
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
        self.kademlia
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {   
        if let Some(ev) = self.pending_events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(ev));
        }

        // Poll the stream that fires when we need to start a random Kademlia query.
        if let Some(kademlia) = self.kademlia.as_mut() {
            if !self.pre_routing {
                if let Some(next_random_walk) = self.next_random_walk.as_mut() {
                    while next_random_walk.poll_unpin(cx).is_ready() {
                        let actually_started =
                            if self.num_connections < self.discovery_only_if_under_num {
                                let random_peer_id = PeerId::random();
                                debug!(
                                    target: TARGET_ROUTING,
                                    "Kademlia => Starting random Kademlia request for {:?}",
                                    random_peer_id,
                                );
                                kademlia.get_closest_peers(random_peer_id);
                                true
                            } else {
                                debug!(
                                    target: TARGET_ROUTING,
                                    "Kademlia paused due to high number of connections ({})",
                                    self.num_connections
                                );
                                false
                            };
    
                        // Schedule the next random query with exponentially increasing delay,
                        // capped at 60 seconds.
                        *next_random_walk = Delay::new(self.duration_to_next_kad);
                        self.duration_to_next_kad =
                            cmp::min(self.duration_to_next_kad * 2, Duration::from_secs(60));
    
                        if actually_started {
                            let ev = Event::RandomKademliaStarted;
                            return Poll::Ready(ToSwarm::GenerateEvent(ev));
                        }
                    }
                }
            }
        }

        while let Poll::Ready(ev) = self.kademlia.poll(cx) {
            match ev {
                ToSwarm::GenerateEvent(ev) => match ev {
                    KademliaEvent::RoutingUpdated { .. } => {
                        //let ev = Event::Discovered(peer);
                        //return Poll::Ready(ToSwarm::GenerateEvent(ev));
                    }
                    KademliaEvent::UnroutablePeer { peer, .. } => {
                        let ev = Event::UnroutablePeer(peer);
                        return Poll::Ready(ToSwarm::GenerateEvent(ev));
                    }
                    KademliaEvent::RoutablePeer { peer, .. } => {
                        let ev = Event::Discovered(peer);
                        return Poll::Ready(ToSwarm::GenerateEvent(ev));
                    }
                    KademliaEvent::PendingRoutablePeer { .. }
                    | KademliaEvent::InboundRequest { .. } => {
                        // We are not interested in this event at the moment.
                    }
                    KademliaEvent::ModeChanged { .. } => {
                        // We are not interested in this event at the moment.
                    }
                    KademliaEvent::OutboundQueryProgressed {
                        result: QueryResult::GetClosestPeers(res),
                        ..
                    } => match res {
                        Err(GetClosestPeersError::Timeout { key, peers }) => {
                            debug!(
                                target: TARGET_ROUTING,
                                "Kademlia => Query for {:?} timed out with {} results",
                                hex::encode(key), peers.len(),
                            );
                        }
                        Ok(ok) => {
                            trace!(
                                target: TARGET_ROUTING,
                                "Kademlia => Query for {:?} yielded {:?} results",
                                hex::encode(&ok.key), ok.peers.len(),
                            );
                            if ok.peers.is_empty() && self.num_connections != 0 {
                                debug!(
                                    target: TARGET_ROUTING,
                                    "Kademlia => Random Kademlia query has yielded empty results",
                                );
                            }
                            if let Ok(peer) = PeerId::from_bytes(&ok.key) {
                                self.active_queries.remove(&peer);
                                return Poll::Ready(ToSwarm::GenerateEvent(Event::ClosestPeers(
                                    peer, ok.peers,
                                )));
                            } else {
                                error!(
                                    target: TARGET_ROUTING,
                                    "Kademlia => Failed to parse peer id from key: {:?}",
                                    hex::encode(&ok.key),
                                );
                            }
                        }
                    },
                    KademliaEvent::OutboundQueryProgressed {
                        result: QueryResult::GetRecord(res),
                        stats,
                        id,
                        ..
                    } => {
                        let ev = match res {
                            Ok(GetRecordOk::FoundRecord(r)) => {
                                debug!(
                                    target: TARGET_ROUTING,
                                    "Kademlia => Found record ({:?}) with value: {:?}",
                                    r.record.key,
                                    r.record.value,
                                );

                                // Let's directly finish the query, as we are only interested in a
                                // quorum of 1.
                                if let Some(kad) = self.kademlia.as_mut() {
                                    if let Some(mut query) = kad.query_mut(&id) {
                                        query.finish();
                                    }
                                }

                                // Will be removed below when we receive
                                // `FinishedWithNoAdditionalRecord`.
                                self.records_to_publish.insert(id, r.record.clone());

                                Event::ValueFound(
                                    vec![(r.record.key, r.record.value)],
                                    stats.duration().unwrap_or_default(),
                                )
                            }
                            Ok(GetRecordOk::FinishedWithNoAdditionalRecord {
                                cache_candidates,
                            }) => {
                                // We always need to remove the record to not leak any data!
                                if let Some(record) = self.records_to_publish.remove(&id) {
                                    if cache_candidates.is_empty() {
                                        continue;
                                    }

                                    // Put the record to the `cache_candidates` that are nearest to
                                    // the record key from our point of view of the network.
                                    if let Some(kad) = self.kademlia.as_mut() {
                                        kad.put_record_to(
                                            record,
                                            cache_candidates.into_iter().map(|v| v.1),
                                            Quorum::One,
                                        );
                                    }
                                }

                                continue;
                            }
                            Err(e @ libp2p::kad::GetRecordError::NotFound { .. }) => {
                                trace!(
                                    target: TARGET_ROUTING,
                                    "Kademlia => Failed to get record: {:?}",
                                    e,
                                );
                                Event::ValueNotFound(
                                    e.into_key(),
                                    stats.duration().unwrap_or_default(),
                                )
                            }
                            Err(e) => {
                                debug!(
                                    target: TARGET_ROUTING,
                                    "Kademlia => Failed to get record: {:?}",
                                    e,
                                );
                                Event::ValueNotFound(
                                    e.into_key(),
                                    stats.duration().unwrap_or_default(),
                                )
                            }
                        };
                        return Poll::Ready(ToSwarm::GenerateEvent(ev));
                    }
                    KademliaEvent::OutboundQueryProgressed {
                        result: QueryResult::PutRecord(res),
                        stats,
                        ..
                    } => {
                        let ev = match res {
                            Ok(ok) => Event::ValuePut(ok.key, stats.duration().unwrap_or_default()),
                            Err(e) => {
                                debug!(
                                    target: "sub-libp2p",
                                    "Libp2p => Failed to put record: {:?}",
                                    e,
                                );
                                Event::ValuePutFailed(
                                    e.into_key(),
                                    stats.duration().unwrap_or_default(),
                                )
                            }
                        };
                        return Poll::Ready(ToSwarm::GenerateEvent(ev));
                    }
                    KademliaEvent::OutboundQueryProgressed {
                        result: QueryResult::RepublishRecord(res),
                        ..
                    } => match res {
                        Ok(ok) => debug!(
                            target: "sub-libp2p",
                            "Libp2p => Record republished: {:?}",
                            ok.key,
                        ),
                        Err(e) => debug!(
                            target: "sub-libp2p",
                            "Libp2p => Republishing of record {:?} failed with: {:?}",
                            e.key(), e,
                        ),
                    },
                    // We never start any other type of query.
                    KademliaEvent::OutboundQueryProgressed { result: e, .. } => {
                        warn!(target: "sub-libp2p", "Libp2p => Unhandled Kademlia event: {:?}", e)
                    }
                },
                ToSwarm::Dial { opts } => return Poll::Ready(ToSwarm::Dial { opts }),
                ToSwarm::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                } => {
                    return Poll::Ready(ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    })
                }
                ToSwarm::CloseConnection {
                    peer_id,
                    connection,
                } => {
                    return Poll::Ready(ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                ToSwarm::ExternalAddrConfirmed(e) => {
                    return Poll::Ready(ToSwarm::ExternalAddrConfirmed(e))
                }
                ToSwarm::ExternalAddrExpired(e) => {
                    return Poll::Ready(ToSwarm::ExternalAddrExpired(e))
                }
                ToSwarm::ListenOn { opts } => return Poll::Ready(ToSwarm::ListenOn { opts }),
                ToSwarm::NewExternalAddrCandidate(e) => {
                    return Poll::Ready(ToSwarm::NewExternalAddrCandidate(e))
                }
                ToSwarm::RemoveListener { id } => {
                    return Poll::Ready(ToSwarm::RemoveListener { id })
                }
                _ => {}
            }
        }

        // Poll mDNS.
        while let Poll::Ready(ev) = self.mdns.poll(cx) {
            match ev {
                ToSwarm::GenerateEvent(event) => match event {
                    mdns::Event::Discovered(list) => {
                        if self.num_connections >= self.discovery_only_if_under_num {
                            continue;
                        }

                        for (peer_id, _) in list.iter() {
                            self.pending_events.push_back(Event::Discovered(*peer_id));
                        }

                        if let Some(ev) = self.pending_events.pop_front() {
                            return Poll::Ready(ToSwarm::GenerateEvent(ev));
                        }
                    }
                    mdns::Event::Expired(_) => {}
                },
                ToSwarm::Dial { .. } => {
                    unreachable!("mDNS never dials!");
                }
                ToSwarm::NotifyHandler { event, .. } => match event {}, /* `event` is an */
                // enum with no
                // variant
                ToSwarm::CloseConnection {
                    peer_id,
                    connection,
                } => {
                    return Poll::Ready(ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                ToSwarm::ExternalAddrConfirmed(e) => {
                    return Poll::Ready(ToSwarm::ExternalAddrConfirmed(e))
                }
                ToSwarm::ExternalAddrExpired(e) => {
                    return Poll::Ready(ToSwarm::ExternalAddrExpired(e))
                }
                ToSwarm::ListenOn { opts } => return Poll::Ready(ToSwarm::ListenOn { opts }),
                ToSwarm::NewExternalAddrCandidate(e) => {
                    return Poll::Ready(ToSwarm::NewExternalAddrCandidate(e))
                }
                ToSwarm::RemoveListener { id } => {
                    return Poll::Ready(ToSwarm::RemoveListener { id })
                }
                _ => {}
            }
        }

        Poll::Pending
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), libp2p::swarm::ConnectionDenied> {
        self.kademlia
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        let Some(peer_id) = maybe_peer else {
            return Ok(Vec::new());
        };

        let mut list = self
            .boot_nodes
            .iter()
            .filter_map(|(p, a)| (*p == peer_id).then_some(a.clone()))
            .collect::<Vec<_>>();

        if let Some(ephemeral_addresses) = self.public_nodes.get(&peer_id) {
            list.extend(ephemeral_addresses.clone());
        }

        if let Some(relay_address) = self.relay_nodes.get(&peer_id) {
            list.push(relay_address.clone());
        }

        {
            let mut list_to_filter = self.kademlia.handle_pending_outbound_connection(
                connection_id,
                maybe_peer,
                addresses,
                effective_role,
            )?;

            list_to_filter.extend(self.mdns.handle_pending_outbound_connection(
                connection_id,
                maybe_peer,
                addresses,
                effective_role,
            )?);

            if !self.allow_private_ip {
                list_to_filter.retain(|addr| match addr.iter().next() {
                    Some(Protocol::Ip4(addr)) if !IpNetwork::from(addr).is_global() => false,
                    Some(Protocol::Ip6(addr)) if !IpNetwork::from(addr).is_global() => false,
                    _ => true,
                });
            }

            list.extend(list_to_filter);
        }

        trace!(target: TARGET_ROUTING, "Addresses of {:?}: {:?}", peer_id, list);

        Ok(list)
    }
}

/// Configuration for the routing behaviour.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// Bootnodes to connect to.
    boot_nodes: Vec<RoutingNode>,

    /// Whether to enable random walks in the Kademlia DHT.
    dht_random_walk: bool,

    /// Boolean that activates the random walk if the node has already finished the initial pre-routing phase.
    pre_routing: bool,

    /// Number of active connections over which we interrupt the discovery process.
    discovery_only_if_under_num: u64,

    /// Whether to allow non-global addresses in the DHT.
    allow_non_globals_in_dht: bool,

    /// If false, `addresses_of_peer` won't return any private IPv4/IPv6 address.
    allow_private_ip: bool,

    /// Whether to enable mDNS.
    enable_mdns: bool,

    /// When enabled the number of disjoint paths used equals the configured parallelism.
    kademlia_disjoint_query_paths: bool,

    /// The replication factor determines to how many closest peers a record is replicated.
    kademlia_replication_factor: Option<NonZeroUsize>,

    /// Protocols to be supported by the local node.
    protocol_names: Vec<String>,
}

impl Config {
    /// Creates a new configuration for the discovery behaviour.
    pub fn new(boot_nodes: Vec<RoutingNode>) -> Self {
        let protocol_names = vec!["/kore/routing/1.0.0".to_owned()];
        Self {
            boot_nodes,
            dht_random_walk: true,
            discovery_only_if_under_num: std::u64::MAX,
            allow_non_globals_in_dht: false,
            allow_private_ip: false,
            enable_mdns: true,
            kademlia_disjoint_query_paths: true,
            kademlia_replication_factor: None,
            protocol_names,
            pre_routing: true
        }
    }

    /// Get DHT random walk.
    pub fn get_dht_random_walk(&self) -> bool {
        self.dht_random_walk
    }

    /// Enables or disables random walks in the Kademlia DHT.
    pub fn with_dht_random_walk(mut self, enable: bool) -> Self {
        self.dht_random_walk = enable;
        self
    }

    /// Sets the number of active connections over which we interrupt the discovery process.
    pub fn with_discovery_limit(mut self, num: u64) -> Self {
        self.discovery_only_if_under_num = num;
        self
    }

    /// Whether to allow non-global addresses in the DHT.
    pub fn with_allow_non_globals_in_dht(mut self, allow: bool) -> Self {
        self.allow_non_globals_in_dht = allow;
        self
    }

    /// If false, `addresses_of_peer` won't return any private IPv4/IPv6 address, except for the
    /// ones stored in `permanent_addresses` or `ephemeral_addresses`.
    pub fn with_allow_private_ip(mut self, allow: bool) -> Self {
        self.allow_private_ip = allow;
        self
    }

    /// Enables or disables mDNS.
    pub fn with_mdns(mut self, enable: bool) -> Self {
        self.enable_mdns = enable;
        self
    }

    /// When enabled the number of disjoint paths used equals the configured parallelism.
    pub fn with_kademlia_disjoint_query_paths(mut self, enable: bool) -> Self {
        self.kademlia_disjoint_query_paths = enable;
        self
    }

    /// Sets the replication factor for the Kademlia DHT.
    pub fn with_kademlia_replication_factor(mut self, factor: usize) -> Self {
        if factor == 0 {
            self.kademlia_replication_factor = None;
        } else {
            self.kademlia_replication_factor = Some(NonZeroUsize::new(factor).expect("Can't fail"));
        }
        self
    }

    /// Sets protocol to be supported by the local node.
    pub fn set_protocol(mut self, protocol: &str) -> Self {
        self.protocol_names.push(protocol.to_owned());
        self
    }

    /// Returns the boot nodes.
    pub fn boot_nodes(&self) -> Vec<RoutingNode> {
        self.boot_nodes.clone()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(vec![])
    }
}

/// A node in the routing table.
#[derive(Clone, Debug, Deserialize)]
pub struct RoutingNode {
    /// Peer ID.
    pub peer_id: String,
    /// Address.
    pub address: String,
}

#[derive(Debug, Clone)]
/// DHT value.
pub enum DhtValue {
    /// The value was found.
    Found(Vec<(RecordKey, Vec<u8>)>),

    /// The requested record has not been found in the DHT.
    NotFound(RecordKey),

    /// The record has been successfully inserted into the DHT.
    Put(RecordKey),

    /// An error has occurred while putting a record into the DHT.
    PutFailed(RecordKey),
}

#[cfg(test)]
mod tests {

    use super::*;

    use libp2p::{
        identity::Keypair,
        swarm::{Swarm, SwarmEvent},
        Multiaddr,
    };
    use libp2p_swarm_test::SwarmExt;

    use futures::prelude::*;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn test_routing() {
        let mut boot_nodes = vec![];

        let config = Config::new(boot_nodes.clone())
            .with_allow_non_globals_in_dht(true)
            .with_allow_private_ip(true)
            .with_discovery_limit(50)
            .with_dht_random_walk(true)
            .set_protocol("/kad/tell/1.0.0");

        let (boot_swarm, addr) = build_node(config);

        let peer_id = *boot_swarm.local_peer_id();
        let boot_node = RoutingNode {
            peer_id: peer_id.to_base58(),
            address: addr.to_string(),
        };
        boot_nodes.push(boot_node);

        let mut swarms = (1..25)
            .map(|_| {
                let config = Config::new(boot_nodes.clone())
                    .with_allow_non_globals_in_dht(true)
                    .with_allow_private_ip(true)
                    .with_discovery_limit(50);
                build_node(config)
            })
            .collect::<Vec<_>>();

        swarms.insert(0, (boot_swarm, addr));

        // Build a `Vec<HashSet<PeerId>>` with the list of nodes remaining to be discovered.
        let mut to_discover = (0..swarms.len())
            .map(|n| {
                (0..swarms.len())
                    // Skip the first swarm as all other swarms already know it.
                    .skip(1)
                    .filter(|p| *p != n)
                    .map(|p| *Swarm::local_peer_id(&swarms[p].0))
                    .collect::<HashSet<_>>()
            })
            .collect::<Vec<_>>();

        let fut = futures::future::poll_fn(move |cx| {
            'polling: loop {
                for swarm_n in 0..swarms.len() {
                    match swarms[swarm_n].0.poll_next_unpin(cx) {
                        Poll::Ready(Some(e)) => {
                            match e {
                                SwarmEvent::Behaviour(behavior) => {
                                    match behavior {
                                        Event::UnroutablePeer(other) | Event::Discovered(other) => {
                                            // Call `add_self_reported_address` to simulate identify
                                            // happening.
                                            let addr = swarms
                                                .iter()
                                                .find_map(|(s, a)| {
                                                    if s.behaviour().local_peer_id == other {
                                                        Some(a.clone())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap();
                                            swarms[swarm_n]
                                                .0
                                                .behaviour_mut()
                                                .add_self_reported_address(
                                                    &other,
                                                    &[StreamProtocol::new("/kore/routing/1.0.0")],
                                                    addr,
                                                );

                                            to_discover[swarm_n].remove(&other);
                                        }
                                        Event::RandomKademliaStarted => {}
                                        Event::ClosestPeers(_, _) => {}
                                        e => {
                                            panic!("Unexpected event: {:?}", e)
                                        }
                                    }
                                }
                                // ignore non Behaviour events
                                _ => {}
                            }
                            continue 'polling;
                        }
                        _ => {}
                    }
                }
                break;
            }
            if to_discover.iter().all(|l| l.is_empty()) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        });

        futures::executor::block_on(fut);
    }

    #[tokio::test]
    #[serial]
    async fn test_ignores_peers_with_unknown_protocols() {
        let supported_protocol = "/kore/routing/1.0.0";
        let unsupported_protocol = "/kore/routing/2.0.0";

        let mut routing = {
            let config = Config::new(vec![])
                .with_allow_non_globals_in_dht(true)
                .with_allow_private_ip(true)
                .with_discovery_limit(50);
            Behaviour::new(PeerId::random(), config)
        };

        let predictable_peer_id = |bytes: &[u8; 32]| {
            Keypair::ed25519_from_bytes(bytes.to_owned())
                .unwrap()
                .public()
                .to_peer_id()
        };

        let remote_peer_id = predictable_peer_id(b"00000000000000000000000000000001");
        let remote_addr: Multiaddr = "/memory/1".parse().unwrap();
        let another_peer_id = predictable_peer_id(b"00000000000000000000000000000002");
        let another_addr: Multiaddr = "/memory/2".parse().unwrap();

        // Add a self-reported address with an unsupported protocol.
        routing.add_self_reported_address(
            &another_peer_id,
            &[StreamProtocol::new(unsupported_protocol)],
            another_addr.clone(),
        );
        let kad = routing.kademlia.as_mut().unwrap();
        assert!(kad.kbuckets().next().is_none());

        // Add a self-reported address with a supported protocol.
        routing.add_self_reported_address(
            &remote_peer_id,
            &[StreamProtocol::new(supported_protocol)],
            remote_addr.clone(),
        );
        let kad = routing.kademlia.as_mut().unwrap();
        assert!(kad.kbuckets().next().is_some());
    }

    /// Build test swarm
    fn build_node(config: Config) -> (Swarm<Behaviour>, Multiaddr) {
        let mut swarm = Swarm::new_ephemeral(|key_pair| {
            Behaviour::new(PeerId::from_public_key(&key_pair.public()), config)
        });
        let listen_addr: Multiaddr = format!("/memory/{}", rand::random::<u64>())
            .parse()
            .unwrap();
        let _ = swarm.listen_on(listen_addr.clone()).unwrap();

        swarm.add_external_address(listen_addr.clone());
        let _ = swarm.behaviour_mut().bootstrap();

        (swarm, listen_addr)
    }
}
