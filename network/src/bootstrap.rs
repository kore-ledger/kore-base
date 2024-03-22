// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Bootstrap node implementation.
//! 

use crate::{Config, Error, behaviour::BehaviourService};

use libp2p::{
    dcutr, tcp, noise, yamux,
    identity::{Keypair, PublicKey},
    relay::{Behaviour as RelayServer, client::Behaviour as RelayClient},
    swarm::NetworkBehaviour,
    Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The bootstrap node behaviour.
#[derive(NetworkBehaviour)]
pub struct Behaviour {
    /// Common network behaviour.
    common: crate::behaviour::Behaviour,
    /// Direct Connection Upgrade through Relay (DCUTR) behaviour.
    dcutr: dcutr::Behaviour,
    /// Relay client behaviour.
    relay_client: RelayClient,
    /// Relay server behaviour,
    relay_server: RelayServer,
}
