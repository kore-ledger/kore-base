use libp2p::{
    swarm::{dummy, CloseConnection, ConnectionDenied, NetworkBehaviour, ToSwarm},
    Multiaddr, PeerId,
};
use serde::Deserialize;
use std::{
    collections::{HashSet, VecDeque},
    fmt,
    str::FromStr,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::Duration,
};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::RoutingNode;

const TARGET_CONTROL_LIST: &str = "KoreNetwork-Control-list";

/// Configuration for the control list behaviour.
#[derive(Clone, Debug, Deserialize, Default)]
pub struct Config {
    /// Activate allow and block lists
    enable: bool,

    /// Nodes allowed to make and receive connections
    allow_list: Vec<String>,

    /// Nodes that are not allowed to make and receive connections
    block_list: Vec<String>,

    /// Services where the node will go to query the list of allowed nodes.
    service_allow_list: Vec<String>,

    /// Servicse where the node will go to query the list of blocked nodes.
    service_block_list: Vec<String>,

    /// Time interval to be used for queries updating the lists
    interval_request: Duration,
}

/// Control List Settings
impl Config {
    /// Set enable
    pub fn with_enable(&mut self, enable: bool) -> Self {
        self.enable = enable;
        self.clone()
    }

    /// Set allow list
    pub fn with_allow_list(&mut self, allow_list: Vec<String>) -> Self {
        self.allow_list = allow_list;
        self.clone()
    }

    /// Set block list
    pub fn with_block_list(&mut self, block_list: Vec<String>) -> Self {
        self.block_list = block_list;
        self.clone()
    }

    /// Set Service list to consult allow list
    pub fn with_service_allow_list(&mut self, service_allow_list: Vec<String>) -> Self {
        self.service_allow_list = service_allow_list;
        self.clone()
    }

    /// Set Service list to consult block list
    pub fn with_service_block_list(&mut self, service_block_list: Vec<String>) -> Self {
        self.service_block_list = service_block_list;
        self.clone()
    }

    /// Set interval request
    pub fn with_interval_request(&mut self, interval: Duration) -> Self {
        self.interval_request = interval;
        self.clone()
    }

    /// Set interval request
    pub fn get_interval_request(&self) -> Duration {
        self.interval_request
    }

    /// Get enable
    pub fn get_enable(&self) -> bool {
        self.enable
    }

    /// Get allow list
    pub fn get_allow_list(&self) -> Vec<String> {
        self.allow_list.clone()
    }

    /// Get block list
    pub fn get_block_list(&self) -> Vec<String> {
        self.block_list.clone()
    }

    /// Get Service list to consult allow list
    pub fn get_service_allow_list(&self) -> Vec<String> {
        self.service_allow_list.clone()
    }
    /// Get Service list to consult block list
    pub fn get_service_block_list(&self) -> Vec<String> {
        self.service_block_list.clone()
    }
}

#[derive(Default, Debug, Clone)]
pub struct Behaviour {
    allow_peers: Arc<Mutex<HashSet<PeerId>>>,
    block_peers: Arc<Mutex<HashSet<PeerId>>>,
    close_connections: VecDeque<PeerId>,
    waker: Option<Waker>,
    enable: bool,
}

impl Behaviour {
    /// Creates a new control list `Behaviour`.
    pub fn new(config: Config, boot_nodes: &[RoutingNode]) -> Self {
        if config.enable {
            let mut full_allow_list = config.allow_list.clone();
            for node in boot_nodes {
                full_allow_list.push(node.peer_id.clone());
            }

            Self {
                enable: true,
                allow_peers: Arc::new(Mutex::new(HashSet::from_iter(
                    full_allow_list
                        .iter()
                        .filter_map(|e| PeerId::from_str(e).ok()),
                ))),
                block_peers: Arc::new(Mutex::new(HashSet::from_iter(
                    config
                        .block_list
                        .iter()
                        .filter_map(|e| PeerId::from_str(e).ok()),
                ))),
                ..Default::default()
            }
            .spawn_update_lists(
                &config.service_allow_list,
                &config.service_block_list,
                config.interval_request,
            )
        } else {
            Behaviour::default()
        }
    }

