// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Routing behaviour for the network.
//! Combines identify behaviour and kad routing behaviour.
//!

use libp2p::{
    identify::{self, Config as IdentifyConfig},
    identity::PublicKey,
    kad::{self, store::MemoryStore, Config as KadConfig, QueryId},
    swarm::{derive_prelude, NetworkBehaviour},
    Multiaddr, PeerId, StreamProtocol,
};

use crate::Error;

const LOG_TARGET: &str = "Kore_Ledger::Routing";

/// The `RoutingBehavior` is a custom network behaviour that combines Kademlia and Identify.
#[derive(NetworkBehaviour)]
#[behaviour(prelude = "derive_prelude")]
pub struct RoutingBehavior {
    identify: identify::Behaviour,
    kad: kad::Behaviour<MemoryStore>,
}

impl RoutingBehavior {
    /// Create a new `RoutingBehavior` with the given `PublicKey`.
    pub fn new(
        local_public_key: &PublicKey,
        bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
        config: Option<KadConfig>,
    ) -> Self {
        let identify_config =
            IdentifyConfig::new(super::KORE_PROTOCOL.to_owned(), local_public_key.clone());
        let identify = identify::Behaviour::new(identify_config);
        let peer_id = PeerId::from_public_key(local_public_key);
        let mut kad_config = match config {
            Some(config) => config,
            None => KadConfig::default(),
        };
        let kad_config =
            kad_config.set_protocol_names(vec![StreamProtocol::new(&super::KORE_PROTOCOL)]);
        let mut kad = kad::Behaviour::<MemoryStore>::with_config(
            PeerId::from_public_key(local_public_key),
            MemoryStore::new(peer_id),
            kad_config.clone(),
        );
        for (peer_id, addr) in bootstrap_nodes {
            kad.add_address(&peer_id, addr);
        }
        Self { identify, kad }
    }

    /// Add a known address for the given peer id.
    pub fn add_address(&mut self, peer_id: &PeerId, addr: Multiaddr) {
        self.kad.add_address(peer_id, addr);
    }

    /// Establishes the local node as the provider for that key in the given
    /// shard.
    pub fn start_providing<K>(&mut self, key: &K)
    where
        K: AsRef<[u8]>,
    {
        if let Err(e) = self.kad.start_providing(kad::RecordKey::new(&key)) {
            log::warn!("{}: Failed to start providing: {:?}", LOG_TARGET, e);
        }
    }

    /// Bootstrap the local node to the network.
    pub fn bootstrap(&mut self) -> Result<QueryId, Error> {
        self.kad.bootstrap().map_err(|e| {
            log::warn!("{}: Failed to bootstrap: {:?}", LOG_TARGET, e);
            Error::NetworkError(format!("Failed to bootstrap -> {:?}", e))
        })
    }

