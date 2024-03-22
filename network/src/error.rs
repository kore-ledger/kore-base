// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network errors.
//! 

use thiserror::Error;

/// Network errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("DNS error: {0}")]
    Dns(String),
    #[error("Relay error: {0}")]
    Relay(String),
    #[error("Behaviour error: {0}")]
    Behaviour(String),
}