use std::{collections::VecDeque, net::SocketAddr, str::FromStr, sync::atomic::AtomicU32};

use bencode::ByteBufOwned;
use chrono::{DateTime, TimeDelta, Utc};
use librqbit_core::{compact_ip::CompactSocketAddr, hash_id::Id20};
use parking_lot::RwLock;
use rand::RngCore;
use serde::{
    Deserialize, Serialize,
    ser::{SerializeMap, SerializeStruct},
};
use tracing::{debug, trace};

use crate::bprotocol::{AnnouncePeer, Want};

#[derive(Serialize, Deserialize)]
struct StoredToken {
    token: [u8; 4],
    #[serde(serialize_with = "crate::utils::serialize_id20")]
    node_id: Id20,
    addr: SocketAddr,
    #[serde(default = "Utc::now")]
    time: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct StoredPeer {
    addr: SocketAddr,
    time: DateTime<Utc>,
}

pub struct PeerStore {
    self_id: Id20,
    max_remembered_tokens: u32,
    max_remembered_peers: u32,
    max_distance: Id20,
    tokens: RwLock<VecDeque<StoredToken>>,
    peers: dashmap::DashMap<Id20, Vec<StoredPeer>>,
    peers_len: AtomicU32,
}

impl Serialize for PeerStore {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        struct SerializePeers<'a> {
            peers: &'a dashmap::DashMap<Id20, Vec<StoredPeer>>,
        }

        impl Serialize for SerializePeers<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let mut m = serializer.serialize_map(None)?;
                for entry in self.peers.iter() {
                    m.serialize_entry(&entry.key().as_string(), &entry.value())?;
                }
                m.end()
            }
        }

        let mut s = serializer.serialize_struct("PeerStore", 7)?;
        s.serialize_field("self_id", &self.self_id.as_string())?;
        s.serialize_field("max_remembered_tokens", &self.max_remembered_tokens)?;
        s.serialize_field("max_remembered_peers", &self.max_remembered_peers)?;
        s.serialize_field("max_distance", &self.max_distance.as_string())?;
        s.serialize_field("tokens", &*self.tokens.read())?;
        s.serialize_field("peers", &SerializePeers { peers: &self.peers })?;
        s.serialize_field(
            "peers_len",
            &self.peers_len.load(std::sync::atomic::Ordering::SeqCst),
        )?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for PeerStore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Tmp {
            self_id: Id20,
            max_remembered_tokens: u32,
            max_remembered_peers: u32,
            max_distance: Id20,
            tokens: VecDeque<StoredToken>,
            peers: dashmap::DashMap<Id20, Vec<StoredPeer>>,
        }

        Tmp::deserialize(deserializer).map(|tmp| Self {
            self_id: tmp.self_id,
            max_remembered_tokens: tmp.max_remembered_tokens,
            max_remembered_peers: tmp.max_remembered_peers,
            max_distance: tmp.max_distance,
            tokens: RwLock::new(tmp.tokens),
            peers_len: AtomicU32::new(tmp.peers.iter().map(|e| e.value().len() as u32).sum()),
            peers: tmp.peers,
        })
    }
}

impl PeerStore {
    pub fn new(self_id: Id20) -> Self {
        Self {
            self_id,
            max_remembered_tokens: 1000,
            max_remembered_peers: 1000,
            max_distance: Id20::from_str("00000fffffffffffffffffffffffffffffffffff").unwrap(),
            tokens: RwLock::new(VecDeque::new()),
            peers: dashmap::DashMap::new(),
            peers_len: AtomicU32::new(0),
        }
    }

    pub fn gen_token_for(&self, node_id: Id20, addr: SocketAddr) -> [u8; 4] {
        let mut token = [0u8; 4];
        rand::rng().fill_bytes(&mut token);
        let mut tokens = self.tokens.write();
        tokens.push_back(StoredToken {
            token,
            addr,
            node_id,
            time: Utc::now(),
        });
        if tokens.len() > self.max_remembered_tokens as usize {
            tokens.pop_front();
        }
        token
    }

    pub fn store_peer(&self, announce: &AnnouncePeer<ByteBufOwned>, mut addr: SocketAddr) -> bool {
        // If the info_hash in announce is too far away from us, don't store it.
        // If the token doesn't match, don't store it.
        // If we are out of capacity, don't store it.
        // Otherwise, store it.
        if announce.info_hash.distance(&self.self_id) > self.max_distance {
            trace!("peer store: info_hash too far to store");
            return false;
        }
        if !self.tokens.read().iter().any(|t| {
            t.token[..] == announce.token.as_ref()[..] && t.addr == addr && t.node_id == announce.id
        }) {
            trace!("peer store: can't find this token / addr combination");
            return false;
        }

        if announce.implied_port == 0 {
            addr.set_port(announce.port);
        }

        use dashmap::mapref::entry::Entry;
        let peers_entry = self.peers.entry(announce.info_hash);
        let peers_len = self.peers_len.load(std::sync::atomic::Ordering::SeqCst);
        match peers_entry {
            Entry::Occupied(mut occ) => {
                if let Some(s) = occ.get_mut().iter_mut().find(|s| s.addr == addr) {
                    s.time = Utc::now();
                    return true;
                }
                if peers_len >= self.max_remembered_peers {
                    trace!("peer store: out of capacity");
                    return false;
                }
                occ.get_mut().push(StoredPeer {
                    addr,
                    time: Utc::now(),
                });
            }
            Entry::Vacant(vac) => {
                if peers_len >= self.max_remembered_peers {
                    trace!("peer store: out of capacity");
                    return false;
                }
                vac.insert(vec![StoredPeer {
                    addr,
                    time: Utc::now(),
                }]);
            }
        }

        self.peers_len
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }

