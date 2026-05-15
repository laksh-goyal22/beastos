pub mod tar;
pub mod elf_loader;
pub mod pe_loader;
pub mod macho_loader;
pub mod pe_patcher;
pub mod universal_exec;
pub mod ramfs;
/// Beast FS - Content-Addressable Versioned Filesystem

pub mod token;
pub mod permissions;
pub mod path_cache;
pub mod merkle;
pub mod file_ops;
pub mod ring_transfer;
pub mod vfs;
pub mod fat32;
pub mod devfs;

use crate::kprintln;

pub fn init() {
    kprintln!("  [FS] Initializing Beast FS...");
    vfs::init();
    fat32::init();
    devfs::init();
    kprintln!("  [FS] Ready");
}