    /// Method that launches the tokio spawns that will be in charge of keeping the lists updated
    fn spawn_update_lists(
        &mut self,
        service_allow: &[String],
        service_block: &[String],
        interval: Duration,
    ) -> Self {
        let mut interval = time::interval(interval);
        let service_allow = service_allow.to_vec();
        let service_block = service_block.to_vec();
        let mut clone = self.clone();

        tokio::spawn(async move {
            loop {
                interval.tick().await;

                let ((vec_allow_peers, vec_block_peers), (successful_allow, successful_block)) =
                    Behaviour::request_update_lists(&service_allow, &service_block).await;

                // If at least 1 update of the list was possible
                if successful_allow != 0 {
                    info!(TARGET_CONTROL_LIST, "Updating the allow list.");
                    clone.update_allow_peers(&vec_allow_peers);
                } else {
                    warn!(
                        TARGET_CONTROL_LIST,
                        "No get to the services providing the list of allowed peers was performed."
                    );
                }

                // If at least 1 update of the list was possible
                if successful_block != 0 {
                    info!(TARGET_CONTROL_LIST, "Updating the allow list.");
                    clone.update_block_peers(&vec_block_peers);
                } else {
                    warn!(
                        TARGET_CONTROL_LIST,
                        "No get to the services providing the list of allowed peers was performed."
                    );
                }
            }
        });

        self.clone()
    }

    /// Method that update allow and block lists
    async fn request_update_lists(
        service_allow: &[String],
        service_block: &[String],
    ) -> ((Vec<String>, Vec<String>), (u16, u16)) {
        let mut vec_allow_peers: Vec<String> = vec![];
        let mut vec_block_peers: Vec<String> = vec![];
        let mut successful_allow: u16 = 0;
        let mut successful_block: u16 = 0;
        let client = reqwest::Client::new();

        info!(TARGET_CONTROL_LIST, "Consulting allow lists");
        for service in service_allow {
            debug!(TARGET_CONTROL_LIST, "Consulting {}", service);
            match client.get(service).send().await {
                Ok(res) => {
                    let fail = !res.status().is_success();
                    if !fail {
                        match res.json().await {
                            Ok(peers) => {
                                let peers: Vec<String> = peers;
                                vec_allow_peers.append(&mut peers.clone());
                                successful_allow += 1;
                            }
                            Err(e) => {
                                error!(
                                    TARGET_CONTROL_LIST,
                                    "Error performing Get {}, The server did not return what was expected: {}",
                                    service,
                                    e
                                );
                            }
                        }
                    } else {
                        error!(
                            TARGET_CONTROL_LIST,
                            "Error performing Get {}, The server did not return a correct code: {}",
                            service,
                            res.status()
                        );
                    }
                }
                Err(e) => {
                    error!(
                        TARGET_CONTROL_LIST,
                        "Error performing Get {}: {}", service, e
                    );
                }
            }
        }

        info!(TARGET_CONTROL_LIST, "Consulting block lists");
        for service in service_block {
            debug!(TARGET_CONTROL_LIST, "Consulting {}", service);
            match client.get(service).send().await {
                Ok(res) => {
                    let fail = !res.status().is_success();
                    if !fail {
                        match res.json().await {
                            Ok(peers) => {
                                let peers: Vec<String> = peers;
                                vec_block_peers.append(&mut peers.clone());
                                successful_block += 1;
                            }
                            Err(e) => {
                                error!(
                                    TARGET_CONTROL_LIST,
                                    "Error performing Get {}, The server did not return what was expected: {}",
                                    service,
                                    e
                                );
                            }
                        }
                    } else {
                        error!(
                            TARGET_CONTROL_LIST,
                            "Error performing Get {}, The server did not return a correct code: {}",
                            service,
                            res.status()
                        );
                    }
                }
                Err(e) => {
                    error!(
                        TARGET_CONTROL_LIST,
                        "Error performing Get {}: {}", service, e
                    );
                }
            }
        }

        (
            (vec_allow_peers, vec_block_peers),
            (successful_allow, successful_block),
        )
    }

    /// Method that update allow list
    fn update_allow_peers(&mut self, new_list: &[String]) {
        // New hashset of allow list.
        let new_list: HashSet<PeerId> = HashSet::from_iter(
            new_list
                .to_vec()
                .iter()
                .filter_map(|e| PeerId::from_str(e).ok()),
        );
    
        // Access to state
        if let Ok(mut allow_peer) = self.allow_peers.lock() {
            let close_peers: Vec<PeerId> = allow_peer.difference(&new_list).cloned().collect();

            // Close connections with not allowed peers
            self.close_connections.extend(close_peers);
            // Update new allow list
            allow_peer.clone_from(&new_list);

            if let Some(waker) = self.waker.take() {
                waker.wake()
            }
        } else {
            error!(TARGET_CONTROL_LIST, "Access to allowed nodes state is not possible");
            return;
        };
    }

