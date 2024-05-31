mod common;
use crate::common::VotationType;
use crate::common::{
    add_members_governance_request, create_genesis_event, create_governance_request,
    create_register_event,
};
use common::{generate_mc, McNodeData, NodeBuilder};
use kore_base::request::KoreRequest;
use kore_base::EventRequest;
use kore_base::{
    keys::{Ed25519KeyPair, KeyGenerator, KeyPair},
    DigestDerivator, DigestIdentifier,
};
use kore_base::{Api, ApprovalState};
use network::{NodeType, RoutingNode};
use std::collections::HashSet;

// Create node
fn create_node(
    addr: String,
    node: NodeType,
    vec_routing: Vec<RoutingNode>,
    votation: VotationType,
) -> (NodeBuilder, McNodeData) {
    let key_pair = KeyPair::Ed25519(Ed25519KeyPair::new());
    let mc_data_node1 = generate_mc(key_pair.clone());
    let result = NodeBuilder::build(
        node,
        vec_routing,
        vec![addr.to_string()],
        votation,
        key_pair.clone(),
    )
    .unwrap();
    (result, mc_data_node1)
}

// Create governance and add keys
async fn create_event_governance(node: NodeBuilder, mc_data_node: McNodeData) -> DigestIdentifier {
    let public_key = node
        .api
        .add_keys(kore_base::KeyDerivator::Ed25519)
        .await
        .expect("MC creation failed");
    let event_request = create_governance_request("", public_key, "");
    let request_id = sign_events(event_request, node.clone(), mc_data_node).await;
    let subject_id = get_request_with_votation(request_id, node, VotationType::AlwaysAccept).await;
    subject_id.subject_id.unwrap()
}

// Create genesis event
async fn create_event_genesis(
    node: NodeBuilder,
    mc_data_node: McNodeData,
    gobernance_id: DigestIdentifier,
) -> DigestIdentifier {
    let public_key = node
        .api
        .add_keys(kore_base::KeyDerivator::Ed25519)
        .await
        .expect("MC creation failed");
    let event_request = create_genesis_event(
        gobernance_id,
        "".to_owned(),
        public_key,
        "traceability".to_owned(),
    );
    let request_id = sign_events(event_request, node.clone(), mc_data_node).await;
    let subject_id = get_request_with_votation(request_id, node, VotationType::AlwaysAccept).await;
    subject_id.subject_id.unwrap()
}

// Sign events and verify response
async fn sign_events(
    event: EventRequest,
    node: NodeBuilder,
    mc_data_node: McNodeData,
) -> DigestIdentifier {
    node.api
        .external_request(mc_data_node.sign_event_request(&event, DigestDerivator::Blake3_512))
        .await
        .unwrap()
}

