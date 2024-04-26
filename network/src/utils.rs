// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::Error;
use ip_network::IpNetwork;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use linked_hash_set::LinkedHashSet;

use std::{collections::HashSet, hash::Hash, num::NonZeroUsize, str::FromStr};

/// Wrapper around `LinkedHashSet` with bounded growth.
///
/// In the limit, for each element inserted the oldest existing element will be removed.
#[derive(Debug, Clone)]
pub struct LruHashSet<T: Hash + Eq> {
    set: LinkedHashSet<T>,
    limit: NonZeroUsize,
}

impl<T: Hash + Eq> LruHashSet<T> {
    /// Create a new `LruHashSet` with the given (exclusive) limit.
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            set: LinkedHashSet::new(),
            limit,
        }
    }

    /// Insert element into the set.
    ///
    /// Returns `true` if this is a new element to the set, `false` otherwise.
    /// Maintains the limit of the set by removing the oldest entry if necessary.
    /// Inserting the same element will update its LRU position.
    pub fn insert(&mut self, e: T) -> bool {
        if self.set.insert(e) {
            if self.len() == usize::from(self.limit) {
                self.set.pop_front(); // remove oldest entry
            }
            return true;
        }
        false
    }

    /// `LruHashSet` length.
    pub fn len(&self) -> usize {
        self.set.len()
    }
}

/// Convert boot nodes to `PeerId` and `Multiaddr`.
pub fn convert_boot_nodes(boot_nodes: Vec<(String, String)>) -> Vec<(PeerId, Multiaddr)> {
    boot_nodes
        .iter()
        .map(|(peer_id, addr)| {
            let peer = match bs58::decode(peer_id).into_vec() {
                Ok(peer) => match PeerId::from_bytes(peer.as_slice()) {
                    Ok(peer) => Some(peer),
                    Err(_) => None,
                },
                Err(_) => None,
            };
            let addr = match Multiaddr::from_str(addr) {
                Ok(addr) => Some(addr),
                Err(_) => None,
            };
            (peer, addr)
        })
        .filter(|(peer_id, addr)| peer_id.is_some() && addr.is_some())
        .map(|(peer_id, addr)| (peer_id.unwrap(), addr.unwrap()))
        .collect::<Vec<_>>()
}

/// Gets the list of external (public) addresses for the node from string array.
pub fn convert_external_addresses(addresses: &[String]) -> Result<HashSet<Multiaddr>, Error> {
    let mut external_addresses = HashSet::new();
    for address in addresses {
        if let Some(value) = multiaddr(address) {
            external_addresses.insert(value);
        } else {
            return Err(Error::Address(format!(
                "Invalid MultiAddress conversion in External Address: {}",
                address
            )));
        }
    }
    Ok(external_addresses)
}

/// Parses a string into a `Multiaddr` if possible.
fn multiaddr(addr: &str) -> Option<Multiaddr> {
    match addr.parse::<Multiaddr>() {
        Ok(a) => Some(a),
        Err(_) => None,
    }
}

/// Check if the given `Multiaddr` is reachable.
///
/// This test is successful only for global IP addresses and DNS names.
// NB: Currently all DNS names are allowed and no check for TLD suffixes is done
// because the set of valid domains is highly dynamic and would require frequent
// updates, for example by utilising publicsuffix.org or IANA.
pub fn is_reachable(addr: &Multiaddr) -> bool {
    let ip = match addr.iter().next() {
        Some(Protocol::Ip4(ip)) => IpNetwork::from(ip),
        Some(Protocol::Ip6(ip)) => IpNetwork::from(ip),
        Some(Protocol::Dns(_)) | Some(Protocol::Dns4(_)) | Some(Protocol::Dns6(_)) => return true,
        _ => return false,
    };
    ip.is_global()
}

/// Chech if the given `Multiaddr` is a memory address.
pub fn _is_memory(addr: &Multiaddr) -> bool {
    if let Some(Protocol::Memory(_)) = addr.iter().next() {
        return true;
    }
    false
}

/// Check if the given `Multiaddr` is a relay circuit address.
///
/// A relay circuit address is a `Multiaddr` that contains a `P2pCircuit` protocol.
///
pub fn is_relay_circuit(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
}

/// Compare generic arrays.
///
/// If `b_subset` is `true`, then `b` is a subset of `a`.
/// Otherwise, `a` and `b` are equal.
///
pub fn _compare_arrays<T>(a: &[T], b: &[T], b_subset: bool) -> bool
where
    T: Eq + Hash,
{
    let a: HashSet<_> = a.iter().collect();
    let b: HashSet<_> = b.iter().collect();
    if b_subset {
        b.is_subset(&a)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_lru_hash_set() {
        let mut cache = LruHashSet::new(NonZeroUsize::new(2).unwrap());
        cache.insert("value1");
        assert_eq!(cache.len(), 1);
        cache.insert("value2");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_compare_arrays() {
        let a = vec![1, 2, 3];
        let b = vec![2, 1, 3];
        let c = vec![1, 2, 3, 4];
        let d = vec![1, 2];
        let e = vec![1, 2, 3, 4, 5];
        assert!(_compare_arrays(&a, &b, false));
        assert!(!_compare_arrays(&a, &c, false));
        assert!(_compare_arrays(&a, &d, true));
        assert!(!_compare_arrays(&a, &e, true));
        assert!(_compare_arrays(&e, &a, true));
    }
}
