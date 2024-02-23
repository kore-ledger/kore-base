// Copyright 2023 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Handler
//!
//! This module contains the `Handler` struct for handling inbound and outbound
//! messages.

use crate::codec::Codec;
use crate::protocol::TellProtocol;
use crate::{InboundTellId, OutboundTellId, EMPTY_QUEUE_SHRINK_THRESHOLD};

use libp2p::swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ListenUpgradeError,
    },
    ConnectionHandler,
    ConnectionHandlerEvent, //ConnectionHandlerUpgrErr, KeepAlive,
    StreamUpgradeError,
    SubstreamProtocol,
};

use futures::channel::mpsc;
use futures::prelude::*;
use smallvec::SmallVec;

use std::{
    collections::VecDeque,
    fmt, io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::Poll,
    time::Duration,
};

pub struct TellHandler<TCodec>
where
    TCodec: Codec,
{
    /// The supported inbound protocols.
    inbound_protocols: SmallVec<[TCodec::Protocol; 2]>,
    /// The `tell` codec.
    codec: TCodec,
    /// Queue of events to emit in `poll()`.
    pending_events: VecDeque<TellEvent<TCodec>>,
    /// Outbound upgrades waiting to be emitted.
    pending_outbound: VecDeque<OutboundMessage<TCodec>>,

    requested_outbound: VecDeque<OutboundMessage<TCodec>>,

    /// A channel for receiving inbound messages.
    inbound_receiver: mpsc::Receiver<(InboundTellId, TCodec::Message)>,
    /// The [`mpsc::Sender`] for the above receiver.
    inbound_sender: mpsc::Sender<(InboundTellId, TCodec::Message)>,

    inbound_tell_id: Arc<AtomicU64>,

    worker_streams: futures_bounded::FuturesMap<TellId, Result<TellEvent<TCodec>, io::Error>>,
}

/// The ID of an inbound or outbound tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TellId {
    Inbound(InboundTellId),
    Outbound(OutboundTellId),
}

/// Tell handler implementation.
impl<TCodec> TellHandler<TCodec>
where
    TCodec: Codec + Send + Clone + 'static,
{
    pub(super) fn new(
        inbound_protocols: SmallVec<[TCodec::Protocol; 2]>,
        codec: TCodec,
        substream_timeout: Duration,
        inbound_tell_id: Arc<AtomicU64>,
        max_concurrent_streams: usize,
    ) -> Self {
        let (inbound_sender, inbound_receiver) = mpsc::channel(0);
        Self {
            inbound_protocols,
            codec,
            pending_events: VecDeque::new(),
            pending_outbound: VecDeque::new(),
            requested_outbound: Default::default(),
            inbound_receiver,
            inbound_sender,
            inbound_tell_id,
            worker_streams: futures_bounded::FuturesMap::new(
                substream_timeout,
                max_concurrent_streams,
            ),
        }
    }

    /// Returns the next inbound request ID.
    fn next_inbound_request_id(&mut self) -> InboundTellId {
        InboundTellId(self.inbound_tell_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Resolves negotiation of an inbound stream.
    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: (mut stream, protocol),
            info: (),
        }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        let mut codec = self.codec.clone();
        let tell_id = self.next_inbound_request_id();
        let mut sender = self.inbound_sender.clone();

        let recv = async move {
            let read = codec.read_message(&protocol, &mut stream);
            let message = read.await?;
            sender
                .send((tell_id, message))
                .await
                .expect("`ConnectionHandler` owns both ends of the channel");
            drop(sender);
            Ok(TellEvent::TellProccessed(tell_id))
        };

        if self
            .worker_streams
            .try_push(TellId::Inbound(tell_id), recv.boxed())
            .is_err()
        {
            tracing::warn!("Dropping inbound stream because we are at capacity")
        }
    }

    /// Resolves negotiation of an outbound stream.
    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: (mut stream, protocol),
            info: (),
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        let message = self
            .requested_outbound
            .pop_front()
            .expect("negotiated a stream without a pending message");

        let mut codec = self.codec.clone();
        let tell_id = message.tell_id;

        let send = async move {
            let write = codec.write_message(&protocol, &mut stream, message.message);
            write.await?;
            stream.close().await?;
            Ok(TellEvent::TellSent(tell_id))
        };

        if self
            .worker_streams
            .try_push(TellId::Outbound(tell_id), send.boxed())
            .is_err()
        {
            tracing::warn!("Dropping outbound stream because we are at capacity")
        }
    }

    /// Manages fail upgrading an outbound substream to the given protocol.
    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, info: () }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        let message = self
            .requested_outbound
            .pop_front()
            .expect("negotiated a stream without a pending message");

        match error {
            StreamUpgradeError::Timeout => {
                self.pending_events
                    .push_back(TellEvent::OutboundTimeout(message.tell_id));
            }
            StreamUpgradeError::NegotiationFailed => {
                // The remote merely doesn't support the protocol(s) we requested.
                // This is no reason to close the connection, which may
                // successfully communicate with other protocols already.
                // An event is reported to permit user code to react to the fact that
                // the remote peer does not support the requested protocol(s).
                self.pending_events
                    .push_back(TellEvent::OutboundUnsupportedProtocols(message.tell_id));
            }
            StreamUpgradeError::Apply(e) => void::unreachable(e),
            StreamUpgradeError::Io(e) => {
                tracing::debug!(
                    "outbound stream for request {} failed: {e}, retrying",
                    message.tell_id
                );
                self.requested_outbound.push_back(message);
            }
        }
    }

    /// Manages fail upgrading an inbound substream to the given protocol (can't happen).
    fn on_listen_upgrade_error(
        &mut self,
        ListenUpgradeError { error, .. }: ListenUpgradeError<
            <Self as ConnectionHandler>::InboundOpenInfo,
            <Self as ConnectionHandler>::InboundProtocol,
        >,
    ) {
        void::unreachable(error)
    }
}