    pub fn get_for_info_hash(&self, info_hash: Id20, want: Want) -> Vec<CompactSocketAddr> {
        if let Some(stored_peers) = self.peers.get(&info_hash) {
            return stored_peers
                .iter()
                .filter(|p| {
                    matches!(
                        (p.addr, want),
                        (SocketAddr::V6(..), Want::V6 | Want::Both)
                            | (SocketAddr::V4(..), Want::V4 | Want::Both)
                    )
                })
                .map(|p| p.addr.into())
                .collect();
        }
        Vec::new()
    }

    /// Maximum number of peers to keep per info_hash.
    const MAX_PEERS_PER_INFO_HASH: usize = 100;

    /// Peers older than this are considered stale and removed.
    const PEER_TTL: TimeDelta = TimeDelta::minutes(15);

    /// Tokens older than this are removed.
    const TOKEN_TTL: TimeDelta = TimeDelta::minutes(10);

    /// Run garbage collection on the peer store.
    ///
    /// This performs:
    /// 1. Token cleanup: removes tokens older than 10 minutes.
    /// 2. Peer TTL: removes peers not seen in the last 15 minutes.
    /// 3. Per-info_hash cap: keeps only the most recent peers (up to 100) per hash.
    /// 4. Global cap: if still over `max_remembered_peers`, evicts oldest peers first.
    pub fn garbage_collect_peers(&self) {
        let now = Utc::now();

        // 1. Clean up expired tokens.
        {
            let mut tokens = self.tokens.write();
            let token_cutoff = now - Self::TOKEN_TTL;
            let before = tokens.len();
            tokens.retain(|t| t.time > token_cutoff);
            let removed = before - tokens.len();
            if removed > 0 {
                debug!("peer store GC: removed {removed} expired tokens");
            }
        }

        // 2. Remove stale peers (older than PEER_TTL) and enforce per-info_hash cap.
        let peer_cutoff = now - Self::PEER_TTL;
        let mut total_removed: u32 = 0;
        let mut empty_hashes = Vec::new();

        for mut entry in self.peers.iter_mut() {
            let info_hash = *entry.key();
            let peers = entry.value_mut();
            let before = peers.len();

            // Remove peers older than the TTL.
            peers.retain(|p| p.time > peer_cutoff);

            // Enforce per-info_hash cap: keep only the most recent peers.
            if peers.len() > Self::MAX_PEERS_PER_INFO_HASH {
                peers.sort_by(|a, b| b.time.cmp(&a.time));
                peers.truncate(Self::MAX_PEERS_PER_INFO_HASH);
            }

            let removed = before - peers.len();
            total_removed += removed as u32;

            if peers.is_empty() {
                empty_hashes.push(info_hash);
            }
        }

        // Remove empty info_hash entries.
        for hash in &empty_hashes {
            self.peers.remove(hash);
        }

        // 3. Enforce global cap by evicting oldest peers first.
        //    Recompute the actual count after TTL/per-hash cleanup.
        let actual_count: u32 = self.peers.iter().map(|e| e.value().len() as u32).sum();
        let max = self.max_remembered_peers;

        if actual_count > max {
            let to_evict = actual_count - max;
            let mut evicted = 0u32;

            // Collect (info_hash, oldest_time) pairs so we can evict from the
            // entries with the oldest peers first.
            let mut entries_by_oldest: Vec<(Id20, DateTime<Utc>)> = self
                .peers
                .iter()
                .filter_map(|entry| {
                    entry
                        .value()
                        .iter()
                        .map(|p| p.time)
                        .min()
                        .map(|oldest| (*entry.key(), oldest))
                })
                .collect();
            entries_by_oldest.sort_by_key(|(_hash, oldest)| *oldest);

            for (hash, _) in entries_by_oldest {
                if evicted >= to_evict {
                    break;
                }
                if let Some(mut entry) = self.peers.get_mut(&hash) {
                    let peers = entry.value_mut();
                    peers.sort_by(|a, b| a.time.cmp(&b.time));
                    while !peers.is_empty() && evicted < to_evict {
                        peers.remove(0);
                        evicted += 1;
                    }
                    if peers.is_empty() {
                        drop(entry);
                        self.peers.remove(&hash);
                    }
                }
            }
            total_removed += evicted;
        }

        // Update the atomic counter to the true count.
        let final_count: u32 = self.peers.iter().map(|e| e.value().len() as u32).sum();
        self.peers_len
            .store(final_count, std::sync::atomic::Ordering::SeqCst);

        if total_removed > 0 {
            debug!(
                "peer store GC: removed {total_removed} peers, {remaining} remaining, {hashes} info_hashes",
                remaining = final_count,
                hashes = self.peers.len(),
            );
        }
    }
}
