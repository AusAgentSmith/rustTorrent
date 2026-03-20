use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use dashmap::DashMap;
use librqbit_core::lengths::ValidPieceIndex;
use parking_lot::RwLock;
use peer_binary_protocol::{Message, Request};

use tracing::debug;

use crate::{
    Error,
    peer_connection::WriterRequest,
    torrent_state::utils::{TimedExistence, atomic_inc},
    type_aliases::{BF, PeerHandle},
};

use self::stats::{AggregatePeerStats, AggregatePeerStatsAtomic};

use super::peer::{LivePeerState, Peer, PeerRx, PeerState, PeerTx};

pub mod stats;

pub(crate) struct PeerStates {
    pub session_stats: Arc<AggregatePeerStatsAtomic>,

    // This keeps track of live addresses we connected to, for PEX.
    pub live_outgoing_peers: RwLock<HashSet<PeerHandle>>,
    pub stats: AggregatePeerStatsAtomic,
    pub states: DashMap<PeerHandle, Peer>,
}

impl Drop for PeerStates {
    fn drop(&mut self) {
        for (_, p) in std::mem::take(&mut self.states).into_iter() {
            p.destroy(self);
        }
    }
}

impl PeerStates {
    pub fn stats(&self) -> AggregatePeerStats {
        self.stats.snapshot()
    }

    pub fn add_if_not_seen(&self, addr: SocketAddr) -> Option<PeerHandle> {
        use dashmap::mapref::entry::Entry;
        match self.states.entry(addr) {
            Entry::Occupied(_) => None,
            Entry::Vacant(vac) => {
                vac.insert(Peer::new_with_outgoing_address(addr));
                atomic_inc(&self.stats.queued);
                atomic_inc(&self.session_stats.queued);

                atomic_inc(&self.stats.seen);
                atomic_inc(&self.session_stats.seen);
                Some(addr)
            }
        }
    }
    pub fn with_peer<R>(&self, addr: PeerHandle, f: impl FnOnce(&Peer) -> R) -> Option<R> {
        self.states.get(&addr).map(|e| f(e.value()))
    }

    pub fn with_peer_mut<R>(
        &self,
        addr: PeerHandle,
        reason: &'static str,
        f: impl FnOnce(&mut Peer) -> R,
    ) -> Option<R> {
        use crate::torrent_state::utils::timeit;
        timeit(reason, || self.states.get_mut(&addr))
            .map(|e| f(TimedExistence::new(e, reason).value_mut()))
    }

    pub fn with_live<R>(&self, addr: PeerHandle, f: impl FnOnce(&LivePeerState) -> R) -> Option<R> {
        self.with_peer(addr, |peer| peer.get_live().map(f))
            .flatten()
    }

    pub fn with_live_mut<R>(
        &self,
        addr: PeerHandle,
        reason: &'static str,
        f: impl FnOnce(&mut LivePeerState) -> R,
    ) -> Option<R> {
        self.with_peer_mut(addr, reason, |peer| peer.get_live_mut().map(f))
            .flatten()
    }

    pub fn drop_peer(&self, handle: PeerHandle) -> Option<Peer> {
        let p = self.states.remove(&handle).map(|r| r.1)?;
        let s = p.get_state();
        self.stats.dec(s);
        self.session_stats.dec(s);

        Some(p)
    }

    pub fn is_peer_not_interested_and_has_full_torrent(
        &self,
        handle: PeerHandle,
        total_pieces: usize,
    ) -> bool {
        self.with_live(handle, |live| {
            !live.peer_interested && live.has_full_torrent(total_pieces)
        })
        .unwrap_or(false)
    }

    pub fn mark_peer_interested(&self, handle: PeerHandle, is_interested: bool) -> Option<bool> {
        self.with_live_mut(handle, "mark_peer_interested", |live| {
            let prev = live.peer_interested;
            live.peer_interested = is_interested;
            prev
        })
    }

    pub fn update_bitfield(&self, handle: PeerHandle, bitfield: BF) -> Option<()> {
        self.with_live_mut(handle, "update_bitfield", |live| {
            live.bitfield = bitfield;
        })
    }

    pub fn mark_peer_connecting(&self, h: PeerHandle) -> crate::Result<(PeerRx, PeerTx)> {
        let rx = self
            .with_peer_mut(h, "mark_peer_connecting", |peer| {
                peer.idle_to_connecting(self)
                    .ok_or(Error::BugInvalidPeerState)
            })
            .ok_or(Error::BugPeerNotFound)??;
        Ok(rx)
    }

    pub fn reset_peer_backoff(&self, handle: PeerHandle) {
        self.with_peer_mut(handle, "reset_peer_backoff", |p| {
            p.stats.reset_backoff();
        });
    }

    pub fn mark_peer_not_needed(&self, handle: PeerHandle) -> Option<PeerState> {
        let prev = self.with_peer_mut(handle, "mark_peer_not_needed", |peer| {
            peer.set_not_needed(self)
        })?;
        Some(prev)
    }

    pub(crate) fn on_steal(
        &self,
        from_peer: SocketAddr,
        to_peer: SocketAddr,
        stolen_idx: ValidPieceIndex,
    ) {
        self.with_peer(to_peer, |p| {
            atomic_inc(&p.stats.counters.times_i_stole);
        });
        self.with_peer(from_peer, |p| {
            atomic_inc(&p.stats.counters.times_stolen_from_me);
        });
        self.stats.inc_steals();
        self.session_stats.inc_steals();

        self.with_live_mut(from_peer, "send_cancellations", |live| {
            let tx = &live.tx;
            live.inflight_requests.retain(|req| {
                if req.piece_index == stolen_idx {
                    let _ = tx.send(WriterRequest::Message(Message::Cancel(Request {
                        index: stolen_idx.get(),
                        begin: req.offset,
                        length: req.size,
                    })));
                    false
                } else {
                    true
                }
            });
        });
    }

