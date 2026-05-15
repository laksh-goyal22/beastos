//! Time Travel - Snapshots and Version History
//!
//! Every filesystem state is captured as a Merkle root hash.
//! Snapshots are just stored root hashes with metadata.

use crate::storage::Hash;
use crate::fs::merkle::MerkleTree;
use crate::fs::path_cache::PathCache;
use crate::fs::token::{Token, TokenId, Permissions};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use core::cmp::Ordering;

/// A point-in-time snapshot of the entire filesystem
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Unique ID (hash of snapshot metadata)
    pub id: Hash,
    /// Merkle root hash of filesystem at this time
    pub root_hash: Hash,
    /// Parent snapshot (for history chain)
    pub parent: Option<Hash>,
    /// When snapshot was created
    pub timestamp: u64,
    /// User-provided description
    pub message: String,
    /// User-assigned tags (e.g., "backup", "v1.0", "before-update")
    pub tags: Vec<String>,
    /// Which user created this snapshot
    pub created_by: u64,
}

impl Snapshot {
    /// Create a new snapshot
    pub fn new(root_hash: Hash, parent: Option<Hash>, message: &str, created_by: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(root_hash.as_bytes());
        if let Some(p) = parent {
            hasher.update(p.as_bytes());
        }
        hasher.update(message.as_bytes());
        hasher.update(&created_by.to_le_bytes());
        
        Self {
            id: Hash::from_bytes(hasher.finalize().as_bytes()),
            root_hash,
            parent,
            timestamp: Self::current_time(),
            message: message.to_string(),
            tags: Vec::new(),
            created_by,
        }
    }
    
    /// Add a tag to this snapshot
    pub fn add_tag(&mut self, tag: &str) {
        if !self.tags.contains(&tag.to_string()) {
            self.tags.push(tag.to_string());
        }
    }
    
    /// Check if snapshot has a tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag.to_string())
    }
    
    /// Get age in seconds
    pub fn age_seconds(&self) -> u64 {
        Self::current_time().saturating_sub(self.timestamp)
    }
    
    /// Format age for display
    pub fn age_string(&self) -> String {
        let seconds = self.age_seconds();
        if seconds < 60 {
            format!("{} seconds ago", seconds)
        } else if seconds < 3600 {
            format!("{} minutes ago", seconds / 60)
        } else if seconds < 86400 {
            format!("{} hours ago", seconds / 3600)
        } else if seconds < 604800 {
            format!("{} days ago", seconds / 86400)
        } else {
            format!("{} weeks ago", seconds / 604800)
        }
    }
    
    fn current_time() -> u64 {
        crate::drivers::pit::get_ticks() / 100
    }
}

/// Snapshot management system
pub struct TimeMachine {
    /// All snapshots indexed by ID
    snapshots: Mutex<BTreeMap<Hash, Snapshot>>,
    /// Snapshots indexed by tag (for quick lookup)
    tag_index: Mutex<BTreeMap<String, Vec<Hash>>>,
    /// Maximum number of snapshots to keep (0 = unlimited)
    max_snapshots: u64,
    /// Auto-snapshot interval in seconds (0 = disabled)
    auto_interval: u64,
    /// Last auto-snapshot time
    last_auto: Mutex<u64>,
}

impl TimeMachine {
    pub fn new() -> Self {
        Self {
            snapshots: Mutex::new(BTreeMap::new()),
            tag_index: Mutex::new(BTreeMap::new()),
            max_snapshots: 1000,
            auto_interval: 3600, //每小时快照一次
            last_auto: Mutex::new(0),
        }
    }
    
    /// Create a new snapshot
    pub fn snapshot(
        &self,
        root_hash: Hash,
        message: &str,
        created_by: u64,
    ) -> Hash {
        let parent = self.get_latest_snapshot();
        let snapshot = Snapshot::new(root_hash, parent, message, created_by);
        let id = snapshot.id;
        
        {
            let mut snapshots = self.snapshots.lock();
            snapshots.insert(id, snapshot);
            
            // Enforce maximum snapshots
            if self.max_snapshots > 0 && snapshots.len() > self.max_snapshots as usize {
                self.enforce_limit(&mut snapshots);
            }
        }
        
        id
    }
    