    /// Method that update block list
    fn update_block_peers(&mut self, new_list: &[String]) {
        // New hashset of block list.
        let new_list: HashSet<PeerId> = HashSet::from_iter(
            new_list
                .to_vec()
                .iter()
                .filter_map(|e| PeerId::from_str(e).ok()),
        );

        // Close connections with new blocked peers
        self.close_connections.extend(new_list.clone());

        // Update new block list
        if let Ok(mut block_peers) = self.block_peers.lock() {
            block_peers.clone_from(&new_list);
        } else {
            error!("Access to blocked nodes state is not possible");
            return;
        };

        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }

    /// Method that check if a peer is in allow list
    fn check_allow(&self, peer: &PeerId) -> Result<(), ConnectionDenied> {
        if let Ok(allow_peers) = self.allow_peers.lock() {
            if allow_peers.contains(peer) {
                return Ok(());
            }
        }

        warn!(TARGET_CONTROL_LIST, "Node {} has been blocked, it is not in the allowed list.", peer);
        return Err(ConnectionDenied::new(NotAllowed { peer: *peer }));
    }

    /// Method that check if a peer is in block list
    fn check_block(&self, peer: &PeerId) -> Result<(), ConnectionDenied> {
        if let Ok(block_peers) = self.block_peers.lock() {
            if !block_peers.contains(peer) {
                return Ok(());
            }
        }

        warn!(TARGET_CONTROL_LIST, "Node {} has been blocked, it is in the blocked list.", peer);
        return Err(ConnectionDenied::new(Blocked { peer: *peer }));
    }

    /// Method that check all List
    fn check_lists(&self, peer: &PeerId) -> Result<(), ConnectionDenied> {
        if self.enable {
            self.check_block(peer)?;
            self.check_allow(peer)?;
        }

        Ok(())
    }
}

/// A connection to this peer is not explicitly allowed and was thus [`denied`](ConnectionDenied).
#[derive(Debug)]
pub struct NotAllowed {
    peer: PeerId,
}

impl fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {} is not in the allow list", self.peer)
    }
}

impl std::error::Error for NotAllowed {}

/// A connection to this peer was explicitly blocked and was thus [`denied`](ConnectionDenied).
#[derive(Debug)]
pub struct Blocked {
    peer: PeerId,
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {} is in the block list", self.peer)
    }
}

impl std::error::Error for Blocked {}

