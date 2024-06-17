mod common;
use crate::common::VotationType;
use crate::common::{
    add_members_governance_request, add_providers, create_and_test_nodes,
    create_and_test_nodes_with_trazabilty, create_event_governance, create_node,
    create_nodes_and_connections, create_nodes_massive, get_request_with_votation, sign_events,
    verify_copy_ledger,
};
use common::{create_event_genesis, create_register_event, Role};
use common::{McNodeData, NodeBuilder};
use instant::Duration;
use kore_base::EventRequest;
use kore_base::{request::FactRequest, ValueWrapper};
use network::{NodeType, RoutingNode};
use std::vec;
#[test]
fn test_role() {
    let creator = Role::CREATOR;
    let evaluator = Role::EVALUATOR;
    let validator = Role::VALIDATOR;
    let approver = Role::APPROVER;
    let issuer = Role::ISSUER;

    assert!(matches!(creator, Role::CREATOR));
    assert!(matches!(approver, Role::APPROVER));
    assert!(matches!(issuer, Role::ISSUER));
    assert!(matches!(evaluator, Role::EVALUATOR));
    assert!(matches!(validator, Role::VALIDATOR));
}
#[test]
fn test_votation() {
    let normal_vote = VotationType::Normal;
    let accept_vote = VotationType::AlwaysAccept;
    let reject_vote = VotationType::AlwaysReject;

    assert!(matches!(normal_vote, VotationType::Normal));
    assert!(matches!(accept_vote, VotationType::AlwaysAccept));
    assert!(matches!(reject_vote, VotationType::AlwaysReject));
}

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
        VotationType::AlwaysAccept,
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
    get_request_with_votation(request_id, node, VotationType::AlwaysAccept).await;
}

#[tokio::test]
async fn governance_and_member_with_bootstraps_node() {
    let nodes = create_nodes_massive(3, 0, 0, 3000).await;
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
    //let vec_copy = vec_nodes.clone();
    /*         let first_governance = tokio::spawn(async move {
        create_and_test_nodes(vec_copy,3).await;
    }); */
    create_and_test_nodes(vec_nodes, 0).await;
    //assert!(first_governance.await.is_ok());
}

#[tokio::test]
#[ignore]
async fn genesis_event_one_node() {
    let (api, mc_data_node1) = create_node(
        "/ip4/127.0.0.1/tcp/50000".to_string(),
        NodeType::Bootstrap,
        vec![],
        VotationType::AlwaysAccept,
    );

    create_and_test_nodes_with_trazabilty(vec![(api, mc_data_node1)], 0, 0).await;
}

#[tokio::test]
#[ignore]
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
    let nodes = create_nodes_massive(1, 1, 1, 5040).await;
    let vec_nodes = nodes
        .into_iter()
        .flatten()
        .collect::<Vec<(NodeBuilder, McNodeData)>>();
    // Create governance
    let subject_id = create_event_governance(vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
    // Add members to governance with contract and roles
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
                            "role": Role::CREATOR.to_string(),
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
    // Verify de event completer
    let request_id = sign_events(event0, vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
    get_request_with_votation(
        request_id,
        vec_nodes[0].0.clone(),
        VotationType::AlwaysAccept,
    )
    .await;
    // Add providers to governance
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
    // Verify de event completer
    sign_events(event1, vec_nodes[0].0.clone(), vec_nodes[0].1.clone()).await;
    for i in 0..vec_nodes.len() {
        add_providers(vec_nodes[i].0.clone(), vec![], subject_id.clone()).await;
    }
    std::thread::sleep(std::time::Duration::from_secs(3));

    verify_copy_ledger(vec_nodes, subject_id, Some(2)).await;
}

#[tokio::test]
async fn copy_of_ledger_many_events_in_a_short_time() {
    let (api_node1, mc_data_node1) = create_node(
        "/ip4/127.0.0.1/tcp/50000".to_string(),
        NodeType::Bootstrap,
        vec![],
        VotationType::AlwaysAccept,
    );
    let peer_node1 = mc_data_node1.get_peer_id().to_string();

    let (api_node2, mc_data_node2) = create_node(
        "/ip4/127.0.0.1/tcp/50001".to_string(),
        NodeType::Bootstrap,
        vec![RoutingNode {
            address: vec!["/ip4/127.0.0.1/tcp/50000".to_owned()],
            peer_id: peer_node1,
        }],
        VotationType::AlwaysAccept,
    );
    let gov = create_event_governance(api_node1.clone(), mc_data_node1.clone()).await;

    let event0 = EventRequest::Fact(FactRequest {
        subject_id: gov.clone(),
        payload: ValueWrapper(serde_json::json!(
            { "Patch": {
                "data": [
                    {
                        "op": "add",
                        "path": "/members/0",
                        "value": {
                            "id": mc_data_node1.get_controller_id(),
                            "name": "KoreNode0"
                        }
                    },
                    {
                        "op": "add",
                        "path": "/members/1",
                        "value": {
                            "id": mc_data_node2.get_controller_id(),
                            "name": "KoreNode1"
                        }
                    },
                    {
                        "op": "add",
                        "path": "/roles/1",
                        "value": {
                            "namespace": "",
                            "role": Role::CREATOR.to_string(),
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
                            "role": "WITNESS",
                            "schema": {
                                "ID": "traceability"
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
    add_providers(api_node2.clone(), vec![], gov.clone()).await;
    // Verify de event completer
    let _ = sign_events(event0, api_node1.clone(), mc_data_node1.clone()).await;
    let subj = create_event_genesis(api_node1.clone(), mc_data_node1.clone(), gov.clone()).await;

    let events = create_register_event(subj.clone());
    for _ in 0..90 {
        let _ = sign_events(events.clone(), api_node1.clone(), mc_data_node1.clone()).await;    
    }
    verify_copy_ledger(vec![(api_node1.clone(), mc_data_node1.clone())], subj.clone(), Some(40)).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    verify_copy_ledger(vec![(api_node2.clone(), mc_data_node2.clone())], subj.clone(), Some(40)).await;

    for _ in 0..90 {
        let _ = sign_events(events.clone(), api_node1.clone(), mc_data_node1.clone()).await;    
    }
    verify_copy_ledger(vec![(api_node1.clone(), mc_data_node1.clone())], subj.clone(), Some(80)).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    verify_copy_ledger(vec![(api_node2.clone(), mc_data_node2.clone())], subj.clone(), Some(80)).await;
}

#[tokio::test]
async fn create_governance_one_node_a() {
    let (api, mc_data_node1) = create_node(
        "/ip4/127.0.0.1/tcp/4998".to_string(),
        NodeType::Bootstrap,
        vec![],
        VotationType::AlwaysAccept,
    );
    let subject_id = create_event_governance(api.clone(), mc_data_node1).await;

    println!("/////////////////");
    add_providers(api.clone(), vec![], subject_id.clone()).await;
    let a  = api.api.get_all_allowed_subjects_and_providers(Some(format!("{}", subject_id.to_string())), None);
    println!("{:?}",a.await);
}