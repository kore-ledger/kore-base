use std::mem;

use kore_base::{
    keys::{KeyMaterial, KeyPair},
    request::{FactRequest, StartRequest},
    signature::{Signature, Signed},
    Api, DigestDerivator, DigestIdentifier, EventRequest, KeyIdentifier, ValueWrapper,
};
use libp2p::identity::{ed25519, Keypair};
use libp2p::PeerId;
use rand::seq::IteratorRandom;
use serde_json::Value;
use sha2::digest::generic_array::iter;

#[derive(Debug, Clone)]
pub struct McNodeData {
    keys: KeyPair,
    peer_id: PeerId,
}
#[derive(Debug, Clone)]
pub enum Role {
    WITNESS,
    APPROVER,
    EVALUATOR,
    CREATOR,
    ISSUER,
    VALIDATOR,
}

impl Role {
    fn try_from(&self) -> Result<String, String> {
        match self {
            Role::WITNESS => Ok("WITNESS".to_string()),
            Role::APPROVER => Ok("APPROVER".to_string()),
            Role::EVALUATOR => Ok("EVALUATOR".to_string()),
            Role::CREATOR => Ok("CREATOR".to_string()),
            Role::ISSUER => Ok("ISSUER".to_string()),
            Role::VALIDATOR => Ok("VALIDATOR".to_string()),
        }
    }
}

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

pub fn create_governance_request<S: Into<String>>(
    namespace: S,
    public_key: KeyIdentifier,
    name: S,
) -> EventRequest {
    EventRequest::Create(StartRequest {
        governance_id: DigestIdentifier::default(),
        schema_id: "governance".into(),
        namespace: namespace.into(),
        name: name.into(),
        public_key,
    })
}

pub fn add_members_governance_request(
    vec_nodes: &Vec<(Api, McNodeData, String)>,
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
                Option::from(Role::ISSUER),
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
        let role_string = role.clone().unwrap().try_from().unwrap();
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
