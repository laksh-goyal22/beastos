//! FAT32 Filesystem Driver
//!
//! Read-only implementation supporting:
//! - MBR partition table parsing
//! - FAT32 BPB (BIOS Parameter Block)
//! - Cluster chain traversal
//! - VFAT long filename (LFN) entries
//! - File open, read, stat via VFS File trait
//! - Directory listing via read_dir

use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use crate::drivers::AHCI;
use crate::fs::vfs::{File, FileSystem, FsError};
use crate::kprintln;
use crate::sync::Spinlock;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SECTOR_SIZE: usize = 512;
const DIR_ENTRY_SIZE: usize = 32;

#[allow(dead_code)]
const ATTR_READ_ONLY: u8 = 0x01;
#[allow(dead_code)]
const ATTR_HIDDEN: u8 = 0x02;
#[allow(dead_code)]
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
#[allow(dead_code)]
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LFN: u8 = 0x0F;

#[allow(dead_code)]
const DIR_FREE: u8 = 0x00;
const DIR_DELETED: u8 = 0xE5;
const DIR_END: u8 = 0x00;

const FAT_EOC_MIN: u32 = 0x0FFFFFF8;
#[allow(dead_code)]
const FAT_BAD: u32 = 0x0FFFFFF7;
#[allow(dead_code)]
const FAT_FREE: u32 = 0x00000000;

// Partition type for FAT32 (MBR)
const PART_FAT32_LBA: u8 = 0x0C;
const PART_FAT32_CHS: u8 = 0x0B;

// ---------------------------------------------------------------------------
// MBR Partition Entry
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[repr(C, packed)]
struct MbrPartitionEntry {
    status: u8,       // 0x80 = bootable
    chs_start: [u8; 3],
    partition_type: u8,
    chs_end: [u8; 3],
    lba_start: u32,
    sector_count: u32,
}

// ---------------------------------------------------------------------------
// FAT32 BPB (BIOS Parameter Block) — fields we need
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Fat32Bpb {
    bytes_per_sector: u16,      // offset 0x0B
    sectors_per_cluster: u8,    // offset 0x0D
    reserved_sectors: u16,      // offset 0x0E
    fat_count: u8,              // offset 0x10
    total_sectors_32: u32,      // offset 0x20
    sectors_per_fat_32: u32,    // offset 0x24
    root_cluster: u32,          // offset 0x2C
    fs_info_sector: u16,        // offset 0x30
    backup_boot_sector: u16,    // offset 0x32
}

// ---------------------------------------------------------------------------
// FAT32 Directory Entry (short 8.3 name)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[repr(C, packed)]
struct FatDirEntry {
    name: [u8; 11],         // 0x00
    attr: u8,                // 0x0B
    nt_res: u8,              // 0x0C
    creation_tenths: u8,     // 0x0D
    creation_time: u16,      // 0x0E
    creation_date: u16,      // 0x10
    last_access_date: u16,   // 0x12
    first_cluster_hi: u16,   // 0x14 (FAT32)
    write_time: u16,         // 0x16
    write_date: u16,         // 0x18
    first_cluster_lo: u16,   // 0x1A
    file_size: u32,          // 0x1C
}

// ---------------------------------------------------------------------------
// VFAT Long Directory Entry
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[repr(C, packed)]
struct FatLfnEntry {
    sequence: u8,            // 0x00 (bit 6 = last entry)
    name1: [u16; 5],         // 0x01-0x0A (UTF-16LE, chars 1-5)
    attr: u8,                // 0x0B (must be 0x0F)
    entry_type: u8,          // 0x0C (must be 0)
    checksum: u8,            // 0x0D
    name2: [u16; 6],         // 0x0E-0x19 (chars 6-11)
    _reserved: u16,          // 0x1A
    name3: [u16; 2],         // 0x1C-0x1F (chars 12-13)
}

