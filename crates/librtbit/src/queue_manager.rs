use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::session::TorrentId;

/// Configuration for queue limits.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueueLimitsConfig {
    /// Max concurrent downloading torrents. 0 means unlimited.
    pub max_active_downloads: u32,
    /// Max concurrent seeding (upload-only) torrents. 0 means unlimited.
    pub max_active_uploads: u32,
    /// Max total active (downloading + seeding) torrents. 0 means unlimited.
    pub max_active_total: u32,
}

/// The queue state of a torrent from the queue manager's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueState {
    /// Torrent is actively downloading or seeding (has a slot).
    Active,
    /// Torrent is waiting for a slot to become available.
    Queued,
    /// Torrent was manually paused by the user (not managed by queue).
    ManuallyPaused,
}

/// Entry in the queue for a specific torrent.
#[derive(Debug)]
struct QueueEntry {
    id: TorrentId,
    /// Position in the queue (lower = higher priority). Only meaningful for Queued state.
    position: u32,
    state: QueueState,
}

/// The queue manager tracks which torrents are active, queued, or manually paused,
/// and enforces configurable limits on concurrent active downloads/uploads.
pub struct QueueManager {
    max_active_downloads: AtomicU32,
    max_active_uploads: AtomicU32,
    max_active_total: AtomicU32,
    entries: RwLock<Vec<QueueEntry>>,
}

impl QueueManager {
    pub fn new(config: QueueLimitsConfig) -> Self {
        Self {
            max_active_downloads: AtomicU32::new(config.max_active_downloads),
            max_active_uploads: AtomicU32::new(config.max_active_uploads),
            max_active_total: AtomicU32::new(config.max_active_total),
            entries: RwLock::new(Vec::new()),
        }
    }

    pub fn get_max_active_downloads(&self) -> u32 {
        self.max_active_downloads.load(Ordering::Relaxed)
    }

    pub fn set_max_active_downloads(&self, v: u32) {
        self.max_active_downloads.store(v, Ordering::Relaxed);
    }

    pub fn get_max_active_uploads(&self) -> u32 {
        self.max_active_uploads.load(Ordering::Relaxed)
    }

    pub fn set_max_active_uploads(&self, v: u32) {
        self.max_active_uploads.store(v, Ordering::Relaxed);
    }

    pub fn get_max_active_total(&self) -> u32 {
        self.max_active_total.load(Ordering::Relaxed)
    }

    pub fn set_max_active_total(&self, v: u32) {
        self.max_active_total.store(v, Ordering::Relaxed);
    }

    /// Register a new torrent in the queue. Returns the queue state it should be in.
    /// `is_downloading` indicates whether the torrent is still downloading (vs seeding).
    pub fn register_torrent(
        &self,
        id: TorrentId,
        is_downloading: bool,
        manually_paused: bool,
    ) -> QueueState {
        let mut entries = self.entries.write();

        if manually_paused {
            let position = next_position(&entries);
            entries.push(QueueEntry {
                id,
                position,
                state: QueueState::ManuallyPaused,
            });
            return QueueState::ManuallyPaused;
        }

        if self.has_available_slot(&entries, is_downloading) {
            entries.push(QueueEntry {
                id,
                position: 0,
                state: QueueState::Active,
            });
            QueueState::Active
        } else {
            let position = next_position(&entries);
            entries.push(QueueEntry {
                id,
                position,
                state: QueueState::Queued,
            });
            QueueState::Queued
        }
    }

    /// Remove a torrent from the queue entirely (e.g., on delete/forget).
    /// Returns IDs of torrents that should be promoted from queued to active.
    pub fn remove_torrent(&self, id: TorrentId) -> Vec<TorrentId> {
        let mut entries = self.entries.write();
        entries.retain(|e| e.id != id);
        self.promote_queued(&mut entries)
    }

