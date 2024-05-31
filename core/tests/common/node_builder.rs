use kore_base::{
    Api, Error, MemoryManager, Settings,
};
use kore_base::{keys::KeyPair, Node};
use network::{NodeType, RoutingNode};
use prometheus_client::registry::Registry;

#[derive(Clone, Debug)]
pub struct NodeBuilder {
    pub key_pair: KeyPair,
    pub address: Vec<String>,
    pub votation: VotationType,
    pub api: Api,
}

#[derive(Clone, Debug)]
pub enum VotationType {
    Normal,
    AlwaysAccept,
    AlwaysReject,
}
#[allow(dead_code)]
impl NodeBuilder {
    pub fn build(nodetype:NodeType,boot_nodes: Vec<RoutingNode>, listen_addresses: Vec<String>, votation: VotationType, key_pair: KeyPair) -> Result<Self, Error>{
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
        settings.network.listen_addresses = listen_addresses.clone();

        // node config
        settings.node.passvotation = votation.clone() as u8;
        let path = format!("/tmp/.kore/sc");
        std::fs::create_dir_all(&path).expect("TMP DIR could not be created");
        settings.node.smartcontracts_directory = path;
        let database = MemoryManager::new();
        let mut registry = Registry::default();

        // generate API to send events
        let  api = Node::build(settings, key_pair.clone(), &mut registry, database)?;

        Ok(Self {
            key_pair,
            address: listen_addresses,
            votation,
            api,
        })
        
    }
}

