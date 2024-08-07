use std::collections::HashMap;

//use super::errors::ListenAddrErrors;
use crate::identifier::derive::{digest::DigestDerivator, KeyDerivator};
use config::Value;
use network::Config as NetworkSettings;
use serde::Deserialize;

/// Configuration parameters of a TAPLE node divided into categories.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct Settings {
    pub network: NetworkSettings,
    pub node: NodeSettings,
}

/*
fn default_max_concurrent_streams() -> usize {
    100
}

fn default_message_timeout() -> u64 {
    10
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            listen_addr: vec![ListenAddr::default()],
            known_nodes: Vec::<String>::new(),
            external_address: vec![],
            max_concurrent_streams: 100,
            message_timeout: 10,
        }
    }
}

const DEFAULT_PORT: u32 = 40040;

/// Represents a valid listening address for TAPLE. Internally, they are constituted as a MultiAddr.
#[derive(Debug, Deserialize, Clone)]
pub enum ListenAddr {
    /// Represents in-memory addressing.
    Memory { port: Option<u32> },
    /// Represents an ip4 address
    IP4 {
        addr: Option<std::net::Ipv4Addr>,
        port: Option<u32>,
    },
    /// Represents an ip6 address
    IP6 {
        addr: Option<std::net::Ipv6Addr>,
        port: Option<u32>,
    },
}

impl Default for ListenAddr {
    fn default() -> Self {
        Self::IP4 {
            addr: Some(std::net::Ipv4Addr::new(0, 0, 0, 0)),
            port: Some(DEFAULT_PORT),
        }
    }
}

impl ListenAddr {
    /// Allows to obtain the port of the listening address
    pub fn get_port(&self) -> Option<u32> {
        match self {
            Self::IP4 { port, .. } => *port,
            Self::IP6 { port, .. } => *port,
            Self::Memory { port } => *port,
        }
    }

    /// Allows to increment the port of the listening address by a specified value.
    pub fn increment_port(&mut self, offset: u32) {
        match self {
            Self::IP4 { port, .. } => port.as_mut().map(|p| *p += offset),
            Self::IP6 { port, .. } => port.as_mut().map(|p| *p += offset),
            Self::Memory { port } => port.as_mut().map(|p| *p += offset),
        };
    }

    /// Allows to obtain, as a string, the listening address in MultiAddr format.
    pub fn to_string(&self) -> Result<String, ListenAddrErrors> {
        let result = match self {
            ListenAddr::Memory { port } => {
                let mut result = "/memory".to_owned();
                if let Some(port) = port {
                    result.push_str(&format!("/{}", port));
                }
                result
            }
            ListenAddr::IP4 { addr, port } => {
                let mut result = "/ip4".to_owned();
                if let Some(ip) = addr {
                    result.push_str(&format!(
                        "/{}/tcp/{}",
                        ip,
                        port.ok_or(ListenAddrErrors::InvalidCombination)?
                    ));
                }
                result
            }
            ListenAddr::IP6 { addr, port } => {
                let mut result = "/ip6".to_string();
                if let Some(ip) = addr {
                    result.push_str(&format!(
                        "/{}/tcp/{}",
                        ip,
                        port.ok_or(ListenAddrErrors::InvalidCombination)?
                    ));
                }
                result
            }
        };
        Ok(result)
    }
}

impl TryFrom<String> for ListenAddr {
    type Error = ListenAddrErrors;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut sections = value.split('/');
        // Addr must start with "/"
        let Some(data) = sections.next() else {
            return Err(ListenAddrErrors::InvalidListenAddr);
        };
        if !data.is_empty() {
            return Err(ListenAddrErrors::InvalidListenAddr);
        }
        // The specification of the protocol
        let Some(protocol) = sections.next() else {
            return Err(ListenAddrErrors::InvalidListenAddr);
        };
        match protocol {
            "ip4" => {
                if let Some(ip) = sections.next() {
                    let ip = ip
                        .parse::<std::net::Ipv4Addr>()
                        .map_err(|_| ListenAddrErrors::InvalidIP4)?;
                    // Check TCP
                    let Some(tcp) = sections.next() else {
                        return Err(ListenAddrErrors::NoTransportProtocolSpecified);
                    };
                    if tcp != "tcp" {
                        return Err(ListenAddrErrors::NoTCP);
                    }
                    if let Some(port) = sections.next() {
                        // Port must be u32
                        let port = port
                            .parse::<u32>()
                            .map_err(|_| ListenAddrErrors::NoU32Port)?;
                        Ok(ListenAddr::IP4 {
                            addr: Some(ip),
                            port: Some(port),
                        })
                    } else {
                        Ok(ListenAddr::IP4 {
                            addr: Some(ip),
                            port: None,
                        })
                    }
                } else {
                    Ok(ListenAddr::IP4 {
                        addr: None,
                        port: None,
                    })
                }
            }
            "ip6" => {
                if let Some(ip) = sections.next() {
                    let ip = ip
                        .parse::<std::net::Ipv6Addr>()
                        .map_err(|_| ListenAddrErrors::InvalidIP6)?;
                    // Check TCP
                    let Some(tcp) = sections.next() else {
                        return Err(ListenAddrErrors::NoTransportProtocolSpecified);
                    };
                    if tcp != "tcp" {
                        return Err(ListenAddrErrors::NoTCP);
                    }
                    if let Some(port) = sections.next() {
                        // Port must be u32
                        let port = port
                            .parse::<u32>()
                            .map_err(|_| ListenAddrErrors::NoU32Port)?;
                        Ok(ListenAddr::IP6 {
                            addr: Some(ip),
                            port: Some(port),
                        })
                    } else {
                        Ok(ListenAddr::IP6 {
                            addr: Some(ip),
                            port: None,
                        })
                    }
                } else {
                    Ok(ListenAddr::IP6 {
                        addr: None,
                        port: None,
                    })
                }
            }
            "memory" => {
                // Check for the port
                if let Some(port) = sections.next() {
                    // Port must be u32
                    let port = port
                        .parse::<u32>()
                        .map_err(|_| ListenAddrErrors::NoU32Port)?;
                    Ok(ListenAddr::Memory { port: Some(port) })
                } else {
                    Ok(ListenAddr::Memory { port: None })
                }
            }
            _ => Err(ListenAddrErrors::InvalidProtocolSpecified),
        }
    }
}
*/
#[derive(Debug, Deserialize, Clone)]
pub struct AccessPoint {
    #[serde(rename = "peer-id")]
    pub peer_id: String,
    pub addr: String,
}

