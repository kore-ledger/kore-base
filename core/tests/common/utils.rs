use std::collections::HashSet;

use super::{NodeBuilder, VotationType};
use identity::keys::{Ed25519KeyPair, KeyGenerator};
use kore_base::{
    keys::{KeyMaterial, KeyPair},
    request::{FactRequest, KoreRequest, RequestState, StartRequest},
    signature::{Signature, Signed},
    DigestDerivator, DigestIdentifier, EventRequest, KeyIdentifier, ValueWrapper,
};
use libp2p::identity::{ed25519, Keypair};
use libp2p::PeerId;
use network::{NodeType, RoutingNode};
use serde_json::Value;

// Struct to store the node data
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct McNodeData {
    keys: KeyPair,
    peer_id: PeerId,
}
// Enum to define the role
#[cfg(test)]
#[derive(Debug, Clone)]
pub enum Role {
    WITNESS,
    APPROVER,
    EVALUATOR,
    CREATOR,
    ISSUER,
    VALIDATOR,
}

#[cfg(test)]
impl Role {
    // Function to convert the role to string
    pub fn to_string(&self) -> String {
        match self {
            Role::WITNESS => "WITNESS".to_string(),
            Role::APPROVER => "APPROVER".to_string(),
            Role::EVALUATOR => "EVALUATOR".to_string(),
            Role::CREATOR => "CREATOR".to_string(),
            Role::ISSUER => "ISSUER".to_string(),
            Role::VALIDATOR => "VALIDATOR".to_string(),
        }
    }
}

#[cfg(test)]
impl McNodeData {
    #[allow(dead_code)]
    pub fn get_controller_id(&self) -> KeyIdentifier {
        KeyIdentifier::new(self.keys.get_key_derivator(), &self.keys.public_key_bytes())
    }

    #[allow(dead_code)]
    pub fn get_peer_id(&self) -> PeerId {
        self.peer_id.clone()
    }

    pub fn sign_event_request(
        &self,
        content: &EventRequest,
        derivator: DigestDerivator,
    ) -> Signed<EventRequest> {
        Signed {
            content: content.clone(),
            signature: Signature::new(content, &self.keys, derivator).unwrap(),
        }
    }
}
// Function to generate the MC node data
#[cfg(test)]
pub fn generate_mc(keys: KeyPair) -> McNodeData {
    let peer_id = {
        let sk =
            ed25519::SecretKey::try_from_bytes(keys.secret_key_bytes()).expect("Invalid keypair");
        let kp = ed25519::Keypair::from(sk);
        let key_pair = Keypair::from(kp);
        PeerId::from_public_key(&key_pair.public())
    };
    McNodeData { keys, peer_id }
}
// Function to create the governance request
#[cfg(test)]
pub fn create_governance_request(
    namespace: String,
    public_key: KeyIdentifier,
    name: String,
) -> EventRequest {
    EventRequest::Create(StartRequest {
        governance_id: DigestIdentifier::default(),
        schema_id: "governance".to_string(),
        namespace: namespace,
        name: name,
        public_key,
    })
}

