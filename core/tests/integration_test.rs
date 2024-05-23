mod common;
use crate::common::{add_members_governance_request, create_governance_request};
use common::{generate_mc, McNodeData, NodeBuilder};
use kore_base::request::RequestState;
use kore_base::{
    keys::{Ed25519KeyPair, KeyGenerator, KeyPair},
    DigestDerivator, DigestIdentifier,
};
use network::{NodeType, RoutingNode};
use serial_test::serial;

use crate::common::Role;
use kore_base::Api;
use kore_base::EventRequest;
use std::collections::HashSet;

fn create_node(
    addr: String,
    node: NodeType,
    vec_routing: Vec<RoutingNode>,
) -> (Api, McNodeData) {
    let kp = KeyPair::Ed25519(Ed25519KeyPair::new());
    let mc_data_node1 = generate_mc(kp.clone());
    println!("MC: {}", mc_data_node1.get_peer_id().to_base58());
    let result = NodeBuilder::new(kp.clone()).build(node, vec_routing, vec![addr.to_string()]);
    assert!(result.is_ok());
    let api = result.unwrap();
    (api, mc_data_node1)
}

async fn create_event_governance(api: Api, mc_data_node: McNodeData) -> DigestIdentifier {
    let public_key = api
        .add_keys(kore_base::KeyDerivator::Ed25519)
        .await
        .expect("MC creation failed");
    let event_request = create_governance_request("", public_key, "");
    let request_id = api
        .external_request(
            mc_data_node.sign_event_request(&event_request, DigestDerivator::Blake3_512),
        )
        .await
        .unwrap();
    let mut res;
    loop {
        res = api.get_request(request_id.clone()).await.unwrap();
        if res.state != RequestState::Finished {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        } else {
            break;
        }
    }
    res.subject_id.unwrap()
}

async fn sign_events(event: EventRequest, api: Api, mc_data_node: McNodeData) {
    let request_id = api
        .external_request(mc_data_node.sign_event_request(&event, DigestDerivator::Blake3_512))
        .await
        .unwrap();
    let mut res;
    loop {
        res = api.get_request(request_id.clone()).await.unwrap();
        match  res.success {
            Some(val) => {
                assert!(val);
                break;
            }
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            
        }
    }
}

async fn add_providers(api: Api, vec_providers: Vec<McNodeData>, subject_id: DigestIdentifier) {
    let mut providers = HashSet::new();
    for vec_provider in vec_providers.iter() {
        providers.insert(vec_provider.get_controller_id());
    }
    api.add_preauthorize_subject(&subject_id.clone(), &providers)
        .await
        .unwrap();
}

async fn create_and_test_nodes(vec_nodes: Vec<(Api, McNodeData)>) {
    let subject_id = create_event_governance(vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
    let mut members = vec![];
    let mut first_iteration = true;
    let mut iteration = 0;

    for node in vec_nodes.iter() {
        if first_iteration {
            members.push((
                node.1.get_controller_id(),
                format!("test{}", iteration),
                None,
            ));
            first_iteration = false;
        } else {
            members.push((
                node.1.get_controller_id(),
                format!("test{}", iteration),
                Option::from(Role::WITNESS),
            ));
        }
        iteration += 1;
    }

    let event_request = add_members_governance_request(members, subject_id.clone());

    sign_events(
        event_request,
        vec_nodes[0].0.clone(),
        vec_nodes[0].1.clone(),
    )
    .await;

    // autorizamos la copia del ledger en el resto de nodos
    for i in 0..vec_nodes.len() {
        add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await
    }

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
    println!("Node Response: {:?}", node_response);
}

async fn create_nodes(
    size_bootstrap: usize,
    size_addresable: usize,
    size_ephermeral: usize,
    initial_port: u32,
) -> [Vec<(Api, McNodeData)>; 3] {
    let mut bootstrap_node: Vec<(Api, McNodeData)> = vec![];
    let mut routing_node: Vec<RoutingNode> = vec![];
    let mut initial_port = initial_port;
    for i in 0..size_bootstrap {

        let addr = format!("/ip4/127.0.0.1/tcp/{:?}", initial_port);
        let node_type = NodeType::Bootstrap;
        if i == 0{
            bootstrap_node.push(create_node(addr.clone(), node_type, vec![]));
        } else {
        bootstrap_node.push(create_node(addr.clone(), node_type, routing_node.clone()));
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
        adressable_node.push(create_node(addr.clone(), node_type, routing_node.clone()));
        initial_port += 1;
    }
    let mut ephemeral_node = vec![];
    for _i in 0..size_ephermeral {
        let addr = format!("/ip4/127.0.0.1/tcp/{:?}", initial_port);
        let node_type = NodeType::Ephemeral;
        ephemeral_node.push(create_node(addr.clone(), node_type, routing_node.clone()));
        initial_port += 1;
    }
    [bootstrap_node, adressable_node, ephemeral_node]
}

#[cfg(test)]
mod test {
    use std::vec;
    use super::*;
    #[tokio::test]
    async fn create_governance() {
        let (api, mc_data_node1) = create_node(
            "/ip4/127.0.0.1/tcp/5000".to_string(),
            NodeType::Bootstrap,
            vec![],
        );
        create_event_governance(api, mc_data_node1).await;
    }   

    #[tokio::test]
    async fn governance_and_member_with_bootstrap_node() {
        let nodes = create_nodes(3, 0, 0, 5001).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_ephemeral() {
        let nodes = create_nodes(1, 0, 1, 5006).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_addressable() {
        let nodes = create_nodes(1, 3, 0, 50011).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes).await;
    } 

    #[tokio::test]
    async fn governance_and_member_with_all_types() {
        let nodes = create_nodes(1, 1, 1, 5014).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes).await;
    } 
}
