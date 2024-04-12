// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tell Protocol
//!

mod cache;
pub mod codec;
mod handler;
mod protocol;

pub use codec::Codec;
use handler::TellHandler;
pub use protocol::ProtocolSupport;

#[cfg(feature = "binary")]
pub use codec::binary;
#[cfg(feature = "borsh")]
pub use codec::borsh;
#[cfg(feature = "json")]
pub use codec::json;

use cache::PeerAddresses;

use crate::handler::OutboundMessage;

use libp2p::{
    core::ConnectedPoint,
    swarm::{
        dial_opts::DialOpts, AddressChange, ConnectionClosed, ConnectionHandler, ConnectionId,
        DialFailure, FromSwarm, NetworkBehaviour, NotifyHandler, ToSwarm,
    },
    Multiaddr, PeerId,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt, io,
    sync::{atomic::AtomicU64, Arc},
    task::Poll,
    time::Duration,
};

/// Internal threshold for when to shrink the capacity
/// of empty queues. If the capacity of an empty queue
/// exceeds this threshold, the associated memory is
/// released.
const EMPTY_QUEUE_SHRINK_THRESHOLD: usize = 100;

/// A message to be sent to a remote peer.
#[derive(Debug)]
pub struct TellMessage<TMessage> {
    pub message: TMessage,
    pub inbound_id: InboundTellId,
}

/// The events emitted by a tell [`Behaviour`].
#[derive(Debug)]
pub enum Event<TMessage> {
    Message {
        peer_id: PeerId,
        message: TellMessage<TMessage>,
    },
    MessageProcessed {
        peer_id: PeerId,
        inbound_id: InboundTellId,
    },
    MessageSent {
        peer_id: PeerId,
        outbound_id: OutboundTellId,
    },
    InboundFailure {
        peer_id: PeerId,
        inbound_id: InboundTellId,
        error: InboundFailure,
    },
    OutboundFailure {
        peer_id: PeerId,
        outbound_id: OutboundTellId,
        error: OutboundFailure,
    },
}

/// The ID of an inbound tell.
///
/// Note: [`InboundTellId`]'s uniqueness is only guaranteed between
/// inbound tells of the same originating [`Behaviour`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InboundTellId(u64);

impl fmt::Display for InboundTellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The ID of an outbound tell.
///
/// Note: [`OutboundTellId`]'s uniqueness is only guaranteed between
/// outbound tells of the same originating [`Behaviour`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutboundTellId(u64);

impl fmt::Display for OutboundTellId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Possible failures occurring in the context of sending
/// an outbound request and receiving the response.
#[derive(Debug)]
pub enum OutboundFailure {
    /// The request could not be sent because a dialing attempt failed.
    DialFailure,
    /// The request timed out before a response was received.
    ///
    /// It is not known whether the request may have been
    /// received (and processed) by the remote peer.
    Timeout,
    /// The connection closed before a response was received.
    ///
    /// It is not known whether the request may have been
    /// received (and processed) by the remote peer.
    ConnectionClosed,
    /// The remote supports none of the requested protocols.
    UnsupportedProtocols,
    /// An IO failure happened on an outbound stream.
    Io(io::Error),
}

impl fmt::Display for OutboundFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutboundFailure::DialFailure => write!(f, "Failed to dial the requested peer"),
            OutboundFailure::Timeout => write!(f, "Timeout while waiting for a response"),
            OutboundFailure::ConnectionClosed => {
                write!(f, "Connection was closed before a response was received")
            }
            OutboundFailure::UnsupportedProtocols => {
                write!(f, "The remote supports none of the requested protocols")
            }
            OutboundFailure::Io(e) => write!(f, "IO error on outbound stream: {e}"),
        }
    }
}

impl std::error::Error for OutboundFailure {}