async fn get_request_with_votation(
    request_id: DigestIdentifier,
    node: NodeBuilder,
    votation: VotationType,
) -> KoreRequest {
    let mut res;
    loop {
        res = node.api.get_request(request_id.clone()).await.unwrap();
        match res.success {
            Some(val) => match votation {
                VotationType::AlwaysAccept => {
                    assert_eq!(val, true);
                    break;
                }
                VotationType::AlwaysReject => {
                    assert_eq!(val, false);
                    break;
                }
                VotationType::Normal => {
                    let res = node
                        .api
                        .approval_request(request_id.clone(), true)
                        .await
                        .unwrap();
                    if res.state == ApprovalState::RespondedAccepted {
                        break;
                    } else {
                        break;
                    }
                }
            },
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
    res
}

// Authorize members to governance
async fn add_providers(
    node: NodeBuilder,
    vec_providers: Vec<McNodeData>,
    subject_id: DigestIdentifier,
) {
    let mut providers = HashSet::new();
    for vec_provider in vec_providers.iter() {
        providers.insert(vec_provider.get_controller_id());
    }
    node.api
        .add_preauthorize_subject(&subject_id.clone(), &providers)
        .await
        .unwrap();
}

// Verify that the ledger is copied to all nodes
async fn verify_copy_ledger(
    vec_nodes: Vec<(NodeBuilder, McNodeData)>,
    subject_id: DigestIdentifier,
    sn: Option<u64>,
) {
    let mut pre_response = vec![];
    for i in 0..vec_nodes.len() {
        loop {
            pre_response = vec_nodes[i]
                .0
                .api
                .get_events(subject_id.clone(), None, None)
                .await
                .unwrap();
            if pre_response.len() > 0
                && pre_response[pre_response.len() - 1].content.sn == sn.unwrap_or(0)
            {
                break;
            } else {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }
        }
    }
}

// Deploy many nodes
async fn create_nodes_massive(
    size_bootstrap: usize,
    size_addresable: usize,
    size_ephermeral: usize,
    initial_port: u32,
) -> [Vec<(NodeBuilder, McNodeData)>; 3] {
    let mut bootstrap_node: Vec<(NodeBuilder, McNodeData)> = vec![];
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
            address: vec![format!("/ip4/127.0.0.1/tcp/{:?}", initial_port)],
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

// Create nodes with specific connections
fn create_nodes_and_connections(
    boostrap: Vec<Vec<usize>>,
    addresable: Vec<Vec<usize>>,
    ephemeral: Vec<Vec<usize>>,
    initial_port: u32,
) -> Vec<Vec<(NodeBuilder, McNodeData)>> {
    let mut bootstrap_node: Vec<(NodeBuilder, McNodeData)> = vec![];
    let mut addressable_node: Vec<(NodeBuilder, McNodeData)> = vec![];
    let mut ephemeral_node: Vec<(NodeBuilder, McNodeData)> = vec![];
    let mut routing_node: Vec<RoutingNode> = vec![];
    let mut port = initial_port;
    for i in 0..boostrap.len() {
        if boostrap[i].len() == 0 {
            bootstrap_node.push(create_node(
                format!("/ip4/127.0.0.1/tcp/{:?}", port),
                NodeType::Bootstrap,
                vec![],
                VotationType::AlwaysAccept,
            ));
            port += 1;
        } else {
            // iterate over the bootstrap nodes
            for j in 0..boostrap[i].len() {
                routing_node.push(RoutingNode {
                    peer_id: bootstrap_node[boostrap[i][j]].1.get_peer_id().to_string(),
                    address: vec![bootstrap_node[boostrap[i][j]].0.address[0].clone()],
                });
                // last item
                if j == boostrap[i].len() - 1 {
                    bootstrap_node.push(create_node(
                        format!("/ip4/127.0.0.1/tcp/{:?}", port),
                        NodeType::Bootstrap,
                        routing_node.clone(),
                        VotationType::AlwaysAccept,
                    ));
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
                address: vec![bootstrap_node[addresable[i][j]].0.address[0].clone()],
            });
            if j == addresable[i].len() - 1 {
                addressable_node.push(create_node(
                    format!("/ip4/127.0.0.1/tcp/{:?}", port),
                    NodeType::Addressable,
                    routing_node.clone(),
                    VotationType::AlwaysAccept,
                ));
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
                address: vec![bootstrap_node[ephemeral[i][j]].0.address[0].clone()],
            });
            if j == ephemeral[i].len() - 1 {
                ephemeral_node.push(create_node(
                    format!("/ip4/127.0.0.1/tcp/{:?}", port),
                    NodeType::Ephemeral,
                    routing_node.clone(),
                    VotationType::AlwaysAccept,
                ));
                port += 1;
            }
        }
        routing_node.clear();
    }

    vec![bootstrap_node, addressable_node, ephemeral_node]
}

// Test nodes with governance in diferent scenarios
async fn create_and_test_nodes(vec_nodes: Vec<(NodeBuilder, McNodeData)>, index_governance: usize) {
    // Create governance
    let subject_id = create_event_governance(
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
    )
    .await;
    // Add members to governance
    let request_id = sign_events(
        add_members_governance_request(
            &vec_nodes,
            subject_id.clone(),
            index_governance,
            "traceability".to_owned(),
        ),
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
    )
    .await;
    get_request_with_votation(
        request_id,
        vec_nodes[index_governance].0.clone(),
        VotationType::AlwaysAccept,
    )
    .await;

    // Add providers to governance
    for i in 0..vec_nodes.len() {
        add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await
    }
    // Verify copy ledger
    //verify_copy_ledger(vec_nodes.clone(), subject_id.clone(), None).await;
}

// Test nodes with governance in diferent scenarios
async fn create_and_test_nodes_with_trazabilty(
    vec_nodes: Vec<(NodeBuilder, McNodeData)>,
    index_governance: usize,
    index_emit: usize,
) {
    // Create governance
    let subject_id = create_event_governance(
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
    )
    .await;
    // Add members to governance
    let request_id = sign_events(
        add_members_governance_request(
            &vec_nodes,
            subject_id.clone(),
            index_governance,
            "traceability".to_owned(),
        ),
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
    )
    .await;
    get_request_with_votation(
        request_id,
        vec_nodes[index_governance].0.clone(),
        VotationType::AlwaysAccept,
    )
    .await;
    // Add providers to governance
    for i in 0..vec_nodes.len() {
        add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await
    }
    // Verify copy ledger
    //verify_copy_ledger(vec_nodes.clone(), subject_id.clone(), None).await;

    // generate trazability with add keys and sign
    let trazabilty_subject: DigestIdentifier = create_event_genesis(
        vec_nodes[index_governance].0.clone(),
        vec_nodes[index_governance].1.clone(),
        subject_id.clone(),
    )
    .await;

    let request_id = sign_events(
        create_register_event(trazabilty_subject.clone()),
        vec_nodes[index_emit].0.clone(),
        vec_nodes[index_emit].1.clone(),
    )
    .await;

    get_request_with_votation(
        request_id,
        vec_nodes[index_emit].0.clone(),
        VotationType::AlwaysAccept,
    )
    .await;

    // verify that the ledger is copied to all nodes
    loop {
        let response = vec_nodes[0]
            .0
            .api
            .get_events(trazabilty_subject.clone(), None, None)
            .await
            .unwrap();
        if response.len() > 0 {
            break;
        } else {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        }
    }
}

#[cfg(test)]
mod test {
    use kore_base::{request::FactRequest, ValueWrapper};

    use super::*;
    use std::vec;
    #[tokio::test]
    async fn create_governance_one_node() {
        let (api, mc_data_node1) = create_node(
            "/ip4/127.0.0.1/tcp/4998".to_string(),
            NodeType::Bootstrap,
            vec![],
            VotationType::AlwaysAccept,
        );
        create_event_governance(api, mc_data_node1).await;
    }

    #[tokio::test]
    async fn create_governance_one_node_reject() {
        let (node, mc_data_node1) = create_node(
            "/ip4/127.0.0.1/tcp/4999".to_string(),
            NodeType::Bootstrap,
            vec![],
            VotationType::AlwaysReject,
        );
        let subject_id = create_event_governance(node.clone(), mc_data_node1.clone()).await;
        let request_id = sign_events(
            add_members_governance_request(
                &vec![(node.clone(), mc_data_node1.clone())],
                subject_id.clone(),
                0,
                "traceability".to_owned(),
            ),
            node.clone(),
            mc_data_node1,
        )
        .await;
        get_request_with_votation(request_id, node, VotationType::AlwaysReject).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_bootstraps_node() {
        let nodes = create_nodes_massive(3, 0, 0, 5001).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_ephemeral() {
        let nodes = create_nodes_massive(1, 0, 1, 5004).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_addressable() {
        let nodes = create_nodes_massive(1, 3, 0, 5007).await;
        let mut vec_nodes = vec![];
        vec_nodes.extend(nodes[0].clone());
        vec_nodes.extend(nodes[1].clone());
        vec_nodes.extend(nodes[2].clone());
        create_and_test_nodes(vec_nodes, 0).await;
    }

    #[tokio::test]
    async fn governance_and_member_with_all_types() {
        let nodes = create_nodes_massive(1, 1, 1, 5013).await;
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
        let vec_nodes = nodes
            .into_iter()
            .flatten()
            .collect::<Vec<(NodeBuilder, McNodeData)>>();
        let vec_copy = vec_nodes.clone();
        /*         let first_governance = tokio::spawn(async move {
            create_and_test_nodes(vec_copy,3).await;
        }); */
        create_and_test_nodes(vec_nodes, 3).await;
        //assert!(first_governance.await.is_ok());
    }

    #[tokio::test]
    async fn genesis_event_one_node() {
        let (api, mc_data_node1) = create_node(
            "/ip4/127.0.0.1/tcp/5025".to_string(),
            NodeType::Bootstrap,
            vec![],
            VotationType::AlwaysAccept,
        );

        create_and_test_nodes_with_trazabilty(vec![(api, mc_data_node1)], 0, 0).await;
    }

    #[tokio::test]
    async fn genesis_event_addresable() {
        let nodes = create_nodes_and_connections(vec![vec![]], vec![vec![0]], vec![vec![0]], 5027);
        let vec_nodes = nodes
            .into_iter()
            .flatten()
            .collect::<Vec<(NodeBuilder, McNodeData)>>();
        create_and_test_nodes_with_trazabilty(vec_nodes, 1, 1).await;
    }

    #[tokio::test]
    async fn copy_of_ledger_with_hight_sn() {
        let nodes = create_nodes_massive(1, 2, 0, 5030).await;
        let vec_nodes = nodes
            .into_iter()
            .flatten()
            .collect::<Vec<(NodeBuilder, McNodeData)>>();
        // Create governance
        let subject_id =
            create_event_governance(vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
        // Add members to governance
        let event0 = EventRequest::Fact(FactRequest {
            subject_id: subject_id.clone(),
            payload: ValueWrapper(serde_json::json!(
                { "Patch": {
                    "data": [
                        {
                            "op": "add",
                            "path": "/members/0",
                            "value": {
                                "id": vec_nodes[0].1.get_controller_id(),
                                "name": "KoreNode0"
                            }
                        },
                        {
                            "op": "add",
                            "path": "/members/1",
                            "value": {
                                "id": vec_nodes[1].1.get_controller_id(),
                                "name": "KoreNode1"
                            }
                        },
                        {
                            "op": "add",
                            "path": "/members/2",
                            "value": {
                                "id": vec_nodes[2].1.get_controller_id(),
                                "name": "KoreNode2"
                            }
                        },
                        {
                            "op": "add",
                            "path": "/roles/1",
                            "value": {
                                "namespace": "",
                                "role": "CREATOR",
                                "schema": {
                                    "ID": "traceability"
                                },
                                "who": {
                                    "NAME": "KoreNode0"
                                }
                            }
                        },
                        {
                            "op": "add",
                            "path": "/roles/2",
                            "value": {
                                "namespace": "",
                                "role": "APPROVER",
                                "schema": {
                                    "ID": "governance"
                                },
                                "who": {
                                    "NAME": "KoreNode0"
                                }
                            }
                        },
                        {
                            "op": "add",
                            "path": "/roles/3",
                            "value": {
                                "namespace": "",
                                "role": "APPROVER",
                                "schema": {
                                    "ID": "governance"
                                },
                                "who": {
                                    "NAME": "KoreNode1"
                                }
                            }
                        },
                        {
                            "op": "add",
                            "path": "/policies/1",
                            "value": {
                                "approve": {
                                    "quorum": {
                                        "FIXED": 1
                                    }
                                },
                                "evaluate": {
                                    "quorum": "MAJORITY"
                                },
                                "id": "traceability",
                                "validate": {
                                    "quorum": "MAJORITY"
                                }
                            }
                        },
                        {
                            "op": "add",
                            "path": "/schemas/0",
                            "value": {
                                "contract": {
                                    "raw": "dXNlIHRhcGxlX3NjX3J1c3QgYXMgc2RrOwp1c2Ugc2VyZGU6OntEZXNlcmlhbGl6ZSwgU2VyaWFsaXplfTsKCiNbZGVyaXZlKFNlcmlhbGl6ZSwgRGVzZXJpYWxpemUsIENsb25lKV0Kc3RydWN0IFN0YXRlIHsKICAgIHB1YiB3b3JrX2lkOiBTdHJpbmcsCiAgICBwdWIgdXNlcm5hbWU6IFN0cmluZywKICAgIHB1YiB3b3JrZXJfaWQ6IFN0cmluZywKICAgIHB1YiB3aGF0OiBWZWM8U3RyaW5nPiwKICAgIHB1YiBncm91cF9uYW1lOiBTdHJpbmcsCiAgICBwdWIgYXZlcmFnZV90cmFjZWFiaWxpdHk6IGYzMiwKfQoKCiNbZGVyaXZlKFNlcmlhbGl6ZSwgRGVzZXJpYWxpemUpXQplbnVtIFN0YXRlRXZlbnQgewogICAgUmVnaXN0ZXJPcmRlciB7CiAgICAgICAgd29ya19pZDogU3RyaW5nLAogICAgICAgIHVzZXJuYW1lOiBTdHJpbmcsCiAgICAgICAgd29ya2VyX2lkOiBTdHJpbmcsCiAgICAgICAgd2hhdDogVmVjPFN0cmluZz4sCiAgICAgICAgZ3JvdXBfbmFtZTogU3RyaW5nLAogICAgICAgIGF2ZXJhZ2VfdHJhY2VhYmlsaXR5OiBmMzIsCiAgICB9LAp9CgojW25vX21hbmdsZV0KcHViIHVuc2FmZSBmbiBtYWluX2Z1bmN0aW9uKHN0YXRlX3B0cjogaTMyLCBldmVudF9wdHI6IGkzMiwgaXNfb3duZXI6IGkzMikgLT4gdTMyIHsKICAgIHNkazo6ZXhlY3V0ZV9jb250cmFjdChzdGF0ZV9wdHIsIGV2ZW50X3B0ciwgaXNfb3duZXIsIGNvbnRyYWN0X2xvZ2ljKQp9CmZuIGNvbnRyYWN0X2xvZ2ljKAogICAgY29udGV4dDogJnNkazo6Q29udGV4dDxTdGF0ZSwgU3RhdGVFdmVudD4sCiAgICBjb250cmFjdF9yZXN1bHQ6ICZtdXQgc2RrOjpDb250cmFjdFJlc3VsdDxTdGF0ZT4sCikgewogICAgbGV0IHN0YXRlID0gJm11dCBjb250cmFjdF9yZXN1bHQuZmluYWxfc3RhdGU7CiAgICBtYXRjaCAmY29udGV4dC5ldmVudCB7CiAgICAgICAgU3RhdGVFdmVudDo6UmVnaXN0ZXJPcmRlciB7CiAgICAgICAgICAgIHdvcmtfaWQsCiAgICAgICAgICAgIHVzZXJuYW1lLAogICAgICAgICAgICB3b3JrZXJfaWQsCiAgICAgICAgICAgIHdoYXQsCiAgICAgICAgICAgIGdyb3VwX25hbWUsCiAgICAgICAgICAgIGF2ZXJhZ2VfdHJhY2VhYmlsaXR5LAogICAgICAgIH0gPT4gewogICAgICAgICAgICAgICAgc3RhdGUud29ya19pZCA9IHdvcmtfaWQudG9fc3RyaW5nKCk7CiAgICAgICAgICAgICAgICBzdGF0ZS51c2VybmFtZSA9IHVzZXJuYW1lLnRvX3N0cmluZygpOwogICAgICAgICAgICAgICAgc3RhdGUud29ya2VyX2lkID0gd29ya2VyX2lkLnRvX3N0cmluZygpOwogICAgICAgICAgICAgICAgc3RhdGUud2hhdCA9IHdoYXQudG9fdmVjKCk7CiAgICAgICAgICAgICAgICBzdGF0ZS5ncm91cF9uYW1lID0gZ3JvdXBfbmFtZS50b19zdHJpbmcoKTsKICAgICAgICAgICAgICAgIHN0YXRlLmF2ZXJhZ2VfdHJhY2VhYmlsaXR5ID0gKmF2ZXJhZ2VfdHJhY2VhYmlsaXR5OwoKICAgICAgICAgICAgICAgIGNvbnRyYWN0X3Jlc3VsdC5hcHByb3ZhbF9yZXF1aXJlZCA9IHRydWU7CiAgICAgICAgICAgICAgICBjb250cmFjdF9yZXN1bHQuc3VjY2VzcyA9IHRydWU7CiAgICAgICAgfQogICAgfQp9Cg=="
                                },
                                "id": "traceability",
                                "initial_value": {
                                    "work_id": "",
                                    "username": "",
                                    "worker_id": "",
                                    "what": [],
                                    "group_name": "",
                                    "average_traceability": 0.0
                                },
                                "schema": {
                                            "description": "Traceability registration",
                                            "type": "object",
                                            "properties": {
                                                "work_id": {
                                                    "description": "Work order identifier",
                                                    "type": "string"
                                                },
                                                "username": {
                                                    "description": "Worker username",
                                                    "type": "string"
                                                },
                                                "worker_id": {
                                                    "description": "Worker identifier",
                                                    "type": "string"
                                                },
                                                "what": {
                                                    "description": "Actions performed on the work order",
                                                    "type": "array",
                                                    "items": {
                                                        "type": "string"
                                                    }
                                                },
                                                "group_name": {
                                                    "description": "Group of work order",
                                                    "type": "string"
                                                },
                                                "average_traceability": {
                                                    "description": "Average value of traceability of actions performed on the job",
                                                    "type": "number"
                                                }
                                    },
                                    "required": [
                                        "work_id", "username", "worker_id", "what", "group_name", "average_traceability"
                                    ],
                                    "additionalProperties": false
                                }
                            }
                },

                    ]
                }

                }
            )),
        });
        let request_id = sign_events(event0, vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
        get_request_with_votation(
            request_id,
            vec_nodes[0].0.clone(),
            VotationType::AlwaysAccept,
        )
        .await;
        let event1 = EventRequest::Fact(FactRequest {
            subject_id: subject_id.clone(),
            payload: ValueWrapper(serde_json::json!(
                {         "Patch": {
                    "data": [
                        {
                            "op": "add",
                            "path": "/members/3",
                            "value": {
                                "id": "EnyisBz0lX9sRvvV0H-BXTrVtARjUa0YDHzaxFHWH-N4",
                                "name": "KoreNode3"
                            }
                        },

                    ]
                }

                }
            )),
        });
        sign_events(event1, vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
        for i in 0..vec_nodes.len() {
            add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));

        verify_copy_ledger(vec_nodes, subject_id, Some(2)).await;
    }
}
