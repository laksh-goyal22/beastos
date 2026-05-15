// //! Disk backend for CAS (persistence)

// use crate::storage::hash::Hash;
// use crate::drivers::ahci::AhciDriver;
// use crate::kprintln;
// use alloc::vec::Vec;
// use core::mem;

// /// Block store for persistent CAS
// pub struct BlockStore {
//     ahci: &'static AhciDriver,
//     port: u8,
//     start_lba: u64,
// }

// impl BlockStore {
//     pub fn new(ahci: &'static AhciDriver, port: u8, start_lba: u64) -> Self {
//         Self { ahci, port, start_lba }
//     }
    
//     /// Write object to disk
//     pub fn write_object(&self, hash: &Hash, data: &[u8]) -> bool {
//         let lba = self.hash_to_lba(hash);
//         let sectors_needed = (data.len() + 511) / 512;
        
//         for i in 0..sectors_needed {
//             let offset = i * 512;
//             let end = (offset + 512).min(data.len());
//             let mut sector = [0u8; 512];
//             sector[..(end - offset)].copy_from_slice(&data[offset..end]);
            
//             if !self.ahci.write_sector(lba + i as u64, &sector) {
//                 return false;
//             }
//         }
//         true
//     }
    
//     /// Read object from disk
//     pub fn read_object(&self, hash: &Hash, size: usize) -> Option<Vec<u8>> {
//         let lba = self.hash_to_lba(hash);
//         let sectors_needed = (size + 511) / 512;
//         let mut data = Vec::with_capacity(size);
        
//         for i in 0..sectors_needed {
//             let mut sector = [0u8; 512];
//             if !self.ahci.read_sector(lba + i as u64, &mut sector) {
//                 return None;
//             }
//             data.extend_from_slice(&sector[..(512.min(size - data.len()))]);
//         }
        
//         Some(data)
//     }
    
//     /// Map hash to LBA address
//     fn hash_to_lba(&self, hash: &Hash) -> u64 {
//         // Simple mapping: use first 8 bytes of hash as offset
//         let offset = u64::from_le_bytes(hash.0[0..8].try_into().unwrap());
//         self.start_lba + (offset % 1_000_000) // Limit to 1M sectors for now
//     }
// }