    /// Add a tag to a snapshot
    pub fn tag_snapshot(&self, snapshot_id: Hash, tag: &str) -> bool {
        let mut snapshots = self.snapshots.lock();
        if let Some(snapshot) = snapshots.get_mut(&snapshot_id) {
            snapshot.add_tag(tag);
            
            // Update tag index
            let mut tag_index = self.tag_index.lock();
            tag_index.entry(tag.to_string())
                .or_insert_with(Vec::new)
                .push(snapshot_id);
            true
        } else {
            false
        }
    }
    
    /// Get snapshot by ID
    pub fn get_snapshot(&self, id: Hash) -> Option<Snapshot> {
        self.snapshots.lock().get(&id).cloned()
    }
    
    /// Find snapshot by tag (returns most recent)
    pub fn find_by_tag(&self, tag: &str) -> Option<Hash> {
        let tag_index = self.tag_index.lock();
        if let Some(ids) = tag_index.get(tag) {
            // Return the most recent (largest timestamp)
            let snapshots = self.snapshots.lock();
            let mut latest: Option<Hash> = None;
            let mut latest_time = 0;
            for id in ids {
                if let Some(snap) = snapshots.get(id) {
                    if snap.timestamp > latest_time {
                        latest_time = snap.timestamp;
                        latest = Some(*id);
                    }
                }
            }
            latest
        } else {
            None
        }
    }
    
    /// Get latest snapshot
    pub fn get_latest_snapshot(&self) -> Option<Hash> {
        let snapshots = self.snapshots.lock();
        snapshots.iter()
            .max_by_key(|(_, snap)| snap.timestamp)
            .map(|(id, _)| *id)
    }
    
    /// Get snapshot history (chain from latest to oldest)
    pub fn get_history(&self) -> Vec<Snapshot> {
        let mut history = Vec::new();
        let mut current = self.get_latest_snapshot();
        
        while let Some(id) = current {
            if let Some(snap) = self.get_snapshot(id) {
                history.push(snap.clone());
                current = snap.parent;
            } else {
                break;
            }
        }
        
        history
    }
    
    /// Restore filesystem to a snapshot
    pub fn restore(
        &self,
        snapshot_id: Hash,
        path_cache: &PathCache,
        merkle_tree: &MerkleTree,
    ) -> Result<(), RestoreError> {
        let snapshot = self.get_snapshot(snapshot_id)
            .ok_or(RestoreError::SnapshotNotFound)?;
        
        // Restoring is just setting the Merkle tree root hash.
        // Actual objects are immutable in CAS, so they're still there.
        merkle_tree.set_root_hash(snapshot.root_hash);
        
        crate::kprintln!("[TIMEMACHINE] Restored to snapshot from {}: {}", 
            snapshot.age_string(), snapshot.message);
        
        Ok(())
    }
    
    /// List all snapshots (formatted for user)
    pub fn list_snapshots(&self) -> Vec<SnapshotInfo> {
        let snapshots = self.snapshots.lock();
        let mut infos: Vec<SnapshotInfo> = snapshots.iter()
            .map(|(_, snap)| SnapshotInfo {
                id: snap.id,
                timestamp: snap.timestamp,
                message: snap.message.clone(),
                tags: snap.tags.clone(),
                age: snap.age_string(),
            })
            .collect();
        
        infos.sort_by_key(|info| info.timestamp);
        infos.reverse();
        infos
    }
    
    /// Delete old snapshots (retention policy)
    fn enforce_limit(&self, snapshots: &mut BTreeMap<Hash, Snapshot>) {
        // Remove oldest snapshots beyond limit
        let mut snap_vec: Vec<(Hash, Snapshot)> = snapshots.drain().collect();
        snap_vec.sort_by_key(|(_, snap)| snap.timestamp);
        snap_vec.reverse(); // Keep newest
        
        let keep = self.max_snapshots as usize;
        if snap_vec.len() > keep {
            let to_remove = snap_vec.split_off(keep);
            for (id, _) in to_remove {
                // Also remove from tag index
                let mut tag_index = self.tag_index.lock();
                for tags in tag_index.values_mut() {
                    tags.retain(|&i| i != id);
                }
            }
        }
        
        for (id, snap) in snap_vec {
            snapshots.insert(id, snap);
        }
    }
    