    /// Remove peers that have been in Dead or NotNeeded state for longer than
    /// `retention`. Returns the number of peers pruned.
    pub fn prune_dead_peers(&self, retention: Duration) -> usize {
        let now = std::time::Instant::now();
        let mut pruned = 0usize;
        self.states.retain(|_addr, peer| {
            let dominated = matches!(peer.get_state(), PeerState::Dead | PeerState::NotNeeded);
            if dominated && now.duration_since(peer.last_state_change) > retention {
                // Decrement counters before removing.
                for counter in [&self.session_stats, &self.stats] {
                    counter.dec(peer.get_state());
                }
                pruned += 1;
                false
            } else {
                true
            }
        });
        if pruned > 0 {
            debug!(pruned, remaining = self.states.len(), "pruned dead/not-needed peers");
        }
        pruned
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    fn make_peer_states() -> PeerStates {
        PeerStates {
            session_stats: Arc::new(AggregatePeerStatsAtomic::default()),
            stats: AggregatePeerStatsAtomic::default(),
            states: DashMap::new(),
            live_outgoing_peers: RwLock::new(HashSet::new()),
        }
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::new(Ipv4Addr::new(127, 0, 0, 1).into(), port)
    }

    #[test]
    fn test_dead_peer_pruning_removes_stale_dead_peers() {
        let ps = make_peer_states();

        // Add 5 peers, mark them Dead
        for port in 1..=5 {
            let a = addr(port);
            ps.add_if_not_seen(a);
            ps.with_peer_mut(a, "set_dead", |peer| {
                peer.set_state(PeerState::Dead, &ps);
            });
        }
        assert_eq!(ps.states.len(), 5);

        // With a retention of 0, all dead peers should be pruned immediately
        let pruned = ps.prune_dead_peers(Duration::ZERO);
        assert_eq!(pruned, 5);
        assert_eq!(ps.states.len(), 0);
    }

    #[test]
    fn test_dead_peer_pruning_removes_stale_not_needed_peers() {
        let ps = make_peer_states();

        for port in 1..=3 {
            let a = addr(port);
            ps.add_if_not_seen(a);
            ps.with_peer_mut(a, "set_not_needed", |peer| {
                peer.set_not_needed(&ps);
            });
        }
        assert_eq!(ps.states.len(), 3);

        let pruned = ps.prune_dead_peers(Duration::ZERO);
        assert_eq!(pruned, 3);
        assert_eq!(ps.states.len(), 0);
    }

    #[test]
    fn test_dead_peer_pruning_preserves_recent_peers() {
        let ps = make_peer_states();

        // Add peers and mark dead
        for port in 1..=5 {
            let a = addr(port);
            ps.add_if_not_seen(a);
            ps.with_peer_mut(a, "set_dead", |peer| {
                peer.set_state(PeerState::Dead, &ps);
            });
        }

        // With 1 hour retention, nothing should be pruned (peers just became dead)
        let pruned = ps.prune_dead_peers(Duration::from_secs(3600));
        assert_eq!(pruned, 0);
        assert_eq!(ps.states.len(), 5);
    }

    #[test]
    fn test_dead_peer_pruning_preserves_queued_and_connecting() {
        let ps = make_peer_states();

        // Queued peer (default state from add_if_not_seen)
        let queued_addr = addr(1);
        ps.add_if_not_seen(queued_addr);

        // Dead peer
        let dead_addr = addr(2);
        ps.add_if_not_seen(dead_addr);
        ps.with_peer_mut(dead_addr, "set_dead", |peer| {
            peer.set_state(PeerState::Dead, &ps);
        });

        assert_eq!(ps.states.len(), 2);

        // Zero retention should only prune the dead peer, not the queued one
        let pruned = ps.prune_dead_peers(Duration::ZERO);
        assert_eq!(pruned, 1);
        assert_eq!(ps.states.len(), 1);
        assert!(ps.states.contains_key(&queued_addr));
        assert!(!ps.states.contains_key(&dead_addr));
    }

    #[test]
    fn test_dead_peer_pruning_counters_are_decremented() {
        let ps = make_peer_states();

        // Add 3 peers, mark them dead
        for port in 1..=3 {
            let a = addr(port);
            ps.add_if_not_seen(a);
            ps.with_peer_mut(a, "set_dead", |peer| {
                peer.set_state(PeerState::Dead, &ps);
            });
        }

        let stats_before = ps.stats.snapshot();
        assert_eq!(stats_before.dead, 3);

        ps.prune_dead_peers(Duration::ZERO);

        let stats_after = ps.stats.snapshot();
        assert_eq!(stats_after.dead, 0);
    }

    #[test]
    fn test_semaphore_permit_returned_on_drop() {
        // Verify that OwnedSemaphorePermit releases on drop (RAII behavior).
        // This confirms our _permit_guard pattern works correctly.
        let sem = Arc::new(tokio::sync::Semaphore::new(2));

        // Acquire 2 permits
        let p1 = sem.clone().try_acquire_owned().unwrap();
        let p2 = sem.clone().try_acquire_owned().unwrap();
        assert!(sem.clone().try_acquire_owned().is_err());

        // Drop one - should free a slot
        drop(p1);
        assert!(sem.clone().try_acquire_owned().is_ok());

        // Rename to _guard (simulating our pattern) and let it drop naturally
        let _guard = p2;
        // p2 is moved into _guard, will be dropped at end of scope

        drop(_guard);
        assert_eq!(sem.available_permits(), 2);
    }
}
