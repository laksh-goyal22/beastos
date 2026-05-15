//! File Operations with Zero-Copy Tokens

use crate::storage::cas::ContentAddressableStorage;
use crate::fs::token::{Token, TokenId, Permissions};
use crate::fs::path_cache::PathCache;
use crate::fs::merkle::MerkleTree;
use crate::alloc::string::ToString;
use crate::kprintln;

pub struct FileSystem {
    pub cas: ContentAddressableStorage,
    pub path_cache: PathCache,
    pub merkle_tree: MerkleTree,
}

impl FileSystem {
    pub fn new() -> Self {
        Self {
            cas: ContentAddressableStorage::new(),
            path_cache: PathCache::new(),
            merkle_tree: MerkleTree::new(),
        }
    }
    
    pub fn save_file(&mut self, path: &str, data: &[u8], owner: u64, permissions: Permissions) -> Result<TokenId, FileError> {
        let content_hash = match self.cas.put(data) {
            Ok(h) => h,
            Err(_) => return Err(FileError::OutOfSpace),
        };
        
        let token = Token::new_file(path, content_hash, owner, data.len() as u64, permissions);
        let token_id = token.id;
        token.register();
        
        self.path_cache.insert(path, token_id);
        
        if let Some(parent_path) = crate::fs::path_cache::PathNormalizer::parent(path) {
            if let Some(parent_token) = self.path_cache.resolve(&parent_path) {
                let mut parent = Token::lookup(parent_token).unwrap();
                parent.children.push(token_id);
                parent.modified = 0;
                parent.register();
                let _ = self.merkle_tree.update_hash(parent_token);
            }
        }
        
        let _ = self.merkle_tree.update_hash(token_id);
        
        kprintln!("[FS] Saved: {}", path);
        Ok(token_id)
    }
    
    pub fn read_file(&mut self, path: &str, user_id: u64) -> Result<&[u8], FileError> {
        let token_id = self.path_cache.resolve(path).ok_or(FileError::NotFound)?;
        let token = Token::lookup(token_id).ok_or(FileError::NotFound)?;
        
        if !token.check_permission(Permissions::READ, user_id) {
            return Err(FileError::PermissionDenied);
        }
        
        let data = self.cas.get(&token.content_hash).ok_or(FileError::Corrupted)?;
        Ok(data)
    }
    
    pub fn move_file(&mut self, old_path: &str, new_path: &str, user_id: u64) -> Result<(), FileError> {
        let token_id = self.path_cache.resolve(old_path).ok_or(FileError::NotFound)?;
        let token = Token::lookup(token_id).ok_or(FileError::NotFound)?;
        
        if !token.check_permission(Permissions::WRITE, user_id) {
            return Err(FileError::PermissionDenied);
        }
        
        self.path_cache.update_path(old_path, new_path);
        
        let mut token = token;
        token.path = new_path.to_string();
        token.modified = 0;
        token.register();
        
        if let Some(old_parent) = crate::fs::path_cache::PathNormalizer::parent(old_path) {
            if let Some(parent_token) = self.path_cache.resolve(&old_parent) {
                let mut parent = Token::lookup(parent_token).unwrap();
                parent.children.retain(|&id| id != token_id);
                parent.register();
                let _ = self.merkle_tree.update_hash(parent_token);
            }
        }
        
        if let Some(new_parent) = crate::fs::path_cache::PathNormalizer::parent(new_path) {
            if let Some(parent_token) = self.path_cache.resolve(&new_parent) {
                let mut parent = Token::lookup(parent_token).unwrap();
                parent.children.push(token_id);
                parent.register();
                let _ = self.merkle_tree.update_hash(parent_token);
            }
        }
        
        let _ = self.merkle_tree.update_hash(token_id);
        
        kprintln!("[FS] Moved: {} → {}", old_path, new_path);
        Ok(())
    }
    
    pub fn copy_file(&mut self, src_path: &str, dst_path: &str, user_id: u64) -> Result<TokenId, FileError> {
        let src_token_id = self.path_cache.resolve(src_path).ok_or(FileError::NotFound)?;
        let src_token = Token::lookup(src_token_id).ok_or(FileError::NotFound)?;
        
        if !src_token.check_permission(Permissions::READ, user_id) {
            return Err(FileError::PermissionDenied);
        }
        
        let new_token = Token::new_file(dst_path, src_token.content_hash, user_id, src_token.size, src_token.permissions);
        let new_token_id = new_token.id;
        new_token.register();
        
        let _ = self.cas.ref_inc(&src_token.content_hash);
        self.path_cache.insert(dst_path, new_token_id);
        
        if let Some(parent_path) = crate::fs::path_cache::PathNormalizer::parent(dst_path) {
            if let Some(parent_token) = self.path_cache.resolve(&parent_path) {
                let mut parent = Token::lookup(parent_token).unwrap();
                parent.children.push(new_token_id);
                parent.register();
                let _ = self.merkle_tree.update_hash(parent_token);
            }
        }
        
        kprintln!("[FS] Copied: {} → {}", src_path, dst_path);
        Ok(new_token_id)
    }
    
    pub fn delete_file(&mut self, path: &str, user_id: u64) -> Result<(), FileError> {
        let token_id = self.path_cache.resolve(path).ok_or(FileError::NotFound)?;
        let token = Token::lookup(token_id).ok_or(FileError::NotFound)?;
        
        if !token.check_permission(Permissions::DELETE, user_id) {
            return Err(FileError::PermissionDenied);
        }
        
        let _ = self.cas.ref_dec(&token.content_hash);
        
        if let Some(parent_path) = crate::fs::path_cache::PathNormalizer::parent(path) {
            if let Some(parent_token) = self.path_cache.resolve(&parent_path) {
                let mut parent = Token::lookup(parent_token).unwrap();
                parent.children.retain(|&id| id != token_id);
                parent.register();
                let _ = self.merkle_tree.update_hash(parent_token);
            }
        }
        
        self.path_cache.remove(path);
        Token::revoke(token_id);
        
        kprintln!("[FS] Deleted: {}", path);
        Ok(())
    }
}

#[derive(Debug)]
pub enum FileError {
    NotFound,
    PermissionDenied,
    Corrupted,
    OutOfSpace,
    AlreadyExists,
    InvalidPath,
}