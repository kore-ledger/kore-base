//

use tell::{InboundFailure, InboundTellId, OutboundFailure, OutboundTellId, ProtocolSupport};

use libp2p::{swarm::Swarm, PeerId, StreamProtocol};

use anyhow::{bail, Result};
use futures::prelude::*;
use libp2p_swarm_test::SwarmExt;

use std::{io, iter, pin::pin, time::Duration};

#[async_std::test]
async fn test_outbound_failure() {
    let (peer1_id, mut swarm1) = new_swarm();
    let (_peer2_id, mut swarm2) = new_swarm();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    // Expects no events because `Event::Request` is produced after `read_request`.
    // Keep the connection alive, otherwise swarm2 may receive `ConnectionClosed` instead.
    let server_task = wait_no_events(&mut swarm1);

    // Expects OutboundFailure::Io failure with `FailOnWriteRequest` error.
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_message(&peer1_id, Action::FailOnWriteMessage);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        let error = match error {
            OutboundFailure::Io(e) => e,
            e => panic!("Unexpected error: {e:?}"),
        };

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.into_inner().unwrap().to_string(),
            "FailOnWriteRequest"
        );
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

#[async_std::test]
async fn test_inbound_failure() {
    let (peer1_id, mut swarm1) = new_swarm();
    let (_peer2_id, mut swarm2) = new_swarm();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    // Expects inbound failure.
    let server_task = async move {
        let (peer, _, error) = wait_inbound_failure(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer1_id);

        let error = match error {
            InboundFailure::Io(e) => e,
            e => panic!("Unexpected error: {e:?}"),
        };

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.into_inner().unwrap().to_string(),
            "FailOnWriteRequest"
        );
    };

    // Expects OutboundFailure::Io failure with `FailOnReadRequest` error.
    let client_task = async move {
        let _ = swarm2
            .behaviour_mut()
            .send_message(&peer1_id, Action::FailOnReadMessage);
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

async fn wait_no_events(swarm: &mut Swarm<tell::Behaviour<TestCodec>>) {
    loop {
        if let Ok(ev) = swarm.select_next_some().await.try_into_behaviour_event() {
            panic!("Unexpected event: {ev:?}")
        }
    }
}

async fn wait_outbound_failure(
    swarm: &mut Swarm<tell::Behaviour<TestCodec>>,
) -> Result<(PeerId, OutboundTellId, OutboundFailure)> {
    loop {
        match swarm.select_next_some().await.try_into_behaviour_event() {
            Ok(tell::Event::OutboundFailure {
                peer_id,
                outbound_id,
                error,
            }) => {
                return Ok((peer_id, outbound_id, error));
            }
            Ok(ev) => bail!("Unexpected event: {ev:?}"),
            Err(..) => {}
        }
    }
}

async fn wait_inbound_failure(
    swarm: &mut Swarm<tell::Behaviour<TestCodec>>,
) -> Result<(PeerId, InboundTellId, InboundFailure)> {
    loop {
        match swarm.select_next_some().await.try_into_behaviour_event() {
            Ok(tell::Event::InboundFailure {
                peer_id,
                inbound_id,
                error,
            }) => {
                return Ok((peer_id, inbound_id, error));
            }
            Ok(ev) => bail!("Unexpected event: {ev:?}"),
            Err(..) => {}
        }
    }
}

fn new_swarm_with_timeout(timeout: Duration) -> (PeerId, Swarm<tell::Behaviour<TestCodec>>) {
    let protocols = iter::once((
        StreamProtocol::new("/test/1"),
        ProtocolSupport::InboundOutbound,
    ));
    let cfg = tell::Config::default().with_message_timeout(timeout);

    let swarm = Swarm::new_ephemeral(|_| tell::Behaviour::<TestCodec>::new(protocols, cfg));
    let peed_id = *swarm.local_peer_id();

    (peed_id, swarm)
}

fn new_swarm() -> (PeerId, Swarm<tell::Behaviour<TestCodec>>) {
    new_swarm_with_timeout(Duration::from_millis(100))
}

#[derive(Clone, Default)]
struct TestCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    FailOnReadMessage,
    FailOnWriteMessage,
}

impl From<Action> for u8 {
    fn from(value: Action) -> Self {
        match value {
            Action::FailOnReadMessage => 0,
            Action::FailOnWriteMessage => 1,
        }
    }
}

impl TryFrom<u8> for Action {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Action::FailOnReadMessage),
            1 => Ok(Action::FailOnWriteMessage),
            _ => Err(io::Error::new(io::ErrorKind::Other, "invalid action")),
        }
    }
}

#[async_trait::async_trait]
impl tell::Codec for TestCodec {
    type Protocol = StreamProtocol;
    type Message = Action;

    async fn read_message<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Message>
    where
        T: async_std::io::Read + Unpin + Send,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;

        if buf.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        assert_eq!(buf.len(), 1);

        match buf[0].try_into()? {
            Action::FailOnReadMessage => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnReadRequest"))
            }
            action => Ok(action),
        }
    }

    async fn write_message<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Message,
    ) -> std::io::Result<()>
    where
        T: async_std::io::Write + Unpin + Send,
    {
        match req {
            Action::FailOnWriteMessage => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnWriteRequest"))
            }
            action => {
                let bytes = [action.into()];
                io.write_all(&bytes).await?;
                Ok(())
            }
        }
    }
}