/// Possible failures occurring in the context of receiving an
/// inbound request and sending a response.
#[derive(Debug)]
pub enum InboundFailure {
    /// The inbound request timed out, either while reading the
    /// incoming request or before a response is sent, e.g. if
    /// [`Behaviour::send_response`] is not called in a
    /// timely manner.
    Timeout,
    /// The connection closed before a response could be send.
    ConnectionClosed,
    /// The local peer supports none of the protocols requested
    /// by the remote.
    UnsupportedProtocols,
    /// An IO failure happened on an inbound stream.
    Io(io::Error),
}

impl fmt::Display for InboundFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InboundFailure::Timeout => {
                write!(f, "Timeout while receiving request or sending response")
            }
            InboundFailure::ConnectionClosed => {
                write!(f, "Connection was closed before a response could be sent")
            }
            InboundFailure::UnsupportedProtocols => write!(
                f,
                "The local peer supports none of the protocols requested by the remote"
            ),
            InboundFailure::Io(e) => write!(f, "IO error on inbound stream: {e}"),
        }
    }
}

impl std::error::Error for InboundFailure {}

/// The configuration for a `Behaviour` protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    message_timeout: Duration,
    max_concurrent_streams: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            message_timeout: Duration::from_secs(10),
            max_concurrent_streams: 100,
        }
    }
}

impl Config {
    /// Sets the timeout for inbound and outbound requests.
    pub fn with_message_timeout(mut self, timeout: Duration) -> Self {
        self.message_timeout = timeout;
        self
    }

    /// Sets the upper bound for the number of concurrent inbound + outbound streams.
    pub fn with_max_concurrent_streams(mut self, num_streams: usize) -> Self {
        self.max_concurrent_streams = num_streams;
        self
    }
}

/// A tell protocol for some message codec.
pub struct Behaviour<TCodec>
where
    TCodec: Codec + Clone + Send + 'static,
{
    /// The supported inbound protocols.
    inbound_protocols: SmallVec<[TCodec::Protocol; 2]>,
    /// The supported outbound protocols.
    outbound_protocols: SmallVec<[TCodec::Protocol; 2]>,
    /// The next (local) request ID.
    next_outbound_tell_id: OutboundTellId,
    /// The next (inbound) request ID.
    next_inbound_request_id: Arc<AtomicU64>,
    /// The protocol configuration.
    config: Config,
    /// The protocol codec for reading and writing messages.
    codec: TCodec,
    /// Pending events to return from `poll`.
    pending_events: VecDeque<ToSwarm<Event<TCodec::Message>, OutboundMessage<TCodec>>>,
    /// The currently connected peers.
    connected: HashMap<PeerId, SmallVec<[Connection; 2]>>,
    /// Externally managed addresses via `add_address` and `remove_address`.
    addresses: PeerAddresses,
    /// Externally managed addresses via `add_address` and `remove_address`.
    /// addresses: PeerAddresses,
    /// Requests that have not yet been sent and are waiting for a connection
    /// to be established.
    pending_outbound_messages: HashMap<PeerId, SmallVec<[OutboundMessage<TCodec>; 10]>>,
}

impl<TCodec> Behaviour<TCodec>
where
    TCodec: Codec + Default + Clone + Send + 'static,
{
    /// Creates a new `Behaviour` for the given protocols and configuration, using [`Default`] to construct the codec.
    pub fn new<I>(protocols: I, cfg: Config) -> Self
    where
        I: IntoIterator<Item = (TCodec::Protocol, ProtocolSupport)>,
    {
        Self::with_codec(TCodec::default(), protocols, cfg)
    }
}

