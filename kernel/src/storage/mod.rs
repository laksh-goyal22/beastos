//! Beast FS Storage Layer

pub mod hash;
pub mod cas;
pub mod block_store;

use crate::kprintln;

pub fn init() {
    kprintln!("  [STORAGE] Initializing Content-Addressable Storage...");
}

// Re-export main types
pub use hash::Hash;
pub use cas::ContentAddressableStorage;