//
//

/// A "tell" behaviour using [`serde_json`] for serializing and deserializing the
/// messages.
///

mod codec {
    use async_trait::async_trait;
    use futures::prelude::*;
    use futures::{AsyncRead, AsyncWrite};
    use libp2p::StreamProtocol;
    use serde::{de::DeserializeOwned, Serialize};
    use std::{io, marker::PhantomData};

    /// A codec for encoding and decoding messages using [`serde_json`].
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
    impl<M> crate::Codec for Codec<M>
    where
        M: Serialize + DeserializeOwned + Send,
    {
        type Protocol = StreamProtocol;
        type Message = M;

        async fn read_message<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
        ) -> io::Result<Self::Message>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();
            io.take(self.max_message_size).read_to_end(&mut vec).await?;
            Ok(serde_json::from_slice(vec.as_slice())?)
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
            let vec = serde_json::to_vec(&msg)?;
            io.write_all(vec.as_slice()).await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {

    use crate::Codec;
    use futures::AsyncWriteExt;
    use futures_ringbuf::Endpoint;
    use libp2p::StreamProtocol;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestMessage {
        payload: String,
    }

    #[async_std::test]
    async fn test_codec() {
        let expected_message = TestMessage {
            payload: "test_payload".to_string(),
        };
        let protocol = StreamProtocol::new("/test_json/1");
        let mut codec: super::codec::Codec<TestMessage> = super::codec::Codec::default();

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