// ---------------------------------------------------------------------------
// Helper: read a sector via AHCI
// ---------------------------------------------------------------------------

fn read_sector(lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    AHCI.get().map_or(false, |ahci| ahci.read_sector(lba, buf))
}

/// Read multiple sectors into a buffer. Returns bytes read.
fn read_sectors(lba: u64, count: usize, buf: &mut [u8]) -> usize {
    let mut offset = 0;
    for i in 0..count {
        if offset + SECTOR_SIZE > buf.len() {
            break;
        }
        let mut sector = [0u8; SECTOR_SIZE];
        if !read_sector(lba + i as u64, &mut sector) {
            break;
        }
        buf[offset..offset + SECTOR_SIZE].copy_from_slice(&sector);
        offset += SECTOR_SIZE;
    }
    offset
}

// ---------------------------------------------------------------------------
// FAT32 Instance (per partition)
// ---------------------------------------------------------------------------

pub struct Fat32Instance {
    bpb: Fat32Bpb,
    fat_start_lba: u64,
    data_start_lba: u64,
}

impl Fat32Instance {
    /// Try to initialize from an MBR at LBA 0.
    /// Scans the MBR partition table for a FAT32 partition.
    pub fn new() -> Option<Self> {
        if AHCI.get().is_none() {
            return None;
        }

        let mut mbr = [0u8; SECTOR_SIZE];
        if !read_sector(0, &mut mbr) {
            return None;
        }

        // Check boot signature 0x55AA
        if mbr[510] != 0x55 || mbr[511] != 0xAA {
            kprintln!("  [FAT32] No MBR signature");
            return None;
        }

        // Scan 4 partition entries at offset 0x1BE
        let entries_offset = 0x1BE;
        for i in 0..4 {
            let off = entries_offset + i * 16;
            let ptype = mbr[off + 4];
            if ptype == PART_FAT32_LBA || ptype == PART_FAT32_CHS {
                let lba_start = u32::from_le_bytes(
                    mbr[off + 8..off + 12].try_into().ok()?,
                ) as u64;
                return Self::from_partition(lba_start);
            }
        }

        None
    }

    /// Initialize from a known partition start LBA.
    fn from_partition(partition_lba: u64) -> Option<Self> {
        let mut vbr = [0u8; SECTOR_SIZE];
        if !read_sector(partition_lba, &mut vbr) {
            return None;
        }

        if vbr[510] != 0x55 || vbr[511] != 0xAA {
            kprintln!("  [FAT32] No boot signature in VBR at LBA {}", partition_lba);
            return None;
        }

        let bpb = Fat32Bpb {
            bytes_per_sector: u16::from_le_bytes([vbr[0x0B], vbr[0x0C]]),
            sectors_per_cluster: vbr[0x0D],
            reserved_sectors: u16::from_le_bytes([vbr[0x0E], vbr[0x0F]]),
            fat_count: vbr[0x10],
            total_sectors_32: u32::from_le_bytes([
                vbr[0x20], vbr[0x21], vbr[0x22], vbr[0x23],
            ]),
            sectors_per_fat_32: u32::from_le_bytes([
                vbr[0x24], vbr[0x25], vbr[0x26], vbr[0x27],
            ]),
            root_cluster: u32::from_le_bytes([
                vbr[0x2C], vbr[0x2D], vbr[0x2E], vbr[0x2F],
            ]),
            fs_info_sector: u16::from_le_bytes([vbr[0x30], vbr[0x31]]),
            backup_boot_sector: u16::from_le_bytes([vbr[0x32], vbr[0x33]]),
        };

        // Validate BPB
        if bpb.bytes_per_sector != SECTOR_SIZE as u16 {
            kprintln!("  [FAT32] Unsupported sector size: {}", bpb.bytes_per_sector);
            return None;
        }
        if bpb.sectors_per_cluster == 0 || (bpb.sectors_per_cluster & (bpb.sectors_per_cluster - 1)) != 0 {
            kprintln!("  [FAT32] Invalid sectors per cluster: {}", bpb.sectors_per_cluster);
            return None;
        }
        if bpb.sectors_per_fat_32 == 0 {
            kprintln!("  [FAT32] sectors_per_fat is 0");
            return None;
        }

        let fat_start_lba = partition_lba + bpb.reserved_sectors as u64;
        let data_start_lba = partition_lba
            + bpb.reserved_sectors as u64
            + (bpb.fat_count as u64) * bpb.sectors_per_fat_32 as u64;

        kprintln!("  [FAT32] Partition LBA={}, root_cluster={}, spc={}, fats={}",
            partition_lba, bpb.root_cluster, bpb.sectors_per_cluster, bpb.fat_count);

        Some(Self { bpb, fat_start_lba, data_start_lba })
    }

