//! Permission management for Beast FS

use crate::fs::token::{Permissions, TokenId, Token};
use alloc::collections::BTreeMap;
use spin::Mutex;
use alloc::vec::Vec;

/// User ID type
pub type UserId = u64;

/// Access Control List (ACL) for files/folders
pub struct AclEntry {
    pub user_id: UserId,
    pub permissions: Permissions,
}

/// Permission manager
pub struct PermissionManager {
    acls: Mutex<BTreeMap<TokenId, Vec<AclEntry>>>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            acls: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Set permissions for a user on a token
    pub fn set_permission(&self, token_id: TokenId, user_id: UserId, perms: Permissions) {
        let mut acls = self.acls.lock();
        let entries = acls.entry(token_id).or_insert_with(Vec::new);
        
        // Remove existing entry for this user
        entries.retain(|e| e.user_id != user_id);
        entries.push(AclEntry { user_id, permissions: perms });
    }
    
    /// Check if user has permission
    pub fn check(&self, token_id: TokenId, user_id: UserId, required: Permissions) -> bool {
        // First check token's own permissions
        if let Some(token) = Token::lookup(token_id) {
            if token.owner == user_id {
                return token.permissions.has(required);
            }
        }
        
        // Then check ACL
        let acls = self.acls.lock();
        if let Some(entries) = acls.get(&token_id) {
            for entry in entries {
                if entry.user_id == user_id && entry.permissions.has(required) {
                    return true;
                }
            }
        }
        false
    }
}