    /// Get the closest peers to the given peer id.
    pub fn get_closest_peers(&mut self, peer_id: PeerId) -> QueryId {
        self.kad.get_closest_peers(peer_id)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::network::KORE_PROTOCOL;

    use futures::{executor::block_on, future::poll_fn, join, prelude::*};
    use libp2p::{
        kad::{self, Config, QueryResult},
        multiaddr::Protocol,
        swarm::SwarmEvent,
        Multiaddr, Swarm,
    };
    use libp2p_swarm_test::SwarmExt;
    use quickcheck::{Arbitrary, Gen, QuickCheck};
    use rand::{random, rngs::StdRng, Rng, SeedableRng};

    use std::{collections::HashSet, pin::pin, task::Poll};

    use RoutingBehaviorEvent::*;

    type TestSwarm = Swarm<RoutingBehavior>;

    #[async_std::test]
    async fn test_routing_behaviour() {
        let mut bootstrap_nodes = vec![];
        let (boot_addr, mut boot_swarm) = build_node(bootstrap_nodes.clone());
        let peer_boot = *boot_swarm.local_peer_id();
        bootstrap_nodes.push((peer_boot, boot_addr));

        let (_, mut swarm1) = build_node(bootstrap_nodes.clone());
        let (_, mut swarm2) = build_node(bootstrap_nodes.clone());

        let result = swarm1.behaviour_mut().bootstrap();
        assert!(result.is_ok());

        let result = swarm2.behaviour_mut().bootstrap();
        assert!(result.is_ok());

        let boot = main_loop(&mut boot_swarm);
        let loop1 = main_loop(&mut swarm1);
        let loop2 = main_loop(&mut swarm2);

        let boot = pin!(boot);
        let loop1 = pin!(loop1);
        let loop2 = pin!(loop2);

        assert_eq!(join!(boot, loop1, loop2), (true, true, true));
    }

    #[async_std::test]
    async fn test_bootstrap() {
        fn prop(seed: Seed) {
            let mut rng = StdRng::from_seed(seed.0);
            let num_total = rng.gen_range(2..20);
            // When looking for the closest node to a key, Kademlia considers
            // K_VALUE nodes to query at initialization. If `num_group` is larger
            // than K_VALUE the remaining locally known nodes will not be
            // considered. Given that no other node is aware of them, they would be
            // lost entirely. To prevent the above restrict `num_group` to be equal
            // or smaller than K_VALUE.
            let num_group = rng.gen_range(1..(num_total % kad::K_VALUE.get()) + 2);

            let mut cfg = Config::default();
            cfg.set_protocol_names(vec![StreamProtocol::new(&KORE_PROTOCOL)]);
            if rng.gen() {
                cfg.disjoint_query_paths(true);
            }

            let mut swarms = build_connected_nodes_with_config(num_total, num_group, cfg)
                .into_iter()
                .map(|(_, swarm)| swarm)
                .collect::<Vec<_>>();

            let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

            let qid = swarms[0].behaviour_mut().bootstrap().unwrap();

            // Expected known peers
            let expected_known = swarm_ids.iter().skip(1).cloned().collect::<HashSet<_>>();
            let mut first = true;

            // Run test
            block_on(poll_fn(move |ctx| {
                for (i, swarm) in swarms.iter_mut().enumerate() {
                    loop {
                        match swarm.poll_next_unpin(ctx) {
                            Poll::Ready(Some(SwarmEvent::Behaviour(
                                RoutingBehaviorEvent::Kad(kad::Event::OutboundQueryProgressed {
                                    id,
                                    result: QueryResult::Bootstrap(Ok(ok)),
                                    ..
                                }),
                            ))) => {
                                assert_eq!(id, qid);
                                assert_eq!(i, 0);
                                if first {
                                    // Bootstrapping must start with a self-lookup.
                                    assert_eq!(ok.peer, swarm_ids[0]);
                                }
                                first = false;
                                if ok.num_remaining == 0 {
                                    assert_eq!(
                                        swarm.behaviour_mut().kad.iter_queries().count(),
                                        0,
                                        "Expect no remaining queries when `num_remaining` is zero.",
                                    );
                                    let mut known = HashSet::new();
                                    for b in swarm.behaviour_mut().kad.kbuckets() {
                                        for e in b.iter() {
                                            known.insert(*e.node.key.preimage());
                                        }
                                    }
                                    assert_eq!(expected_known, known);
                                    return Poll::Ready(());
                                }
                            }
                            // Ignore any other event.
                            Poll::Ready(Some(_)) => (),
                            e @ Poll::Ready(_) => panic!("Unexpected return value: {e:?}"),
                            Poll::Pending => break,
                        }
                    }
                }
                Poll::Pending
            }));
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _)
    }

    /// Reusable main loop for the swarm.
    async fn main_loop(swarm: &mut Swarm<RoutingBehavior>) -> bool {
        let local_peer_id = *swarm.local_peer_id();

        let mut count = 0;
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(Identify(event)) => match event {
                    identify::Event::Received { peer_id, .. } => {
                        println!(
                            "{} -> Received identify event from {}",
                            local_peer_id, peer_id
                        );
                    }
                    identify::Event::Sent { peer_id, .. } => {
                        println!("{} -> Sent identify event to {}", local_peer_id, peer_id);
                        count += 1;
                    }
                    _ => {}
                },
                SwarmEvent::Behaviour(Kad(event)) => match event {
                    kad::Event::OutboundQueryProgressed { result, .. } => {
                        if let QueryResult::Bootstrap(Ok(_)) = result {
                            println!("{} -> Bootstrap query completed", local_peer_id);
                            return true;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if count == 2 {
                return true;
            }
        }
    }

    /// Build node.
    fn build_node(bootstrap_nodes: Vec<(PeerId, Multiaddr)>) -> (Multiaddr, TestSwarm) {
        build_node_with_config(kad::Config::default(), bootstrap_nodes)
    }

    /// Build node with config.
    fn build_node_with_config(
        config: Config,
        bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
    ) -> (Multiaddr, TestSwarm) {
        let mut swarm = Swarm::new_ephemeral(|key_pair| {
            RoutingBehavior::new(&key_pair.public(), bootstrap_nodes, Some(config))
        });

        let address: Multiaddr = Protocol::Memory(random::<u64>()).into();
        swarm.listen_on(address.clone()).unwrap();
        swarm.add_external_address(address.clone());

        (address, swarm)
    }

    fn build_nodes_with_config(num: usize, cfg: Config) -> Vec<(Multiaddr, TestSwarm)> {
        (0..num)
            .map(|_| build_node_with_config(cfg.clone(), vec![]))
            .collect::<Vec<_>>()
    }

    fn build_connected_nodes_with_config(
        total: usize,
        step: usize,
        cfg: Config,
    ) -> Vec<(Multiaddr, TestSwarm)> {
        let mut swarms = build_nodes_with_config(total, cfg);
        let swarm_ids: Vec<_> = swarms
            .iter()
            .map(|(addr, swarm)| (addr.clone(), *swarm.local_peer_id()))
            .collect();

        let mut i = 0;
        for (j, (addr, peer_id)) in swarm_ids.iter().enumerate().skip(1) {
            if i < swarm_ids.len() {
                swarms[i]
                    .1
                    .behaviour_mut()
                    .add_address(peer_id, addr.clone());
            }
            if j % step == 0 {
                i += step;
            }
        }

        swarms
    }

    #[derive(Clone, Debug)]
    struct Seed([u8; 32]);

    impl Arbitrary for Seed {
        fn arbitrary(g: &mut Gen) -> Seed {
            let seed = core::array::from_fn(|_| u8::arbitrary(g));
            Seed(seed)
        }
    }
}
