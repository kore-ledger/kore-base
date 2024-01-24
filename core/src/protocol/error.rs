// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use thiserror::Error;

/// Errors that may occur when using the Kore protocol.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ProtocolErrors {
    #[error("Ask command not supported")]
    AskCommandDetected,
    #[error("Channel closes")]
    ChannelClosed,
}