impl<TCodec> Behaviour<TCodec>
where
    TCodec: Codec + Clone + Send + 'static,
{
    /// Creates a new `Behaviour` for the given
    /// protocols, codec and configuration.
    pub fn with_codec<I>(codec: TCodec, protocols: I, cfg: Config) -> Self
    where
        I: IntoIterator<Item = (TCodec::Protocol, ProtocolSupport)>,
    {
        let mut inbound_protocols = SmallVec::new();
        let mut outbound_protocols = SmallVec::new();
        for (p, s) in protocols {
            if s.inbound() {
                inbound_protocols.push(p.clone());
            }
            if s.outbound() {
                outbound_protocols.push(p.clone());
            }
        }
        Behaviour {
            inbound_protocols,
            outbound_protocols,
            next_outbound_tell_id: OutboundTellId(1),
            next_inbound_request_id: Arc::new(AtomicU64::new(1)),
            config: cfg,
            codec,
            pending_events: VecDeque::new(),
            connected: HashMap::new(),
            addresses: PeerAddresses::default(),
            pending_outbound_messages: HashMap::new(),
        }
    }

    /// Send a message to the given peer.
    pub fn send_message(&mut self, peer: &PeerId, message: TCodec::Message) -> OutboundTellId {
        let tell_id = self.next_outbound_request_id();
        let message = OutboundMessage {
            tell_id,
            message,
            protocols: self.outbound_protocols.clone(),
        };

        if let Some(request) = self.try_send_message(peer, message) {
            self.pending_events.push_back(ToSwarm::Dial {
                opts: DialOpts::peer_id(*peer).build(),
            });
            self.pending_outbound_messages
                .entry(*peer)
                .or_default()
                .push(request);
        }

        tell_id
    }

    /// Adds a new address to the peer's known addresses.
    pub fn add_address(&mut self, peer: &PeerId, address: Multiaddr) -> bool {
        self.addresses.add(*peer, address)
    }

    /// Removes an address from the peer's known addresses.
    pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        self.addresses.remove(peer, address)
    }

    /// Returns the known addresses of a peer.
    pub fn addresses_of_peer(&mut self, peer: &PeerId) -> impl Iterator<Item = Multiaddr> + '_ {
        self.addresses.get(peer)
    }

    /// Returns the next outbound request ID.
    fn next_outbound_request_id(&mut self) -> OutboundTellId {
        let request_id = self.next_outbound_tell_id;
        self.next_outbound_tell_id.0 += 1;
        request_id
    }

    /// Tries to send a message by queueing an appropriate event to be
    /// emitted to the `Swarm`. If the peer is not currently connected,
    /// the given request is return unchanged.
    fn try_send_message(
        &mut self,
        peer: &PeerId,
        message: OutboundMessage<TCodec>,
    ) -> Option<OutboundMessage<TCodec>> {
        if let Some(connections) = self.connected.get_mut(peer) {
            if connections.is_empty() {
                return Some(message);
            }
            let ix = (message.tell_id.0 as usize) % connections.len();
            let conn = &mut connections[ix];
            conn.pending_outbound_messages.insert(message.tell_id);
            self.pending_events.push_back(ToSwarm::NotifyHandler {
                peer_id: *peer,
                handler: NotifyHandler::One(conn.id),
                event: message,
            });
            None
        } else {
            Some(message)
        }
    }

    /// Checks whether a peer is currently connected.
    pub fn is_connected(&self, peer: &PeerId) -> bool {
        if let Some(connections) = self.connected.get(peer) {
            !connections.is_empty()
        } else {
            false
        }
    }

    /// Check if request is still pending to be sent.
    pub fn is_pending_outbound(&self, peer: &PeerId, tell_id: &OutboundTellId) -> bool {
        // Check if request is already sent on established connection.
        let est_conn = self
            .connected
            .get(peer)
            .map(|cs| {
                cs.iter()
                    .any(|c| c.pending_outbound_messages.contains(tell_id))
            })
            .unwrap_or(false);
        // Check if request is still pending to be sent.
        let pen_conn = self
            .pending_outbound_messages
            .get(peer)
            .map(|rps| rps.iter().any(|rp| rp.tell_id == *tell_id))
            .unwrap_or(false);
        est_conn | pen_conn
    }

    /// Preloads a new [`Handler`] with messages that are waiting to be sent to the newly connected peer.
    fn preload_new_handler(
        &mut self,
        handler: &mut TellHandler<TCodec>,
        peer: PeerId,
        connection_id: ConnectionId,
        remote_address: Option<Multiaddr>,
    ) {
        let mut connection = Connection::new(connection_id, remote_address);

        if let Some(pending_messages) = self.pending_outbound_messages.remove(&peer) {
            for message in pending_messages {
                connection.pending_outbound_messages.insert(message.tell_id);
                handler.on_behaviour_event(message);
            }
        }

        self.connected.entry(peer).or_default().push(connection);
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            remaining_established,
            ..
        }: ConnectionClosed,
    ) {
        let connections = self
            .connected
            .get_mut(&peer_id)
            .expect("Expected some established connection to peer before closing.");

        let connection = connections
            .iter()
            .position(|c| c.id == connection_id)
            .map(|p: usize| connections.remove(p))
            .expect("Expected connection to be established before closing.");

        debug_assert_eq!(connections.is_empty(), remaining_established == 0);
        if connections.is_empty() {
            self.connected.remove(&peer_id);
        }

        for tell_id in connection.pending_outbound_messages {
            self.pending_events
                .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                    peer_id,
                    outbound_id: tell_id,
                    error: OutboundFailure::ConnectionClosed,
                }));
        }
    }

    fn on_dial_failure(&mut self, DialFailure { peer_id, .. }: DialFailure) {
        if let Some(peer) = peer_id {
            // If there are pending outgoing requests when a dial failure occurs,
            // it is implied that we are not connected to the peer, since pending
            // outgoing requests are drained when a connection is established and
            // only created when a peer is not connected when a request is made.
            // Thus these requests must be considered failed, even if there is
            // another, concurrent dialing attempt ongoing.
            if let Some(pending) = self.pending_outbound_messages.remove(&peer) {
                for message in pending {
                    self.pending_events
                        .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                            peer_id: peer,
                            outbound_id: message.tell_id,
                            error: OutboundFailure::DialFailure,
                        }));
                }
            }
        }
    }

    fn on_address_change(
        &mut self,
        AddressChange {
            peer_id,
            connection_id,
            new,
            ..
        }: AddressChange,
    ) {
        let new_address = match new {
            ConnectedPoint::Dialer { address, .. } => Some(address.clone()),
            ConnectedPoint::Listener { .. } => None,
        };
        let connections = self
            .connected
            .get_mut(&peer_id)
            .expect("Address change can only happen on an established connection.");

        let connection = connections
            .iter_mut()
            .find(|c| c.id == connection_id)
            .expect("Address change can only happen on an established connection.");
        connection.remote_address = new_address;
    }

    /// Returns a mutable reference to the connection in `self.connected`
    /// corresponding to the given [`PeerId`] and [`ConnectionId`].
    fn get_connection_mut(
        &mut self,
        peer: &PeerId,
        connection: ConnectionId,
    ) -> Option<&mut Connection> {
        self.connected
            .get_mut(peer)
            .and_then(|connections| connections.iter_mut().find(|c| c.id == connection))
    }
}

