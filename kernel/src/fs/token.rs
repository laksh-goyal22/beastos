//! 128-bit Capability Tokens

use crate::storage::Hash;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use alloc::string::ToString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenId(pub u128);

impl TokenId {
    pub fn random() -> Self {
        // Use RDRAND instruction for random number generation
        let high: u64;
        let low: u64;
        unsafe {
            core::arch::asm!(
                "rdrand rax",
                out("rax") high,
                options(nomem, nostack)
            );
            core::arch::asm!(
                "rdrand rax",
                out("rax") low,
                options(nomem, nostack)
            );
        }
        Self(((high as u128) << 64) | (low as u128))
    }
    
    pub fn from_raw(high: u64, low: u64) -> Self {
        Self(((high as u128) << 64) | (low as u128))
    }
    
    pub fn high(&self) -> u64 {
        (self.0 >> 64) as u64
    }
    
    pub fn low(&self) -> u64 {
        self.0 as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions(pub u8);

impl Permissions {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const EXECUTE: Self = Self(1 << 2);
    pub const DELETE: Self = Self(1 << 3);
    pub const ADMIN: Self = Self(1 << 4);
    pub const SHARE: Self = Self(1 << 5);
    
    pub const READ_WRITE: Self = Self(Self::READ.0 | Self::WRITE.0);
    pub const READ_ONLY: Self = Self(Self::READ.0);
    pub const ALL: Self = Self(0b00111111);
    
    pub fn has(&self, perm: Self) -> bool {
        (self.0 & perm.0) != 0
    }
    
    pub fn add(&mut self, perm: Self) {
        self.0 |= perm.0;
    }
    
    pub fn remove(&mut self, perm: Self) {
        self.0 &= !perm.0;
    }
    
    pub fn bits(&self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    File,
    Folder,
    Share,
    Move,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub id: TokenId,
    pub path: alloc::string::String,
    pub content_hash: Hash,
    pub token_type: TokenType,
    pub permissions: Permissions,
    pub owner: u64,
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub parent: Option<TokenId>,
    pub expires: Option<u64>,
    pub children: Vec<TokenId>,
}

static TOKEN_REGISTRY: Mutex<BTreeMap<TokenId, Token>> = Mutex::new(BTreeMap::new());

impl Token {
    pub fn new_file(path: &str, content_hash: Hash, owner: u64, size: u64, permissions: Permissions) -> Self {
        Self {
            id: TokenId::random(),
            path: path.to_string(),
            content_hash,
            token_type: TokenType::File,
            permissions,
            owner,
            size,
            created: 0,
            modified: 0,
            parent: None,
            expires: None,
            children: Vec::new(),
        }
    }
    
    pub fn new_folder(path: &str, owner: u64, permissions: Permissions) -> Self {
        Self {
            id: TokenId::random(),
            path: path.to_string(),
            content_hash: Hash::zero(),
            token_type: TokenType::Folder,
            permissions,
            owner,
            size: 0,
            created: 0,
            modified: 0,
            parent: None,
            expires: None,
            children: Vec::new(),
        }
    }
    
    pub fn new_share(target_token: TokenId, expires_in_seconds: u64, allowed_permissions: Permissions) -> Self {
        let deadline = crate::drivers::pit::get_ticks() + expires_in_seconds * crate::drivers::pit::get_hz();
        Self {
            id: TokenId::random(),
            path: alloc::string::String::new(),
            content_hash: crate::storage::Hash::zero(),
            token_type: TokenType::Share,
            permissions: allowed_permissions,
            owner: 0,
            size: 0,
            created: 0,
            modified: 0,
            parent: Some(target_token),
            expires: Some(deadline),
            children: Vec::new(),
        }
    }
    
    pub fn register(&self) {
        TOKEN_REGISTRY.lock().insert(self.id, self.clone());
    }
    
    pub fn lookup(id: TokenId) -> Option<Self> {
        TOKEN_REGISTRY.lock().get(&id).cloned()
    }
    
    pub fn revoke(id: TokenId) -> bool {
        TOKEN_REGISTRY.lock().remove(&id).is_some()
    }

    /// Return all registered token IDs (for GC enumeration).
    pub fn all_ids() -> Vec<TokenId> {
        TOKEN_REGISTRY.lock().keys().cloned().collect()
    }
    
    pub fn check_permission(&self, required: Permissions, user_id: u64) -> bool {
        if self.is_expired() {
            return false;
        }
        if self.owner == user_id {
            return self.permissions.has(required);
        }
        if self.token_type == TokenType::Share {
            if let Some(parent) = self.parent {
                if let Some(parent_token) = Self::lookup(parent) {
                    return parent_token.check_permission(required, user_id);
                }
            }
        }
        false
    }
    
    pub fn is_expired(&self) -> bool {
        if let Some(deadline) = self.expires {
            crate::drivers::pit::get_ticks() >= deadline
        } else {
            false
        }
    }
}