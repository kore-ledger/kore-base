// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Transport layer.
//!

use crate::Error;

use libp2p::{
    core::{
        muxing::StreamMuxerBox,
        transport::{upgrade::Version, Boxed},
    },
    identity::Keypair,
    metrics::{BandwidthTransport, Registry},

    relay::client::{new, Behaviour},
    tcp, dns, noise, yamux, PeerId, Transport, 
};

pub type KoreTransport = Boxed<(PeerId, StreamMuxerBox)>;
pub type RelayClient = Behaviour;

/// Builds the transport.
///
/// # Arguments
/// 
/// * `registry` - The Prometheus registry.
/// * `peer_id` - The peer ID.
/// * `keys` - The keypair.
/// 
/// # Returns
/// 
/// The transport and relay client.
/// 
/// # Errors
/// 
/// If the transport cannot be built.
/// 
pub fn build_transport(
    registry: &mut Registry,
    peer_id: PeerId,
    keys: &Keypair,
) -> Result<(KoreTransport, RelayClient), Error> {
    // Build the relay client and transport.
    let (transport, relay_client) = new(peer_id);

    // Build the noise authentication.
    let noise = noise::Config::new(&keys)
        .map_err(|e| Error::Transport(format!("Noise authentication {:?}", e)))?;

    // Alloew Memory transport for testing
    #[cfg(test)]
    let transport = transport.or_transport(libp2p::core::transport::MemoryTransport::default());

    // Allow TCP transport.
    let transport = transport.or_transport(tcp::tokio::Transport::default());

    // Upgrade the transport with the noise authentication and yamux multiplexing.
    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(yamux::Config::default());

    // Allow the DNS transport.
    let transport = dns::tokio::Transport::system(transport)
        .map_err(|e| Error::Transport(format!("DNS error {:?}", e)))?;


    // Wrap the transport with bandwidth metrics for Prometheus.
    let transport = BandwidthTransport::new(transport, registry)
        .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)));

    Ok((transport.boxed(), relay_client))
}