// Create event SN1 to add members with role and schema with smart contract
#[cfg(test)]
pub fn add_members_governance_request(
    vec_nodes: &Vec<(NodeBuilder, McNodeData)>,
    governance_id: DigestIdentifier,
    creator_index: usize,
    schema_name: String,
) -> EventRequest {
    let mut operations: Vec<Value> = vec![];
    let mut members = vec![];
    let mut iteration = 0;

    // Create members
    for i in 0..vec_nodes.len() {
        if i == creator_index {
            members.push((
                vec_nodes[i].1.get_controller_id(),
                format!("test{}", iteration),
                Option::from(Role::CREATOR),
            ));
        } else {
            members.push((
                vec_nodes[i].1.get_controller_id(),
                format!("test{}", iteration),
                Option::from(Role::WITNESS),
            ));
        }
        iteration += 1;
    }

    // Add members
    for (index, (public_key, name, _)) in members.iter().enumerate() {
        let member_value = serde_json::json!({
            "op":"add",
            "path": format!("/members/{}", index),
            "value":{
                "id": public_key.to_string(),
                "name": name,
            }
        });
        operations.push(member_value);
    }

    // Add roles
    for (index, (_, name, role)) in members.iter().enumerate() {
        if role.is_none() {
            continue;
        }
        let role_string = role.clone().unwrap().to_string();
        let member_value = serde_json::json!({
            "op":"add",
            "path": format!("/roles/{}", index),
            "value":{
                "namespace": "",
                "role": role_string,
                "schema": {
                    "ID": schema_name.clone()
                },
                "who" : {
                    "NAME": name,
                }

            }
        });
        operations.push(member_value);
    }

    operations.push(serde_json::json!(
        {
            "op":"add",
            "path": format!("/roles/{}", members.len()),
            "value":{
                "namespace": "",
                "role": "ISSUER",
                "schema": {
                    "ID": schema_name.clone()
                },
                "who" : {
                    "NAME": format!("test{}", creator_index),
                }

            }
        }
    ));
    // add policies
    operations.push(serde_json::json!(
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
                "id": schema_name.clone(),
                "validate": {
                    "quorum": "MAJORITY"
                }
            }
        }
    ));
    // add schemas
    operations.push(serde_json::json!(
        {
                "op": "add",
                "path": "/schemas/0",
                "value": {
                    "contract": {
                        "raw": "dXNlIHRhcGxlX3NjX3J1c3QgYXMgc2RrOwp1c2Ugc2VyZGU6OntEZXNlcmlhbGl6ZSwgU2VyaWFsaXplfTsKCiNbZGVyaXZlKFNlcmlhbGl6ZSwgRGVzZXJpYWxpemUsIENsb25lKV0Kc3RydWN0IFN0YXRlIHsKICAgIHB1YiB3b3JrX2lkOiBTdHJpbmcsCiAgICBwdWIgdXNlcm5hbWU6IFN0cmluZywKICAgIHB1YiB3b3JrZXJfaWQ6IFN0cmluZywKICAgIHB1YiB3aGF0OiBWZWM8U3RyaW5nPiwKICAgIHB1YiBncm91cF9uYW1lOiBTdHJpbmcsCiAgICBwdWIgYXZlcmFnZV90cmFjZWFiaWxpdHk6IGYzMiwKfQoKCiNbZGVyaXZlKFNlcmlhbGl6ZSwgRGVzZXJpYWxpemUpXQplbnVtIFN0YXRlRXZlbnQgewogICAgUmVnaXN0ZXJPcmRlciB7CiAgICAgICAgd29ya19pZDogU3RyaW5nLAogICAgICAgIHVzZXJuYW1lOiBTdHJpbmcsCiAgICAgICAgd29ya2VyX2lkOiBTdHJpbmcsCiAgICAgICAgd2hhdDogVmVjPFN0cmluZz4sCiAgICAgICAgZ3JvdXBfbmFtZTogU3RyaW5nLAogICAgICAgIGF2ZXJhZ2VfdHJhY2VhYmlsaXR5OiBmMzIsCiAgICB9LAp9CgojW25vX21hbmdsZV0KcHViIHVuc2FmZSBmbiBtYWluX2Z1bmN0aW9uKHN0YXRlX3B0cjogaTMyLCBldmVudF9wdHI6IGkzMiwgaXNfb3duZXI6IGkzMikgLT4gdTMyIHsKICAgIHNkazo6ZXhlY3V0ZV9jb250cmFjdChzdGF0ZV9wdHIsIGV2ZW50X3B0ciwgaXNfb3duZXIsIGNvbnRyYWN0X2xvZ2ljKQp9CmZuIGNvbnRyYWN0X2xvZ2ljKAogICAgY29udGV4dDogJnNkazo6Q29udGV4dDxTdGF0ZSwgU3RhdGVFdmVudD4sCiAgICBjb250cmFjdF9yZXN1bHQ6ICZtdXQgc2RrOjpDb250cmFjdFJlc3VsdDxTdGF0ZT4sCikgewogICAgbGV0IHN0YXRlID0gJm11dCBjb250cmFjdF9yZXN1bHQuZmluYWxfc3RhdGU7CiAgICBtYXRjaCAmY29udGV4dC5ldmVudCB7CiAgICAgICAgU3RhdGVFdmVudDo6UmVnaXN0ZXJPcmRlciB7CiAgICAgICAgICAgIHdvcmtfaWQsCiAgICAgICAgICAgIHVzZXJuYW1lLAogICAgICAgICAgICB3b3JrZXJfaWQsCiAgICAgICAgICAgIHdoYXQsCiAgICAgICAgICAgIGdyb3VwX25hbWUsCiAgICAgICAgICAgIGF2ZXJhZ2VfdHJhY2VhYmlsaXR5LAogICAgICAgIH0gPT4gewogICAgICAgICAgICAgICAgc3RhdGUud29ya19pZCA9IHdvcmtfaWQudG9fc3RyaW5nKCk7CiAgICAgICAgICAgICAgICBzdGF0ZS51c2VybmFtZSA9IHVzZXJuYW1lLnRvX3N0cmluZygpOwogICAgICAgICAgICAgICAgc3RhdGUud29ya2VyX2lkID0gd29ya2VyX2lkLnRvX3N0cmluZygpOwogICAgICAgICAgICAgICAgc3RhdGUud2hhdCA9IHdoYXQudG9fdmVjKCk7CiAgICAgICAgICAgICAgICBzdGF0ZS5ncm91cF9uYW1lID0gZ3JvdXBfbmFtZS50b19zdHJpbmcoKTsKICAgICAgICAgICAgICAgIHN0YXRlLmF2ZXJhZ2VfdHJhY2VhYmlsaXR5ID0gKmF2ZXJhZ2VfdHJhY2VhYmlsaXR5OwogICAgICAgICAgICAgICAgCiAgICAgICAgICAgICAgICBjb250cmFjdF9yZXN1bHQuc3VjY2VzcyA9IHRydWU7CiAgICAgICAgfQogICAgfQp9"
                    },
                    "id": schema_name.clone(),
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
    }));

    let payload = serde_json::json!({
        "Patch": {
            "data": operations
        }
    });
    println!("Payload: {:?}", payload);
    // Compact the request
    EventRequest::Fact(FactRequest {
        subject_id: governance_id,
        payload: ValueWrapper(payload),
    })
}

