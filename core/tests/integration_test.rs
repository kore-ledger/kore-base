mod common;
use crate::common::VotationType;
use crate::common::{add_members_governance_request, create_governance_request};
use common::{generate_mc, McNodeData, NodeBuilder};
use kore_base::request::KoreRequest;
use kore_base::Api;
use kore_base::EventRequest;
use kore_base::{
    keys::{Ed25519KeyPair, KeyGenerator, KeyPair},
    DigestDerivator, DigestIdentifier,
};
use network::{NodeType, RoutingNode};
use std::collections::HashSet;

// Create node
fn create_node(
    addr: String,
    node: NodeType,
    vec_routing: Vec<RoutingNode>,
    votation: VotationType,
) -> (Api, McNodeData, String) {
    let kp = KeyPair::Ed25519(Ed25519KeyPair::new());
    let mc_data_node1 = generate_mc(kp.clone());
    let result =
        NodeBuilder::new(kp.clone()).build(node, vec_routing, vec![addr.to_string()], votation);
    assert!(result.is_ok());
    let api = result.unwrap();
    (api, mc_data_node1, addr)
}

// Create governance and add keys
async fn create_event_governance(api: Api, mc_data_node: McNodeData) -> DigestIdentifier {
    let public_key = api
        .add_keys(kore_base::KeyDerivator::Ed25519)
        .await
        .expect("MC creation failed");
    let event_request = create_governance_request("", public_key, "");
    let res = sign_events(event_request, api, mc_data_node).await;
    res.subject_id.unwrap()
}