/// Event Struct for implement control list Behaviour in main Behaviour
#[derive(Debug)]
pub enum Event {}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
        self.check_lists(&peer)?;

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        peer: Option<PeerId>,
        _: &[libp2p::Multiaddr],
        _: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        if let Some(peer) = peer {
            self.check_lists(&peer)?;
        }

        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
        self.check_lists(&peer)?;

        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, _: libp2p::swarm::FromSwarm) {
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: libp2p::swarm::ConnectionId,
        _: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        if let Some(peer) = self.close_connections.pop_front() {
            return Poll::Ready(ToSwarm::CloseConnection {
                peer_id: peer,
                connection: CloseConnection::All,
            });
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use libp2p::{
        swarm::{dial_opts::DialOpts, ConnectionError, DialError, ListenError, SwarmEvent},
        Swarm,
    };
    use libp2p_swarm_test::SwarmExt;
    use serial_test::serial;

    use super::*;

    impl Behaviour {
        pub fn block_peer(&mut self, peer: PeerId) {
            self.block_peers.lock().unwrap().insert(peer);
            self.close_connections.push_back(peer);

            if let Some(waker) = self.waker.take() {
                waker.wake()
            }
        }

        pub fn allow_peer(&mut self, peer: PeerId) {
            self.allow_peers.lock().unwrap().insert(peer);
            if let Some(waker) = self.waker.take() {
                waker.wake()
            }
        }
        pub fn set_enable(&mut self, enable: bool) -> Self {
            self.enable = enable;
            self.clone()
        }
    }

    fn dial(dialer: &mut Swarm<Behaviour>, listener: &Swarm<Behaviour>) -> Result<(), DialError> {
        dialer.dial(
            DialOpts::peer_id(*listener.local_peer_id())
                .addresses(listener.external_addresses().cloned().collect())
                .build(),
        )
    }

    #[tokio::test]
    #[serial]
    async fn cannot_dial_blocked_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        listener.listen().with_memory_addr_external().await;

        dialer.behaviour_mut().block_peer(*listener.local_peer_id());

        let DialError::Denied { cause } = dial(&mut dialer, &listener).unwrap_err() else {
            panic!("unexpected dial error")
        };
        assert!(cause.downcast::<Blocked>().is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn cannot_dial_not_allowed_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        listener.listen().with_memory_addr_external().await;

        let DialError::Denied { cause } = dial(&mut dialer, &listener).unwrap_err() else {
            panic!("unexpected dial error")
        };
        assert!(cause.downcast::<NotAllowed>().is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn can_dial_allowed_not_blocked_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));

        listener.listen().with_memory_addr_external().await;

        dialer.behaviour_mut().allow_peer(*listener.local_peer_id());

        dial(&mut dialer, &listener).unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn cannot_dial_allowed_blocked_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        listener.listen().with_memory_addr_external().await;

        dialer.behaviour_mut().block_peer(*listener.local_peer_id());
        dialer.behaviour_mut().allow_peer(*listener.local_peer_id());

        let DialError::Denied { cause } = dial(&mut dialer, &listener).unwrap_err() else {
            panic!("unexpected dial error")
        };
        assert!(cause.downcast::<Blocked>().is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn blocked_peer_cannot_dial_us() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        listener.listen().with_memory_addr_external().await;

        dialer.behaviour_mut().allow_peer(*listener.local_peer_id());
        listener.behaviour_mut().block_peer(*dialer.local_peer_id());

        dial(&mut dialer, &listener).unwrap();
        tokio::spawn(dialer.loop_on_next());

        let cause = listener
            .wait(|e| match e {
                SwarmEvent::IncomingConnectionError {
                    error: ListenError::Denied { cause },
                    ..
                } => Some(cause),
                _ => None,
            })
            .await;
        assert!(cause.downcast::<Blocked>().is_ok());
    }

    #[tokio::test]
    #[serial]
    async fn not_allowed_peer_cannot_dial_us() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        listener.listen().with_memory_addr_external().await;

        dialer.behaviour_mut().allow_peer(*listener.local_peer_id());

        dial(&mut dialer, &listener).unwrap();

        let listener_loop = async move {
            loop {
                match listener.select_next_some().await {
                    SwarmEvent::IncomingConnectionError { error, .. } => {
                        let ListenError::Denied { cause } = error else {
                            panic!("Invalid Error")
                        };
                        assert!(cause.downcast::<NotAllowed>().is_ok());
                        break;
                    }
                    _ => {}
                }
            }
        };

        let dialer_loop = async move {
            loop {
                match dialer.select_next_some().await {
                    SwarmEvent::ConnectionClosed { cause, .. } => {
                        if let Some(error) = cause {
                            match error {
                                ConnectionError::IO(e) => {
                                    assert_eq!(e.to_string(), "Right(Io(Kind(BrokenPipe)))");
                                    break;
                                }
                                _ => {
                                    panic!("Invalid error");
                                }
                            }
                        } else {
                            panic!("Missing error");
                        };
                    }
                    _ => {}
                }
            }
        };
        tokio::task::spawn(Box::pin(dialer_loop));
        listener_loop.await;
    }

    #[tokio::test]
    #[serial]
    async fn connections_get_closed_upon_disallow() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::default().set_enable(true));
        listener.listen().with_memory_addr_external().await;

        dialer.behaviour_mut().allow_peer(*listener.local_peer_id());
        listener.behaviour_mut().allow_peer(*dialer.local_peer_id());
        let dialer_peer = *dialer.local_peer_id();

        dial(&mut dialer, &listener).unwrap();

        let listener_loop = async move {
            loop {
                match listener.select_next_some().await {
                    SwarmEvent::ConnectionEstablished { .. } => {
                        listener.behaviour_mut().block_peer(dialer_peer);
                    }
                    SwarmEvent::ConnectionClosed { .. } => {
                        break;
                    }
                    _ => {}
                }
            }
        };

        let dialer_loop = async move {
            loop {
                match dialer.select_next_some().await {
                    SwarmEvent::ConnectionEstablished { .. } => {}
                    SwarmEvent::ConnectionClosed { cause, .. } => {
                        if let Some(error) = cause {
                            match error {
                                ConnectionError::IO(e) => {
                                    assert_eq!(e.to_string(), "Right(Closed)");
                                    break;
                                }
                                _ => {
                                    panic!("Invalid error");
                                }
                            }
                        } else {
                            panic!("Missing error");
                        };
                    }
                    _ => {}
                }
            }
        };

        tokio::task::spawn(Box::pin(dialer_loop));
        listener_loop.await;
    }
}