impl<TCodec> NetworkBehaviour for Behaviour<TCodec>
where
    TCodec: Codec + Send + Clone + 'static,
{
    type ConnectionHandler = TellHandler<TCodec>;
    type ToSwarm = Event<TCodec::Message>;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        let mut handler = TellHandler::new(
            self.inbound_protocols.clone(),
            self.codec.clone(),
            self.config.message_timeout,
            self.next_inbound_request_id.clone(),
            self.config.max_concurrent_streams,
        );
        self.preload_new_handler(&mut handler, peer, connection_id, None);

        Ok(handler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        let peer = match maybe_peer {
            None => return Ok(vec![]),
            Some(peer) => peer,
        };

        let mut addresses = Vec::new();
        if let Some(connections) = self.connected.get(&peer) {
            addresses.extend(connections.iter().filter_map(|c| c.remote_address.clone()))
        }
        Ok(addresses)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        let mut handler = TellHandler::new(
            self.inbound_protocols.clone(),
            self.codec.clone(),
            self.config.message_timeout,
            self.next_inbound_request_id.clone(),
            self.config.max_concurrent_streams,
        );
        self.preload_new_handler(&mut handler, peer, connection_id, Some(addr.clone()));

        Ok(handler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        let _ = self.addresses.on_swarm_event(&event);
        match event {
            FromSwarm::ConnectionEstablished(_) => {}
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::AddressChange(address_change) => self.on_address_change(address_change),
            FromSwarm::DialFailure(dial_failure) => self.on_dial_failure(dial_failure),
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            handler::TellEvent::TellReceived { tell_id, data } => {
                match self.get_connection_mut(&peer_id, connection_id) {
                    Some(_) => {
                        let message = TellMessage {
                            message: data,
                            inbound_id: tell_id,
                        };
                        self.pending_events
                            .push_back(ToSwarm::GenerateEvent(Event::Message { peer_id, message }));
                    }
                    None => {
                        // The connection was closed before the message was received.
                        // The message is dropped.
                        tracing::debug!("Connection ({connection_id}) closed after `Event::Request` ({tell_id}) has been emitted.");
                    }
                }
            }
            handler::TellEvent::TellSent(tell_id) => {
                self.pending_outbound_messages.remove(&peer_id);
                if let Some(connection) = self.get_connection_mut(&peer_id, connection_id) {
                    connection.pending_outbound_messages.remove(&tell_id);
                }
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::MessageSent {
                        peer_id,
                        outbound_id: tell_id,
                    }));
            }
            handler::TellEvent::TellProccessed(tell_id) => {
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::MessageProcessed {
                        peer_id,
                        inbound_id: tell_id,
                    }));
            }
            handler::TellEvent::InboundStreamFailed { tell_id, error } => {
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::InboundFailure {
                        peer_id,
                        inbound_id: tell_id,
                        error: InboundFailure::Io(error),
                    }));
            }
            handler::TellEvent::OutboundStreamFailed { tell_id, error } => {
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                        peer_id,
                        outbound_id: tell_id,
                        error: OutboundFailure::Io(error),
                    }));
            }
            handler::TellEvent::InboundTimeout(tell_id) => {
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::InboundFailure {
                        peer_id,
                        inbound_id: tell_id,
                        error: InboundFailure::Timeout,
                    }));
            }
            handler::TellEvent::OutboundTimeout(tell_id) => {
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                        peer_id,
                        outbound_id: tell_id,
                        error: OutboundFailure::Timeout,
                    }));
            }
            handler::TellEvent::OutboundUnsupportedProtocols(tell_id) => {
                self.pending_events
                    .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                        peer_id,
                        outbound_id: tell_id,
                        error: OutboundFailure::UnsupportedProtocols,
                    }));
            }
        }
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if let Some(ev) = self.pending_events.pop_front() {
            return Poll::Ready(ev);
        }
        if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        Poll::Pending
    }
}

