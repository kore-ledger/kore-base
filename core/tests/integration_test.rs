mod common;
use common::{generate_mc, NodeBuilder};
use kore_base::{
    crypto::{Ed25519KeyPair, KeyGenerator, KeyPair},
    DigestDerivator,
};

use serial_test::serial;

use crate::common::{check_subject, create_governance_request};

#[test]
#[serial]
fn init_node() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        std::env::set_var("RUST_LOG", "info");
        let kp = KeyPair::Ed25519(Ed25519KeyPair::new());
        let result = NodeBuilder::new(kp).build();
        assert!(result.is_ok());
        result.unwrap().shutdown().await;
    });
}

#[test]
#[serial]
fn create_governance() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let kp = KeyPair::Ed25519(Ed25519KeyPair::new());
        let mc_data_node1 = generate_mc(kp.clone());
        let result = NodeBuilder::new(kp).build();
        assert!(result.is_ok());
        let mut node = result.unwrap();
        let node_api = node.get_api();
        let public_key = node_api
            .add_keys(kore_base::KeyDerivator::Ed25519)
            .await
            .expect("MC creation failed");
        let event_request = create_governance_request("", public_key, "");
        assert!(node_api
            .external_request(
                mc_data_node1.sign_event_request(&event_request, DigestDerivator::Blake3_512)
            )
            .await
            .is_ok());
        // Wait for the subject creation notification
        let result = node.wait_for_new_subject().await;
        assert!(result.is_ok());
        let subject_id = result.unwrap();
        // Check the subject asking the api about it
        check_subject(&node_api, &subject_id, Some(0)).await;
        node.shutdown().await
    });
}
