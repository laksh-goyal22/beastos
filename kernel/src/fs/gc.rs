//! Garbage Collection for Content-Addressable Storage
//!
//! Since Beast FS stores immutable objects forever, we need
//! garbage collection to clean up unreferenced data.

use crate::storage::cas::ContentAddressableStorage;
use crate::fs::token::{Token, TokenId};
use crate::fs::path_cache::PathCache;
use alloc::collections::{BTreeSet, BTreeMap};
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::kprintln;

/// Garbage collector states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GCState {
    Idle,
    Marking,
    Sweeping,
    Compacting,
}

/// Garbage collection statistics
#[derive(Debug, Clone, Default)]
pub struct GCStats {
    pub objects_marked: u64,
    pub objects_swept: u64,
    pub bytes_freed: u64,
    pub objects_kept: u64,
    pub last_gc_duration_ms: u64,
    pub last_gc_time: u64,
}

/// Garbage collector for Beast FS
pub struct GarbageCollector {
    state: Mutex<GCState>,
    stats: Mutex<GCStats>,
    running: AtomicBool,
    cas: Option<&'static ContentAddressableStorage>,
    path_cache: Option<&'static PathCache>,
}

impl GarbageCollector {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(GCState::Idle),
            stats: Mutex::new(GCStats::default()),
            running: AtomicBool::new(false),
            cas: None,
            path_cache: None,
        }
    }
    
    /// Initialize with references to storage and cache
    pub fn init(&mut self, cas: &'static ContentAddressableStorage, path_cache: &'static PathCache) {
        self.cas = Some(cas);
        self.path_cache = Some(path_cache);
    }
    
    /// Start garbage collection
    pub fn collect(&self) -> GCStats {
        if !self.try_start_gc() {
            return self.stats.lock().clone();
        }
        
        let start_time = Self::current_time_ms();
        
        // Phase 1: Mark reachable objects
        let marked = self.mark_phase();
        
        // Phase 2: Sweep unreachable objects
        let freed = self.sweep_phase(&marked);
        
        // Phase 3: Optional compaction
        self.compact_phase();
        
        let duration = Self::current_time_ms() - start_time;
        
        // Update stats
        {
            let mut stats = self.stats.lock();
            stats.objects_marked = marked.len() as u64;
            stats.objects_swept = freed.0;
            stats.bytes_freed = freed.1;
            stats.objects_kept = marked.len() as u64 - freed.0;
            stats.last_gc_duration_ms = duration;
            stats.last_gc_time = Self::current_time_ms();
        }
        
        kprintln!("[GC] Collected {} objects ({} bytes) in {}ms", 
                  freed.0, freed.1, duration);
        
        self.stop_gc();
        self.stats.lock().clone()
    }
    
    /// Mark phase: traverse all reachable objects starting from path cache
    fn mark_phase(&self) -> BTreeSet<Hash> {
        let mut marked = BTreeSet::new();
        
        let _path_cache = match self.path_cache {
            Some(pc) => pc,
            None => return marked,
        };
        
        let tokens = crate::fs::token::Token::all_ids();
        for token_id in tokens {
            if let Some(token) = Token::lookup(token_id) {
                marked.insert(token.content_hash);
            }
        }
        
        marked
    }
    
    /// Sweep phase: delete objects not in marked set
    fn sweep_phase(&self, marked: &BTreeSet<Hash>) -> (u64, u64) {
        let cas = match self.cas {
            Some(c) => c,
            None => return (0, 0),
        };
        
        let mut objects_freed = 0;
        let mut bytes_freed = 0;
        
        let cas_mut = unsafe { &mut *(cas as *const ContentAddressableStorage as *mut ContentAddressableStorage) };
        for hash in cas_mut.all_hashes() {
            if !marked.contains(&hash) {
                if let Some(data) = cas_mut.get(&hash) {
                    bytes_freed += data.len() as u64;
                    objects_freed += 1;
                }
                cas_mut.remove(&hash);
            }
        }
        
        (objects_freed, bytes_freed)
    }
    
    /// Compact phase: optimize storage layout
    fn compact_phase(&self) {
        // Optional: Rewrite storage to eliminate fragmentation
        // Only needed for spinning disks, less important for SSDs
    }
    
    /// Get all token IDs currently in system
    fn get_all_tokens(&self) -> Vec<TokenId> {
        Token::all_ids()
    }
    
    /// Trigger garbage collection when disk space is low
    pub fn collect_if_needed(&self, threshold_percent: u8) -> bool {
        // Check if free space below threshold
        // If yes, trigger GC
        // Implementation depends on disk space monitoring
        false
    }
    
    /// Run background garbage collection
    pub fn run_background(&self) {
        self.running.store(true, Ordering::Relaxed);
        
        // Spawn background task
        // For now, just do one pass
        self.collect();
        
        self.running.store(false, Ordering::Relaxed);
    }
    
    /// Check if garbage collection is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    
    /// Get current state
    pub fn state(&self) -> GCState {
        *self.state.lock()
    }
    
    /// Get statistics
    pub fn stats(&self) -> GCStats {
        self.stats.lock().clone()
    }
    
    /// Try to acquire GC lock
    fn try_start_gc(&self) -> bool {
        let mut state = self.state.lock();
        if *state != GCState::Idle {
            return false;
        }
        *state = GCState::Marking;
        true
    }
    
    /// Release GC lock
    fn stop_gc(&self) {
        let mut state = self.state.lock();
        *state = GCState::Idle;
    }
    
    fn current_time_ms() -> u64 {
        crate::drivers::pit::get_ticks() * 10
    }
}

/// Reference counting helper for objects
pub struct RefCounted<T> {
    inner: T,
    refs: atomic::AtomicU64,
}

impl<T> RefCounted<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            refs: atomic::AtomicU64::new(1),
        }
    }
    
    pub fn inc(&self) {
        self.refs.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn dec(&self) -> bool {
        let prev = self.refs.fetch_sub(1, Ordering::Relaxed);
        prev == 1
    }
    
    pub fn refs(&self) -> u64 {
        self.refs.load(Ordering::Relaxed)
    }
    
    pub fn get(&self) -> &T {
        &self.inner
    }
    
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}