    /// Check if auto-snapshot should be taken
    pub fn check_auto_snapshot(&self, root_hash: Hash) -> Option<Hash> {
        if self.auto_interval == 0 {
            return None;
        }
        
        let now = Snapshot::current_time();
        let mut last = self.last_auto.lock();
        
        if now - *last >= self.auto_interval {
            *last = now;
            Some(self.snapshot(root_hash, "auto", 0))
        } else {
            None
        }
    }
    
    /// Compare two snapshots (what changed?)
    pub fn diff(&self, old_id: Hash, new_id: Hash) -> Result<SnapshotDiff, RestoreError> {
        let old_snap = self.get_snapshot(old_id).ok_or(RestoreError::SnapshotNotFound)?;
        let new_snap = self.get_snapshot(new_id).ok_or(RestoreError::SnapshotNotFound)?;
        
        // Since we have Merkle roots, we can compare hashes
        // Full diff would require walking the tree comparing nodes
        
        Ok(SnapshotDiff {
            old_root: old_snap.root_hash,
            new_root: new_snap.root_hash,
            changed: old_snap.root_hash != new_snap.root_hash,
            // Detailed changes would need tree walk
        })
    }
}

/// Snapshot info for UI display
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id: Hash,
    pub timestamp: u64,
    pub message: String,
    pub tags: Vec<String>,
    pub age: String,
}

/// Result of comparing two snapshots
#[derive(Debug)]
pub struct SnapshotDiff {
    pub old_root: Hash,
    pub new_root: Hash,
    pub changed: bool,
}

#[derive(Debug)]
pub enum RestoreError {
    SnapshotNotFound,
    InvalidRoot,
    PermissionDenied,
}

/// Automatic snapshot policies
pub struct SnapshotPolicy {
    pub keep_hourly: u32,      // Keep last N hourly snapshots
    pub keep_daily: u32,       // Keep last N daily snapshots
    pub keep_weekly: u32,      // Keep last N weekly snapshots
    pub keep_monthly: u32,     // Keep last N monthly snapshots
}

impl SnapshotPolicy {
    pub fn default() -> Self {
        Self {
            keep_hourly: 48,    // 2 days of hourly
            keep_daily: 30,     // 30 days of daily
            keep_weekly: 12,    // 12 weeks
            keep_monthly: 12,   // 1 year of monthly
        }
    }
    
    /// Apply retention policy to snapshot list
    pub fn apply(&self, snapshots: &mut Vec<SnapshotInfo>) {
        let now = crate::drivers::pit::get_ticks() / 100;
        
        // Group by age
        let mut hourly = Vec::new();
        let mut daily = Vec::new();
        let mut weekly = Vec::new();
        let mut monthly = Vec::new();
        
        for snap in snapshots.iter() {
            let age_seconds = now - snap.timestamp;
            let age_days = age_seconds / 86400;
            let age_weeks = age_days / 7;
            let age_months = age_days / 30;
            
            if age_seconds < 86400 {
                hourly.push(snap.clone());
            } else if age_seconds < 604800 {
                daily.push(snap.clone());
            } else if age_seconds < 2592000 {
                weekly.push(snap.clone());
            } else {
                monthly.push(snap.clone());
            }
        }
        
        // Keep only the most recent N from each category
        let keep = |list: &mut Vec<SnapshotInfo>, n: usize| {
            if list.len() > n {
                list.truncate(n);
            }
        };
        
        keep(&mut hourly, self.keep_hourly as usize);
        keep(&mut daily, self.keep_daily as usize);
        keep(&mut weekly, self.keep_weekly as usize);
        keep(&mut monthly, self.keep_monthly as usize);
        
        // Rebuild snapshot list
        snapshots.clear();
        snapshots.extend(hourly);
        snapshots.extend(daily);
        snapshots.extend(weekly);
        snapshots.extend(monthly);
        
        // Sort by timestamp
        snapshots.sort_by_key(|s| s.timestamp);
        snapshots.reverse();
    }
}