/// Events emitted by the `TellHandler`.
pub enum TellEvent<TCodec>
where
    TCodec: Codec,
{
    /// An outbound tell timed out while waiting for the message
    OutboundTimeout(OutboundTellId),
    /// An outbound stream failed
    OutboundStreamFailed {
        tell_id: OutboundTellId,
        error: io::Error,
    },
    /// The remote does not support the requested protocol(s)
    OutboundUnsupportedProtocols(OutboundTellId),
    /// An inbound tell timed out while waiting for the message
    InboundTimeout(InboundTellId),
    /// An inbound stream failed
    InboundStreamFailed {
        tell_id: InboundTellId,
        error: io::Error,
    },
    /// A request has been sent
    TellSent(OutboundTellId),
    /// A request has arrived
    TellReceived {
        tell_id: InboundTellId,
        data: TCodec::Message,
    },
    /// A tell has been processed.
    TellProccessed(InboundTellId),
}

impl<TCodec: Codec> fmt::Debug for TellEvent<TCodec> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TellEvent::OutboundTimeout(id) => write!(f, "OutboundTimeout({})", id),
            TellEvent::OutboundStreamFailed { tell_id, error } => {
                write!(f, "OutboundStreamFailed({:?}, {:?})", tell_id, error)
            }
            TellEvent::OutboundUnsupportedProtocols(id) => {
                write!(f, "OutboundUnsupportedProtocols({})", id)
            }
            TellEvent::InboundTimeout(id) => write!(f, "InboundTimeout({})", id),
            TellEvent::InboundStreamFailed { tell_id, error } => {
                write!(f, "InboundStreamFailed({:?}, {:?})", tell_id, error)
            }
            TellEvent::TellSent(id) => write!(f, "TellSent({})", id),
            TellEvent::TellReceived { tell_id, data: _ } => {
                write!(f, "TellReceived({:?})", tell_id)
            }
            TellEvent::TellProccessed(id) => write!(f, "TellProccessed({})", id),
        }
    }
}

/// Outbound message.
pub struct OutboundMessage<TCodec: Codec> {
    pub(crate) tell_id: OutboundTellId,
    pub(crate) message: TCodec::Message,
    pub(crate) protocols: SmallVec<[TCodec::Protocol; 2]>,
}

impl<TCodec> fmt::Debug for OutboundMessage<TCodec>
where
    TCodec: Codec,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutboundMessage").finish_non_exhaustive()
    }
}

impl<TCodec> ConnectionHandler for TellHandler<TCodec>
where
    TCodec: Codec + Send + Clone + 'static,
{
    type FromBehaviour = OutboundMessage<TCodec>;
    type ToBehaviour = TellEvent<TCodec>;
    type InboundProtocol = TellProtocol<TCodec::Protocol>;
    type OutboundProtocol = TellProtocol<TCodec::Protocol>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(
            TellProtocol {
                protocols: self.inbound_protocols.clone(),
            },
            (),
        )
    }

    fn on_behaviour_event(&mut self, request: Self::FromBehaviour) {
        self.pending_outbound.push_back(request);
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<TellProtocol<TCodec::Protocol>, (), Self::ToBehaviour>> {
        match self.worker_streams.poll_unpin(cx) {
            Poll::Ready((_, Ok(Ok(event)))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
            }
            Poll::Ready((TellId::Inbound(id), Ok(Err(e)))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    TellEvent::InboundStreamFailed {
                        tell_id: id,
                        error: e,
                    },
                ));
            }
            Poll::Ready((TellId::Outbound(id), Ok(Err(e)))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    TellEvent::OutboundStreamFailed {
                        tell_id: id,
                        error: e,
                    },
                ));
            }
            Poll::Ready((TellId::Inbound(id), Err(futures_bounded::Timeout { .. }))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    TellEvent::InboundTimeout(id),
                ));
            }
            Poll::Ready((TellId::Outbound(id), Err(futures_bounded::Timeout { .. }))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    TellEvent::OutboundTimeout(id),
                ));
            }
            Poll::Pending => {}
        }

        // Drain pending events that were produced by `worker_streams`.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        } else if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        // Check for inbound requests.
        if let Poll::Ready(Some((id, msg))) = self.inbound_receiver.poll_next_unpin(cx) {
            // We received an inbound request.
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                TellEvent::TellReceived {
                    tell_id: id,
                    data: msg,
                },
            ));
        }

        // Emit outbound requests.
        if let Some(message) = self.pending_outbound.pop_front() {
            let protocols = message.protocols.clone();
            self.requested_outbound.push_back(message);

            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(TellProtocol { protocols }, ()),
            });
        }

        debug_assert!(self.pending_outbound.is_empty());

        if self.pending_outbound.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_outbound.shrink_to_fit();
        }

        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {}
