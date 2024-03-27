// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network service
//!

use crate::{
    behaviour::Behaviour, utils::convert_boot_nodes, Command, Config, Error, Event, NodeType,
    TransportType,
};

use identity::keys::{KeyMaterial, KeyPair};

use libp2p::{
    core::{
        muxing::StreamMuxerBox,
        transport::{Boxed, OptionalTransport},
        upgrade,
    },
    dns,
    identity::{ed25519, Keypair},
    noise,
    swarm::NetworkBehaviour,
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use either::Either;
use prometheus_client::registry::Registry;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// The network service.
pub struct NetworkService {
    /// The command sender to communicate with the worker.
    command_sender: Sender<Command>,

    /// The metrics registry.
    registry: Arc<Mutex<Registry>>,
}

impl NetworkService {
    /// Create a new `NetworkService`.
    pub fn new(
        command_sender: Sender<Command>,
        registry: Arc<Mutex<Registry>>,
    ) -> Result<Self, Error> {
        Ok(Self {
            command_sender,
            registry,
        })
    }
}
