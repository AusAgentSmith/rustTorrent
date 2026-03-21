//! BEP 16: Super-seeding implementation.
//!
//! In super-seed mode, the seeder pretends to have no pieces and selectively
//! reveals one piece at a time to each peer via HAVE messages. A new piece is
//! only revealed after the previously-offered piece has been observed at another
//! peer (confirming propagation).

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
};

use parking_lot::RwLock;
use tracing::debug;

use crate::type_aliases::BF;

/// Tracks per-peer state for super-seeding.
#[derive(Debug, Clone)]
struct PeerSuperSeedState {
    /// The raw piece index we last offered to this peer (via HAVE).
    last_offered_piece: Option<u32>,
    /// Whether the peer's last offered piece has been confirmed as
    /// propagated (seen at another peer).
    piece_propagated: bool,
}

/// Global super-seed state shared across all peers for one torrent.
pub(crate) struct SuperSeedState {
    enabled: AtomicBool,
    inner: RwLock<SuperSeedInner>,
}

struct SuperSeedInner {
    /// Per-peer tracking.
    peers: HashMap<SocketAddr, PeerSuperSeedState>,
    /// How many distinct peers have received each piece via super-seed HAVE.
    /// This is used to prefer undistributed pieces.
    piece_distribution_count: Vec<u32>,
}

