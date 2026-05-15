//! Merkle Tree for Folder Hashing
//!
//! Every folder's hash depends on its name, parent, and all children.
//! This creates a cryptographic commitment to the entire filesystem state.

use crate::storage::Hash;
use crate::fs::token::{TokenId, Token};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use crate::alloc::string::ToString;

/// Node in the Merkle tree (represents a file or folder)
#[derive(Debug, Clone)]
pub struct MerkleNode {
    pub hash: Hash,
    pub token_id: TokenId,
    pub name: String,
    pub parent_hash: Option<Hash>,
    pub children: BTreeMap<String, Hash>,
    pub is_directory: bool,
    pub size: u64,
}

/// Merkle Tree for filesystem state
pub struct MerkleTree {
    /// Root hash of the entire tree
    root_hash: Mutex<Option<Hash>>,
    
    /// Cache of node hashes for quick access
    node_cache: Mutex<BTreeMap<Hash, MerkleNode>>,
    
    /// Path to node hash mapping (for O(1) lookup)
    path_to_hash: Mutex<BTreeMap<String, Hash>>,
}

impl MerkleTree {
    /// Create new empty Merkle tree
    pub fn new() -> Self {
        Self {
            root_hash: Mutex::new(None),
            node_cache: Mutex::new(BTreeMap::new()),
            path_to_hash: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Simple XOR-based hash (for now - replace with BLAKE3 later)
    fn simple_hash(data: &[u8]) -> [u8; 32] {
        let mut hash = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    }
    
    /// Calculate hash for a file
    pub fn hash_file(content_hash: Hash, name: &str, parent_hash: Option<Hash>) -> Hash {
        let mut combined = Vec::new();
        
        if let Some(parent) = parent_hash {
            combined.extend_from_slice(parent.as_bytes());
        }
        combined.extend_from_slice(name.as_bytes());
        combined.extend_from_slice(content_hash.as_bytes());
        
        Hash::from_bytes(&Self::simple_hash(&combined))
    }
    
    /// Calculate hash for a folder
    pub fn hash_folder(
        name: &str,
        parent_hash: Option<Hash>,
        children: &BTreeMap<String, Hash>,
    ) -> Hash {
        let mut combined = Vec::new();
        
        if let Some(parent) = parent_hash {
            combined.extend_from_slice(parent.as_bytes());
        }
        combined.extend_from_slice(name.as_bytes());
        
        // Sort children by name for deterministic hashing
        for (child_name, child_hash) in children.iter() {
            combined.extend_from_slice(child_name.as_bytes());
            combined.extend_from_slice(child_hash.as_bytes());
        }
        
        Hash::from_bytes(&Self::simple_hash(&combined))
    }
    
    /// Update the hash for a token (recalculates up to root)
    pub fn update_hash(&self, token_id: TokenId) -> Result<Hash, MerkleError> {
        let token = Token::lookup(token_id).ok_or(MerkleError::TokenNotFound)?;
        
        let new_hash = if token.token_type == crate::fs::token::TokenType::File {
            Self::hash_file(token.content_hash, &token.path, None)
        } else {
            // For folder, need to get current children hashes
            let children_hashes = self.get_children_hashes(&token)?;
            Self::hash_folder(&token.path, None, &children_hashes)
        };
        
        // Update cache
        {
            let mut cache = self.node_cache.lock();
            if let Some(node) = cache.get_mut(&new_hash) {
                node.hash = new_hash;
            } else {
                cache.insert(new_hash, MerkleNode {
                    hash: new_hash,
                    token_id,
                    name: token.path.clone(),
                    parent_hash: None,
                    children: BTreeMap::new(),
                    is_directory: token.token_type == crate::fs::token::TokenType::Folder,
                    size: token.size,
                });
            }
        }
        
        // Update path mapping
        {
            let mut path_map = self.path_to_hash.lock();
            path_map.insert(token.path.clone(), new_hash);
        }
        
        // Update root if this is root
        if token.parent.is_none() {
            let mut root = self.root_hash.lock();
            *root = Some(new_hash);
        }
        
        Ok(new_hash)
    }
    
    /// Get children hashes for a folder
    fn get_children_hashes(&self, token: &Token) -> Result<BTreeMap<String, Hash>, MerkleError> {
        let mut children_hashes = BTreeMap::new();
        
        for child_id in &token.children {
            let child_token = Token::lookup(*child_id).ok_or(MerkleError::TokenNotFound)?;
            let child_hash = self.get_hash_for_token(*child_id)?;
            children_hashes.insert(child_token.path.clone(), child_hash);
        }
        
        Ok(children_hashes)
    }
    
    /// Get hash for a token
    pub fn get_hash_for_token(&self, token_id: TokenId) -> Result<Hash, MerkleError> {
        // First check cache
        {
            let cache = self.node_cache.lock();
            for (hash, node) in cache.iter() {
                if node.token_id == token_id {
                    return Ok(*hash);
                }
            }
        }
        
        // Not in cache, compute
        self.update_hash(token_id)
    }
    
    /// Get current root hash (snapshot ID)
    pub fn root_hash(&self) -> Option<Hash> {
        *self.root_hash.lock()
    }

    /// Set the root hash (used by snapshot restore).
    pub fn set_root_hash(&self, hash: Hash) {
        *self.root_hash.lock() = Some(hash);
    }
    
    /// Verify entire tree integrity
    pub fn verify_integrity(&self) -> Result<bool, MerkleError> {
        let root = self.root_hash.lock();
        let root_hash = root.as_ref().ok_or(MerkleError::NoRoot)?;
        self.verify_node(*root_hash)
    }
    
    /// Verify a node and all its descendants
    fn verify_node(&self, hash: Hash) -> Result<bool, MerkleError> {
        let cache = self.node_cache.lock();
        let node = cache.get(&hash).ok_or(MerkleError::NodeNotFound)?;
        
        // Recompute hash from children
        let recomputed = if node.is_directory {
            let children_hashes = self.get_children_hashes_for_node(node)?;
            Self::hash_folder(&node.name, node.parent_hash, &children_hashes)
        } else {
            let token = Token::lookup(node.token_id).ok_or(MerkleError::TokenNotFound)?;
            Self::hash_file(token.content_hash, &node.name, node.parent_hash)
        };
        
        if recomputed != hash {
            return Ok(false);
        }
        
        // Verify all children
        for child_hash in node.children.values() {
            if !self.verify_node(*child_hash)? {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    /// Get children hashes for a node
    fn get_children_hashes_for_node(&self, node: &MerkleNode) -> Result<BTreeMap<String, Hash>, MerkleError> {
        Ok(node.children.clone())
    }
    
    /// Get root hash for snapshot
    pub fn get_snapshot_root(&self) -> Option<Hash> {
        self.root_hash()
    }
    
    /// Pretty print tree (for debugging)
    pub fn print_tree(&self) {
        if let Some(root) = self.root_hash() {
            self.print_node(root, 0);
        }
    }
    
    fn print_node(&self, hash: Hash, depth: usize) {
        let indent = "  ".repeat(depth);
        let cache = self.node_cache.lock();
        
        if let Some(node) = cache.get(&hash) {
            let type_str = if node.is_directory { "📁" } else { "📄" };
            crate::kprintln!("{}{} {} ({:?})", indent, type_str, node.name, node.hash);
            
            for (child_name, child_hash) in &node.children {
                crate::kprintln!("{}  ├─ {}", indent, child_name);
                self.print_node(*child_hash, depth + 1);
            }
        }
    }
}

#[derive(Debug)]
pub enum MerkleError {
    TokenNotFound,
    NodeNotFound,
    NoRoot,
    HashMismatch,
}

/// Snapshot manager for time travel
pub struct SnapshotManager {
    snapshots: Mutex<BTreeMap<Hash, Snapshot>>,
    tree: MerkleTree,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub hash: Hash,
    pub timestamp: u64,
    pub parent: Option<Hash>,
    pub message: String,
    pub tags: Vec<String>,
}

impl SnapshotManager {
    pub fn new() -> Self {
        Self {
            snapshots: Mutex::new(BTreeMap::new()),
            tree: MerkleTree::new(),
        }
    }
    
    /// Create a new snapshot
    pub fn snapshot(&self, message: &str) -> Option<Hash> {
        let root = self.tree.root_hash()?;
        let mut snapshots = self.snapshots.lock();
        
        let snapshot = Snapshot {
            hash: root,
            timestamp: Self::current_time(),
            parent: None,
            message: message.to_string(),
            tags: Vec::new(),
        };
        
        snapshots.insert(root, snapshot);
        Some(root)
    }
    
    /// Add tag to snapshot
    pub fn tag(&self, snapshot_hash: Hash, tag: &str) -> bool {
        let mut snapshots = self.snapshots.lock();
        if let Some(snapshot) = snapshots.get_mut(&snapshot_hash) {
            snapshot.tags.push(tag.to_string());
            true
        } else {
            false
        }
    }
    
    /// Find snapshot by tag
    pub fn find_by_tag(&self, tag: &str) -> Option<Hash> {
        let snapshots = self.snapshots.lock();
        for (hash, snapshot) in snapshots.iter() {
            if snapshot.tags.contains(&tag.to_string()) {
                return Some(*hash);
            }
        }
        None
    }
    
    /// List all snapshots
    pub fn list_snapshots(&self) -> Vec<(Hash, Snapshot)> {
        let snapshots = self.snapshots.lock();
        snapshots.iter().map(|(h, s)| (*h, s.clone())).collect()
    }
    
    fn current_time() -> u64 {
        crate::drivers::pit::get_ticks() / 100
    }
}