    /// Convert a cluster number to the LBA of its first sector.
    fn cluster_to_lba(&self, cluster: u32) -> u64 {
        self.data_start_lba + ((cluster as u64 - 2) * self.bpb.sectors_per_cluster as u64)
    }

    /// Read the FAT entry for a given cluster.
    fn read_fat_entry(&self, cluster: u32) -> Option<u32> {
        // Each FAT entry is 4 bytes (32 bits, top 4 reserved)
        let fat_offset = cluster as u64 * 4;
        let sector_index = fat_offset / SECTOR_SIZE as u64;
        let entry_offset = (fat_offset % SECTOR_SIZE as u64) as usize;

        let mut sector = [0u8; SECTOR_SIZE];
        if !read_sector(self.fat_start_lba + sector_index, &mut sector) {
            return None;
        }

        let raw = u32::from_le_bytes(
            sector[entry_offset..entry_offset + 4].try_into().ok()?,
        );
        Some(raw & 0x0FFFFFFF)
    }

    /// Follow a cluster chain, collecting all cluster numbers.
    fn cluster_chain(&self, start_cluster: u32) -> Vec<u32> {
        let mut clusters = Vec::new();
        let mut cl = start_cluster;
        loop {
            if cl < 2 || cl >= FAT_EOC_MIN {
                break;
            }
            clusters.push(cl);
            match self.read_fat_entry(cl) {
                Some(next) => {
                    if next < 2 || next >= FAT_EOC_MIN {
                        break;
                    }
                    cl = next;
                }
                None => break,
            }
        }
        clusters
    }

    /// Read raw cluster data into a buffer.
    fn read_cluster_chain(&self, clusters: &[u32], buf: &mut [u8]) -> usize {
        let spc = self.bpb.sectors_per_cluster as usize;
        let cluster_bytes = spc * SECTOR_SIZE;
        let mut total = 0;

        for &cl in clusters {
            let lba = self.cluster_to_lba(cl);
            if total + cluster_bytes > buf.len() {
                let remaining = buf.len() - total;
                total += read_sectors(lba, (remaining + SECTOR_SIZE - 1) / SECTOR_SIZE, &mut buf[total..]);
                break;
            }
            total += read_sectors(lba, spc, &mut buf[total..]);
        }
        total
    }

