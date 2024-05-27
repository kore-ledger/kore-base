use kore_base::{
    Api, Error, MemoryManager, Settings,
};
use kore_base::{keys::KeyPair, Node};
use network::{NodeType, RoutingNode};
use prometheus_client::registry::Registry;

pub struct NodeBuilder {
    p2p_port: Option<u32>,
    access_points: Vec<String>,
    pass_votation: Option<u8>,
    key_pair: KeyPair,
}

pub enum VotationType {
    Normal,
    AlwaysAccept,
    AlwaysReject,
}
#[allow(dead_code)]
impl NodeBuilder {
    pub fn new(kp: KeyPair) -> Self {
        Self {
            p2p_port: None,
            access_points: Vec::new(),
            pass_votation: None,
            key_pair: kp,
        }
    }

    pub fn build(self, nodetype:NodeType,boot_nodes: Vec<RoutingNode>, listen_addresses: Vec<String>, votation: VotationType) -> Result<Api, Error> {
        let mut settings = Settings::default();

        // routing network config
        let config_routing = network::RoutingConfig::new(boot_nodes.clone())
        .with_allow_non_globals_in_dht(true)
        .with_allow_private_ip(true)
        .with_discovery_limit(50)
        .with_dht_random_walk(false);
 
        // network config
        settings.network.routing = config_routing;
        settings.network.user_agent = "kore::node".to_owned();
        settings.network.node_type = nodetype;
        settings.network.tell = Default::default();
        settings.network.listen_addresses = listen_addresses;

        // node config
        settings.node.passvotation = votation as u8;
        let path = format!("/tmp/.kore/sc");
        std::fs::create_dir_all(&path).expect("TMP DIR could not be created");
        settings.node.smartcontracts_directory = path;
        let database = MemoryManager::new();
        let mut registry = Registry::default();

        // generate API to send events
        let  api = Node::build(settings, self.key_pair, &mut registry, database)?;
        Ok(api)
    }
}

