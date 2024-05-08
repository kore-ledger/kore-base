// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network errors.
//!

use thiserror::Error;

/// Network errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Worker error.
    #[error("Worker error: {0}")]
    Worker(String),
    /// Network error.
    #[error("Network error: {0}")]
    Network(String),
    /// Transport error.
    #[error("Transport error: {0}")]
    Transport(String),
    /// DNS error
    #[error("DNS error: {0}")]
    Dns(String),
    /// Relay error.
    #[error("Relay error: {0}")]
    Relay(String),
    /// Behaviour error.
    #[error("Behaviour error: {0}")]
    Behaviour(String),
    /// Address error.
    #[error("Address error: {0}")]
    Address(String),
    /// Command error.
    #[error("Command error: {0}")]
    Command(String),
}