    /// Compute the 8.3 checksum for validating LFN entries.
    fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
        let mut sum = 0u8;
        for i in 0..11 {
            sum = ((sum & 1) << 7) | (sum >> 1);
            sum = sum.wrapping_add(short_name[i]);
        }
        sum
    }

    /// Parse all directory entries in a cluster chain, returning names and metadata.
    fn read_directory(&self, cluster: u32) -> Result<Vec<DirEntry>, FsError> {
        let clusters = self.cluster_chain(cluster);
        if clusters.is_empty() {
            return Err(FsError::NotFound);
        }

        let spc = self.bpb.sectors_per_cluster as usize;
        let cluster_bytes = spc * SECTOR_SIZE;
        let mut data = vec![0u8; clusters.len() * cluster_bytes];
        self.read_cluster_chain(&clusters, &mut data);

        let mut entries: Vec<DirEntry> = Vec::new();
        let mut lfn_buf: Vec<[u16; 13]> = Vec::new(); // LFN fragments (reversed)

        let entry_count = data.len() / DIR_ENTRY_SIZE;
        for i in 0..entry_count {
            let off = i * DIR_ENTRY_SIZE;
            let attr = data[off + 0x0B];

            // Detect end of directory
            if data[off] == DIR_END && attr != ATTR_LFN {
                // Only stop if NOT an LFN entry (DIR_END in LFN seq number = 0x00 is valid)
                if i == 0 || data[off - DIR_ENTRY_SIZE + 0x0B] != ATTR_LFN {
                    break;
                }
            }

            if attr == ATTR_LFN {
                // VFAT Long File Name entry
                let seq = data[off];
                if seq == DIR_DELETED {
                    lfn_buf.clear();
                    continue;
                }
                let lfn = Self::parse_lfn_entry(&data[off..off + DIR_ENTRY_SIZE]);
                if let Some(fragment) = lfn {
                    lfn_buf.push(fragment);
                }
                continue;
            }

            if data[off] == DIR_DELETED || data[off] == DIR_END {
                lfn_buf.clear();
                continue;
            }

            // Skip volume labels
            if attr & ATTR_VOLUME_ID != 0 {
                lfn_buf.clear();
                continue;
            }

            // Parse short name
            let mut short_name = [0u8; 11];
            short_name.copy_from_slice(&data[off..off + 11]);

            let first_cluster = u16::from_le_bytes([data[off + 0x1A], data[off + 0x1B]]) as u32
                | (u16::from_le_bytes([data[off + 0x14], data[off + 0x15]]) as u32) << 16;

            let file_size = u32::from_le_bytes([
                data[off + 0x1C], data[off + 0x1D],
                data[off + 0x1E], data[off + 0x1F],
            ]);

            let is_dir = (attr & ATTR_DIRECTORY) != 0;

            // LFN checksum (not yet validated against entries)
            let _checksum = Self::lfn_checksum(&short_name);
            let long_name = if !lfn_buf.is_empty() {
                // Reconstruct the name from fragments (entries are in reverse order)
                let mut name_utf16: Vec<u16> = Vec::new();
                for fragment in lfn_buf.iter().rev() {
                    for &c in fragment.iter() {
                        if c == 0 {
                            break;
                        }
                        name_utf16.push(c);
                    }
                }
                // Convert from UTF-16LE to lossy String
                let name = String::from_utf16_lossy(&name_utf16);
                // Strip trailing spaces/dots (padding)
                let name = name.trim_end_matches('\u{FFFF}').trim_end_matches('.').trim_end().to_string();
                if !name.is_empty() {
                    name
                } else {
                    Self::format_short_name(&short_name)
                }
            } else {
                Self::format_short_name(&short_name)
            };

            // Skip '.' and '..' entries
            if short_name[0] == b'.' {
                lfn_buf.clear();
                continue;
            }

            entries.push(DirEntry {
                name: long_name,
                is_directory: is_dir,
                first_cluster,
                file_size: file_size as u64,
            });

            lfn_buf.clear();
        }

        Ok(entries)
    }

    fn parse_lfn_entry(data: &[u8]) -> Option<[u16; 13]> {
        if data.len() < 32 {
            return None;
        }
        let mut name = [0u16; 13];
        let mut idx = 0;

        // Chars 1-5
        for i in 0..5 {
            let c = u16::from_le_bytes([data[1 + i * 2], data[2 + i * 2]]);
            name[idx] = c;
            idx += 1;
        }
        // Chars 6-11
        for i in 0..6 {
            let c = u16::from_le_bytes([data[14 + i * 2], data[15 + i * 2]]);
            name[idx] = c;
            idx += 1;
        }
        // Chars 12-13
        for i in 0..2 {
            let c = u16::from_le_bytes([data[28 + i * 2], data[29 + i * 2]]);
            name[idx] = c;
            idx += 1;
        }

        Some(name)
    }

    /// Format an 8.3 short name as a string (e.g. "README.TXT").
    fn format_short_name(name: &[u8; 11]) -> String {
        let mut s = String::new();
        let name_part = &name[0..8];
        let ext_part = &name[8..11];

        let name_str = core::str::from_utf8(name_part).unwrap_or("");
        let name_str = name_str.trim_end();

        let ext_str = core::str::from_utf8(ext_part).unwrap_or("");
        let ext_str = ext_str.trim_end();

        s.push_str(name_str);
        if !ext_str.is_empty() {
            s.push('.');
            s.push_str(ext_str);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Directory entry struct
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct DirEntry {
    name: String,
    is_directory: bool,
    first_cluster: u32,
    file_size: u64,
}

// ---------------------------------------------------------------------------
// FAT32 File handle
// ---------------------------------------------------------------------------

pub struct Fat32File {
    instance: *const Fat32Instance,
    clusters: Vec<u32>,
    position: u64,
    file_size: u64,
    /// First cluster of the file (needed for lazy cluster chain loading)
    #[allow(dead_code)]
    start_cluster: u32,
}

unsafe impl Send for Fat32File {}
unsafe impl Sync for Fat32File {}

impl Fat32File {
    fn new(instance: &Fat32Instance, start_cluster: u32, file_size: u64, clusters: Vec<u32>) -> Self {
        Self {
            instance: instance as *const Fat32Instance,
            clusters,
            position: 0,
            file_size,
            start_cluster,
        }
    }
}

impl File for Fat32File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        let inst = unsafe { self.instance.as_ref() }.ok_or(FsError::IoError)?;

        if self.clusters.is_empty() {
            return Ok(0);
        }

        let spc = inst.bpb.sectors_per_cluster as usize;
        let cluster_bytes = spc * SECTOR_SIZE;
        let start_byte = self.position as usize;
        let start_cluster_idx = start_byte / cluster_bytes;
        let start_cluster_offset = start_byte % cluster_bytes;

        if start_cluster_idx >= self.clusters.len() || self.position >= self.file_size {
            return Ok(0);
        }

        let mut bytes_read = 0usize;
        let max_to_read = buf.len().min((self.file_size - self.position) as usize);

        for ci in start_cluster_idx..self.clusters.len() {
            if bytes_read >= max_to_read {
                break;
            }
            // First cluster may have an intra-cluster offset; subsequent clusters start at 0
            let cluster_offset = if ci == start_cluster_idx { start_cluster_offset } else { 0 };
            let lba = inst.cluster_to_lba(self.clusters[ci]) + (cluster_offset as u64 / SECTOR_SIZE as u64);
            let sector_offset = cluster_offset % SECTOR_SIZE;
            let remaining_in_cluster = cluster_bytes - cluster_offset;
            let to_read = (max_to_read - bytes_read).min(remaining_in_cluster);
            let sectors_needed = (sector_offset + to_read + SECTOR_SIZE - 1) / SECTOR_SIZE;
            let mut tmp = vec![0u8; sectors_needed * SECTOR_SIZE];
            let n = read_sectors(lba, sectors_needed, &mut tmp);
            let actual_copy = n.saturating_sub(sector_offset).min(to_read);
            buf[bytes_read..bytes_read + actual_copy].copy_from_slice(&tmp[sector_offset..sector_offset + actual_copy]);
            bytes_read += actual_copy;
        }

        self.position += bytes_read as u64;
        Ok(bytes_read)
    }

    fn write(&mut self, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::PermissionDenied)
    }

    fn size(&self) -> u64 {
        self.file_size
    }

    fn seek(&mut self, offset: u64) -> Result<u64, FsError> {
        if offset <= self.file_size {
            self.position = offset;
            Ok(offset)
        } else {
            Err(FsError::IoError)
        }
    }
}

