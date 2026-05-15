//! O(1) Path to Token Resolution
//!
//! Uses hash map instead of directory traversal.
//! Every path → token mapping is stored for instant lookup.

use crate::fs::token::TokenId;
use alloc::collections::BTreeMap;
use alloc::string::String;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::string::ToString;
use alloc::vec::Vec;

/// Statistics for path cache performance
#[derive(Debug, Clone, Copy)]
pub struct PathCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
}

/// O(1) Path to Token Cache
pub struct PathCache {
    /// Main mapping: path → token_id
    path_to_token: Mutex<BTreeMap<String, TokenId>>,
    
    /// Reverse mapping: token_id → path (for cleanup)
    token_to_path: Mutex<BTreeMap<TokenId, String>>,
    
    /// Performance statistics
    hits: AtomicU64,
    misses: AtomicU64,
}

impl PathCache {
    /// Create new empty path cache
    pub fn new() -> Self {
        Self {
            path_to_token: Mutex::new(BTreeMap::new()),
            token_to_path: Mutex::new(BTreeMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }
    
    /// Resolve a path to a token ID (O(1))
    pub fn resolve(&self, path: &str) -> Option<TokenId> {
        let map = self.path_to_token.lock();
        if let Some(&token_id) = map.get(path) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            Some(token_id)
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
    
    /// Insert a path → token mapping
    pub fn insert(&self, path: &str, token_id: TokenId) {
        let mut map = self.path_to_token.lock();
        let mut reverse = self.token_to_path.lock();
        map.insert(path.to_string(), token_id);
        reverse.insert(token_id, path.to_string());
    }
    
    /// Remove a path mapping
    pub fn remove(&self, path: &str) -> Option<TokenId> {
        let mut map = self.path_to_token.lock();
        let mut reverse = self.token_to_path.lock();
        
        if let Some(&token_id) = map.get(path) {
            map.remove(path);
            reverse.remove(&token_id);
            Some(token_id)
        } else {
            None
        }
    }
    
    /// Update path when file/folder is moved (O(1))
    pub fn update_path(&self, old_path: &str, new_path: &str) -> bool {
        let mut map = self.path_to_token.lock();
        let mut reverse = self.token_to_path.lock();
        
        if let Some(&token_id) = map.get(old_path) {
            map.remove(old_path);
            map.insert(new_path.to_string(), token_id);
            reverse.insert(token_id, new_path.to_string());
            true
        } else {
            false
        }
    }
    
    /// Invalidate all cache entries for a subtree (on massive changes)
    pub fn invalidate_subtree(&self, prefix: &str) -> usize {
        let mut map = self.path_to_token.lock();
        let mut reverse = self.token_to_path.lock();
        
        let to_remove: Vec<String> = map.keys()
            .filter(|path| path.starts_with(prefix))
            .cloned()
            .collect();
        
        let count = to_remove.len();
        for path in to_remove {
            if let Some(token) = map.remove(&path) {
                reverse.remove(&token);
            }
        }
        count
    }
    
    /// Check if a path exists in cache
    pub fn contains(&self, path: &str) -> bool {
        self.path_to_token.lock().contains_key(path)
    }
    
    /// Get token ID for path, with fallback to resolve
    pub fn get_or_resolve<F>(&self, path: &str, resolver: F) -> Option<TokenId>
    where
        F: FnOnce(&str) -> Option<TokenId>,
    {
        if let Some(token) = self.resolve(path) {
            return Some(token);
        }
        
        if let Some(token) = resolver(path) {
            self.insert(path, token);
            Some(token)
        } else {
            None
        }
    }
    
    /// Get cache statistics
    pub fn stats(&self) -> PathCacheStats {
        PathCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: self.path_to_token.lock().len(),
        }
    }
    
    /// Hit ratio (0.0 to 1.0)
    pub fn hit_ratio(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 { 1.0 } else { hits as f64 / total as f64 }
    }
    
    /// Clear entire cache
    pub fn clear(&self) {
        self.path_to_token.lock().clear();
        self.token_to_path.lock().clear();
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

/// Path normalization utilities
pub struct PathNormalizer;

impl PathNormalizer {
    /// Normalize a path (remove redundant separators, resolve . and ..)
    pub fn normalize(path: &str) -> String {
        let mut components: Vec<&str> = Vec::new();
        
        for part in path.split('/') {
            match part {
                "" | "." => continue,
                ".." => {
                    components.pop();
                }
                _ => components.push(part),
            }
        }
        
        if components.is_empty() {
            "/".to_string()
        } else {
            let mut result = String::new();
            for comp in components {
                result.push('/');
                result.push_str(comp);
            }
            result
        }
    }
    
    /// Join two path components
    pub fn join(base: &str, child: &str) -> String {
        let mut full = String::from(base);
        if !full.ends_with('/') {
            full.push('/');
        }
        full.push_str(child);
        Self::normalize(&full)
    }
    
    /// Get parent directory path
    pub fn parent(path: &str) -> Option<String> {
        let normalized = Self::normalize(path);
        if normalized == "/" {
            None
        } else {
            let last_slash = normalized.rfind('/')?;
            if last_slash == 0 {
                Some("/".to_string())
            } else {
                Some(normalized[..last_slash].to_string())
            }
        }
    }
    
    /// Get base name (last component)
    pub fn basename(path: &str) -> String {
        let normalized = Self::normalize(path);
        if normalized == "/" {
            "/".to_string()
        } else {
            normalized.split('/').last().unwrap_or("").to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_normalize() {
        assert_eq!(PathNormalizer::normalize("/a//b/./c/../d"), "/a/b/d");
        assert_eq!(PathNormalizer::normalize("a/b/c"), "/a/b/c");
        assert_eq!(PathNormalizer::normalize("/"), "/");
        assert_eq!(PathNormalizer::normalize("."), "/");
        assert_eq!(PathNormalizer::normalize(".."), "/");
        assert_eq!(PathNormalizer::normalize("/a/../../b"), "/b");
    }
    
    #[test]
    fn test_join() {
        assert_eq!(PathNormalizer::join("/home", "user"), "/home/user");
        assert_eq!(PathNormalizer::join("/home/", "/user"), "/home/user");
        assert_eq!(PathNormalizer::join("/", "user"), "/user");
    }
    
    #[test]
    fn test_parent() {
        assert_eq!(PathNormalizer::parent("/home/user/doc.txt"), Some("/home/user".to_string()));
        assert_eq!(PathNormalizer::parent("/home"), Some("/".to_string()));
        assert_eq!(PathNormalizer::parent("/"), None);
    }
    
    #[test]
    fn test_basename() {
        assert_eq!(PathNormalizer::basename("/home/user/doc.txt"), "doc.txt");
        assert_eq!(PathNormalizer::basename("/home/user/"), "user");
        assert_eq!(PathNormalizer::basename("/"), "/");
    }
}