    /// Mark a torrent as manually paused.
    /// Returns IDs of torrents that should be promoted from queued to active.
    pub fn pause_torrent(&self, id: TorrentId) -> Vec<TorrentId> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            entry.state = QueueState::ManuallyPaused;
        }
        self.promote_queued(&mut entries)
    }

    /// Mark a torrent as wanting to be active (unpause).
    /// Returns the queue state it should be in.
    pub fn unpause_torrent(&self, id: TorrentId, is_downloading: bool) -> QueueState {
        let mut entries = self.entries.write();
        let idx = entries.iter().position(|e| e.id == id);
        if let Some(idx) = idx {
            let has_slot = self.has_available_slot(&entries, is_downloading);
            if has_slot {
                entries[idx].state = QueueState::Active;
                QueueState::Active
            } else {
                let position = next_position(&entries);
                entries[idx].position = position;
                entries[idx].state = QueueState::Queued;
                QueueState::Queued
            }
        } else {
            // Not tracked yet, register it
            drop(entries);
            self.register_torrent(id, is_downloading, false)
        }
    }

    /// Notify that a torrent has completed downloading (now seeding).
    /// Returns IDs of torrents that should be promoted from queued to active.
    pub fn torrent_completed(&self, _id: TorrentId) -> Vec<TorrentId> {
        // A download slot freed up (torrent went from downloading to seeding).
        // But it's still active. Check if we can promote any queued torrents.
        let mut entries = self.entries.write();
        self.promote_queued(&mut entries)
    }

    /// Get the queue state and position for a specific torrent.
    pub fn get_queue_state(&self, id: TorrentId) -> Option<(QueueState, Option<u32>)> {
        let entries = self.entries.read();
        entries.iter().find(|e| e.id == id).map(|e| {
            let pos = if e.state == QueueState::Queued {
                // Return 1-based position among queued torrents
                let mut queued: Vec<_> = entries
                    .iter()
                    .filter(|e| e.state == QueueState::Queued)
                    .collect();
                queued.sort_by_key(|e| e.position);
                queued
                    .iter()
                    .position(|q| q.id == id)
                    .map(|p| (p + 1) as u32)
            } else {
                None
            };
            (e.state, pos)
        })
    }

    /// Move a torrent to the top of the queue.
    pub fn move_to_top(&self, id: TorrentId) {
        let mut entries = self.entries.write();
        if let Some(min_pos) = entries
            .iter()
            .filter(|e| e.state == QueueState::Queued)
            .map(|e| e.position)
            .min()
        {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id && e.state == QueueState::Queued) {
                entry.position = min_pos.saturating_sub(1);
            }
        }
    }

    /// Move a torrent to the bottom of the queue.
    pub fn move_to_bottom(&self, id: TorrentId) {
        let mut entries = self.entries.write();
        let position = next_position(&entries);
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id && e.state == QueueState::Queued) {
            entry.position = position;
        }
    }

    /// Move a torrent up one position in the queue.
    pub fn move_up(&self, id: TorrentId) {
        let mut entries = self.entries.write();
        let mut queued: Vec<(usize, u32)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.state == QueueState::Queued)
            .map(|(idx, e)| (idx, e.position))
            .collect();
        queued.sort_by_key(|&(_, pos)| pos);

        if let Some(qi) = queued.iter().position(|&(idx, _)| entries[idx].id == id) {
            if qi > 0 {
                let prev_idx = queued[qi - 1].0;
                let curr_idx = queued[qi].0;
                let tmp = entries[prev_idx].position;
                entries[prev_idx].position = entries[curr_idx].position;
                entries[curr_idx].position = tmp;
            }
        }
    }

    /// Move a torrent down one position in the queue.
    pub fn move_down(&self, id: TorrentId) {
        let mut entries = self.entries.write();
        let mut queued: Vec<(usize, u32)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.state == QueueState::Queued)
            .map(|(idx, e)| (idx, e.position))
            .collect();
        queued.sort_by_key(|&(_, pos)| pos);

        if let Some(qi) = queued.iter().position(|&(idx, _)| entries[idx].id == id) {
            if qi + 1 < queued.len() {
                let next_idx = queued[qi + 1].0;
                let curr_idx = queued[qi].0;
                let tmp = entries[next_idx].position;
                entries[next_idx].position = entries[curr_idx].position;
                entries[curr_idx].position = tmp;
            }
        }
    }

    /// Check if there's an available slot for a torrent.
    fn has_available_slot(&self, entries: &[QueueEntry], is_downloading: bool) -> bool {
        let max_total = self.max_active_total.load(Ordering::Relaxed);
        let max_downloads = self.max_active_downloads.load(Ordering::Relaxed);
        let max_uploads = self.max_active_uploads.load(Ordering::Relaxed);

        let active_count = entries.iter().filter(|e| e.state == QueueState::Active).count() as u32;

        // Check total limit
        if max_total > 0 && active_count >= max_total {
            return false;
        }

        if is_downloading {
            if max_downloads > 0 {
                // We need to count active downloading torrents, but we don't track
                // whether each active torrent is downloading or seeding in queue entries.
                // We'll check against total active as a conservative bound.
                // The session will provide actual counts when needed.
                if active_count >= max_downloads {
                    return false;
                }
            }
        } else if max_uploads > 0 && active_count >= max_uploads {
            return false;
        }

        true
    }

    /// Try to promote queued torrents to active state.
    /// Returns IDs of torrents that were promoted.
    fn promote_queued(&self, entries: &mut Vec<QueueEntry>) -> Vec<TorrentId> {
        let mut promoted = Vec::new();

        // Sort queued entries by position to promote in order
        let mut queued_indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.state == QueueState::Queued)
            .map(|(i, _)| i)
            .collect();

        queued_indices.sort_by_key(|&i| entries[i].position);

        for idx in queued_indices {
            // Re-check slot availability after each promotion
            if self.has_available_slot(entries, true) {
                entries[idx].state = QueueState::Active;
                promoted.push(entries[idx].id);
            } else {
                break;
            }
        }

        promoted
    }
}

fn next_position(entries: &[QueueEntry]) -> u32 {
    entries
        .iter()
        .filter(|e| e.state == QueueState::Queued)
        .map(|e| e.position)
        .max()
        .map(|p| p + 1)
        .unwrap_or(0)
}
