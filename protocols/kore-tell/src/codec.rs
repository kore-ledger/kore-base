// Copyright 2023 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Codec
//! This module contains the codec trait for encoding and decoding messages.
//!

use async_trait::async_trait;
use futures::prelude::*;
use std::io;

/// A codec trait for encoding and decoding messages.
#[async_trait]
pub trait Codec {
    /// The type of protocol(s) or protocol versions being negotiated.
    type Protocol: AsRef<str> + Send + Clone;
    /// The type of inbound and outbound message.
    type Message: Send;

    /// Reads a message from the given I/O stream according to the
    /// negotiated protocol.
    async fn read_message<T>(
        &mut self,
        protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Message>
    where
        T: AsyncRead + Unpin + Send;

    /// Writes a message to the given I/O stream according to the
    /// negotiated protocol.
    async fn write_message<T>(
        &mut self,
        protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Message,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send;
}
