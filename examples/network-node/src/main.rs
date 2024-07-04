#![doc = include_str!("../README.md")]

use config::Config;
use identity::keys::{Ed25519KeyPair, KeyGenerator, KeyPair};
use network::{
    Config as NetworkConfig, NetworkWorker, NodeType, RoutingConfig, RoutingNode, TellConfig,
};
use prometheus_client::registry::Registry;
use serde::Deserialize;
use std::error::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

/// Configuration
#[derive(Debug, Deserialize)]
pub struct Settings {
    node_type: NodeType,
    peer_id: String,
    private_key: String,
    listen_addresses: Vec<String>,
    external_addresses: Vec<String>,
    boot_nodes: Vec<RoutingNode>,
    dht_random_walk: bool,
    allow_non_globals_in_dht: bool,
    allow_private_ip: bool,
    mdns: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Load settings
    let settings = Config::builder()
        .add_source(config::File::with_name("examples/network-node/config"))
        .build()
        .unwrap();
    let settings = settings.try_deserialize::<Settings>()?;

    // Build network configuration
    let network_config = build_config(&settings);

    // Create key pair
    let secret_bytes = hex::decode(&settings.private_key)?;
    let kp = KeyPair::Ed25519(Ed25519KeyPair::from_secret_key(&secret_bytes));

    // Create network worker
    let token = CancellationToken::new();
    let mut registry = Registry::default();
    let (event_sender, mut event_receiver) = mpsc::channel(100);
    let mut worker = NetworkWorker::new(
        &mut registry,
        kp,
        network_config,
        event_sender,
        token.clone(),
    )
    .unwrap();

    assert_eq!(worker.local_peer_id().to_string(), settings.peer_id);

    // Wait for connection.
    worker.run_connection().await?;

    // Spawn the worker.
    tokio::spawn(async move {
        worker.run_main().await;
    });

    loop {
        tokio::select! {
            event = event_receiver.recv() => {
                if event.is_some() {
                }
            }
            _ = token.cancelled() => {
                break;
            }
        }
    }
    Ok(())
}

/// Build configuration
fn build_config(settings: &Settings) -> NetworkConfig {
    let routing_config = RoutingConfig::new(settings.boot_nodes.clone())
        .with_dht_random_walk(settings.dht_random_walk)
        .with_allow_non_globals_in_dht(settings.allow_non_globals_in_dht)
        .with_allow_private_ip(settings.allow_private_ip)
        .with_mdns(settings.mdns);
    NetworkConfig {
        user_agent: "kore-node".to_owned(),
        node_type: settings.node_type.clone(),
        external_addresses: settings.external_addresses.clone(),
        listen_addresses: settings.listen_addresses.clone(),
        tell: TellConfig::default(),
        routing: routing_config,
        port_reuse: false,
    }
}
