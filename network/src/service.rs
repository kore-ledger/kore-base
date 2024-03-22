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
    swarm::NetworkBehaviour,
    dns,
    identity::{ed25519, Keypair},
    noise, tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use either::Either;
use prometheus_client::registry::Registry;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// The network service.
pub struct NetworkService {
    /// The command sender.
    command_sender: mpsc::Sender<Command>,
    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,
    /// The event sender.
    event_sender: mpsc::Sender<Event>,
    /// The cancellation token.
    cancel: CancellationToken,
    /// The network metrics registry.
    metrics: Registry,
    /// The external addresses.
    external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,

}

impl NetworkService {

    /// Create a new `NetworkService`.
    pub fn new(
        key_pair: KeyPair,
        event_sender: mpsc::Sender<Event>,
        cancel: CancellationToken,
        config: Config,
    ) -> Result<Self, Error> {
        // Create channels to communicate events and commands
        let (command_sender, command_receiver) = mpsc::channel(10000);

        // Prepare the network crypto key.
        let key = {
            let sk = ed25519::SecretKey::try_from_bytes(key_pair.secret_key_bytes())
                .expect("Invalid keypair");
            let kp = ed25519::Keypair::from(sk);
            Keypair::from(kp)
        };

        // Create the external addresses set.
        let external_addresses = Arc::new(Mutex::new(HashSet::new()));

        // Create metrics registry.
        let mut metrics = Registry::default();

        // Create the network behaviour.

        Ok(Self {
            command_sender,
            command_receiver,
            event_sender,
            cancel,
            metrics,
            external_addresses,
        })
    }

}
