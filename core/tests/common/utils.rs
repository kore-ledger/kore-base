use kore_base::{
    keys::{KeyMaterial, KeyPair},
    request::{FactRequest, StartRequest},
    signature::{Signature, Signed}, DigestDerivator, DigestIdentifier, EventRequest, KeyIdentifier, ValueWrapper,
};
use libp2p::identity::{ed25519, Keypair};
use libp2p::PeerId;
use serde_json::Value;


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
    node_info: Vec<(KeyIdentifier, String, Option<Role>)>,
    governance_id: DigestIdentifier,
) -> EventRequest {
    let mut operations: Vec<Value> = vec![];

    // Add members
    for (index, (public_key, name, _)) in node_info.iter().enumerate() {
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
    for (index, (_, name, role)) in node_info.iter().enumerate() {
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
                    "ID": "governance"
                },
                "who" : {
                    "NAME": name,
                }

            }
        });
        operations.push(member_value);
    }

    let payload = serde_json::json!({
        "Patch": {
            "data": operations
        }
    });
    // Compact the request
    EventRequest::Fact(FactRequest {
        subject_id: governance_id,
        payload: ValueWrapper(payload),
    })
}