// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Cache for peers in the "tell" protocol.
//! This is a simple LRU cache that stores the last N peers that we have
//! communicated with.
//!

use libp2p::{
    swarm::{DialError, DialFailure, FromSwarm, NewExternalAddrCandidate, NewListenAddr},
    Multiaddr, PeerId,
};
use lru::LruCache;

use std::num::NonZeroUsize;

/// Cache for peers external addresses in the "tell" protocol.
#[derive(Debug)]
pub struct PeerAddresses(LruCache<PeerId, LruCache<Multiaddr, ()>>);

impl PeerAddresses {
    /// Creates a new cache with the given capacity.
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self(LruCache::new(capacity))
    }

    /// Feed a [`FromSwarm`] event to this struct.
    ///
    /// Returns whether the event changed peer's known external addresses.
    pub fn on_swarm_event(&mut self, event: &FromSwarm) -> bool {
        match event {
            FromSwarm::NewListenAddr(NewListenAddr {
                listener_id: _,
                addr,
            })
            | FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate { addr }) => {
                match peer_id_from_addr(addr) {
                    Some(peer_id) => self.add(peer_id, (*addr).clone()),
                    None => false,
                }
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id: Some(peer_id),
                error: DialError::Transport(errors),
                ..
            }) => {
                for (addr, _error) in errors {
                    self.remove(peer_id, addr);
                }
                true
            }
            _ => false,
        }
    }

    /// Adds an address to the cache of a peer.
    pub fn add(&mut self, peer: PeerId, addr: Multiaddr) -> bool {
        match prepare_addr(&peer, &addr) {
            Ok(address) => {
                if let Some(cached) = self.0.get_mut(&peer) {
                    cached.put(address, ()).is_none()
                } else {
                    let mut set = LruCache::new(NonZeroUsize::new(10).expect("10 > 0"));
                    set.put(address, ());
                    self.0.put(peer, set);

                    true
                }
            }
            Err(_) => false,
        }
    }

    /// Returns peer's external addresses.
    pub fn get(&mut self, peer: &PeerId) -> impl Iterator<Item = Multiaddr> + '_ {
        self.0
            .get(peer)
            .into_iter()
            .flat_map(|c| c.iter().map(|(m, ())| m))
            .cloned()
    }

    /// Removes address from peer addresses cache.
    /// Returns true if the address was removed.
    pub fn remove(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        match self.0.get_mut(peer) {
            Some(addrs) => match prepare_addr(peer, address) {
                Ok(address) => addrs.pop(&address).is_some(),
                Err(_) => false,
            },
            None => false,
        }
    }
}

impl Default for PeerAddresses {
    fn default() -> Self {
        PeerAddresses::new(NonZeroUsize::new(100).unwrap())
    }
}

/// Appends the given PeerId if not yet present at the end of this multiaddress.Fails if this
/// address ends in a different PeerId. In that case, the original, unmodified address is
/// returned.
fn prepare_addr(peer: &PeerId, addr: &Multiaddr) -> Result<Multiaddr, Multiaddr> {
    addr.clone().with_p2p(*peer)
}

/// Returns the PeerId from the given multiaddress, if present.
fn peer_id_from_addr(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| {
        if let libp2p::core::multiaddr::Protocol::P2p(peer_id) = p {
            Some(peer_id)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use libp2p::{
        core::multiaddr::Protocol,
        swarm::{FromSwarm, NewExternalAddrCandidate},
        PeerId,
    };

    use once_cell::sync::Lazy;

    #[test]
    fn new_peer_addr_returns_correct_changed_value() {
        let mut cache = PeerAddresses::default();

        let event = new_external_addr_of_peer1();

        let changed = cache.on_swarm_event(&event);
        assert!(changed);

        let changed = cache.on_swarm_event(&event);
        assert!(!changed);
    }

    fn new_external_addr_of_peer1() -> FromSwarm<'static> {
        FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate {
            addr: &MEMORY_ADDR_1000,
        })
    }

    static MEMORY_ADDR_1000: Lazy<Multiaddr> = {
        Lazy::new(|| {
            let addr = Multiaddr::empty().with(Protocol::Memory(1000));
            match addr.with_p2p(PeerId::random()) {
                Ok(addr) => addr,
                Err(addr) => addr,
            }
        })
    };

    /*static MEMORY_ADDR_2000: Lazy<Multiaddr> = {
        Lazy::new(|| {
            let addr = Multiaddr::empty().with(Protocol::Memory(2000));
            match addr.with_p2p(PeerId::random()) {
                Ok(addr) => addr,
                Err(addr) => addr,
            }
        })
    };*/
}
