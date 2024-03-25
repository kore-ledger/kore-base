// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::Error;
use libp2p::{Multiaddr, PeerId};
use linked_hash_set::LinkedHashSet;

use std::{hash::Hash, num::NonZeroUsize, str::FromStr};

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
            if self.set.len() == usize::from(self.limit) {
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
pub fn external_addresses(addresses: &[String]) -> Result<Vec<Multiaddr>, Error> {
    let mut external_addresses: Vec<Multiaddr> = Vec::new();
    for address in addresses {
        if let Some(value) = multiaddr(address) {
            external_addresses.push(value);
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
}
