//! Universal Executive — File Format Detection and Dispatch
//!
//! Supports ELF (native), PE (Windows), and Mach-O (macOS).

use crate::fs::vfs::FsError;
use crate::memory::vmm;
use crate::fs::elf_loader;
use crate::fs::pe_loader;
use crate::fs::macho_loader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableFormat {
    Elf,
    Pe,      // Windows .exe, .dll
    MachO,   // macOS .dmg, .app, .dylib
    Unknown,
}

pub fn detect_format(data: &[u8]) -> ExecutableFormat {
    if data.len() < 4 {
        return ExecutableFormat::Unknown;
    }
    
    match &data[0..4] {
        // ELF magic: 0x7F 'E' 'L' 'F'
        [0x7F, b'E', b'L', b'F'] => ExecutableFormat::Elf,
        
        // PE magic: 'M' 'Z' (DOS header)
        [b'M', b'Z', _, _] => ExecutableFormat::Pe,
        
        // Mach-O magic: 0xFEEDFACF (64-bit) or 0xFEEDFACF (32-bit)
        [0xCF, 0xFA, 0xED, 0xFE] => ExecutableFormat::MachO, // Little Endian
        [0xFE, 0xED, 0xFA, 0xCF] => ExecutableFormat::MachO, // Big Endian
        
        _ => ExecutableFormat::Unknown,
    }
}

pub struct LoadedImage {
    pub entry: u64,
}

#[derive(Debug)]
pub enum LoaderError {
    Fs(FsError),
    InvalidFormat,
    OutOfMemory,
}

impl From<FsError> for LoaderError {
    fn from(err: FsError) -> Self {
        LoaderError::Fs(err)
    }
}

/// Unified loader that dispatches to the correct format-specific loader.
pub fn load(data: &[u8], vmm: &mut vmm::VirtualMemoryManager) -> Result<LoadedImage, LoaderError> {
    let format = detect_format(data);
    match format {
        ExecutableFormat::MachO => {
            macho_loader::MachOLoader::load(data, vmm)
        }
        ExecutableFormat::Pe => {
            pe_loader::PeLoader::load(data, vmm)
        }
        ExecutableFormat::Elf => {
            match elf_loader::LoadedElf::load(data, vmm) {
                Ok(elf) => Ok(LoadedImage { entry: elf.entry }),
                Err(_) => Err(LoaderError::InvalidFormat),
            }
        }
        ExecutableFormat::Unknown => Err(LoaderError::InvalidFormat),
    }
}