// ---------------------------------------------------------------------------
// FAT32 Filesystem — implements FileSystem trait
// ---------------------------------------------------------------------------

pub struct Fat32Fs {
    instance: Spinlock<Option<Fat32Instance>>,
}

impl Fat32Fs {
    pub fn new() -> Self {
        Self {
            instance: Spinlock::new(None),
        }
    }

    pub fn init_from(&self, partition_lba: u64) -> bool {
        if let Some(inst) = Fat32Instance::from_partition(partition_lba) {
            *self.instance.lock() = Some(inst);
            true
        } else {
            false
        }
    }

    /// Find the entry for a given path relative to the mount point.
    fn find_entry(&self, path: &str) -> Result<DirEntry, FsError> {
        let inst_lock = self.instance.lock();
        let inst = inst_lock.as_ref().ok_or(FsError::NotFound)?;

        // Normalize path: trim leading '/', split into components
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            // Return root directory
            return Ok(DirEntry {
                name: String::from("/"),
                is_directory: true,
                first_cluster: inst.bpb.root_cluster,
                file_size: 0,
            });
        }

        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_cluster = inst.bpb.root_cluster;

        for (idx, component) in components.iter().enumerate() {
            let entries = inst.read_directory(current_cluster)?;
            let is_last = idx == components.len() - 1;

            let found = entries.iter().find(|e| e.name.eq_ignore_ascii_case(component));

            match found {
                Some(entry) => {
                    if entry.is_directory && !is_last {
                        current_cluster = entry.first_cluster;
                    } else if is_last {
                        return Ok(entry.clone());
                    } else {
                        return Err(FsError::NotDirectory);
                    }
                }
                None => return Err(FsError::NotFound),
            }
        }