impl SuperSeedState {
    pub fn new(total_pieces: u32, enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            inner: RwLock::new(SuperSeedInner {
                peers: HashMap::new(),
                piece_distribution_count: vec![0; total_pieces as usize],
            }),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            // Clear per-peer state when disabling so we start fresh if re-enabled.
            let mut inner = self.inner.write();
            inner.peers.clear();
        }
    }

    /// Called when a new peer connects. Initializes per-peer tracking.
    pub fn on_peer_connected(&self, addr: SocketAddr) {
        if !self.is_enabled() {
            return;
        }
        let mut inner = self.inner.write();
        inner.peers.insert(
            addr,
            PeerSuperSeedState {
                last_offered_piece: None,
                piece_propagated: true, // initially true so the first piece is offered immediately
            },
        );
    }

    /// Called when a peer disconnects.
    pub fn on_peer_disconnected(&self, addr: SocketAddr) {
        let mut inner = self.inner.write();
        inner.peers.remove(&addr);
    }

    /// Called when we see a HAVE message from a peer for a given piece.
    /// This is how we detect that a piece has propagated -- when a *different*
    /// peer than the one we offered it to reports having it.
    ///
    /// Returns the list of peer addresses whose propagation was just confirmed,
    /// so the caller can send them new super-seed HAVEs.
    pub fn on_have_received(&self, from_addr: SocketAddr, piece_index: u32) -> Vec<SocketAddr> {
        if !self.is_enabled() {
            return Vec::new();
        }
        let mut inner = self.inner.write();
        let mut confirmed_peers = Vec::new();

        // Check if any *other* peer had this piece as their last_offered_piece
        // and hasn't yet been confirmed. If so, mark them as propagated.
        for (addr, state) in inner.peers.iter_mut() {
            if *addr == from_addr {
                continue;
            }
            if state.last_offered_piece == Some(piece_index) && !state.piece_propagated {
                debug!(
                    peer = %addr,
                    piece = piece_index,
                    confirmed_by = %from_addr,
                    "super-seed: piece propagation confirmed"
                );
                state.piece_propagated = true;
                confirmed_peers.push(*addr);
            }
        }

        confirmed_peers
    }

    /// Select the next piece to reveal to this peer. Returns the raw piece
    /// index, or `None` if we should not reveal a new piece yet (the previous
    /// one hasn't propagated, or no suitable piece exists).
    ///
    /// `peer_bitfield`: the remote peer's bitfield (what they already have).
    /// `our_bitfield`: our own bitfield (what we have -- should be all 1s for a seeder).
    pub fn select_piece_for_peer(
        &self,
        addr: SocketAddr,
        peer_bitfield: &BF,
        our_bitfield: &BF,
    ) -> Option<u32> {
        if !self.is_enabled() {
            return None;
        }
        let mut inner = self.inner.write();
        let total = inner.piece_distribution_count.len();

        let peer_state = inner.peers.get(&addr)?;

        // Only reveal a new piece if the previous one propagated (or this is the first).
        if !peer_state.piece_propagated {
            return None;
        }

        // Find the best piece to offer:
        // 1. We must have it
        // 2. The peer must NOT have it
        // 3. Prefer pieces with lowest distribution count (fewest peers have seen it)
        let mut best: Option<(u32, u32)> = None; // (piece_index, distribution_count)

        for idx in 0..total {
            // We must have this piece
            if our_bitfield.get(idx).map(|b| *b) != Some(true) {
                continue;
            }
            // Peer must not have this piece
            if peer_bitfield.get(idx).map(|b| *b) == Some(true) {
                continue;
            }
            let dist = inner.piece_distribution_count[idx];
            match best {
                None => best = Some((idx as u32, dist)),
                Some((_, best_dist)) if dist < best_dist => {
                    best = Some((idx as u32, dist));
                }
                _ => {}
            }
        }

        let (piece_idx, _) = best?;

        // Update state
        inner.piece_distribution_count[piece_idx as usize] += 1;
        let peer_state = inner.peers.get_mut(&addr).unwrap();
        peer_state.last_offered_piece = Some(piece_idx);
        peer_state.piece_propagated = false;

        Some(piece_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_super_seed_flow() {
        let ss = SuperSeedState::new(10, true);
        let peer_a: SocketAddr = "1.2.3.4:1000".parse().unwrap();
        let peer_b: SocketAddr = "5.6.7.8:2000".parse().unwrap();

        ss.on_peer_connected(peer_a);
        ss.on_peer_connected(peer_b);

        // Our bitfield: all pieces
        let our_bf = BF::from_boxed_slice(vec![0xFF; 2].into_boxed_slice());
        // Peer bitfield: empty
        let peer_bf = BF::from_boxed_slice(vec![0x00; 2].into_boxed_slice());

        // First piece should be offered to peer_a
        let p1 = ss.select_piece_for_peer(peer_a, &peer_bf, &our_bf);
        assert!(p1.is_some());

        // Peer_a should not get another piece until propagation confirmed
        let p1_again = ss.select_piece_for_peer(peer_a, &peer_bf, &our_bf);
        assert!(p1_again.is_none());

        // Simulate peer_b reporting they have the piece (propagation confirmed)
        let confirmed = ss.on_have_received(peer_b, p1.unwrap());
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0], peer_a);

        // Now peer_a should be eligible for a new piece
        let p2 = ss.select_piece_for_peer(peer_a, &peer_bf, &our_bf);
        assert!(p2.is_some());
        // Should prefer a different piece (lower distribution count)
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_disabled_super_seed() {
        let ss = SuperSeedState::new(10, false);
        let peer: SocketAddr = "1.2.3.4:1000".parse().unwrap();
        ss.on_peer_connected(peer);

        let our_bf = BF::from_boxed_slice(vec![0xFF; 2].into_boxed_slice());
        let peer_bf = BF::from_boxed_slice(vec![0x00; 2].into_boxed_slice());

        assert!(ss.select_piece_for_peer(peer, &peer_bf, &our_bf).is_none());
    }

    #[test]
    fn test_toggle_super_seed() {
        let ss = SuperSeedState::new(10, false);
        assert!(!ss.is_enabled());

        ss.set_enabled(true);
        assert!(ss.is_enabled());

        let peer: SocketAddr = "1.2.3.4:1000".parse().unwrap();
        ss.on_peer_connected(peer);

        let our_bf = BF::from_boxed_slice(vec![0xFF; 2].into_boxed_slice());
        let peer_bf = BF::from_boxed_slice(vec![0x00; 2].into_boxed_slice());

        assert!(ss.select_piece_for_peer(peer, &peer_bf, &our_bf).is_some());

        ss.set_enabled(false);
        assert!(!ss.is_enabled());
        // After disabling, should not select any piece
        assert!(ss.select_piece_for_peer(peer, &peer_bf, &our_bf).is_none());
    }
}
