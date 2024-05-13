//mod error;
//pub mod network_processor;

pub mod processor;
mod routing;

use crate::Error;
use libp2p::{Multiaddr, PeerId};

const KORE_PROTOCOL: &str = "/kore/1.0.0";

/// Gets the list of known peers.
fn network_access_points(points: &[String]) -> Result<Vec<(PeerId, Multiaddr)>, Error> {
    let mut access_points: Vec<(PeerId, Multiaddr)> = Vec::new();
    for point in points {
        let data: Vec<&str> = point.split("/p2p/").collect();
        if data.len() != 2 {
            return Err(Error::AcessPointError(point.to_string()));
        }
        if let Some(value) = multiaddr(point) {
            if let Ok(id) = data[1].parse::<PeerId>() {
                access_points.push((id, value));
            } else {
                return Err(Error::AcessPointError(format!(
                    "Invalid PeerId conversion: {}",
                    point
                )));
            }
        } else {
            return Err(Error::AcessPointError(format!(
                "Invalid MultiAddress conversion: {}",
                point
            )));
        }
    }
    Ok(access_points)
}

/// Parses a string into a `Multiaddr` if possible.
fn multiaddr(addr: &str) -> Option<Multiaddr> {
    match addr.parse::<Multiaddr>() {
        Ok(a) => Some(a),
        Err(_) => None,
    }
}

/// Gets the list of external (public) addresses for the node from string array.
fn external_addresses(addresses: &[String]) -> Result<Vec<Multiaddr>, Error> {
    let mut external_addresses: Vec<Multiaddr> = Vec::new();
    for address in addresses {
        if let Some(value) = multiaddr(address) {
            external_addresses.push(value);
        } else {
            return Err(Error::AcessPointError(format!(
                "Invalid MultiAddress conversion in External Address: {}",
                address
            )));
        }
    }
    Ok(external_addresses)
}

