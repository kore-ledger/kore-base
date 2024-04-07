// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Network worker.
//!

use crate::{
    behaviour::{Behaviour, Event as BehaviourEvent},
    service::NetworkService,
    transport::{build_transport, KoreTransport, RelayClient},
    utils::convert_external_addresses,
    Command, Config, Error, Event as NetworkEvent, NodeType,
};

use identity::keys::{KeyMaterial, KeyPair};

use libp2p::{
    dcutr::Event as DcutrEvent,
    identity::{ed25519, Keypair},
    metrics::{Metrics, Recorder},
    noise,
    swarm::{self, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};

use futures::StreamExt;
use prometheus_client::registry::Registry;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use tracing::{error, info, trace, warn};

use std::{
    collections::{HashSet, HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

const TARGET_WORKER: &str = "KoreNetwork-Worker";

/// Main network worker. Must be polled in order for the network to advance.
///
/// The worker is responsible for handling the network events and commands.
///
pub struct NetworkWorker {
    /// Network service.
    service: Arc<NetworkService>,

    /// Addresses that the node is listening on.
    /// Updated by the `NetworkWorker` and loaded by the `NetworkService`.
    external_addresses: Arc<Mutex<HashSet<Multiaddr>>>,

    /// The libp2p swarm.
    swarm: Swarm<Behaviour>,

    /// The command receiver.
    command_receiver: mpsc::Receiver<Command>,

    /// The event sender.
    event_sender: mpsc::Sender<NetworkEvent>,

    /// The cancellation token.
    cancel: CancellationToken,

    /// The metrics registry.
    metrics: Metrics,

    /// Pendings messages to peer
    pending_messages: HashMap<PeerId, VecDeque<Vec<u8>>>,
}

impl NetworkWorker {
    /// Create a new `NetworkWorker`.
    pub fn new(
        registry: &mut Registry,
        keys: KeyPair,
        config: Config,
        event_sender: mpsc::Sender<NetworkEvent>,
        cancel: CancellationToken,
    ) -> Result<Self, Error> {
        // Create channels to communicate events and commands
        let (command_sender, command_receiver) = mpsc::channel(10000);

        // Prepare the network crypto key.
        let key = {
            let sk = ed25519::SecretKey::try_from_bytes(keys.secret_key_bytes())
                .expect("Invalid keypair");
            let kp = ed25519::Keypair::from(sk);
            Keypair::from(kp)
        };

        // Generate the `PeerId` from the public key.
        let local_peer_id = key.public().to_peer_id();

        // Create the external addresses set.
        let external_addresses = match config.node_type.clone() {
            NodeType::Bootstrap { external_addresses }
            | NodeType::Addressable { external_addresses } => {
                Arc::new(Mutex::new(convert_external_addresses(&external_addresses)?))
            }
            _ => Arc::new(Mutex::new(HashSet::new())),
        };

        let ext_addr = external_addresses.clone();

        // Build transport.
        let (transport, relay_client) = build_transport(registry, local_peer_id, &key)?;

        // Create the swarm.
        let swarm = Swarm::new(
            transport,
            Behaviour::new(
                &key.public(),
                config,
                external_addresses.clone(),
                relay_client,
            ),
            local_peer_id,
            swarm::Config::with_tokio_executor(),
        );

        let metrics = Metrics::new(registry);

        let service = Arc::new(NetworkService::new(command_sender)?);

        Ok(Self {
            service,
            external_addresses,
            swarm,
            command_receiver,
            event_sender,
            cancel,
            metrics,
            pending_messages: HashMap::default(),
        })
    }

    /// Send message to a peer.
    ///
    /// If the peer is reachable, it invokes it directly. Otherwise, it selects the closest
    /// accessible peer, to establish a relay circuit with the peer.
    pub async fn send_message(&mut self, peer: PeerId, message: Vec<u8>) {
        let behaviour = self.swarm.behaviour_mut();
        if let Some(node) = behaviour.node(&peer) {
            trace!(TARGET_WORKER, "Sending message to known peer: {:?}", peer);
            if !node.reachable() {
                trace!(TARGET_WORKER, "Peer is not reachable: {:?}", peer);
            }
        }
        behaviour.send_message(&peer, message);
    }

    /// Run network worker.
    pub async fn run(&mut self) {
        trace!(TARGET_WORKER, "Running main loop");
        loop {
            tokio::select! {
                command = self.command_receiver.recv() => {
                    // Handle commands.
                    if let Some(command) = command {
                        self.handle_command(command).await;
                    }
                }
                event = self.swarm.select_next_some() => {
                    // Handle events.
                    self.handle_event(event).await;
                }
                _ = self.cancel.cancelled() => {
                    break;
                }
            }
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::SendMessage { peer, message } => {
                //self.swarm.send_message(peer, message);
            }
            Command::Bootstrap => {
                trace!(TARGET_WORKER, "Bootstrap en la red kore");
                self.swarm.behaviour_mut().bootstrap();
            }
            Command::StartProviding { keys } => {
                //self.swarm.start_providing(keys);
            }
            _ => {
                self.cancel.cancel();
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Dcutr(DcutrEvent {
                remote_peer_id,
                result,
            })) => {
                if result.is_ok() {
                    // Send pending messages for ephemeral peer.
                    trace!(TARGET_WORKER, "Sending pending messages to peer {} via dcutr.", remote_peer_id);
                    let pending_messages = self.pending_messages.remove(&remote_peer_id);
                    if let Some(pending_messages) = pending_messages {
                        for message in pending_messages.into_iter() {
                            self.swarm.behaviour_mut().send_message(&remote_peer_id, message);
                        }
                    } else {
                        trace!(TARGET_WORKER, "");
                    }
            
                } else {
                    error!(TARGET_WORKER, "Error in dcutr connection to peer {}.", remote_peer_id);
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Message { peer_id, message }) => {
                let result = self
                    .event_sender
                    .send(NetworkEvent::MessageReceived {
                        message: message.message,
                    })
                    .await;
                if result.is_err() {
                    error!(
                        TARGET_WORKER,
                        "Could not receive message from peer {}", peer_id
                    );
                } else {
                    trace!(TARGET_WORKER, "Message received from peer {}", peer_id);
                    
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    use libp2p::{
        core::{muxing::StreamMuxerBox, transport::Boxed, upgrade},
        identity, plaintext, yamux, Multiaddr, PeerId, Transport,
    };

    use futures::{
        channel::mpsc::unbounded,
        io::{AsyncRead, AsyncWrite},
    };
    use libp2p_swarm_test::SwarmExt;

    // Build a relay server.
    fn build_worker(config: Config) -> Swarm<Behaviour> {
        unimplemented!()
    }
}
