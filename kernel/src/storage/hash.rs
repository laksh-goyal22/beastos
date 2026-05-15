//! Simple hash placeholder (replace with BLAKE3 later)

use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn zero() -> Self {
        Self([0; 32])
    }
    
    pub fn of_data(data: &[u8]) -> Self {
        let mut hash = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            if i < 32 {
                hash[i] = *byte;
            }
        }
        Self(hash)
    }
    
    pub fn of_chunks(chunks: &[Hash]) -> Self {
        let mut combined = [0u8; 32];
        for chunk in chunks {
            for i in 0..32 {
                combined[i] ^= chunk.0[i];
            }
        }
        Self(combined)
    }
    
    pub fn of_typed(_obj_type: &str, data: &[u8]) -> Self {
        Self::of_data(data)
    }
    
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0[..8] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, "...")
    }
}