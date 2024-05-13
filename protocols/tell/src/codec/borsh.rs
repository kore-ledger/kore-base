// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

/// A "tell" behaviour using [`serde_json`] for serializing and deserializing the
/// messages.
///
use async_trait::async_trait;
use borsh::{from_slice, to_vec, BorshDeserialize, BorshSerialize};
use futures::prelude::*;
use futures::{AsyncRead, AsyncWrite};
use libp2p::StreamProtocol;
use std::{io, marker::PhantomData};

/// Behaviour for encoding and decoding messages using [`borsh`].
pub type Behaviour<M> = crate::Behaviour<Codec<M>>;

/// A codec for encoding and decoding messages using [`borsh`].
///
pub struct Codec<M> {
    pub max_message_size: u64,
    pub _phantom: PhantomData<M>,
}

impl<M> Default for Codec<M> {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024 * 8,
            _phantom: PhantomData,
        }
    }
}

impl<M> Clone for Codec<M> {
    fn clone(&self) -> Self {
        Self {
            max_message_size: self.max_message_size,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<M> super::Codec for Codec<M>
where
    M: BorshSerialize + BorshDeserialize + Send,
{
    type Protocol = StreamProtocol;
    type Message = M;

    async fn read_message<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Message>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();
        io.take(self.max_message_size).read_to_end(&mut vec).await?;
        Ok(from_slice(vec.as_slice())?)
    }

    async fn write_message<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        msg: Self::Message,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let vec = to_vec(&msg)?;
        io.write_all(vec.as_slice()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {

    use super::super::Codec as _;
    use super::*;
    use borsh::{BorshDeserialize, BorshSerialize};
    use futures::AsyncWriteExt;
    use futures_ringbuf::Endpoint;
    use libp2p::StreamProtocol;

    #[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
    struct TestMessage {
        payload: String,
    }

    #[async_std::test]
    async fn test_codec() {
        let expected_message = TestMessage {
            payload: "Hello, World!".to_string(),
        };

        let protocol = StreamProtocol::new("/test_json/1");
        let mut codec: Codec<TestMessage> = Codec::default();

        let (mut a, mut b) = Endpoint::pair(124, 124);
        codec
            .write_message(&protocol, &mut a, expected_message.clone())
            .await
            .expect("Should write request");
        a.close().await.unwrap();

        let actual_message = codec
            .read_message(&protocol, &mut b)
            .await
            .expect("Should read request");
        b.close().await.unwrap();

        assert_eq!(actual_message, expected_message);
    }
}
