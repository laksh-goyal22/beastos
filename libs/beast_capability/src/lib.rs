#![no_std]

//! Beast OS Capability System
//!
//! Object-capability security model with 128-bit tokens.
//! O(1) lookup via direct handle indexing into a per-process table.
//!
//! # Design
//! Each capability is a 128-bit token encoding:
//! - Object ID (48 bits) — identifies the kernel object
//! - Rights mask (16 bits) — permitted operations
//! - Generation (32 bits) — revocation counter (stale caps rejected)
//! - Owner (16 bits) — process ID
//! - Random nonce (16 bits) — anti-forgery

/// A 128-bit capability token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Capability {
    /// Lower 64 bits: [object_id(48) | rights(16)]
    lo: u64,
    /// Upper 64 bits: [generation(32) | owner(16) | nonce(16)]
    hi: u64,
}

/// Rights bitmask constants.
pub mod rights {
    pub const READ: u16 = 1 << 0;
    pub const WRITE: u16 = 1 << 1;
    pub const EXECUTE: u16 = 1 << 2;
    pub const MAP: u16 = 1 << 3;       // mmap the object
    pub const TRANSFER: u16 = 1 << 4;  // pass cap to another process
    pub const REVOKE: u16 = 1 << 5;    // revoke child capabilities
    pub const CREATE: u16 = 1 << 6;    // create child objects
    pub const DESTROY: u16 = 1 << 7;   // destroy the object
    pub const ALL: u16 = 0xFF;
}

impl Capability {
    /// Create a new capability.
    pub const fn new(object_id: u64, rights_mask: u16, generation: u32, owner: u16, nonce: u16) -> Self {
        let lo = (object_id & 0xFFFF_FFFF_FFFF) | ((rights_mask as u64) << 48);
        let hi = (generation as u64) | ((owner as u64) << 32) | ((nonce as u64) << 48);
        Self { lo, hi }
    }

    /// Create a null (invalid) capability.
    pub const fn null() -> Self {
        Self { lo: 0, hi: 0 }
    }

    /// Check if this capability is null/invalid.
    pub fn is_null(&self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    /// Get the object ID (48 bits).
    pub fn object_id(&self) -> u64 {
        self.lo & 0xFFFF_FFFF_FFFF
    }

    /// Get the rights mask (16 bits).
    pub fn rights(&self) -> u16 {
        (self.lo >> 48) as u16
    }

    /// Check if a specific right is granted.
    pub fn has_right(&self, right: u16) -> bool {
        self.rights() & right == right
    }

    /// Get the generation counter (revocation tracking).
    pub fn generation(&self) -> u32 {
        self.hi as u32
    }

    /// Get the owner process ID.
    pub fn owner(&self) -> u16 {
        (self.hi >> 32) as u16
    }

    /// Get the anti-forgery nonce.
    pub fn nonce(&self) -> u16 {
        (self.hi >> 48) as u16
    }

    /// Derive a new capability with reduced rights (monotonic reduction).
    ///
    /// You can only remove rights, never add them.
    pub fn restrict(&self, new_rights: u16) -> Self {
        let restricted = self.rights() & new_rights;
        Self::new(self.object_id(), restricted, self.generation(), self.owner(), self.nonce())
    }
}

/// Per-process capability table.
///
/// O(1) lookup by handle index. Max 256 capabilities per process.
pub const CAP_TABLE_SIZE: usize = 256;

#[repr(C)]
pub struct CapabilityTable {
    entries: [CapEntry; CAP_TABLE_SIZE],
    count: usize,
}

/// A single entry in the capability table.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CapEntry {
    pub cap: Capability,
    pub active: bool,
}

impl CapEntry {
    pub const fn empty() -> Self {
        Self { cap: Capability::null(), active: false }
    }
}

impl CapabilityTable {
    /// Create a new empty capability table.
    pub const fn new() -> Self {
        Self {
            entries: [CapEntry::empty(); CAP_TABLE_SIZE],
            count: 0,
        }
    }

    /// Insert a capability, returning its handle (index).
    pub fn insert(&mut self, cap: Capability) -> Option<usize> {
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if !entry.active {
                entry.cap = cap;
                entry.active = true;
                self.count += 1;
                return Some(i);
            }
        }
        None // Table full
    }

    /// Lookup a capability by handle.
    /// O(1) — direct array index.
    pub fn lookup(&self, handle: usize) -> Option<&Capability> {
        if handle < CAP_TABLE_SIZE && self.entries[handle].active {
            Some(&self.entries[handle].cap)
        } else {
            None
        }
    }

    /// Validate a capability: check handle exists and has required rights.
    pub fn validate(&self, handle: usize, required_rights: u16) -> Result<&Capability, CapError> {
        let cap = self.lookup(handle).ok_or(CapError::InvalidHandle)?;
        if !cap.has_right(required_rights) {
            return Err(CapError::InsufficientRights);
        }
        Ok(cap)
    }

    /// Revoke (remove) a capability by handle.
    pub fn revoke(&mut self, handle: usize) -> bool {
        if handle < CAP_TABLE_SIZE && self.entries[handle].active {
            self.entries[handle].active = false;
            self.entries[handle].cap = Capability::null();
            self.count -= 1;
            true
        } else {
            false
        }
    }

    /// Number of active capabilities.
    pub fn len(&self) -> usize {
        self.count
    }
}

/// Capability validation errors.
#[derive(Debug, Clone, Copy)]
pub enum CapError {
    InvalidHandle,
    InsufficientRights,
    StaleGeneration,
    Revoked,
}