#[cfg(test)]
mod tests {
    /*
    pub use crate::message::{MessageReceiver, MessageSender, NetworkEvent};
    use crate::network::network_node::{KoreNetworkEvent, NetworkProcessor};
    use crate::{message::Command, ListenAddr};
    use libp2p::kad::RoutingUpdate;
    use log::debug;
    use tests::routing::RoutingBehaviorEvent;
    use tokio_util::sync::CancellationToken;

    use super::*;

    use futures::StreamExt;

    use tokio::{runtime::Runtime, sync::mpsc};

    use libp2p::{
        core::{
            transport::{MemoryTransport, Transport},
            upgrade,
        },
        identity::Keypair,
        kad::{Event as KademliaEvent, PeerRecord, QueryResult, Quorum, Record},
        multiaddr::Protocol,
        multihash::Multihash,
        noise,
        swarm::{Swarm, SwarmEvent},
        yamux, Multiaddr, PeerId,
    };
    use libp2p_swarm_test::SwarmExt;
    use tokio_stream::wrappers::ReceiverStream;

    use crate::commons::crypto::{Ed25519KeyPair, KeyGenerator, KeyMaterial, KeyPair};
    use std::time::Duration;

    const LOG_TARGET: &str = "NETWORK_TEST";

    #[test]
    fn create_network() {
        let rt = Runtime::new().unwrap();

        rt.block_on(async {
            let mc1 = KeyPair::Ed25519(crate::commons::crypto::Ed25519KeyPair::from_seed(
                format!("pepe").as_bytes(),
            ));
            let (sender_boot, receiver_boot) = mpsc::channel(10000);
            // TODO: Could be notificaction_rx removed?
            //let (notification_tx, _notification_rx) = mpsc::channel(1000);
            let token = CancellationToken::new();
            let bootstrap_network = NetworkProcessor::new(
                vec![ListenAddr::try_from(String::from("/memory/647988")).unwrap()],
                vec![],
                mc1,
                token.clone(),
                sender_boot.clone(),
                vec![]
            );
            let msg_sender_boot = bootstrap_network.client();
            let mut msg_rcv_boot = ReceiverStream::new(receiver_boot);

            let bt_pid = bootstrap_network.local_peer_id().clone();
            let mc2 = KeyPair::Ed25519(crate::commons::crypto::Ed25519KeyPair::from_seed(
                format!("paco").as_bytes(),
            ));
            let (sender1, receiver1) = mpsc::channel(10000);
            let node1_network = NetworkProcessor::new(
                vec![ListenAddr::try_from(String::from("/memory/647999")).unwrap()],
                vec![(bt_pid, String::from("/memory/647988").parse().unwrap())],
                mc2,
                token.clone(),
                sender1,
                vec![]
            );
            let msg_sender_1 = node1_network.client();
            let mut msg_rcv_1 = ReceiverStream::new(receiver1);

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        event = msg_rcv_1.next() => match event {
                            Some(NetworkEvent::MessageReceived { message }) => {
                                debug!("{}: Recibido el mensaje: {}.", LOG_TARGET, std::str::from_utf8(&message).unwrap());
                                assert_eq!(std::str::from_utf8(&message).unwrap(), "Hola si");
                                msg_sender_1.send(Command::SendMessage { receptor: mc_bytes_from_seed(&String::from("pepe")), message: "que tal".as_bytes().to_vec() })
                                .await.unwrap();
                            },
                            None => {}
                        },
                    }
                }
            });

            tokio::spawn(async move {
                bootstrap_network.run().await;
            });
            tokio::spawn(async move {
                node1_network.run().await;
            });

            std::thread::sleep(Duration::from_secs(5));
            msg_sender_boot.send(Command::SendMessage { receptor: mc_bytes_from_seed(&String::from("paco")), message: "Hola si".as_bytes().to_vec() }).await.unwrap();

            loop {
                tokio::select! {
                    event = msg_rcv_boot.next() => match event {
                        Some(NetworkEvent::MessageReceived { message }) => {
                            // The message will be a string for now
                            debug!("{}: Recibido el mensaje: {}.", LOG_TARGET, std::str::from_utf8(&message).unwrap());
                            assert_eq!(std::str::from_utf8(&message).unwrap(), "que tal");
                            break;
                        },
                        None => {}
                    },
                }
            }
        })
    }

    #[test]
    fn test_node_behaviour_works_simple() {
        let rt = Runtime::new().unwrap();

        rt.block_on(async {
            let addr_boot: Multiaddr = "/memory/647988".parse().unwrap();
            let mut boot1 = build_swarm(addr_boot.clone(), None).await;
            let addr1: Multiaddr = "/memory/647999".parse().unwrap();
            let mut node1 = build_swarm(
                addr1.clone(),
                Some((boot1.local_peer_id(), addr_boot.clone())),
            )
            .await;
            let addr2: Multiaddr = "/memory/648000".parse().unwrap();
            let mut node2 = build_swarm(
                addr2.clone(),
                Some((boot1.local_peer_id(), addr_boot.clone())),
            )
            .await;

            let boot1_peer_id = boot1.local_peer_id().clone();
            let node1_peer_id = node1.local_peer_id().clone();
            let node2_peer_id = node2.local_peer_id().clone();
            println!("BOOT1: {:?}, {:?}", boot1_peer_id, addr_boot);
            println!("NODE1: {:?}, {:?}", node1_peer_id, addr1);
            println!("NODE2: {:?}, {:?}", node2_peer_id, addr2);

            // Currently we have this drawing n1 -> b1 <- n2 To make the connections between nodes and bootstraps bidirectional and
            // to make them known to each other, either PUT or BOOTSTRAP must be done in both of them
            // Boot 1 loop
            tokio::spawn(async move {
                loop {
                    match boot1.select_next_some().await {
                        SwarmEvent::Dialing {
                            peer_id,
                            connection_id,
                        } => {
                            println!(
                                "{}: Dialing to peer: {:?} with connection_id: {:?}",
                                LOG_TARGET, peer_id, connection_id
                            );
                        }
                        SwarmEvent::Behaviour(KoreNetworkEvent::Tell(tell::Event::Message {
                            peer_id: _,
                            message,
                        })) => {
                            println!("Tell RECEIVED 1");
                            assert_eq!(&message.message, b"Hello Node1!");
                        }
                        SwarmEvent::Behaviour(KoreNetworkEvent::Tell(
                            tell::Event::MessageSent {
                                peer_id,
                                outbound_id: _,
                            },
                        )) => {
                            assert_eq!(node2_peer_id, peer_id);
                        }
                        SwarmEvent::Behaviour(KoreNetworkEvent::Routing(ev)) => match ev {
                            RoutingBehaviorEvent::Kad(KademliaEvent::RoutingUpdated {
                                peer,
                                ..
                            }) => {
                                println!("ROUTING UPDATED B1 {:?}", peer);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            });

            // Node 1 loop
            node1.dial(boot1_peer_id).unwrap();
            tokio::spawn(async move {
                loop {
                    match node1.select_next_some().await {
                        SwarmEvent::Dialing {
                            peer_id,
                            connection_id,
                        } => {
                            println!(
                                "{}: Dialing to peer: {:?} with connection_id: {:?}",
                                LOG_TARGET, peer_id, connection_id
                            );
                        }
                        SwarmEvent::Behaviour(KoreNetworkEvent::Tell(tell::Event::Message {
                            peer_id,
                            message,
                        })) => {
                            println!("Tell RECEIVED 1");
                            assert_eq!(peer_id, node2_peer_id);
                            assert_eq!(&message.message, b"Hello Node1!");
                        }
                        SwarmEvent::Behaviour(KoreNetworkEvent::Tell(
                            tell::Event::MessageSent {
                                peer_id,
                                outbound_id: _,
                            },
                        )) => {
                            println!("TELL SENDED 1");
                            assert_eq!(node2_peer_id, peer_id);
                        }
                        SwarmEvent::Behaviour(KoreNetworkEvent::Routing(ev)) => match ev {
                            RoutingBehaviorEvent::Kad(KademliaEvent::RoutingUpdated {
                                peer,
                                ..
                            }) => {
                                println!("ROUTING UPDATED 1 {:?}", peer);
                                if peer == node2_peer_id {
                                    // Sent tell to node 2
                                    node1
                                        .behaviour_mut()
                                        .send_message(&node2_peer_id, b"Hello Node2!".to_vec());
                                } else if peer == boot1_peer_id {
                                    println!("HACIENDO BOOTSTRAP 1");
                                    //node1.behaviour_mut().bootstrap();
                                }
                            }
                            _ => {
                                //node1.behaviour_mut().h
                            }
                        },
                        _ => {}
                    }
                }
            });

            node2.dial(boot1_peer_id).unwrap();

            // Node 2 loop
            loop {
                match node2.select_next_some().await {
                    SwarmEvent::Dialing {
                        peer_id,
                        connection_id,
                    } => {
                        println!(
                            "{}: Dialing to peer: {:?} with connection_id: {:?}",
                            LOG_TARGET, peer_id, connection_id
                        );
                    }
                    SwarmEvent::Behaviour(KoreNetworkEvent::Tell(tell::Event::Message {
                        peer_id,
                        message,
                    })) => {
                        println!("Tell RECEIVED 2");
                        assert_eq!(peer_id, node1_peer_id);
                        assert_eq!(&message.message, b"Hello Node1!");
                    }
                    SwarmEvent::Behaviour(KoreNetworkEvent::Tell(tell::Event::MessageSent {
                        peer_id,
                        outbound_id: _,
                    })) => {
                        println!("TELL SENDED 2");
                        assert_eq!(node2_peer_id, peer_id);
                    }
                    SwarmEvent::Behaviour(KoreNetworkEvent::Routing(ev)) => match ev {
                        RoutingBehaviorEvent::Kad(KademliaEvent::RoutingUpdated {
                            peer, ..
                        }) => {
                            println!("ROUTING UPDATED 2 {:?}", peer);
                            if peer == node1_peer_id {
                                // Sent tell to node 2
                                node2
                                    .behaviour_mut()
                                    .send_message(&node1_peer_id, b"Hello Node1!".to_vec());
                            } else if peer == boot1_peer_id {
                                println!("HACIENDO BOOTSTRAP 1");
                                //node2.behaviour_mut().bootstrap();
                            }
                        }
                        _ => {
                            //node1.behaviour_mut().h
                        }
                    },
                    _ => {}
                }
            }
        });
    }

    /*
        #[test]
        fn test_node_behaviour_works() {
            let rt = Runtime::new().unwrap();

            rt.block_on(async {
                let (mut boot1, boot_addr1) = build_swarm(None);
                let (mut node1, _addr1) =
                    build_swarm(Some((boot1.local_peer_id(), boot_addr1.clone())));
                let (mut boot2, boot_addr2) = build_swarm(None);
                let (mut node2, _addr2) =
                    build_swarm(Some((boot2.local_peer_id(), boot_addr2.clone())));

                let boot1_peer_id = boot1.local_peer_id().clone();
                let boot2_peer_id = boot2.local_peer_id().clone();
                let node1_peer_id = node1.local_peer_id().clone();
                let node2_peer_id = node2.local_peer_id().clone();

                println!("BOOT1: {:?}, {:?}", boot1_peer_id, boot_addr1);
                println!("BOOT2: {:?}, {:?}", boot2_peer_id, boot_addr2);
                println!("NODE1: {:?}, {:?}", node1_peer_id, _addr1);
                println!("NODE2: {:?}, {:?}", node2_peer_id, _addr2);

                // Communicate bootstrap nodes to share routing table.
                boot1.dial(boot_addr2).unwrap();

                // Currently we have this drawing n1 -> b1 <-> b2 <- n2 In order to make the connections between nodes
                // and bootstraps bidirectional and to make them known to each other, either PUT or BOOTSTRAP must be done in both of them
                // Boot 1 loop
                tokio::spawn(async move {
                    loop {
                        match boot1.select_next_some().await {
                            SwarmEvent::Dialing(peer_id) => {
                                println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Received { peer_id: _, data },
                            )) => {
                                println!("Tell RECEIVED 1");
                                assert_eq!(&data, b"Hello Node1!");
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Sent { peer_id },
                            )) => {
                                assert_eq!(node2_peer_id, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => match ev {
                                routing_old::RoutingComposedEvent::KademliaEvent(
                                    KademliaEvent::RoutingUpdated { peer, .. },
                                ) => {
                                    println!("ROUTING UPDATED B1 {:?}", peer);
                                }
                                _ => {
                                    boot1.behaviour_mut().handle_rout_ev(ev);
                                }
                            },
                            _ => {}
                        }
                    }
                });

                // Boot 2 loop
                tokio::spawn(async move {
                    loop {
                        match boot2.select_next_some().await {
                            SwarmEvent::Dialing(peer_id) => {
                                println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Received { peer_id: _, data },
                            )) => {
                                println!("Tell RECEIVED 1");
                                assert_eq!(&data, b"Hello Node1!");
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Sent { peer_id },
                            )) => {
                                assert_eq!(node2_peer_id, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => match ev {
                                routing_old::RoutingComposedEvent::KademliaEvent(
                                    KademliaEvent::RoutingUpdated { peer, .. },
                                ) => {
                                    println!("ROUTING UPDATED B2 {:?}", peer);
                                }
                                _ => {
                                    boot2.behaviour_mut().handle_rout_ev(ev);
                                }
                            },
                            _ => {}
                        }
                    }
                });

                // Node 1 loop
                tokio::spawn(async move {
                    loop {
                        match node1.select_next_some().await {
                            SwarmEvent::Dialing(peer_id) => {
                                println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Received { peer_id: _, data },
                            )) => {
                                println!("Tell RECEIVED 1");
                                assert_eq!(&data, b"Hello Node1!");
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Sent { peer_id },
                            )) => {
                                assert_eq!(node2_peer_id, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => {
                                match ev {
                                    routing_old::RoutingComposedEvent::KademliaEvent(
                                        KademliaEvent::RoutingUpdated { peer, .. },
                                    ) => {
                                        println!("ROUTING UPDATED 1 {:?}", peer);
                                        if peer == node2_peer_id {
                                            // Sent tell to node 1
                                            node1
                                                .behaviour_mut()
                                                .send_message(&node2_peer_id, b"Hello Node2!");
                                        } else if peer == boot1_peer_id {
                                            println!("HACIENDO BOOTSTRAP 1");
                                            node1.behaviour_mut().bootstrap();
                                        }
                                    }
                                    _ => {
                                        node1.behaviour_mut().handle_rout_ev(ev);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                });

                // Node 2 loop
                loop {
                    match node2.select_next_some().await {
                        SwarmEvent::Dialing(peer_id) => {
                            println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                        }
                        SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                            TellBehaviourEvent::Received { peer_id: _, data },
                        )) => {
                            println!("Tell RECEIVED 2");
                            assert_eq!(&data, b"Hello Node2!");
                            break;
                        }
                        SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                            TellBehaviourEvent::Sent { peer_id },
                        )) => {
                            assert_eq!(node1_peer_id, peer_id);
                        }
                        SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => {
                            match ev {
                                routing_old::RoutingComposedEvent::KademliaEvent(
                                    KademliaEvent::RoutingUpdated { peer, .. },
                                ) => {
                                    println!("ROUTING UPDATED 2 {:?}", peer);
                                    if peer == node1_peer_id {
                                        // Sent tell to node 1
                                        node2
                                            .behaviour_mut()
                                            .send_message(&node1_peer_id, b"Hello Node1!");
                                    } else if peer == boot2_peer_id {
                                        println!("HACIENDO BOOTSTRAP 2");
                                        node2.behaviour_mut().bootstrap();
                                    }
                                }
                                _ => {
                                    node2.behaviour_mut().handle_rout_ev(ev);
                                    node2.behaviour_mut().bootstrap();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            })
        }

        #[test]
        fn test_node_behaviour_with_put_get() {
            let rt = Runtime::new().unwrap();

            rt.block_on(async {
                let (mut boot1, boot_addr1) = build_swarm(None);
                let (mut node1, _addr1) =
                    build_swarm(Some((boot1.local_peer_id(), boot_addr1.clone())));
                let (mut node2, _addr2) =
                    build_swarm(Some((boot1.local_peer_id(), boot_addr1.clone())));

                let boot1_peer_id = boot1.local_peer_id().clone();
                let node1_peer_id = node1.local_peer_id().clone();
                let node2_peer_id = node2.local_peer_id().clone();
                println!("BOOT1: {:?}, {:?}", boot1_peer_id, boot_addr1);
                println!("NODE1: {:?}, {:?}", node1_peer_id, _addr1);
                println!("NODE2: {:?}, {:?}", node2_peer_id, _addr2);

                // Currently we have this drawing n1 -> b1 <- n2 To make the connections between nodes and bootstraps bidirectional and
                // to make them known to each other, either PUT or BOOTSTRAP must be done in both of them
                // Boot 1 loop
                tokio::spawn(async move {
                    loop {
                        match boot1.select_next_some().await {
                            SwarmEvent::NewListenAddr {
                                listener_id: _,
                                address,
                            } => {
                                let peer_id = boot1.local_peer_id().to_owned();
                                match boot1.behaviour_mut().put_record(
                                    Record {
                                        key: Key::new(&peer_id.to_bytes()),
                                        value: address.to_vec(),
                                        publisher: None,
                                        expires: None,
                                    },
                                    Quorum::One,
                                ) {
                                    Ok(_) => (),
                                    Err(_) => (),
                                }
                            }
                            SwarmEvent::Dialing(peer_id) => {
                                println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Received { peer_id: _, data: _ },
                            )) => {
                                println!("Tell RECEIVED 1");
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Sent { peer_id },
                            )) => {
                                assert_eq!(node1_peer_id, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => match ev {
                                routing_old::RoutingComposedEvent::KademliaEvent(
                                    KademliaEvent::RoutingUpdated { peer, .. },
                                ) => {
                                    println!("ROUTING UPDATED B1 {:?}", peer);
                                    if peer == node2_peer_id {
                                    } else if peer == node1_peer_id {
                                        boot1
                                            .behaviour_mut()
                                            .send_message(&node1_peer_id, b"Hello Node1!");
                                    }
                                }
                                _ => {
                                    boot1.behaviour_mut().handle_rout_ev(ev);
                                }
                            },
                            _ => {}
                        }
                    }
                });

                // Node 1 loop
                tokio::spawn(async move {
                    loop {
                        match node1.select_next_some().await {
                            SwarmEvent::NewListenAddr {
                                listener_id: _,
                                address,
                            } => {
                                let peer_id = node1.local_peer_id().to_owned();
                                match node1.behaviour_mut().put_record(
                                    Record {
                                        key: Key::new(&peer_id.to_bytes()),
                                        value: address.to_vec(),
                                        publisher: None,
                                        expires: None,
                                    },
                                    Quorum::One,
                                ) {
                                    Ok(_) => (),
                                    Err(_) => (),
                                }
                            }
                            SwarmEvent::Dialing(peer_id) => {
                                println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Received { peer_id: _, data },
                            )) => {
                                println!("Tell RECEIVED 1");
                                assert_eq!(&data, b"Hello Node1!");
                                println!("HACIENDO GET DESDE NODO1");
                                let key = Key::new(&node2_peer_id.to_bytes());
                                node1.behaviour_mut().get_record(key, Quorum::One);
                                println!("HACIENDO GET DESDE NODO1 FIN");
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                                TellBehaviourEvent::Sent { peer_id },
                            )) => {
                                println!("TELL SENDED 1");
                                assert_eq!(node2_peer_id, peer_id);
                            }
                            SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => {
                                match ev {
                                    routing_old::RoutingComposedEvent::KademliaEvent(
                                        KademliaEvent::RoutingUpdated { peer, .. },
                                    ) => {
                                        println!("ROUTING UPDATED 1 {:?}", peer);
                                        if peer == node2_peer_id {
                                            // Sent tell to node 1
                                            node1
                                                .behaviour_mut()
                                                .send_message(&node2_peer_id, b"Hello Node2!");
                                        } else if peer == boot1_peer_id {
                                            println!("HACIENDO BOOTSTRAP 1");
                                            node1.behaviour_mut().bootstrap();
                                        }
                                    }
                                    RoutingComposedEvent::KademliaEvent(
                                        KademliaEvent::OutboundQueryCompleted { id: _, result, stats: _ },
                                    ) => match result {
                                        QueryResult::GetRecord(Ok(ok)) => {
                                            for PeerRecord {
                                                record:
                                                    Record {
                                                        key,
                                                        value,
                                                        publisher : _,
                                                        ..
                                                    },
                                                ..
                                            } in ok.records
                                            {
                                                println!(
                                                    "Got record {:?} {:?}",
                                                    key,
                                                    value,
                                                );
                                                match PeerId::from_bytes(&key.to_vec()) {
                                                    Ok(peer_id) => {
                                                        match Multiaddr::try_from(value.to_owned()) {
                                                            Ok(addr) => {
                                                                match node1.dial(addr.with(Protocol::P2p(Multihash::from(peer_id)))) {
                                                                    Ok(_) => println!("Success"),
                                                                    Err(e) => println!("{}", e),
                                                                }
                                                            },
                                                            Err(e) => println!("Problemas al recuperar la Multiaddr del value del Record: {:?}", e),
                                                        }
                                                    }
                                                    Err(e) => println!(
                                                        "Problemas al recuperar el peerId de la Key del Record: {:?}",
                                                        e
                                                    ),
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                    _ => {
                                        node1.behaviour_mut().handle_rout_ev(ev);
                                    }
                                }
                            }
                        _ => {}
                    }}
                });

                // Node 2 loop
                loop {
                    match node2.select_next_some().await {
                        SwarmEvent::NewListenAddr {
                            listener_id: _,
                            address,
                        } => {
                            let peer_id = node2.local_peer_id().to_owned();
                            match node2.behaviour_mut().put_record(
                                Record {
                                    key: Key::new(&peer_id.to_bytes()),
                                    value: address.to_vec(),
                                    publisher: None,
                                    expires: None,
                                },
                                Quorum::One,
                            ) {
                                Ok(_) => (),
                                Err(_) => (),
                            }
                        }
                        SwarmEvent::Dialing(peer_id) => {
                            println!("{}: Dialing to peer: {:?}", LOG_TARGET, peer_id);
                        }
                        SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                            TellBehaviourEvent::Received { peer_id: _, data },
                        )) => {
                            println!("Tell RECEIVED 2");
                            assert_eq!(&data, b"Hello Node2!");
                            break;
                        }
                        SwarmEvent::Behaviour(NetworkComposedEvent::TellBehaviourEvent(
                            TellBehaviourEvent::Sent { peer_id },
                        )) => {
                            println!("TELL SENDED 2");
                            assert_eq!(node1_peer_id, peer_id);
                        }
                        SwarmEvent::Behaviour(NetworkComposedEvent::RoutingEvent(ev)) => {
                            match ev {
                                routing_old::RoutingComposedEvent::KademliaEvent(
                                    KademliaEvent::RoutingUpdated { peer, .. },
                                ) => {
                                    println!("ROUTING UPDATED 2 {:?}", peer);
                                    if peer == node1_peer_id {
                                        // Sent tell to node 1
                                        node2
                                            .behaviour_mut()
                                            .send_message(&node1_peer_id, b"Hello Node1!");
                                    } else if peer == boot1_peer_id {
                                        println!("HACIENDO BOOTSTRAP 2");
                                        node2.behaviour_mut().bootstrap();
                                    }
                                }
                                _ => {
                                    node2.behaviour_mut().handle_rout_ev(ev);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            })
        }
    */
    // Build swarm with `TapleNodeBehaviour`
    async fn build_swarm(
        address: Multiaddr,
        boot_node: Option<(&PeerId, Multiaddr)>,
    ) -> Swarm<network_node::Behaviour> {
        let mut bootstrap_nodes = vec![];
        if let Some((peer_id, addr)) = boot_node {
            bootstrap_nodes.push((peer_id.clone(), addr.clone()));
        }

        let mut node = Swarm::new_ephemeral(|key_pair| {
            network_node::Behaviour::new(&key_pair.public(), bootstrap_nodes.clone())
        });

        let _ = node.listen_on(address).unwrap();

        node
    }

    fn mc_bytes_from_seed(seed: &String) -> Vec<u8> {
        Ed25519KeyPair::from_seed(seed.as_bytes()).public_key_bytes()
    }
    */
}