        Err(FsError::NotFound)
    }
}

impl FileSystem for Fat32Fs {
    fn open(&self, path: &str) -> Result<Box<dyn File>, FsError> {
        let entry = self.find_entry(path)?;
        if entry.is_directory {
            return Err(FsError::IsDirectory);
        }

        let inst_lock = self.instance.lock();
        let inst = inst_lock.as_ref().ok_or(FsError::NotFound)?;

        let clusters = inst.cluster_chain(entry.first_cluster);
        let file = Fat32File::new(inst, entry.first_cluster, entry.file_size, clusters);

        Ok(Box::new(file))
    }

    fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        let entry = self.find_entry(path)?;
        if !entry.is_directory {
            return Err(FsError::NotDirectory);
        }

        let inst_lock = self.instance.lock();
        let inst = inst_lock.as_ref().ok_or(FsError::NotFound)?;

        let cluster = if entry.first_cluster == 0 {
            inst.bpb.root_cluster
        } else {
            entry.first_cluster
        };

        let entries = inst.read_directory(cluster)?;
        Ok(entries.into_iter().map(|e| e.name).collect())
    }

    fn mkdir(&self, _path: &str) -> Result<(), FsError> {
        Err(FsError::PermissionDenied) // read-only
    }

    fn remove(&self, _path: &str) -> Result<(), FsError> {
        Err(FsError::PermissionDenied) // read-only
    }
}

// ---------------------------------------------------------------------------
// Public init function
// ---------------------------------------------------------------------------

use crate::fs::vfs::VFS;

pub fn init() {
    if AHCI.get().is_some() {
        if let Some(inst) = Fat32Instance::new() {
            let fs = Fat32Fs {
                instance: Spinlock::new(Some(inst)),
            };
            VFS.lock().mount("/fat", Box::new(fs));
            kprintln!("  [FAT32] Mounted at /fat");
        } else {
            kprintln!("  [FAT32] No FAT32 partition found");
        }
    } else {
        kprintln!("  [FAT32] AHCI not available");
    }
}