/// Internal information tracked for an established connection.
struct Connection {
    id: ConnectionId,
    remote_address: Option<Multiaddr>,
    /// Pending outbound messages.
    pending_outbound_messages: HashSet<OutboundTellId>,
}

impl Connection {
    fn new(id: ConnectionId, remote_address: Option<Multiaddr>) -> Self {
        Self {
            id,
            remote_address,
            pending_outbound_messages: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    //use futures::prelude::*;
    use libp2p::{PeerId, StreamProtocol, Swarm};
    use libp2p_swarm_test::SwarmExt;
    use serde::{Deserialize, Serialize};
    use std::iter;
    use tracing_subscriber::EnvFilter;

    // Simple Ping Protocol
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Ping(Vec<u8>);

    #[async_std::test]
    async fn test_ping_protocol() {
        use crate::codec::binary::Behaviour;
        use rand::Rng;

        let ping = Ping("ping".to_string().into_bytes());

        let protocols = iter::once((
            StreamProtocol::new("/ping/1"),
            ProtocolSupport::InboundOutbound,
        ));
        let cfg = Config::default();
        let mut swarm1 = Swarm::new_ephemeral(|_| Behaviour::new(protocols.clone(), cfg.clone()));
        let peer1_id = *swarm1.local_peer_id();
        let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::new(protocols.clone(), cfg.clone()));
        let peer2_id = *swarm2.local_peer_id();

        swarm1.listen().with_memory_addr_external().await;
        swarm2.connect(&mut swarm1).await;

        let expected_ping = ping.clone();

        // Swarm 1 main loop
        let peer1 = async move {
            loop {
                match swarm1.next_swarm_event().await.try_into_behaviour_event() {
                    Ok(Event::Message { peer_id, message }) => {
                        assert_eq!(peer_id, peer2_id);
                        assert_eq!(message.message, expected_ping.0);
                    }
                    Ok(Event::MessageProcessed {
                        peer_id,
                        inbound_id,
                    }) => {
                        assert_eq!(peer_id, peer2_id);
                        tracing::info!("Inbound message processed: {}", inbound_id);
                    }
                    Ok(e) => {
                        panic!("Peer1: Unexpected event: {e:?}")
                    }
                    Err(..) => {}
                }
            }
        };

        let num_pings: u8 = rand::thread_rng().gen_range(1..100);

        let peer2 = async move {
            let mut count = 0;

            let mut req_id = swarm2
                .behaviour_mut()
                .send_message(&peer1_id, ping.clone().0);
            assert!(swarm2.behaviour().is_pending_outbound(&peer1_id, &req_id));
            loop {
                match swarm2
                    .next_swarm_event()
                    .await
                    .try_into_behaviour_event()
                    .unwrap()
                {
                    Event::MessageSent {
                        peer_id,
                        outbound_id,
                    } => {
                        count += 1;
                        assert_eq!(peer_id, peer1_id);
                        assert_eq!(outbound_id, req_id);
                        if count >= num_pings {
                            return;
                        } else {
                            req_id = swarm2
                                .behaviour_mut()
                                .send_message(&peer1_id, ping.clone().0);
                        }
                    }
                    e => panic!("Peer2: Unexpected event: {e:?}"),
                }
            }
        };
        async_std::task::spawn(Box::pin(peer1));
        peer2.await;
    }

    #[async_std::test]
    async fn test_pending_outbound() {
        use crate::codec::binary::Behaviour;
        let _ = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .try_init();
        let ping = Ping("ping".to_string().into_bytes());
        let offline_peer = PeerId::random();
        let mut swarm1 = Swarm::new_ephemeral(|_| {
            Behaviour::new(
                vec![(
                    StreamProtocol::new("/ping/1"),
                    ProtocolSupport::InboundOutbound,
                )],
                Config::default(),
            )
        });
        let request_id1 = swarm1
            .behaviour_mut()
            .send_message(&offline_peer, ping.clone().0);

        match swarm1
            .next_swarm_event()
            .await
            .try_into_behaviour_event()
            .unwrap()
        {
            Event::OutboundFailure {
                peer_id,
                outbound_id,
                error: _error,
            } => {
                assert_eq!(peer_id, offline_peer);
                assert_eq!(outbound_id, request_id1);
            }
            e => panic!("Peer: Unexpected event: {e:?}"),
        }
        let request_id2 = swarm1.behaviour_mut().send_message(&offline_peer, ping.0);
        assert_eq!(request_id2.0, request_id1.0 + 1);
        assert!(!swarm1
            .behaviour()
            .is_pending_outbound(&offline_peer, &request_id1));
        assert!(swarm1
            .behaviour()
            .is_pending_outbound(&offline_peer, &request_id2));
    }
}
