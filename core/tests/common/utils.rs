use kore_base::{
    crypto::{KeyMaterial, KeyPair},
    request::StartRequest,
    signature::{Signature, Signed},
    Api, DigestDerivator, DigestIdentifier, EventRequest, KeyIdentifier, SubjectData,
};
use libp2p::identity::ed25519::Keypair as EdKeyPair;
use libp2p::PeerId;

pub async fn check_subject(
    node_api: &Api,
    subject_id: &DigestIdentifier,
    sn: Option<u64>,
) -> SubjectData {
    let result = node_api.get_subject(subject_id.clone()).await;
    assert!(result.is_ok());
    let subject = result.unwrap();
    assert_eq!(subject.subject_id, *subject_id);
    if let Some(sn) = sn {
        assert_eq!(subject.sn, sn);
    }
    subject
}

pub struct McNodeData {
    keys: KeyPair,
    peer_id: PeerId,
}

impl McNodeData {
    // TODO: remove this function
    #[allow(dead_code)]
    pub fn get_controller_id(&self) -> KeyIdentifier {
        KeyIdentifier::new(self.keys.get_key_derivator(), &self.keys.public_key_bytes())
    }

    // TODO: remove this function
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
    let peer_id = PeerId::from_public_key(
        &libp2p::identity::Keypair::Ed25519(
            EdKeyPair::decode(&mut keys.to_bytes()).expect("Decode of Ed25519 possible"),
        )
        .public(),
    );
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