/// General settings of a TAPLE node.
#[derive(Debug, Deserialize, Clone)]
pub struct NodeSettings {
    /// [KeyDerivator] to be used by the secret key.
    #[serde(rename = "keyderivator")]
    pub key_derivator: KeyDerivator,
    /// Secret key to be used by the node
    #[serde(rename = "secretkey")]
    pub secret_key: String,
    /// [DigestDerivator] to be used for future event and subject identifiers
    #[serde(rename = "digestderivator")]
    pub digest_derivator: DigestDerivator,
    /// Percentage of network nodes receiving protocol messages in one iteration
    #[serde(rename = "replicationfactor")]
    pub replication_factor: f64,
    /// Timeout to be used between protocol iterations
    pub timeout: u32,
    #[doc(hidden)]
    pub passvotation: u8,
    //#[cfg(feature = "evaluation")]
    pub smartcontracts_directory: String,
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            key_derivator: KeyDerivator::Ed25519,
            secret_key: String::from(""),
            digest_derivator: crate::identifier::derive::digest::DigestDerivator::Blake3_256,
            replication_factor: 0.25f64,
            timeout: 3000u32,
            passvotation: 0,
            #[cfg(feature = "evaluation")]
            smartcontracts_directory: "./contracts".into(),
        }
    }
}

impl From<AccessPoint> for Value {
    fn from(data: AccessPoint) -> Self {
        let mut map = HashMap::new();
        map.entry("peer_id".to_owned())
            .or_insert(Value::new(None, config::ValueKind::String(data.peer_id)));
        map.entry("addr".to_owned())
            .or_insert(Value::new(None, config::ValueKind::String(data.addr)));
        Self::new(None, config::ValueKind::Table(map))
    }
}

pub enum VotationType {
    Normal,
    AlwaysAccept,
    AlwaysReject,
}

impl From<u8> for VotationType {
    fn from(passvotation: u8) -> Self {
        match passvotation {
            2 => Self::AlwaysReject,
            1 => Self::AlwaysAccept,
            _ => Self::Normal,
        }
    }
}