// Create event to register new traceability subject
#[cfg(test)]
pub fn create_genesis_event(
    governance_id: DigestIdentifier,
    namespace: String,
    public_key: KeyIdentifier,
    name: String,
) -> EventRequest {
    EventRequest::Create(StartRequest {
        governance_id,
        schema_id: "traceability".into(),
        namespace,
        name,
        public_key,
    })
}

// Register new event for traceability subject
#[cfg(test)]
pub fn create_register_event(subject_id: DigestIdentifier) -> EventRequest {
    EventRequest::Fact(FactRequest {
        subject_id: subject_id,
        payload: ValueWrapper(serde_json::json!(
            {
                "RegisterOrder": {
                    "work_id": "26782378995634",
                    "username": "pepe",
                    "worker_id": "22",
                    "what": ["Limpieza", "Sustitución de piezas"],
                    "group_name": "oe3231",
                    "average_traceability": 2
                }
            }
        )),
    })
}

// Create node
#[cfg(test)]
pub fn create_node(
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

#[cfg(test)]
// Create governance and add keys
pub async fn create_event_governance(node: NodeBuilder, mc_data_node: McNodeData) -> DigestIdentifier {
    let public_key = node
        .api
        .add_keys(kore_base::KeyDerivator::Ed25519)
        .await
        .expect("MC creation failed");
    let event_request = create_governance_request("".to_string(), public_key, "".to_string());
    let request_id = sign_events(event_request, node.clone(), mc_data_node).await;
    let subject_id = get_request_with_votation(request_id, node, VotationType::AlwaysAccept).await;
    subject_id.subject_id.unwrap()
}

// Create genesis event
#[cfg(test)]
pub async fn create_event_genesis(
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
#[cfg(test)]
pub async fn sign_events(
    event: EventRequest,
    node: NodeBuilder,
    mc_data_node: McNodeData,
) -> DigestIdentifier {
    node.api
        .external_request(mc_data_node.sign_event_request(&event, DigestDerivator::Blake3_512))
        .await
        .unwrap()
}

#[cfg(test)]
pub async fn get_request_with_votation(
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
                    if res.state == RequestState::Processing {
                        // obtain the pending requests
                        let res = node.api.get_pending_requests().await.unwrap();
                        // aprove or reject the request
                        for request in res.iter() {
                            node.api
                                .approval_request(request.id.clone(), true)
                                .await
                                .unwrap();
                        }
                    } else {
                        assert!(val == true);
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
#[cfg(test)]
pub async fn add_providers(
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
#[cfg(test)]
pub async fn verify_copy_ledger(
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
            println!(" Response: {:?}", pre_response);
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
#[cfg(test)]
pub async fn create_nodes_massive(
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
#[cfg(test)]
pub fn create_nodes_and_connections(
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
#[cfg(test)]
pub async fn create_and_test_nodes(vec_nodes: Vec<(NodeBuilder, McNodeData)>, index_governance: usize) {
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
    verify_copy_ledger(vec_nodes.clone(), subject_id.clone(), Some(1)).await;
}

// Test nodes with governance in diferent scenarios
#[cfg(test)]
pub async fn create_and_test_nodes_with_trazabilty(
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
    verify_copy_ledger(vec_nodes, subject_id, Some(1)).await;
}