// Sign events
async fn sign_events(event: EventRequest, api: Api, mc_data_node: McNodeData) -> KoreRequest {
    let request_id = api
        .external_request(mc_data_node.sign_event_request(&event, DigestDerivator::Blake3_512))
        .await
        .unwrap();
    let mut res;
    loop {
        res = api.get_request(request_id.clone()).await.unwrap();
        match res.success {
            Some(val) => {
                assert!(val);
                break;
            }
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
    }
    res
}

// Authorize members to governance
async fn add_providers(api: Api, vec_providers: Vec<McNodeData>, subject_id: DigestIdentifier) {
    let mut providers = HashSet::new();
    for vec_provider in vec_providers.iter() {
        providers.insert(vec_provider.get_controller_id());
    }
    api.add_preauthorize_subject(&subject_id.clone(), &providers)
        .await
        .unwrap();
}

// Verify that the ledger is copied to all nodes
async fn verify_copy_ledger(
    vec_nodes: Vec<(Api, McNodeData, String)>,
    subject_id: DigestIdentifier,
) {
    let mut response = vec![];
    let mut pre_response = vec![];
    let mut node_response = vec![];
    for i in 0..vec_nodes.len() {
        loop {
            pre_response = vec_nodes[i]
                .0
                .get_events(subject_id.clone(), None, None)
                .await
                .unwrap();
            if pre_response.len() > 0 {
                node_response.push(format!(
                    "Node {} - {}",
                    i,
                    vec_nodes[i].1.get_peer_id().to_base58()
                ));
                response = pre_response;
                break;
            } else {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }
        }
    }
    assert!(response.len() > 0);
}

// Test nodes with governance in diferent scenarios
async fn create_and_test_nodes(vec_nodes: Vec<(Api, McNodeData, String)>, index_governance: usize) {
    // Create governance
    let subject_id = create_event_governance(vec_nodes[index_governance].0.clone(), vec_nodes[index_governance].1.clone()).await;
    // Add members to governance
    sign_events(
        add_members_governance_request(&vec_nodes, subject_id.clone()),
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
    )
    .await;
    // Add providers to governance
    for i in 0..vec_nodes.len() {
        add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await
    }
    // Verify copy ledger
    verify_copy_ledger(vec_nodes.clone(), subject_id.clone()).await;
}

// Test nodes with governance in diferent scenarios
async fn create_and_test_nodes_with_trazabilty(vec_nodes: Vec<(Api, McNodeData, String)>, index_governance: usize) {
    // Create governance
    let subject_id = create_event_governance(vec_nodes[index_governance].0.clone(), vec_nodes[index_governance].1.clone()).await;
    // Add members to governance
    sign_events(
        add_members_governance_request(&vec_nodes, subject_id.clone()),
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
    )
    .await;
    // Add providers to governance
    for i in 0..vec_nodes.len() {
        add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await
    }
    // Verify copy ledger
    verify_copy_ledger(vec_nodes.clone(), subject_id.clone()).await;
}

// Deploy nodes
async fn create_nodes_massive(
    size_bootstrap: usize,
    size_addresable: usize,
    size_ephermeral: usize,
    initial_port: u32,
) -> [Vec<(Api, McNodeData, String)>; 3] {
    let mut bootstrap_node: Vec<(Api, McNodeData, String)> = vec![];
    let mut routing_node: Vec<RoutingNode> = vec![];
    let mut initial_port = initial_port;
    for i in 0..size_bootstrap {
        let addr = format!("/ip4/127.0.0.1/tcp/{:?}", initial_port);
        let node_type = NodeType::Bootstrap;
        if i == 0 {
            bootstrap_node.push(create_node(
                addr.clone(),
                node_type,
                vec![],
                VotationType::AlwaysAccept,
            ));
        } else {
            bootstrap_node.push(create_node(
                addr.clone(),
                node_type,
                routing_node.clone(),
                VotationType::AlwaysAccept,
            ));
        }
        routing_node.push(RoutingNode {
            peer_id: bootstrap_node[i].1.get_peer_id().to_string(),
            address: addr.clone(),
        });
        initial_port += 1;
    }
    let mut adressable_node = vec![];
    for _i in 0..size_addresable {
        let addr = format!("/ip4/127.0.0.1/tcp/{:?}", initial_port);
        let node_type = NodeType::Addressable;
        adressable_node.push(create_node(
            addr.clone(),
            node_type,
            routing_node.clone(),
            VotationType::AlwaysAccept,
        ));
        initial_port += 1;
    }
    let mut ephemeral_node = vec![];
    for _i in 0..size_ephermeral {
        let addr = format!("/ip4/127.0.0.1/tcp/{:?}", initial_port);
        let node_type = NodeType::Ephemeral;
        ephemeral_node.push(create_node(
            addr.clone(),
            node_type,
            routing_node.clone(),
            VotationType::AlwaysAccept,
        ));
        initial_port += 1;
    }
    [bootstrap_node, adressable_node, ephemeral_node]
}


fn create_nodes_and_connections(
    boostrap: Vec<Vec<usize>>,
    addresable: Vec<Vec<usize>>,
    ephemeral: Vec<Vec<usize>>,
    initial_port: u32,
) -> Vec<Vec<(Api, McNodeData, String)>>{
    let mut bootstrap_node: Vec<(Api, McNodeData, String)> = vec![];
    let mut addressable_node: Vec<(Api, McNodeData, String)> = vec![];
    let mut ephemeral_node: Vec<(Api, McNodeData, String)> = vec![];
    let mut routing_node: Vec<RoutingNode> = vec![];
    let first_node = true;
    let mut port = initial_port;
    /* [[],[0],[0,1]]*/
    for i in 0..boostrap.len() {
        if boostrap[i].len() == 0 {
            bootstrap_node.push(create_node(
                format!("/ip4/127.0.0.1/tcp/{:?}", port),
                NodeType::Bootstrap,
                vec![],
                VotationType::AlwaysAccept,
            ));
            println!("Boostrap Node node {:?}", routing_node);
            port += 1;
        } else {
            // iterate over the bootstrap nodes
            for j in 0..boostrap[i].len() {
                routing_node.push(RoutingNode {
                    peer_id: bootstrap_node[boostrap[i][j]].1.get_peer_id().to_string(),
                    address: bootstrap_node[boostrap[i][j]].2.clone(),
                });
                // last item
                if j == boostrap[i].len() - 1 {
                    
                    bootstrap_node.push(create_node(
                        format!("/ip4/127.0.0.1/tcp/{:?}", port),
                        NodeType::Bootstrap,
                        routing_node.clone(),
                        VotationType::AlwaysAccept,
                    ));
                    println!("Boostrap node {:?}", routing_node);
                    println!("Node info {:?}", bootstrap_node[bootstrap_node.len()-1].1.get_peer_id().to_string());
                    port += 1;
                }
            }
            routing_node.clear();
        }
    }
    port += 1;
    routing_node.clear();

    for i in 0..addresable.len() {
        for j in 0..addresable[i].len() {
            routing_node.push(RoutingNode {
                peer_id: bootstrap_node[addresable[i][j]].1.get_peer_id().to_string(),
                address: bootstrap_node[addresable[i][j]].2.clone(),
            });
            if j == addresable[i].len() - 1 {
                println!("Addressable node {:?}", routing_node);
                addressable_node.push(create_node(
                    format!("/ip4/127.0.0.1/tcp/{:?}", port),
                    NodeType::Addressable,
                    routing_node.clone(),
                    VotationType::AlwaysAccept,
                ));
                println!("Node info {:?}", addressable_node[addressable_node.len()-1].1.get_peer_id().to_string());
                port += 1;
            }
        }
        routing_node.clear();
    }

    port += 1;
    routing_node.clear();

    for i in 0..ephemeral.len() {
        for j in 0..ephemeral[i].len() {
            routing_node.push(RoutingNode {
                peer_id: bootstrap_node[ephemeral[i][j]].1.get_peer_id().to_string(),
                address: bootstrap_node[ephemeral[i][j]].2.clone(),
            });
            if j == ephemeral[i].len() - 1 {
                println!("Ephemeral node {:?}", routing_node);
                ephemeral_node.push(create_node(
                    format!("/ip4/127.0.0.1/tcp/{:?}", port),
                    NodeType::Ephemeral,
                    routing_node.clone(),
                    VotationType::AlwaysAccept,
                ));
                println!("Node info {:?}", ephemeral_node[ephemeral_node.len()-1].1.get_peer_id().to_string());
                port += 1;
            }
        }
        routing_node.clear();
    }

    vec![bootstrap_node, addressable_node, ephemeral_node]

}

#[cfg(test)]
mod test {
    use super::*;
    use std::vec;
    #[tokio::test]
    async fn create_governance() {
        let (api, mc_data_node1, _) = create_node(
            "/ip4/127.0.0.1/tcp/5000".to_string(),
            NodeType::Bootstrap,
            vec![],
            VotationType::Normal,
        );
        create_event_governance(api, mc_data_node1).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_bootstrap_node() {
        let nodes = create_nodes_massive(3, 0, 0, 5001).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_ephemeral() {
        let nodes = create_nodes_massive(1, 0, 1, 5006).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_addressable() {
        let nodes = create_nodes_massive(1, 3, 0, 50011).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_all_types() {
        let nodes = create_nodes_massive(1, 1, 1, 5014).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    } 

    #[tokio::test]
    async fn addresable_get_governance() {
        let nodes = create_nodes_and_connections(
            vec![vec![], vec![0], vec![0, 1]],
            vec![vec![0], vec![0]],
            vec![vec![0]],
            5017,
        );
        let vec_nodes = nodes.into_iter().flatten().collect::<Vec<(Api, McNodeData, String)>>();
        let vec_copy = vec_nodes.clone();
        let first_governance = tokio::spawn(async move {
            create_and_test_nodes(vec_copy,3).await;
            
        });
        create_and_test_nodes(vec_nodes,0).await;
        assert!(first_governance.await.is_ok());
    }
}
