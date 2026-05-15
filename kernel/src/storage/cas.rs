//! Content-Addressable Storage
//!
//! Stores immutable objects by their BLAKE3 hash.

use crate::storage::hash::Hash;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Stored object with metadata
pub struct StoredObject {
    /// Actual data
    pub data: Vec<u8>,
    /// Number of tokens referencing this object
    pub ref_count: AtomicU64,
    /// Creation timestamp
    pub created: u64,
    /// Last access timestamp
    pub last_accessed: AtomicU64,
}

/// Content-Addressable Storage
pub struct ContentAddressableStorage {
    /// Main object store: hash → object
    objects: BTreeMap<Hash, StoredObject>,
    /// Total bytes stored
    total_bytes: u64,
    /// Maximum storage size (0 = unlimited)
    max_bytes: u64,
}

impl ContentAddressableStorage {
    /// Create new CAS
    pub fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
            total_bytes: 0,
            max_bytes: 0,
        }
    }
    
    /// Set maximum storage size
    pub fn set_max_bytes(&mut self, max: u64) {
        self.max_bytes = max;
    }
    
    /// Store data, return its hash
    pub fn put(&mut self, data: &[u8]) -> Result<Hash, StorageError> {
        let hash = Hash::of_data(data);
        
        // Check if already exists
        if let Some(existing) = self.objects.get(&hash) {
            existing.ref_count.fetch_add(1, Ordering::Relaxed);
            return Ok(hash);
        }
        
        // Check space
        if self.max_bytes > 0 && self.total_bytes + data.len() as u64 > self.max_bytes {
            // Try garbage collection
            self.garbage_collect(data.len() as u64)?;
            if self.total_bytes + data.len() as u64 > self.max_bytes {
                return Err(StorageError::OutOfSpace);
            }
        }
        
        // Store new object
        let object = StoredObject {
            data: data.to_vec(),
            ref_count: AtomicU64::new(1),
            created: self.current_time(),
            last_accessed: AtomicU64::new(self.current_time()),
        };
        
        self.total_bytes += data.len() as u64;
        self.objects.insert(hash, object);
        
        Ok(hash)
    }
    
    /// Retrieve data by hash
    pub fn get(&mut self, hash: &Hash) -> Option<&[u8]> {
        if let Some(object) = self.objects.get(hash) {
            object.last_accessed.store(self.current_time(), Ordering::Relaxed);
            Some(&object.data)
        } else {
            None
        }
    }
    
    /// Increment reference count for an object
    pub fn ref_inc(&mut self, hash: &Hash) -> Result<(), StorageError> {
        if let Some(object) = self.objects.get_mut(hash) {
            object.ref_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            Err(StorageError::NotFound)
        }
    }
    
    /// Decrement reference count (may delete object)
    pub fn ref_dec(&mut self, hash: &Hash) -> Result<(), StorageError> {
        if let Some(object) = self.objects.get_mut(hash) {
            let new_count = object.ref_count.fetch_sub(1, Ordering::Relaxed) - 1;
            if new_count == 0 {
                // No more references, delete
                self.total_bytes -= object.data.len() as u64;
                self.objects.remove(hash);
            }
            Ok(())
        } else {
            Err(StorageError::NotFound)
        }
    }
    
    /// Check if object exists
    pub fn exists(&self, hash: &Hash) -> bool {
        self.objects.contains_key(hash)
    }
    
    /// Get reference count
    pub fn ref_count(&self, hash: &Hash) -> u64 {
        self.objects.get(hash).map(|o| o.ref_count.load(Ordering::Relaxed)).unwrap_or(0)
    }
    
    /// Garbage collect old unreferenced objects
    fn garbage_collect(&mut self, needed: u64) -> Result<(), StorageError> {
        let mut freed = 0;
        let cutoff = self.current_time() - (30 * 24 * 60 * 60); // 30 days
        
        // Collect old unreferenced objects
        let to_remove: Vec<Hash> = self.objects
            .iter()
            .filter(|(_, obj)| {
                obj.ref_count.load(Ordering::Relaxed) == 0 &&
                obj.last_accessed.load(Ordering::Relaxed) < cutoff
            })
            .map(|(hash, _)| *hash)
            .collect();
        
        for hash in to_remove {
            if let Some(obj) = self.objects.remove(&hash) {
                freed += obj.data.len() as u64;
                if freed >= needed { break; }
            }
        }
        
        self.total_bytes -= freed;
        Ok(())
    }
    
    /// Get all hashes currently stored
    pub fn all_hashes(&self) -> Vec<Hash> {
        self.objects.keys().cloned().collect()
    }

    /// Remove an object by hash (returns true if existed)
    pub fn remove(&mut self, hash: &Hash) -> bool {
        if let Some(obj) = alloc::collections::BTreeMap::remove(&mut self.objects, hash) {
            self.total_bytes -= obj.data.len() as u64;
            true
        } else {
            false
        }
    }

    /// Get current timestamp
    fn current_time(&self) -> u64 {
        crate::drivers::pit::get_ticks() / 100
    }
    
    /// Statistics
    pub fn stats(&self) -> StorageStats {
        StorageStats {
            object_count: self.objects.len(),
            total_bytes: self.total_bytes,
            dedup_ratio: self.calculate_dedup_ratio(),
        }
    }
    
    fn calculate_dedup_ratio(&self) -> f64 {
        // Simplified: count unique vs total
        let unique: u64 = self.objects.len() as u64;
        let total: u64 = self.objects.values()
            .map(|obj| obj.ref_count.load(Ordering::Relaxed))
            .sum();
        
        if total > 0 {
            total as f64 / unique as f64
        } else {
            1.0
        }
    }
}

#[derive(Debug)]
pub enum StorageError {
    OutOfSpace,
    NotFound,
}

pub struct StorageStats {
    pub object_count: usize,
    pub total_bytes: u64,
    pub dedup_ratio: f64,
}