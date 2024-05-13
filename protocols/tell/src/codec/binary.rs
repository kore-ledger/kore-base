// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Binary Codec
//! A "tell" codec using binary data for messasges.

use async_trait::async_trait;
use futures::prelude::*;
use futures::{AsyncRead, AsyncWrite};
use libp2p::StreamProtocol;
use std::io;

/// Behaviour for encoding and decoding binary messages.
pub type Behaviour = crate::Behaviour<Codec>;

/// A codec for encoding and decoding binary messages.
///
pub struct Codec {
    pub max_message_size: u64,
}

impl Default for Codec {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024 * 8,
        }
    }
}

impl Clone for Codec {
    fn clone(&self) -> Self {
        Self {
            max_message_size: self.max_message_size,
        }
    }
}

#[async_trait]
impl super::Codec for Codec {
    type Protocol = StreamProtocol;
    type Message = Vec<u8>;

    async fn read_message<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Message>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();
        io.take(self.max_message_size).read_to_end(&mut vec).await?;
        Ok(vec)
    }

    async fn write_message<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Message,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&req).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::super::Codec as _;
    use futures::AsyncWriteExt;
    use futures_ringbuf::Endpoint;
    use libp2p::StreamProtocol;

    #[async_std::test]
    async fn test_codec() {
        let expected_message = b"Hello, World!".to_vec();
        let protocol = StreamProtocol::new("/test_json/1");
        let mut codec = super::Codec::default();

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
